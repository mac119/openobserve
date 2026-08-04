# 索引文件格式

## 文件扩展名

- `.ttv` — Tantivy 索引文件（Puffin 容器）
- 存储路径：`files/{org}/index/{stream}_{stream_type}/{year}/{month}/{day}/{hour}/{file_id}.ttv`

## 文件名转换规则

```
Parquet: files/default/logs/mystream/2024/02/16/16/7164299619311026293.parquet
Tantivy: files/default/index/mystream_logs/2024/02/16/16/7164299619311026293.ttv
```

代码: `src/config/src/utils/inverted_index.rs` → `convert_parquet_file_name_to_tantivy_file()`

## Puffin 容器格式

来源: Apache Iceberg Puffin Spec（OpenObserve 自定义实现）

### 文件布局

```
┌────────────────────────────────────────────┐
│ MAGIC (4 bytes): [0x50, 0x46, 0x41, 0x31] │  "PFA1"
├────────────────────────────────────────────┤
│ Blob 1: tantivy segment file (.term)       │
├────────────────────────────────────────────┤
│ Blob 2: tantivy segment file (.idx)        │
├────────────────────────────────────────────┤
│ Blob 3: tantivy segment file (.pos)        │
├────────────────────────────────────────────┤
│ Blob 4: tantivy segment file (.fast)       │
├────────────────────────────────────────────┤
│ Blob 5: meta.json                          │
├────────────────────────────────────────────┤
│ Blob 6: footer_cache (预构建)              │
├────────────────────────────────────────────┤
│ Footer Payload (JSON metadata)             │
├────────────────────────────────────────────┤
│ FLAGS (4 bytes)                            │
├────────────────────────────────────────────┤
│ Footer Payload Size (4 bytes, LE)          │
├────────────────────────────────────────────┤
│ MAGIC (4 bytes): [0x50, 0x46, 0x41, 0x31] │
└────────────────────────────────────────────┘
```

### Blob 类型 (BlobTypes)

| 类型 | 描述 |
|------|------|
| `O2TtvV1` | Tantivy 索引文件（.term, .idx, .pos, .fast, meta.json）|
| `O2TtvFooterV1` | 预构建的 footer cache |
| `O2FstV1` | FST 数据（旧格式，已不再使用）|

### 压缩

支持 Zstd 和 Lz4 压缩（`CompressionCodec` 枚举）

### 保存的 Tantivy 文件

```rust
// src/tantivy_utils/src/puffin_directory/mod.rs
const ALLOWED_FILE_EXT: &[&str] = &["term", "idx", "pos", "fast"];
const EMPTY_FILE_EXT: &[&str] = &["fieldnorm", "store"];
const META_JSON: &str = "meta.json";
const FOOTER_CACHE: &str = "footer_cache";
```

| 文件 | 保存 | 说明 |
|------|------|------|
| `.term` | ✅ | Term dictionary（倒排索引核心）|
| `.idx` | ✅ | Posting list 索引 |
| `.pos` | ✅ | 位置信息 |
| `.fast` | ✅ | Fast fields（用于聚合/排序）|
| `meta.json` | ✅ | Segment 元数据 |
| `.fieldnorm` | ❌ | 字段长度归一化（不需要，查询时用空文件填充）|
| `.store` | ❌ | 文档存储（不需要，原始数据在 parquet 中）|

### 属性 (Properties)

每个 .ttv 文件在 Puffin metadata 中记录：
- `PROP_ROW_GROUP_SIZE`: 对应 parquet 的总行数（用于计算 skip threshold 百分比）

## Tantivy Index Schema

每个 .ttv 文件内部的 tantivy index 包含以下字段：

| 字段名 | 类型 | Tokenizer | Fast Field | 用途 |
|--------|------|-----------|------------|------|
| `_all` | TEXT | `o2` (自定义) | No | 全文搜索（所有 FTS 字段内容合并）|
| `_timestamp` | I64 | — | Yes | 时间范围过滤 |
| 各 index_field | TEXT | `raw` (精确匹配) | Yes | 二级索引 + 聚合 |

### 为什么 index_field 用 TEXT + raw + fast field？

- `raw` tokenizer: 整个值作为一个 token，支持精确匹配 (`term query`)
- `fast field`: 支持 TermsAggregation（GROUP BY 聚合），不需要扫描 posting list
- 两者结合: 既能做精确过滤，又能做聚合
