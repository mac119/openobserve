# 索引构建管道

## 入口函数

```rust
// src/service/tantivy/mod.rs
pub(crate) async fn create_tantivy_index(
    caller: &str,           // "INGESTER" 或 "COMPACTOR"
    org_id: &str,
    parquet_file_name: &str,
    fts_fields: &[String],  // 全文搜索字段（内容合并到 _all）
    index_fields: &[String], // 二级索引字段（独立 fast field）
    schema: Arc<Schema>,     // Arrow schema
    buf: Bytes,              // Parquet/Vortex 文件内容
) -> Result<usize, anyhow::Error>
```

返回值: 生成的 .ttv 文件大小（字节）

## 调用时机

### 1. Ingester 阶段

**文件**: `src/job/files/parquet.rs`

```
WAL (.wal)
  → persist to Parquet (Arrow → Parquet bytes)
  → create_tantivy_index("INGESTER", ...)
  → Upload .parquet + .ttv to Object Storage
  → Update file_list metadata (index_size)
```

关键代码:
```rust
let index_size = create_tantivy_index(
    "INGESTER", &org_id, &new_file_key,
    &full_text_search_fields, &index_fields,
    latest_schema.clone(), buf,
).await?;
new_file_meta.index_size = index_size as i64;
```

### 2. Compactor 阶段

**文件**: `src/service/compact/merge.rs`

```
多个小 Parquet 文件
  → DataFusion 合并为一个大 Parquet
  → create_tantivy_index("COMPACTOR", ...)
  → Upload merged .parquet + .ttv
  → Update file_list
```

## 构建流程详解

### Step 1: Schema 构建

```rust
fn build_tantivy_schema(fts_fields, index_fields, schema) -> TantivyIndexSchema
```

- 遍历 Arrow schema 中的列
- 如果列名在 `fts_fields` 中且类型是 Utf8/LargeUtf8 → 标记为 FTS 字段
- 如果列名在 `index_fields` 中 → 创建 TEXT field (raw tokenizer + fast field)
- 创建 `_all` 字段（TEXT, o2 tokenizer）
- 创建 `_timestamp` 字段（I64, fast field）

### Step 2: 选择构建模式

```rust
if cfg.compact.tantivy_builder_thread_num <= 1 {
    sequential::build_index(schema, dir_writer, file_stream)
} else {
    parallel::build_index(schema, dir_writer, chunk_iter, thread_num)
}
```

### Step 3a: Sequential 构建

**文件**: `src/service/tantivy/sequential.rs`

```
RecordBatch stream (通过 channel 传递)
  → blocking task 中:
    single_segment_index_writer(schema, dir, rx)
      → 遍历每行:
        - FTS 字段 → 拼接文本写入 _all
        - index_fields → 各自写入独立字段
        - _timestamp → 写入时间戳
      → commit()
```

### Step 3b: Parallel 构建

**文件**: `src/service/tantivy/parallel.rs`

```
Parquet 按 row_group 切分
  → 每个 row_group 独立构建一个 tantivy index segment
  → merge_indices() 合并所有 segment 为单一 segment
```

### Step 4: 输出 Puffin

```rust
// PuffinDirWriter
dir_writer.set_row_group_size(total_rows);  // 记录行数属性
let puffin_bytes = dir_writer.to_puffin_bytes()?;
// 过滤: 只保留 .term/.idx/.pos/.fast + meta.json + footer_cache
// 压缩: 可选 zstd/lz4
// 输出: 完整的 .ttv 文件字节
```

### Step 5: 上传

```rust
storage::put(&tantivy_file_name, puffin_bytes).await?;
```

## 索引内容对比

| 字段类型 | 写入方式 | 查询用途 |
|----------|----------|----------|
| FTS (`_all`) | 所有 fts_fields 文本拼接，O2Tokenizer 分词 | match_all() 全文搜索 |
| Index field | 原值作为 raw token + fast field | 精确匹配 + GROUP BY 聚合 |
| `_timestamp` | i64 fast field | 时间范围裁剪 |

## file_list 中的 index_size

每个文件在 file_list 元数据中记录 `index_size`:
- `index_size > 0`: 有 tantivy 索引
- `index_size == 0`: 无索引

`split_file_list_by_time_range()` 中检查:
```rust
file.meta.min_ts >= index_updated_at && file.meta.index_size > 0
```

两个条件都满足才走 tantivy 路径。
