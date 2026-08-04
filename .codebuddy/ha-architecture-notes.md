# OpenObserve HA 集群架构笔记

## 架构模式：Active-Active（多活）

所有同角色节点同时工作，不是主备模式：

| 角色 | 模式 | 说明 |
|------|------|------|
| **Ingester** | 多活 | 所有 Ingester 同时接收写入，各自独立 WAL + MemTable |
| **Querier** | 多活 | 查询并行分发到所有 Querier，每个处理一部分文件 |
| **Router** | 多活 | 多个 Router 可同时服务请求（前面挂 LB） |
| **Compactor** | 分区 | 通过一致性哈希分配任务，每个负责不同 stream |

**没有传统 leader election**，只有分布式锁用于临界操作（节点注册、compaction 防重复）。

---

## 数据流与一致性模型

```
                    Router (负载均衡)
                   /        \
            Ingester-A     Ingester-B
            (本地 WAL-A)   (本地 WAL-B)
                 \           /
                  \         /
                   ↓       ↓
              ┌──────────────┐
              │     S3       │  ← 唯一的持久化存储（真相源）
              │  (所有数据)   │
              └──────────────┘
                     ↑
              Querier 查询时从这里读
```

### 关键设计

| 问题 | 答案 |
|------|------|
| 两个 Ingester 有相同数据？ | ❌ 没有。每个 Ingester 只持有自己接收到的那部分数据的 WAL |
| 数据副本在哪？ | 唯一的完整副本在 **S3**（对象存储） |
| WAL 是什么？ | 临时本地缓冲，数据合并为 Parquet 后上传 S3，然后删除本地 WAL |
| 一致性保证 | S3 是 single source of truth；file_list（PostgreSQL）记录所有文件元数据 |

### 与 Elasticsearch 对比

| | Elasticsearch | OpenObserve |
|---|---|---|
| 数据副本 | Primary + Replica shard | **无副本**（依赖 S3 的 11 个 9 持久性） |
| WAL 复制 | Translog 同步到 replica | ❌ 不复制 WAL |
| 故障恢复 | Replica 提升为 Primary | 重启后回放本地 WAL |
| 真相源 | 每个 shard 各自独立 | **S3 是唯一真相** |

---

## Ingester 故障场景

### 场景 1：WAL 已上传 S3 → 无数据丢失

```
数据写入 → WAL → Parquet → 上传 S3 ✅ → 删除本地 WAL
此时挂了 → 没问题，数据已安全在 S3
```

### 场景 2：WAL 还没上传 S3

```
情况 a: 磁盘还在（进程崩溃/重启）
  → 重启后自动回放 WAL → 上传到 S3 ✅ 不丢数据

情况 b: 磁盘丢了（硬件故障/容器无持久卷）
  → WAL 中未上传的数据永久丢失 ❌
```

### 场景 3：正在写入的请求 → Router 自动 failover

```
Router → 发给 Ingester-A → 连接失败
      → 自动重试发给 Ingester-B ✅
```

### 风险窗口

从数据写入到上传 S3 的时间窗口（默认 ~10s）是唯一的数据丢失风险期。

---

## 一致性保证机制

### 写入一致性

```
Ingester 写入文件 → 上传 S3 → 注册到 file_list (PostgreSQL)
                                       ↓
                             gRPC broadcast 通知所有 Querier
                                       ↓
                             Querier 更新本地 file_list 缓存
                                       ↓
                             下次查询可见 ✅
```

延迟窗口：`ZO_FILE_PUSH_INTERVAL`（10s）+ S3 上传 + gRPC 广播 ≈ **10-30 秒**

### 查询一致性

Querier 查询三个数据源：
1. **远程 Ingester 的 MemTable/WAL**（gRPC 实时查询尚未上传的数据）
2. **S3 上的 Parquet 文件**（已持久化的历史数据）
3. **本地磁盘缓存**（S3 文件的副本）

即使数据还在 Ingester WAL 中没上传 S3，Querier 也能查到（通过 gRPC 直接问 Ingester）。

---

## NATS 的四重角色

```
┌─────────────────────────────────────────────────────┐
│                      NATS                            │
│                                                     │
│  ① 集群协调器 (Coordinator)                          │
│     节点注册/发现/心跳/健康检查                        │
│     KV Bucket: /nodes/{uuid}                        │
│     Watch: 实时感知节点上下线                          │
│                                                     │
│  ② 分布式 KV 存储 (Metadata)                         │
│     Schema 变更、配置、Stream 信息                    │
│     按 key 前缀自动分桶                              │
│                                                     │
│  ③ 事件广播 (Event Bus)                              │
│     JetStream: coordinator_events                   │
│     元数据变更广播给所有节点（最终一致性）               │
│                                                     │
│  ④ 分布式锁 (Distributed Lock)                      │
│     基于 KV create-if-not-exists                    │
│     TTL 过期 + keep-alive 续约                      │
│     用于节点注册互斥、compaction 防重复               │
└─────────────────────────────────────────────────────┘
```

### 通过 NATS 传递的事件

| 事件类型 | 传输方式 | 说明 |
|---------|---------|------|
| 节点注册/下线 | NATS KV Watch | 实时感知集群拓扑变化 |
| Schema/配置变更 | NATS JetStream `coordinator_events` | 广播给所有节点 |
| 分布式锁 | NATS KV create | 节点注册互斥、compaction 防重复 |
| File list 同步 | **gRPC**（非 NATS） | 点对点广播给 Querier |
| 查询分发 | **gRPC Flight**（非 NATS） | leader Querier 分发给 worker Querier |

---

## Router 负载均衡策略

### Ingester 路由（写入）

策略由 `ZO_ROUTE_STRATEGY` 控制：
- **Workload**（默认）: `score = connections/2000 × 0.5 + cpu × 0.3 + mem × 0.2`，选分数最低的
- **Random**: 随机选择

### Querier 路由（查询）

Leader Querier 接收请求后，按文件分区策略分发给所有 Querier：
- **FileNum**: 按文件数量均匀分配
- **FileSize**: 按文件大小均匀分配
- **FileHash**: 一致性哈希（利于缓存命中）

---

## 减少数据丢失风险的措施

| 措施 | 配置 |
|------|------|
| 缩短 WAL 保留时间 | `ZO_FILE_PUSH_INTERVAL=5`（5 秒扫描） |
| 持久化 WAL 目录 | Ingester 挂载持久卷（不用 emptyDir） |
| NATS 多副本 | `ZO_NATS_REPLICAS=3` |
| PostgreSQL 高可用 | 主从 + 自动 failover |
| S3 跨区域复制 | 启用 S3 Cross-Region Replication |

---

## 对象存储兼容性

OpenObserve 基于 Rust `object_store` crate，支持所有 S3 兼容协议的对象存储：

| 存储 | `ZO_S3_PROVIDER` | 说明 |
|------|-----------------|------|
| AWS S3 | `s3` 或 `aws` | 原生支持 |
| MinIO | `minio` 或留空 | S3 兼容 |
| 腾讯云 COS | `s3` 或留空 | S3 兼容 ✅ |
| 阿里云 OSS | `s3` 或留空 | S3 兼容 ✅ |
| Google GCS | `gcs` | 原生支持 |
| Azure Blob | `azure` | 原生支持 |
| Ceph RGW | `s3` 或留空 | S3 兼容 ✅ |
| 华为云 OBS | `s3` 或留空 | S3 兼容 ✅ |

### 腾讯云 COS 配置示例

```bash
ZO_S3_PROVIDER=s3
ZO_S3_SERVER_URL=https://cos.ap-guangzhou.myqcloud.com
ZO_S3_ACCESS_KEY=你的SecretId
ZO_S3_SECRET_KEY=你的SecretKey
ZO_S3_BUCKET_NAME=your-bucket-1234567890
ZO_S3_REGION_NAME=ap-guangzhou
```

---

## 一句话总结

> OpenObserve HA 不通过数据复制实现，而是依赖 **S3 作为唯一持久层 + WAL 磁盘恢复 + Querier 实时查询 Ingester WAL**。WAL 未上传的窗口期（~10s）是唯一风险，用持久卷可消除。NATS 是控制面的"大脑"（协调+元数据+事件+锁），数据面走 gRPC。
