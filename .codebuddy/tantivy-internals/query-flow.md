# 查询执行流程

## 概述

Tantivy 在查询中有两种使用方式：

1. **Row filtering（行过滤）**：用 tantivy 查询得到匹配的 row_id 集合，然后只读取这些行的 parquet 数据
2. **Aggregation optimization（聚合优化）**：完全跳过 parquet，直接用 tantivy 的聚合能力返回结果（TopN、Count、Histogram 等）

## 文件分流

**文件**: `src/service/search/grpc/flight.rs` → `handle_tantivy_optimize()`

```rust
async fn handle_tantivy_optimize(
    idx_optimize_rule: &mut Option<IndexOptimizeMode>,
    file_list: Vec<FileKey>,
    index_updated_at: i64,
    time_range: (i64, i64),
) -> Result<(Vec<FileKey>, Vec<FileKey>), Error>
```

### 分流条件

```rust
fn split_file_list_by_time_range(file_list, index_updated_at, time_range) {
    file_list.into_iter().partition(|file| {
        file.meta.min_ts >= index_updated_at  // 文件在 index_fields 配置之后
        && file.meta.index_size > 0           // 有索引文件
        && time_range 约束                     // 可选时间范围限制
    })
}
```

返回: `(tantivy_files, datafusion_files)`

### index_updated_at 的确定

```rust
async fn update_index_updated_at(idx_optimize_rule, index_updated_at) -> i64 {
    let ttv_timestamp_updated_at = db::metas::tantivy_index::get_ttv_timestamp_updated_at().await;
    let index_updated_at = index_updated_at.max(ttv_timestamp_updated_at);
    
    // TopN/TopNMulti/MultiHistogram 还要考虑 secondary index 更新时间
    if matches!(rule, SimpleTopN | SimpleTopNMulti | SimpleMultiHistogram) {
        let ttv_secondary = get_ttv_secondary_index_updated_at().await;
        return index_updated_at.max(ttv_secondary);
    }
    index_updated_at
}
```

## 主查询入口

**文件**: `src/service/search/grpc/storage.rs` → `tantivy_search()`

```rust
pub async fn tantivy_search(
    query: Arc<QueryParams>,
    file_list: &mut Vec<FileKey>,
    index_condition: Option<IndexCondition>,
    idx_optimize_mode: Option<IndexOptimizeMode>,
) -> Result<(usize, bool, TantivyMultiResult), Error>
```

## 单文件搜索

**`search_tantivy_index()`** 流程:

```
1. 文件名转换: .parquet → .ttv
2. 检查 tantivy_result_cache
3. 获取 Tantivy Directory:
   ├── PuffinDirReader::from_path() — 读取 puffin metadata
   ├── FooterCache — 从 puffin 中提取预构建的 footer
   └── CachingDirectory — 包装缓存层
4. 打开 tantivy::Index
5. 注册 O2Tokenizer (Search mode)
6. 创建 reader (ReloadPolicy::Manual, num_warming_threads: 0)
7. warm_up_terms() — 预加载 term dictionary
8. 构建查询:
   ├── IndexCondition.to_tantivy_query() — 用户条件
   └── + RangeQuery(_timestamp, min..max) — 时间范围
9. 根据 optimize_mode 执行不同的搜索策略
10. 缓存结果到 tantivy_result_cache
```

## 执行策略（按 optimize_mode）

| Mode | 执行方法 | 返回类型 |
|------|----------|----------|
| None (普通查询) | `handle_matched_docs()` / `handle_simple_select()` | `RowIds` / `RowIdsBitVec` |
| `SimpleCount` | `handle_simple_count()` | `Count(usize)` |
| `SimpleHistogram` | `handle_simple_histogram()` | `Histogram(Vec<u64>)` |
| `SimpleMultiHistogram` | `handle_simple_multi_histogram()` | `MultiHistogram(Vec<(i64, String, u64)>)` |
| `SimpleTopN` | `handle_simple_top_n()` | `TopN(Vec<(String, u64)>)` |
| `SimpleTopNMulti` | `handle_simple_top_n_multi()` | `TopNMulti(Vec<(Vec<String>, u64)>)` |
| `SimpleDistinct` | `handle_simple_distinct()` | `Distinct(HashSet<String>)` |

## 多文件结果合并

**`TantivyMultiResultBuilder`** 收集所有文件的结果:

- `RowNums`: 累加计数
- `Histogram`: 按 bucket 逐位相加
- `MultiHistogram`: flatten 所有结果
- `TopN` / `TopNMulti`: extend（合并后由 DataFusion 再聚合）
- `Distinct`: HashSet union

## Arrow 转换

**文件**: `src/service/search/datafusion/plan/tantivy_optimize_exec.rs`

`TantivyOptimizeExec` 执行计划节点:
1. 调用 `tantivy_search()` 获取 `TantivyMultiResult`
2. 根据 optimize_mode 转换为 Arrow RecordBatch:
   - `SimpleCount` → 1 列 (count)
   - `SimpleHistogram` → 2 列 (_timestamp, count)
   - `SimpleTopN` → 2 列 (field, count)
   - `SimpleTopNMulti` → 3 列 (field1, field2, count)
   - `SimpleDistinct` → 1 列 (field)
3. 返回 `MemoryStream` 供 DataFusion 后续处理

## Skip Threshold 机制

```rust
// 如果 tantivy 返回的行数 > 35% 的文件总行数，认为索引效果差
let row_ids_percent = row_ids.len() / parquet_file.meta.records * 100;
if row_ids_percent > ZO_INVERTED_INDEX_SKIP_THRESHOLD {
    // 回退到 DataFusion 全量扫描（不用 row_id 过滤）
}
```

这避免了"几乎所有行都匹配"时用 row_id 过滤反而更慢的情况。

## 缓存体系

### 1. Footer Cache（全局 LRU）
- 缓存 .ttv 文件的 tantivy footer 元数据
- 大小: `ZO_INVERTED_INDEX_FOOTER_CACHE_MAX_SIZE`（默认 5% 系统内存）
- 减少重复解析 tantivy segment metadata 的开销

### 2. Tantivy Result Cache
- 缓存查询结果（按 file + condition hash）
- `ZO_INVERTED_INDEX_RESULT_CACHE_ENABLED` = false（默认关闭）
- `ZO_INVERTED_INDEX_RESULT_CACHE_MAX_ENTRIES` = 10000
- `CacheEntry` 类型: Count / Histogram / TopN / TopNMulti / Distinct / RowIds

### 3. Disk/Memory File Cache
- .ttv 文件本身可以被 OpenObserve 的通用文件缓存系统缓存
- 避免重复从对象存储下载
