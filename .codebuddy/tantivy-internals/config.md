# Tantivy 配置项

## 全局开关

| 环境变量 | 默认值 | 描述 |
|----------|--------|------|
| `ZO_ENABLE_INVERTED_INDEX` | `true` | 启用倒排索引生成（写入路径）和优化（查询路径）|

## 索引构建

| 环境变量 | 默认值 | 描述 |
|----------|--------|------|
| `ZO_COMPACT_TANTIVY_BUILDER_THREAD_NUM` | `2` | Compactor 构建索引时每文件的并行 row_group worker 数。<=1 用单线程模式 |
| `ZO_INVERTED_INDEX_MIN_TOKEN_LENGTH` | `2` | O2Tokenizer 最小 token 长度（短于此的被过滤）|
| `ZO_INVERTED_INDEX_MAX_TOKEN_LENGTH` | `64` | O2Tokenizer 最大 token 长度 |
| `ZO_INVERTED_INDEX_OLD_FORMAT` | `false` | 使用旧的文件命名格式（不带 _streamtype 后缀）|

## 查询优化

| 环境变量 | 默认值 | 描述 |
|----------|--------|------|
| `ZO_INVERTED_INDEX_SKIP_THRESHOLD` | `35` | 跳过阈值（%）。如果 tantivy 返回行数 > 文件总行数的此百分比，回退全量扫描 |
| `ZO_INVERTED_INDEX_COUNT_OPTIMIZER_ENABLED` | `true` | 启用 SimpleCount 优化模式 |

## 缓存

| 环境变量 | 默认值 | 描述 |
|----------|--------|------|
| `ZO_INVERTED_INDEX_FOOTER_CACHE_MAX_SIZE` | `0` | Footer 缓存最大内存 (MB)。0 = 自动使用系统内存的 5% |
| `ZO_INVERTED_INDEX_RESULT_CACHE_ENABLED` | `false` | 启用查询结果缓存 |
| `ZO_INVERTED_INDEX_RESULT_CACHE_MAX_ENTRIES` | `10000` | 结果缓存最大条目数 |
| `ZO_INVERTED_INDEX_RESULT_CACHE_MAX_ENTRY_SIZE` | `20480` | 单条缓存最大大小（字节，默认 20KB）|

## Stream 级别配置

通过 API 设置:
```
PUT /api/{org}/streams/{stream}/settings
{
    "index_fields": {"add": ["field1", "field2"], "remove": []},
    "full_text_search_keys": {"add": ["message"], "remove": []}
}
```

| 设置项 | 描述 |
|--------|------|
| `index_fields` | 二级索引字段列表（用于 TopN/Distinct 聚合优化）|
| `full_text_search_keys` | 全文搜索字段列表（内容合并到 `_all` 字段）|
| `index_updated_at` | 设置 index_fields 的时间戳（自动设置，用于文件分流判断）|

## 默认索引字段

即使不手动设置 `index_fields`，以下字段**默认**会建索引：

```rust
// src/config/src/config.rs
pub static SQL_SECONDARY_INDEX_SEARCH_FIELDS: &[&str] = &[
    "operation_name",
    "service_name", 
    "trace_id",
];
```

加上 `ZO_FEATURE_SECONDARY_INDEX_EXTRA_FIELDS` 环境变量中配置的额外字段。

## 关键行为说明

### index_updated_at 的作用

- 设置 `index_fields` 时自动记录当前时间戳
- 查询时用于文件分流: `file.meta.min_ts >= index_updated_at`
- **只有在 index_fields 配置之后创建的文件**才会走 tantivy 聚合优化路径
- 之前的文件走普通 parquet 扫描（即使有 .ttv 索引文件）

### 为什么这样设计？

因为 `index_fields` 配置前生成的 .ttv 文件中不包含这些字段的 fast field，无法做 TermsAggregation。只有 `_all`（FTS）和 `_timestamp` 是始终存在的。
