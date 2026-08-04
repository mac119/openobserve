# Tantivy 整体架构

## 依赖版本

```toml
tantivy = { version = "0.26", features = ["quickwit"] }
tantivy-fst = "0.5"
```

- **官方 crate**（非 fork），来自 crates.io
- 启用 `quickwit` feature（支持 TermsAggregation 等聚合功能）

## 模块分布

```
src/
├── tantivy_utils/                    # 独立 workspace crate
│   └── src/
│       ├── puffin/                   # Puffin 文件格式读写器
│       │   ├── mod.rs               # PuffinBytesReader/Writer, BlobMetadata
│       │   └── ...
│       └── puffin_directory/         # Tantivy Directory trait 实现
│           ├── mod.rs               # PuffinDirWriter（索引写入）
│           ├── reader.rs            # PuffinDirReader（索引读取）
│           ├── caching_directory.rs # CachingDirectory（缓存包装）
│           └── footer_cache.rs      # FooterCache（预构建 footer）
│
├── service/tantivy/                  # 索引构建服务
│   ├── mod.rs                       # create_tantivy_index(), build_tantivy_schema()
│   ├── sequential.rs               # 单线程构建
│   ├── parallel.rs                 # 多线程构建
│   ├── bloom_builder.rs            # Bloom filter 从 term dict 构建
│   └── reader/                     # 数据源读取（parquet/vortex）
│
├── service/search/grpc/
│   ├── tantivy.rs                  # TantivyResult 枚举 + 查询执行方法
│   ├── storage.rs                  # tantivy_search() 主入口
│   ├── tantivy_result_cache.rs     # 查询结果缓存
│   └── flight.rs                   # handle_tantivy_optimize() 文件分流
│
├── service/search/datafusion/
│   ├── plan/tantivy_optimize_exec.rs    # TantivyOptimizeExec（DataFusion 计划节点）
│   └── optimizer/physical_optimizer/
│       └── index_optimizer/             # 查询模式识别器
│           ├── mod.rs                   # LeaderIndexOptimizerRule + 单节点路径
│           ├── topn.rs                  # SimpleTopN 识别
│           ├── topn_multi.rs            # SimpleTopNMulti 识别
│           ├── distinct.rs             # SimpleDistinct 识别
│           ├── count.rs                # SimpleCount 识别
│           ├── histogram.rs            # SimpleHistogram 识别
│           └── select.rs               # SimpleSelect 识别
│
├── config/src/
│   ├── meta/inverted_index.rs      # IndexOptimizeMode 枚举
│   └── utils/tantivy/
│       └── tokenizer/              # O2Tokenizer 自定义分词器
│           ├── mod.rs
│           ├── o2_tokenizer.rs
│           └── remove_short.rs
│
├── job/files/parquet.rs            # Ingester: 写 parquet 后调用 create_tantivy_index()
└── service/compact/merge.rs        # Compactor: 合并后调用 create_tantivy_index()
```

## 核心数据流

```
┌─────────────────────────────────────────────────────────────────────┐
│                         WRITE PATH                                   │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  Ingest API → WAL (.wal) → Persist to Parquet                       │
│                                    │                                 │
│                                    ▼                                 │
│                          create_tantivy_index()                      │
│                                    │                                 │
│                     ┌──────────────┼──────────────┐                  │
│                     ▼              ▼              ▼                  │
│              build_schema()   sequential/   PuffinDirWriter          │
│              (_all + fields)  parallel      .to_puffin_bytes()       │
│                                build()                               │
│                                    │                                 │
│                                    ▼                                 │
│                        Upload .ttv to Object Storage                  │
│                                                                      │
│  Compactor: merge parquet files → create_tantivy_index() → .ttv     │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│                         READ PATH                                    │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  Query API → DataFusion Physical Plan                                │
│       │                                                              │
│       ▼                                                              │
│  LeaderIndexOptimizerRule / SingleNode Optimizer                      │
│       │ (模式识别: TopN? Count? Histogram? Distinct?)                │
│       ▼                                                              │
│  handle_tantivy_optimize()                                           │
│       │ (split_file_list_by_time_range: file.min_ts >= idx_updated)  │
│       │                                                              │
│       ├── tantivy files ──► tantivy_search()                         │
│       │                         │                                    │
│       │                    PuffinDirReader                            │
│       │                    + FooterCache                              │
│       │                    + CachingDirectory                         │
│       │                         │                                    │
│       │                    tantivy::Index::open()                     │
│       │                    O2Tokenizer(Search)                        │
│       │                    Searcher.search()                          │
│       │                         │                                    │
│       │                    TantivyResult                              │
│       │                    (RowIds / Count / TopN / ...)              │
│       │                         │                                    │
│       │                         ▼                                    │
│       │                    TantivyOptimizeExec                        │
│       │                    → Arrow RecordBatch                        │
│       │                                                              │
│       └── datafusion files ► 标准 parquet 扫描                       │
│                                                                      │
│  DataFusion 合并两部分结果 → 返回用户                                │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

## 关键设计决策

1. **Puffin 容器格式**：将多个 tantivy segment 文件打包成一个 .ttv 文件，减少对象存储小文件数量
2. **只保存必要文件**：跳过 .fieldnorm 和 .store（OpenObserve 不需要文档存储和字段长度归一化）
3. **Footer Cache**：预计算 tantivy 文件尾部元数据，避免每次查询重新解析
4. **分流策略**：`split_file_list_by_time_range()` 按 `index_updated_at` 分流，只对有索引的文件走 tantivy
5. **Skip Threshold**：如果 tantivy 返回的 row_id 超过数据的 35%，认为索引过滤效果差，回退到全量扫描
