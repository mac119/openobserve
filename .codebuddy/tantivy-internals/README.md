# OpenObserve Tantivy 内部原理文档

本目录记录 OpenObserve 中 Tantivy 倒排索引的完整架构、原理和使用方式。

## 文档结构

- `architecture.md` — 整体架构与数据流
- `index-format.md` — 索引文件格式（Puffin + .ttv）
- `index-build-pipeline.md` — 索引构建管道（Ingester/Compactor）
- `query-flow.md` — 查询执行流程
- `tokenizer.md` — O2 分词器
- `optimize-modes.md` — 索引优化模式
- `config.md` — 配置项说明
