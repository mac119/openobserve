# O2 Tokenizer

## 路径

`src/config/src/utils/tantivy/tokenizer/`

## 构建

```rust
pub fn o2_tokenizer_build(collect_type: CollectType) -> TextAnalyzer {
    TextAnalyzer::builder(O2Tokenizer::new(collect_type))
        .filter(RemoveShortFilter::limit(min_token_length))  // 默认 2
        .filter(RemoveLongFilter::limit(max_token_length))   // 默认 64
        .filter(LowerCaser)
        .build()
}
```

## CollectType

| 模式 | 用途 | CamelCase 处理 |
|------|------|---------------|
| `Ingest` | 构建索引时 | 输出根 token + 所有子 token（"getUserName" → ["getusername", "get", "user", "name"]）|
| `Search` | 查询时 | 只输出根 token（"getUserName" → ["getusername"]）|

Search 模式输出更少 token 以实现精确匹配。

## 分词规则

### 基本分词
- 按空白字符、标点符号分割
- 所有 token 转小写

### CamelCase 拆分
- 检测大小写边界: `getUserName` → `get` + `User` + `Name`
- Ingest 模式: 输出原始 token + 拆分后的子 token
- Search 模式: 只输出原始 token

### CJK（中日韩字符）
- 非 ASCII 字符逐字符输出（unigram）
- 每个字符独立作为一个 token

### Base64 检测
- 长字符串如果看起来像 Base64，不做拆分
- 避免对编码数据产生无意义的 token

### 特殊处理
- Email 地址: 整体作为一个 token
- IPv4/IPv6: 整体作为一个 token
- 路径分隔符 (`/`, `\`): 作为分词边界

## 注册

```rust
// 构建索引时
index.tokenizers().register(O2_TOKENIZER, o2_tokenizer_build(CollectType::Ingest));

// 查询时
index.tokenizers().register(O2_TOKENIZER, o2_tokenizer_build(CollectType::Search));
```

常量: `pub const O2_TOKENIZER: &str = "o2";`

## raw Tokenizer

index_fields 使用 tantivy 内置的 `raw` tokenizer:
- 整个字段值作为一个 token，不分词
- 用于精确匹配和 terms aggregation
