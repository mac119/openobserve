# 实现文档: Support Two GROUP BY via Tantivy (#12039)

## 功能概述

支持通过 Tantivy 全文搜索引擎执行两字段 GROUP BY 查询，加速如下 SQL：

```sql
SELECT userid, searchphrase, COUNT(*) FROM hits 
GROUP BY userid, searchphrase 
ORDER BY cnt DESC LIMIT 10;
```

## 实现架构

### 数据流

```
SQL → DataFusion PhysicalPlan → LeaderIndexOptimizer 识别模式
  → RemoteScanExec 携带 SimpleTopNMulti 模式
    → Follower 端: tantivy_search() 使用嵌套 TermsAggregation
      → 结果通过 gRPC 传输
        → TantivyOptimizeExec 转换为 Arrow RecordBatch
          → DataFusion 继续处理（排序、合并）
```

### 修改的文件清单

| 文件 | 修改内容 |
|------|----------|
| `src/config/src/meta/inverted_index.rs` | 新增 `SimpleTopNMulti(Vec<String>, usize, bool)` 枚举变体 |
| `src/proto/proto/cluster/plan.proto` | 新增 `SimpleTopNMulti` 消息和 oneof 变体 |
| `src/service/search/datafusion/optimizer/physical_optimizer/index_optimizer/topn_multi.rs` | **新文件**：两字段 GROUP BY 模式识别器 |
| `src/service/search/datafusion/optimizer/physical_optimizer/index_optimizer/mod.rs` | 注册 topn_multi 模块，在 Leader 优化器中调用 |
| `src/service/search/grpc/tantivy.rs` | 新增 `TantivyResult::TopNMulti`、`handle_simple_top_n_multi()`、`TantivyMultiResultBuilder::TopNMulti` |
| `src/service/search/grpc/storage.rs` | 添加 `SimpleTopNMulti` 的 fast_field warming、搜索执行、结果合并 |
| `src/service/search/grpc/flight.rs` | 在文件列表分割逻辑中识别 `SimpleTopNMulti` |
| `src/service/search/grpc/tantivy_result_cache.rs` | 新增 `CacheEntry::TopNMulti` |
| `src/service/search/datafusion/plan/tantivy_optimize_exec.rs` | 新增 `create_top_n_multi_arrow_array()` |

---

## 关键实现细节

### 1. 模式识别（topn_multi.rs）

识别条件：
- `SortPreservingMergeExec` 有 fetch limit
- 主排序不在索引字段上（应该是 count 列）
- `ProjectionExec` 有 3 个表达式（field1, field2, count）
- `AggregateExec` 有 2 个 group_expr，1 个 aggr_expr
- 两个 group_expr 都是 index_fields 中的列
- aggr_expr 是 `count(Int64(1))`

### 2. Tantivy 嵌套聚合

使用 tantivy 的 `TermsAggregation` 嵌套结构：

```rust
// 外层: 按 field[0] 分组
let outer_aggregation = Aggregation {
    agg: AggregationVariants::Terms(TermsAggregation {
        field: fields[0].to_string(),
        ...
    }),
    // 内层: 每个外层 bucket 内按 field[1] 分组
    sub_aggregation: HashMap::from([("inner", Aggregation {
        agg: AggregationVariants::Terms(TermsAggregation {
            field: fields[1].to_string(),
            ...
        }),
        sub_aggregation: HashMap::new(),
    })]),
};
```

结果解析：遍历外层 buckets，对每个外层 bucket 遍历内层 buckets，组合键为 `[outer_key, inner_key]`。

### 3. 多文件结果合并

`TantivyMultiResultBuilder::TopNMulti` 直接 extend 所有文件的结果。DataFusion 后续的 `AggregateExec` 会负责按组合键重新聚合和排序。

### 4. Arrow 转换

`create_top_n_multi_arrow_array` 产生 3 列的 RecordBatch：
- Column 0: field1 values
- Column 1: field2 values
- Column 2: count values

---

## 遇到的问题和解决方案

### 问题 1: Proto 生成代码的命名不一致

**问题**: proto 中定义 `message SimpleTopNMulti`，prost 生成的 struct 名是 `SimpleTopNMulti`（保持原样），但 oneof variant 名是 `SimpleTopnMulti`（n 小写）。在 Rust 代码中引用错误的名称导致编译失败。

**错误信息**: `error[E0422]: cannot find struct, variant or union type 'SimpleTopnMulti' in module 'cluster_rpc'`

**解决方案**: 
- oneof variant（enum variant）: `Mode::SimpleTopnMulti(...)` — 驼峰转换
- struct 引用: `cluster_rpc::SimpleTopNMulti { ... }` — 保持 proto 中的大写

### 问题 2: Tantivy 嵌套聚合 API 使用

**问题**: tantivy 的 `sub_aggregation` 是 `HashMap<String, Aggregation>`，不是 `Vec`。需要正确构造嵌套结构。

**解决方案**: 参考已有的 `handle_simple_multi_histogram` 的嵌套实现模式。

### 问题 3: 非穷举模式匹配

**问题**: 添加新枚举变体后，所有使用 `match` 的地方都需要处理新变体，否则编译报 `non-exhaustive patterns`。

**影响范围**: `storage.rs`（搜索执行 match）和 `tantivy.rs`（builder match）

**解决方案**: 使用 `cargo build` 的错误信息定位所有需要修改的位置，逐一添加处理分支。

### 问题 4: TopNMulti 结果的数据类型选择

**问题**: 单字段 TopN 用 `Vec<(String, u64)>` 表示，多字段需要组合键。

**方案选择**:
- 方案A: `Vec<(String, String, u64)>` — 只支持 2 字段，不可扩展
- 方案B: `Vec<(Vec<String>, u64)>` — 通用，可扩展到 N 字段 ✅

选择了方案B 以保持可扩展性。

---

## 测试验证

```bash
# 单元测试
cargo test --lib test_is_simple_topn_multi  # 模式识别测试
cargo test -p config inverted_index         # 数据结构测试
cargo test --lib topn                       # 所有 topn 相关测试

# 结果: 全部通过
```

## 后续工作

1. 集成测试：需要在有实际 tantivy 索引的环境中验证端到端查询
2. 性能测试：对比开启/关闭优化的查询延迟
3. 考虑扩展到 N 字段 GROUP BY（当前设计已支持，只需放宽模式识别的检查）
