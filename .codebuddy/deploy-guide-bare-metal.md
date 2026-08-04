# OpenObserve 裸金属部署文档（非 Docker）

## 前置要求

- Linux x86_64（Ubuntu 20.04+, CentOS 7+, Debian 10+）
- 至少 4 CPU, 8 GB RAM（推荐 16+ CPU, 32+ GB RAM）
- 磁盘空间：取决于数据量，建议 100GB+ SSD

## 一、获取二进制

### 方式 A：下载 Release 版本（推荐）

```bash
# 从 GitHub Releases 下载
LATEST_VERSION=$(curl -s https://api.github.com/repos/openobserve/openobserve/releases/latest | grep tag_name | cut -d '"' -f 4)
curl -L -o openobserve "https://github.com/openobserve/openobserve/releases/download/${LATEST_VERSION}/openobserve-${LATEST_VERSION}-linux-amd64"
chmod +x openobserve
sudo mv openobserve /usr/local/bin/
```

### 方式 B：源码编译

```bash
# 依赖
sudo apt install -y build-essential pkg-config libssl-dev protobuf-compiler cmake

# Rust 工具链
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# 编译
cd /path/to/openobserve
cargo build --release --features mimalloc

# 安装
sudo cp target/release/openobserve /usr/local/bin/
```

## 二、创建用户和目录

```bash
# 创建系统用户
sudo useradd -r -s /sbin/nologin openobserve

# 创建数据目录
sudo mkdir -p /var/lib/openobserve
sudo chown openobserve:openobserve /var/lib/openobserve

# 创建配置目录
sudo mkdir -p /etc/openobserve
sudo chown openobserve:openobserve /etc/openobserve

# 创建日志目录（可选）
sudo mkdir -p /var/log/openobserve
sudo chown openobserve:openobserve /var/log/openobserve
```

## 三、配置文件

创建 `/etc/openobserve/.env`：

```bash
# ============================================
# OpenObserve 配置 - 单节点裸金属部署
# ============================================

# --- 必填：管理员凭据 ---
ZO_ROOT_USER_EMAIL=admin@your-domain.com
ZO_ROOT_USER_PASSWORD=YourStrongPassword123!

# --- 运行模式 ---
ZO_LOCAL_MODE=true
ZO_LOCAL_MODE_STORAGE=disk

# --- 数据存储 ---
ZO_DATA_DIR=/var/lib/openobserve/

# --- 网络端口 ---
ZO_HTTP_PORT=5080
ZO_GRPC_PORT=5081

# --- 日志 ---
RUST_LOG=info
ZO_LOG_FILE_DIR=/var/log/openobserve
# ZO_LOG_JSON_FORMAT=true    # 可选：JSON 格式日志

# --- 性能调优 ---
ZO_FILE_PUSH_INTERVAL=10
ZO_MAX_FILE_SIZE_ON_DISK=32
ZO_WAL_FSYNC_DISABLED=true

# --- 倒排索引 ---
ZO_ENABLE_INVERTED_INDEX=true

# --- 磁盘缓存 ---
ZO_DISK_CACHE_ENABLED=true
# ZO_DISK_CACHE_MAX_SIZE=102400    # 100GB, 0=自动

# --- 遥测（可选关闭） ---
ZO_TELEMETRY=false

# --- TLS（可选） ---
# ZO_HTTP_TLS_ENABLED=true
# ZO_HTTP_TLS_CERT_PATH=/etc/openobserve/tls/cert.pem
# ZO_HTTP_TLS_KEY_PATH=/etc/openobserve/tls/key.pem
```

## 四、Systemd 服务

创建 `/etc/systemd/system/openobserve.service`：

```ini
[Unit]
Description=OpenObserve Observability Platform
Documentation=https://openobserve.ai/docs
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=openobserve
Group=openobserve
EnvironmentFile=/etc/openobserve/.env
ExecStart=/usr/local/bin/openobserve
Restart=always
RestartSec=5

# 资源限制
LimitNOFILE=65535
LimitMEMLOCK=infinity

# 安全加固
ProtectSystem=full
ProtectHome=true
NoNewPrivileges=true
PrivateTmp=true

# OOM 保护
OOMScoreAdjust=-500

[Install]
WantedBy=multi-user.target
```

启动服务：

```bash
sudo systemctl daemon-reload
sudo systemctl enable openobserve
sudo systemctl start openobserve
sudo systemctl status openobserve
```

## 五、验证部署

```bash
# 检查服务状态
sudo systemctl status openobserve

# 健康检查
curl http://localhost:5080/healthz

# 登录 Web UI
# 浏览器访问 http://<your-ip>:5080
# 用户名: admin@your-domain.com
# 密码: YourStrongPassword123!

# 查看日志
sudo journalctl -u openobserve -f
# 或
tail -f /var/log/openobserve/o2.*.log
```

## 六、Nginx 反向代理（可选）

如果需要通过 80/443 端口访问：

```nginx
server {
    listen 80;
    server_name openobserve.your-domain.com;
    return 301 https://$server_name$request_uri;
}

server {
    listen 443 ssl http2;
    server_name openobserve.your-domain.com;

    ssl_certificate /etc/letsencrypt/live/openobserve.your-domain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/openobserve.your-domain.com/privkey.pem;

    client_max_body_size 256m;

    location / {
        proxy_pass http://127.0.0.1:5080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # WebSocket 支持
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";

        # 超时
        proxy_connect_timeout 60s;
        proxy_send_timeout 300s;
        proxy_read_timeout 300s;
    }
}
```

## 七、数据摄入配置

### OpenTelemetry Collector

```yaml
exporters:
  otlphttp:
    endpoint: http://localhost:5080/api/default
    headers:
      Authorization: "Basic <base64(email:password)>"

service:
  pipelines:
    logs:
      exporters: [otlphttp]
    metrics:
      exporters: [otlphttp]
    traces:
      exporters: [otlphttp]
```

### Fluentd / Fluent Bit

```ini
[OUTPUT]
    Name        http
    Match       *
    Host        localhost
    Port        5080
    URI         /api/default/your_stream/_json
    Format      json
    Header      Authorization Basic <base64(email:password)>
```

### curl 测试摄入

```bash
curl -u admin@your-domain.com:YourStrongPassword123! \
  -X POST http://localhost:5080/api/default/test_stream/_json \
  -H "Content-Type: application/json" \
  -d '[{"level":"info","message":"hello openobserve","timestamp":"2024-01-01T00:00:00Z"}]'
```

## 八、生产环境调优

### 8.1 系统级优化

```bash
# /etc/sysctl.conf
net.core.somaxconn = 65535
net.ipv4.tcp_max_syn_backlog = 65535
vm.max_map_count = 262144
fs.file-max = 2097152

# /etc/security/limits.conf
openobserve soft nofile 65535
openobserve hard nofile 65535
openobserve soft memlock unlimited
openobserve hard memlock unlimited
```

### 8.2 OpenObserve 性能参数

```bash
# 大数据量场景
ZO_MAX_FILE_SIZE_ON_DISK=64          # MB，单个 WAL 文件大小
ZO_FILE_PUSH_INTERVAL=30             # 秒，WAL 推送间隔
ZO_COMPACT_TANTIVY_BUILDER_THREAD_NUM=4  # 索引构建线程数

# 高并发摄入
ZO_WAL_WRITE_QUEUE_ENABLED=true
ZO_WAL_WRITE_QUEUE_SIZE=50000
ZO_WAL_WRITE_BUFFER_SIZE=65536       # 64KB

# 查询优化
ZO_MEMORY_CACHE_ENABLED=true
ZO_MEMORY_CACHE_MAX_SIZE=8192        # 8GB
ZO_DISK_CACHE_ENABLED=true
ZO_DISK_CACHE_MAX_SIZE=204800        # 200GB

# 内存保护
ZO_MEMORY_CIRCUIT_BREAKER_ENABLED=true
ZO_MEMORY_CIRCUIT_BREAKER_RATIO=85
```

### 8.3 存储规划

| 数据量/天 | 推荐磁盘 | 推荐内存 | 推荐 CPU |
|-----------|----------|----------|----------|
| < 10 GB | 500 GB SSD | 8 GB | 4 cores |
| 10-100 GB | 2 TB SSD | 32 GB | 16 cores |
| 100 GB-1 TB | 10 TB SSD/NVMe | 64 GB | 32 cores |
| > 1 TB | 对象存储(S3) + SSD 缓存 | 128 GB | 64 cores |

## 九、备份与恢复

### 单节点本地模式

```bash
# 停止服务
sudo systemctl stop openobserve

# 备份数据目录
sudo tar czf /backup/openobserve-$(date +%Y%m%d).tar.gz /var/lib/openobserve/

# 启动服务
sudo systemctl start openobserve
```

### 恢复

```bash
sudo systemctl stop openobserve
sudo rm -rf /var/lib/openobserve/*
sudo tar xzf /backup/openobserve-YYYYMMDD.tar.gz -C /
sudo chown -R openobserve:openobserve /var/lib/openobserve/
sudo systemctl start openobserve
```

## 十、升级

```bash
# 1. 下载新版本
curl -L -o /tmp/openobserve-new "https://github.com/openobserve/openobserve/releases/download/vX.Y.Z/openobserve-vX.Y.Z-linux-amd64"
chmod +x /tmp/openobserve-new

# 2. 停止服务
sudo systemctl stop openobserve

# 3. 备份旧版本
sudo cp /usr/local/bin/openobserve /usr/local/bin/openobserve.bak

# 4. 替换二进制
sudo mv /tmp/openobserve-new /usr/local/bin/openobserve

# 5. 启动服务
sudo systemctl start openobserve

# 6. 验证
curl http://localhost:5080/healthz
sudo journalctl -u openobserve --no-pager -n 20
```

## 十一、常见问题

### 端口被占用
```bash
sudo ss -tlnp | grep 5080
# 修改 ZO_HTTP_PORT 为其他端口
```

### 磁盘空间不足
```bash
# 检查数据目录大小
du -sh /var/lib/openobserve/*
# 设置数据保留策略（通过 API 或 Web UI 的 Stream Settings）
```

### OOM (Out of Memory)
```bash
# 启用内存熔断器
ZO_MEMORY_CIRCUIT_BREAKER_ENABLED=true
ZO_MEMORY_CIRCUIT_BREAKER_RATIO=80

# 减少 DataFusion 内存池
ZO_MEMORY_CACHE_DATAFUSION_MAX_SIZE=4096  # 4GB
```

### Permission denied
```bash
sudo chown -R openobserve:openobserve /var/lib/openobserve/
sudo chown -R openobserve:openobserve /var/log/openobserve/
```

---

# 多节点 HA 集群部署（裸金属）

## 架构概述

OpenObserve 集群模式通过 `ZO_LOCAL_MODE=false` 启用，将单体拆分为多个角色节点：

```
                        ┌──────────────────────────┐
                        │     Load Balancer         │
                        │   (Nginx / HAProxy)       │
                        └────────────┬─────────────┘
                                     │
                    ┌────────────────┼────────────────┐
                    │                │                │
             ┌──────▼──────┐ ┌──────▼──────┐ ┌──────▼──────┐
             │   Router 1  │ │   Router 2  │ │   Router N  │
             │  (HTTP入口)  │ │  (HTTP入口)  │ │  (HTTP入口)  │
             └──────┬──────┘ └──────┬──────┘ └──────┬──────┘
                    │                │                │
        ┌───────────┼────────────────┼────────────────┼───────────┐
        │           │                │                │           │
 ┌──────▼──────┐  ┌─▼────────┐  ┌───▼─────┐  ┌──────▼──────┐   │
 │  Ingester   │  │ Querier  │  │Compactor│  │AlertManager │   │
 │ (数据摄入)   │  │(查询处理) │  │(合并索引)│  │  (告警)     │   │
 └──────┬──────┘  └────┬─────┘  └───┬─────┘  └─────────────┘   │
        │               │            │                           │
        └───────────────┼────────────┼───────────────────────────┘
                        │            │
           ┌────────────┼────────────┼────────────┐
           │            │            │            │
    ┌──────▼──────┐  ┌──▼──────┐  ┌─▼────────┐
    │    NATS     │  │PostgreSQL│  │  MinIO/S3 │
    │ (协调/队列)  │  │ (元数据)  │  │ (对象存储) │
    └─────────────┘  └─────────┘  └───────────┘
```

### 节点角色说明

| 角色 | 功能 | 可横向扩展 |
|------|------|:---:|
| **router** | HTTP 请求入口，路由分发到 ingester/querier | ✅ |
| **ingester** | 接收数据写入，WAL → S3 | ✅ |
| **querier** | 处理搜索查询，从 S3/缓存读取 | ✅ |
| **compactor** | 后台合并文件、构建倒排索引 | ✅ |
| **alertmanager** | 告警规则评估和通知 | ✅ |
| **flatten_compactor** | 扁平化压缩（可选） | ✅ |

> 小型集群可以合并角色，例如 `ZO_NODE_ROLE="ingester,querier"` 让一个节点同时负责摄入和查询。

---

## 前置条件

### 外部依赖（必须）

| 组件 | 用途 | 最低版本 |
|------|------|----------|
| **NATS** (JetStream) | 集群协调、节点发现、事件广播 | 2.9+ |
| **PostgreSQL** | 元数据存储（集群模式强制） | 15+ |
| **S3 兼容存储** | 数据持久化（AWS S3 / MinIO / GCS / Ceph RGW） | - |

### 最小集群规模

| 部署场景 | 推荐配置 |
|---------|----------|
| **最小 HA** | 1 Router + 1 Ingester + 1 Querier + 1 Compactor |
| **生产推荐** | 2 Router + 2 Ingester + 2 Querier + 1 Compactor + 1 AlertManager |
| **大规模** | 2 Router + 4+ Ingester + 4+ Querier + 2 Compactor |

---

## 第一步：部署外部依赖

### 1.1 NATS (JetStream)

```bash
# 安装 NATS Server
curl -L https://github.com/nats-io/nats-server/releases/latest/download/nats-server-v2.10.24-linux-amd64.tar.gz | tar xz
sudo mv nats-server-v2.10.24-linux-amd64/nats-server /usr/local/bin/

# 创建配置 /etc/nats/nats.conf
sudo mkdir -p /etc/nats /var/lib/nats
cat <<'EOF' | sudo tee /etc/nats/nats.conf
listen: 0.0.0.0:4222
server_name: nats-1

jetstream {
    store_dir: /var/lib/nats/jetstream
    max_mem: 1G
    max_file: 10G
}

# 可选：集群模式（多 NATS 节点）
# cluster {
#     name: o2-nats
#     listen: 0.0.0.0:6222
#     routes: [
#         nats://nats-2:6222,
#         nats://nats-3:6222,
#     ]
# }
EOF

# Systemd 服务
cat <<'EOF' | sudo tee /etc/systemd/system/nats.service
[Unit]
Description=NATS Server
After=network-online.target

[Service]
ExecStart=/usr/local/bin/nats-server -c /etc/nats/nats.conf
Restart=always
RestartSec=5
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now nats
```

### 1.2 PostgreSQL

```bash
# Ubuntu
sudo apt install -y postgresql-15

# 创建数据库和用户
sudo -u postgres psql <<'EOF'
CREATE USER openobserve WITH PASSWORD 'YourPGPassword!';
CREATE DATABASE openobserve OWNER openobserve;
GRANT ALL PRIVILEGES ON DATABASE openobserve TO openobserve;
EOF

# 允许远程连接（/etc/postgresql/15/main/pg_hba.conf）
# host  openobserve  openobserve  10.0.0.0/8  md5

sudo systemctl restart postgresql
```

### 1.3 MinIO（如果不使用 AWS S3）

```bash
# 下载
curl -L https://dl.min.io/server/minio/release/linux-amd64/minio -o /usr/local/bin/minio
chmod +x /usr/local/bin/minio

# 配置
sudo mkdir -p /var/lib/minio
cat <<'EOF' | sudo tee /etc/default/minio
MINIO_ROOT_USER=minioadmin
MINIO_ROOT_PASSWORD=YourMinioPassword!
MINIO_VOLUMES="/var/lib/minio"
EOF

# Systemd 服务
cat <<'EOF' | sudo tee /etc/systemd/system/minio.service
[Unit]
Description=MinIO Object Storage
After=network-online.target

[Service]
EnvironmentFile=/etc/default/minio
ExecStart=/usr/local/bin/minio server $MINIO_VOLUMES --console-address ":9001"
Restart=always
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now minio

# 创建 bucket
mc alias set local http://localhost:9000 minioadmin YourMinioPassword!
mc mb local/openobserve
```

---

## 第二步：配置 OpenObserve 节点

### 2.1 公共配置（所有节点共享）

创建 `/etc/openobserve/.env.common`：

```bash
# ============================================
# OpenObserve HA 集群 - 公共配置
# ============================================

# --- 集群模式（必须） ---
ZO_LOCAL_MODE=false
ZO_CLUSTER_NAME=my_cluster

# --- 集群协调（NATS） ---
ZO_CLUSTER_COORDINATOR=nats
ZO_NATS_ADDR=nats-server:4222
# ZO_NATS_USER=
# ZO_NATS_PASSWORD=
ZO_NATS_REPLICAS=1          # 单 NATS 节点设为 1，3 节点设为 3

# --- 元数据存储（PostgreSQL，集群模式强制） ---
ZO_META_STORE=postgres
ZO_META_POSTGRES_DSN=postgres://openobserve:YourPGPassword!@pg-server:5432/openobserve

# --- 对象存储（S3/MinIO） ---
ZO_S3_PROVIDER=minio         # 或 s3, gcs, azure
ZO_S3_SERVER_URL=http://minio-server:9000
ZO_S3_ACCESS_KEY=minioadmin
ZO_S3_SECRET_KEY=YourMinioPassword!
ZO_S3_BUCKET_NAME=openobserve
ZO_S3_REGION_NAME=us-east-1

# --- 管理员 ---
ZO_ROOT_USER_EMAIL=admin@your-domain.com
ZO_ROOT_USER_PASSWORD=YourStrongPassword123!

# --- 节点间通信安全 ---
ZO_INTERNAL_GRPC_TOKEN=your-secret-grpc-token-here

# --- 端口 ---
ZO_HTTP_PORT=5080
ZO_GRPC_PORT=5081

# --- 功能开关 ---
ZO_ENABLE_INVERTED_INDEX=true
ZO_TELEMETRY=false

# --- 日志 ---
RUST_LOG=info
```

### 2.2 各角色配置

每个节点创建 `/etc/openobserve/.env`，内容 = 公共配置 + 角色覆盖：

**Router 节点** (`node-router-1`)：
```bash
# source 公共配置后覆盖
ZO_NODE_ROLE=router
```

**Ingester 节点** (`node-ingester-1`)：
```bash
ZO_NODE_ROLE=ingester

# Ingester 特有调优
ZO_WAL_WRITE_QUEUE_ENABLED=true
ZO_WAL_WRITE_QUEUE_SIZE=50000
ZO_MAX_FILE_SIZE_ON_DISK=64
ZO_FILE_PUSH_INTERVAL=15
```

**Querier 节点** (`node-querier-1`)：
```bash
ZO_NODE_ROLE=querier

# Querier 特有调优
ZO_MEMORY_CACHE_ENABLED=true
ZO_MEMORY_CACHE_MAX_SIZE=8192
ZO_DISK_CACHE_ENABLED=true
ZO_DISK_CACHE_MAX_SIZE=204800
```

**Compactor 节点** (`node-compactor-1`)：
```bash
ZO_NODE_ROLE=compactor

# Compactor 特有调优
ZO_COMPACT_TANTIVY_BUILDER_THREAD_NUM=4
```

**AlertManager 节点**（可选）：
```bash
ZO_NODE_ROLE=alertmanager
```

### 2.3 合并角色（小集群）

如果机器有限，可以让一个节点同时担任多个角色：

```bash
# 2 节点极简 HA：
# 节点 A: router + ingester + alertmanager
ZO_NODE_ROLE=router,ingester,alertmanager

# 节点 B: querier + compactor
ZO_NODE_ROLE=querier,compactor
```

---

## 第三步：Systemd 服务配置

每个节点使用相同的 systemd service 文件（见前文第四章），但 `.env` 文件内容不同。

推荐做法——合并公共配置和角色配置：

```bash
# /etc/openobserve/.env 完整示例（Router 节点）
# --- 公共 ---
ZO_LOCAL_MODE=false
ZO_CLUSTER_NAME=my_cluster
ZO_CLUSTER_COORDINATOR=nats
ZO_NATS_ADDR=10.0.1.10:4222
ZO_META_STORE=postgres
ZO_META_POSTGRES_DSN=postgres://openobserve:YourPGPassword!@10.0.1.11:5432/openobserve
ZO_S3_PROVIDER=minio
ZO_S3_SERVER_URL=http://10.0.1.12:9000
ZO_S3_ACCESS_KEY=minioadmin
ZO_S3_SECRET_KEY=YourMinioPassword!
ZO_S3_BUCKET_NAME=openobserve
ZO_S3_REGION_NAME=us-east-1
ZO_ROOT_USER_EMAIL=admin@your-domain.com
ZO_ROOT_USER_PASSWORD=YourStrongPassword123!
ZO_INTERNAL_GRPC_TOKEN=my-secret-token
ZO_HTTP_PORT=5080
ZO_GRPC_PORT=5081
ZO_ENABLE_INVERTED_INDEX=true
ZO_TELEMETRY=false
RUST_LOG=info

# --- 角色 ---
ZO_NODE_ROLE=router
```

启动所有节点：
```bash
# 在每台机器上执行
sudo systemctl daemon-reload
sudo systemctl enable --now openobserve
```

---

## 第四步：负载均衡

### Nginx 配置（对 Router 节点做负载均衡）

```nginx
upstream openobserve_routers {
    server 10.0.1.20:5080;   # router-1
    server 10.0.1.21:5080;   # router-2
    keepalive 64;
}

server {
    listen 80;
    server_name openobserve.your-domain.com;
    return 301 https://$server_name$request_uri;
}

server {
    listen 443 ssl http2;
    server_name openobserve.your-domain.com;

    ssl_certificate /etc/letsencrypt/live/openobserve.your-domain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/openobserve.your-domain.com/privkey.pem;

    client_max_body_size 256m;

    location / {
        proxy_pass http://openobserve_routers;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_http_version 1.1;
        proxy_set_header Connection "";

        # WebSocket
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";

        proxy_connect_timeout 60s;
        proxy_send_timeout 300s;
        proxy_read_timeout 300s;
    }
}
```

---

## 第五步：验证集群

```bash
# 1. 检查各节点健康
for host in router-1 ingester-1 querier-1 compactor-1; do
    echo "--- $host ---"
    curl -s http://$host:5080/healthz
    echo
done

# 2. 检查集群节点注册情况（通过 Router 的 API）
curl -s -u admin@your-domain.com:YourStrongPassword123! \
  http://router-1:5080/api/cluster/nodes | python3 -m json.tool

# 3. 测试数据摄入
curl -u admin@your-domain.com:YourStrongPassword123! \
  -X POST http://router-1:5080/api/default/test_stream/_json \
  -H "Content-Type: application/json" \
  -d '[{"level":"info","message":"cluster test","ts":"'$(date -Iseconds)'"}]'

# 4. 测试查询
curl -u admin@your-domain.com:YourStrongPassword123! \
  -X POST http://router-1:5080/api/default/_search \
  -H "Content-Type: application/json" \
  -d '{"query":{"sql":"SELECT * FROM test_stream ORDER BY _timestamp DESC LIMIT 5"}}'
```

---

## 第六步：横向扩展

添加更多同角色节点只需：

1. 在新机器上安装二进制（同版本）
2. 复制对应角色的 `.env` 配置
3. 启动 systemd 服务
4. 节点自动通过 NATS 注册加入集群

```bash
# 例如：添加第 2 个 Querier
# 新机器上配置 ZO_NODE_ROLE=querier，其他公共配置不变
sudo systemctl start openobserve

# 验证：新节点应出现在集群列表中
curl -s -u admin:pass http://router-1:5080/api/cluster/nodes | jq '.[] | select(.role=="Querier")'
```

---

## 常见问题（HA 特有）

### 节点无法加入集群
```bash
# 检查 NATS 连通性
nats-cli pub test "hello" --server nats://10.0.1.10:4222

# 检查 GRPC Token 是否一致
grep ZO_INTERNAL_GRPC_TOKEN /etc/openobserve/.env
```

### Ingester 数据丢失
```bash
# Ingester 重启时会重放 WAL，确保 S3 可写
mc ls local/openobserve/

# 检查 WAL 目录
ls -la /var/lib/openobserve/wal/
```

### Compactor 不工作
```bash
# 检查 compactor 节点日志
sudo journalctl -u openobserve -f | grep -i compact

# 确认 compactor 节点已注册
curl -s -u admin:pass http://router:5080/api/cluster/nodes | jq '.[] | select(.role=="Compactor")'
```

### 使用 AWS S3（而非 MinIO）
```bash
ZO_S3_PROVIDER=s3
ZO_S3_SERVER_URL=https://s3.us-east-1.amazonaws.com
ZO_S3_ACCESS_KEY=AKIAXXXXXXXX
ZO_S3_SECRET_KEY=xxxxxxxxxxxxxxxx
ZO_S3_BUCKET_NAME=your-openobserve-bucket
ZO_S3_REGION_NAME=us-east-1
```

---

## 推荐的生产部署规模

### 中等规模（50-200 GB/天）

| 角色 | 节点数 | 配置 |
|------|:---:|------|
| Router | 2 | 4C / 8G |
| Ingester | 2 | 8C / 16G / 200G SSD |
| Querier | 3 | 16C / 32G / 500G SSD (缓存) |
| Compactor | 1 | 8C / 16G |
| AlertManager | 1 | 4C / 8G |
| NATS | 1 (或 3 集群) | 2C / 4G |
| PostgreSQL | 1 (主从可选) | 4C / 8G |
| MinIO/S3 | 分布式或 AWS S3 | - |

### 大规模（1 TB+/天）

| 角色 | 节点数 | 配置 |
|------|:---:|------|
| Router | 3+ | 8C / 16G |
| Ingester | 4+ | 16C / 32G / 500G NVMe |
| Querier | 6+ | 32C / 64G / 1T NVMe (缓存) |
| Compactor | 2+ | 16C / 32G |
| AlertManager | 2 | 4C / 8G |
