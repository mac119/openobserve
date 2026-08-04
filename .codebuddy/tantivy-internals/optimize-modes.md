# 索引优化模式 (IndexOptimizeMode)

## 定义

**文件**: `src/config/src/meta/inverted_index.rs`

```rust
pub enum IndexOptimizeMode {
    SimpleSelect(usize, bool),                    // (limit, ascend)
    SimpleCount,                                   // count(*)
    SimpleHistogram(i64, u64, usize),             // (min_value, bucket_width, num_buckets)
    SimpleMultiHistogram(i64, i64, u64, String),  // (min_value, max_value, bucket_width, field)
    SimpleTopN(String, usize, bool),              // (field, limit, ascend)
    SimpleTopNMulti(Vec<String>, usize, bool),    // (fields, limit, ascend)
    SimpleDistinct(String, usize, bool),          // (field, limit, ascend)
}
```

## 各模式详解

### SimpleSelect

**触发 SQL**: `SELECT * FROM t WHERE match_all('error') LIMIT 100`

**检测模块**: `select.rs`

**条件**: 有 WHERE + LIMIT，无 GROUP BY

**Tantivy 执行**: `handle_simple_select()` — 用 TopDocs 收集器获取匹配文档的 doc_id

**返回**: `RowIds(HashSet<u32>)` — 只读取这些行的 parquet 数据

---

### SimpleCount

**触发 SQL**: `SELECT COUNT(*) FROM t WHERE match_all('error')`

**检测模块**: `count.rs`

**条件**: 只有 COUNT(*)，有 WHERE，无 GROUP BY

**Tantivy 执行**: `handle_simple_count()` — 用 Count 收集器

**返回**: `Count(usize)` — 直接返回数字，不读 parquet

---

### SimpleHistogram

**触发 SQL**: `SELECT histogram(_timestamp, '1h') as h, COUNT(*) FROM t WHERE ... GROUP BY h`

**检测模块**: `histogram.rs`

**条件**: GROUP BY histogram(_timestamp, interval)，只有 COUNT(*)

**Tantivy 执行**: `handle_simple_histogram()` — 用 HistogramCollector

**返回**: `Histogram(Vec<u64>)` — 每个 bucket 的计数

---

### SimpleMultiHistogram

**触发 SQL**: `SELECT histogram(_timestamp, '1h') as h, field, COUNT(*) FROM t GROUP BY h, field`

**检测模块**: `histogram.rs`

**条件**: histogram + breakdown field

**Tantivy 执行**: `handle_simple_multi_histogram()` — HistogramAggregation + 嵌套 TermsAggregation

**返回**: `MultiHistogram(Vec<(i64, String, u64)>)` — (timestamp, breakdown_value, count)

---

### SimpleTopN

**触发 SQL**: `SELECT field, COUNT(*) as cnt FROM t GROUP BY field ORDER BY cnt DESC LIMIT 10`

**检测模块**: `topn.rs`

**条件**:
- 1 个 group_expr（必须是 index_field）
- 1 个 aggr_expr = count(Int64(1))
- ORDER BY count 列
- 有 LIMIT
- 主排序不能是 index_field

**Tantivy 执行**: `handle_simple_top_n()` — 单层 TermsAggregation

**返回**: `TopN(Vec<(String, u64)>)` — (key, count) 对

**over-fetch**: `limit * 4, max 1000`

---

### SimpleTopNMulti

**触发 SQL**: `SELECT f1, f2, COUNT(*) as cnt FROM t GROUP BY f1, f2 ORDER BY cnt DESC LIMIT 10`

**检测模块**: `topn_multi.rs`

**条件**:
- 2 个 group_expr（都必须是 index_field）
- 1 个 aggr_expr = count(Int64(1))
- ProjectionExec 有 3 列
- ORDER BY count 列
- 有 LIMIT

**Tantivy 执行**: `handle_simple_top_n_multi()` — 嵌套 TermsAggregation（外层 field1, 内层 field2）

**返回**: `TopNMulti(Vec<(Vec<String>, u64)>)` — ([key1, key2], count)

**over-fetch**: `limit * 2, max 100`（优化后避免桶膨胀）

---

### SimpleDistinct

**触发 SQL**: `SELECT DISTINCT field FROM t ORDER BY field LIMIT 100`

**检测模块**: `distinct.rs`

**条件**:
- GROUP BY + Distinct 或 DISTINCT
- 1 个字段（index_field）
- ORDER BY 该字段
- 有 LIMIT

**Tantivy 执行**: `handle_simple_distinct()` — 遍历 term dictionary

**返回**: `Distinct(HashSet<String>)` — 去重值集合

---

## 注册位置

### 集群模式 (Leader 节点)

**文件**: `src/service/search/datafusion/optimizer/physical_optimizer/index_optimizer/mod.rs`

`LeaderIndexOptimizerRule` → `LeaderIndexOptimizer::f_up()`:
- 检测 `SortPreservingMergeExec` 节点
- 依次调用 `is_simple_topn` → `is_simple_topn_multi` → `is_simple_distinct`
- 匹配后通过 `IndexOptimizerRewrite` 设置到 `RemoteScanExec`

### 单节点模式

同文件，`FollowerIndexOptimizer::f_up()`:
- `is_single_node()` 分支中
- 依次调用 `is_simple_topn` → `is_simple_topn_multi` → `is_simple_distinct`
- 匹配后设置到 `self.index_optimizer_mode`

### 其他模式（Select/Count/Histogram）

在 `FollowerIndexOptimizer` 中直接检测（不依赖 `is_single_node()`）:
- `SortPreservingMergeExec` → `is_simple_select`
- `AggregateExec` → `is_simple_count`, `is_simple_histogram`
