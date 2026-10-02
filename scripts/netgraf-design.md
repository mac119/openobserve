# netgraf 技术方案与项目结构设计

> 从 Categraf 剥离网络设备采集能力，独立实现专注于网络设备监控的采集器。
> 输出对接 OpenObserve（Prometheus Remote Write）。

版本：v0.1 (设计稿)
日期：2026-08-04

---

## 1. 项目背景与目标

### 1.1 背景
[Categraf](https://github.com/flashcatcloud/categraf) 是夜莺（Nightingale）生态的 All-in-One 采集器，内置近百种插件（主机、中间件、数据库、网络设备等）。其中网络设备采集能力（SNMP/Ping/Trap/gNMI 等）成熟可靠，但混在庞大的通用采集器中，带来以下问题：

- **体积臃肿**：为采几台交换机，需要携带 MySQL/Redis/K8s/GPU 等上百个无关插件的依赖。
- **职责不清**：网络运维只关心网络设备，却要理解整个 Categraf 的配置体系。
- **迭代耦合**：网络采集的特性演进受限于 Categraf 主线节奏。

### 1.2 目标
剥离 Categraf 的网络相关插件与必要框架，fork 裁剪出独立程序 **netgraf**：

- 专注网络设备监控：SNMP 轮询、SNMP Trap、Ping、端口/DNS/HTTP 探测、gNMI 遥测、交换机采集等。
- 保留 Categraf 成熟的 **agent 调度 + 插件框架 + Remote Write** 机制，降低移植风险。
- 输出对接 **OpenObserve**（Prometheus Remote Write 协议）。
- 更小的二进制、更聚焦的配置、独立的版本演进。

### 1.3 非目标（本期不做）
- 不做主机系统指标（cpu/mem/disk 等已由其它采集器覆盖）。
- 不做日志采集（logs 模块）。
- 不做 ibex/heartbeat 等夜莺特有的任务执行与心跳（除非后续需要）。

---

## 2. 采集能力范围（保留的插件）

按"全部网络相关插件"选择，从 Categraf `inputs/` 中保留以下插件：

### 2.1 网络设备核心（SNMP 家族）
| 插件 | 作用 | 优先级 |
|---|---|---|
| `snmp` | SNMP v1/v2c/v3 轮询设备 OID（接口流量、CPU、温度等） | P0 核心 |
| `snmp_trap` | 被动接收设备主动上报的 SNMP Trap 告警 | P0 |
| `snmp_zabbix` | Zabbix 风格 SNMP 采集 | P1 |
| `switch_legacy` | 传统交换机批量采集 | P1 |
| `gnmi` | gNMI 流式遥测（现代网络设备，如思科/Arista） | P1 |

### 2.2 网络探测 / 连通性
| 插件 | 作用 | 优先级 |
|---|---|---|
| `ping` | ICMP 探测：可达性、时延、丢包 | P0 核心 |
| `net_response` | TCP/UDP 端口探测 | P0 |
| `http_response` | HTTP(S) 探测 | P1 |
| `dns_query` | DNS 解析探测 | P1 |
| `arp_packet` | ARP 报文采集 | P2 |

### 2.3 主机侧网络状态（可选，采集器自身/边缘网关网络）
| 插件 | 作用 | 优先级 |
|---|---|---|
| `net` | 网络接口流量统计 | P2 |
| `netstat` / `netstat_filter` | 连接状态统计 | P2 |
| `conntrack` | 连接跟踪表 | P2 |
| `ethtool` | 网卡统计 | P2 |
| `sockstat` | Socket 统计 | P2 |

> P0 = MVP 必须；P1 = 首个正式版；P2 = 按需启用。裁剪时可全部保留代码，用配置开关控制启用。

---

## 3. 从 Categraf 保留 / 移除清单

### 3.1 保留的框架模块（原样移植）
| Categraf 目录 | 作用 | netgraf 处理 |
|---|---|---|
| `main.go` / `main_posix.go` / `main_windows.go` / `main_other.go` | 多平台入口 | 保留，改产品名/版本号 |
| `agent/` | 采集调度器（插件生命周期、interval 调度、并发） | 保留，裁剪对 logs/ibex 的引用 |
| `config/` | 配置加载（TOML 解析、providers） | 保留 |
| `writer/` | Remote Write 输出 | 保留，默认指向 OpenObserve |
| `parser/` | 数据解析 | 保留（部分探测插件需要） |
| `types/` | Sample/Metric 数据类型 | 保留 |
| `pkg/` | 通用工具（含 snmp/ping 依赖的底层） | 按依赖裁剪 |
| `prometheus/` | Prometheus 兼容 | 保留（Remote Write 相关部分） |
| `inputs/inputs.go` 等注册中心 | 插件注册表 | 保留，只注册网络插件 |

### 3.2 移除的模块
- `inputs/` 下所有非网络插件（mysql/redis/kafka/docker/k8s/gpu/... 约 80+ 个目录）。
- `logs/`（日志采集）
- `ibex/`（任务执行）
- `heartbeat/`（夜莺心跳，除非要保留设备存活上报）
- `k8s/`（K8s 部署清单，netgraf 另写）
- 相关的 `go.mod` 依赖（移除插件后 `go mod tidy` 自动瘦身）

### 3.3 关键：插件注册裁剪
Categraf 通过 `inputs/` 下的注册机制（各插件 `init()` 调用 `inputs.Add(name, creator)`）自动注册。裁剪核心动作：
1. 删除非网络插件目录。
2. 在插件汇总注册文件（Categraf 中通常是 `inputs/all/` 或分散的 import）里，**只保留网络插件的 blank import**。
3. `go mod tidy` 清理依赖。

---

## 4. netgraf 目标项目结构

```
netgraf/
├── main.go    # 入口（fork 自 categraf，改名/版本）
├── main_posix.go
├── main_windows.go
├── main_other.go
├── Makefile
├── go.mod        # module github.com/<org>/netgraf
├── go.sum
├── README.md
│
├── agent/            # 采集调度框架（移植保留）
│├── agent.go                # 插件启停、调度
│   ├── metrics_agent.go
│   └── ...
│
├── config/            # 配置加载（移植保留）
│   ├── config.go
│   └── ...
│
├── writer/         # Remote Write 输出（移植保留 + 默认O2）
│   ├── writer.go
│   └── ...
│
├── parser/       # 数据解析（移植保留）
├── types/  # 数据类型（移植保留）
├── pkg/            # 通用工具（按依赖裁剪）
│   ├── snmp/       #   SNMP 底层封装
│   └── ...
│
├── inputs/           # 只保留网络插件
│   ├── inputs.go               #   注册中心
│   ├── register.go   #   ★只 blank-import 网络插件
│   ├── snmp/    #   ★SNMP 轮询
│   ├── snmp_trap/        #   ★SNMP Trap
│   ├── snmp_zabbix/
│   ├── switch_legacy/
│   ├── gnmi/
│   ├── ping/       #   ★Ping
│   ├── net_response/           #   ★端口探测
│   ├── http_response/
│   ├── dns_query/
│   ├── arp_packet/
│   ├── net/
│   ├── netstat/
│   ├── conntrack/
│   ├── ethtool/
│   └── sockstat/
│
└── conf/              # 配置样例
    ├── config.toml          #   全局 + writer(OpenObserve)
    ├── logs.toml     #   （可删）
    ├── input.snmp/
    │   └── snmp.toml
    ├── input.snmp_trap/
    │   └── snmp_trap.toml
    ├── input.ping/
    │   └── ping.toml
    ├── input.net_response/
    │   └── net_response.toml
    └── ... (每个启用插件一个目录)
```

★ = MVP 必备。

---

## 5. 数据流与架构

```
┌────────────────────────────────────────────────────────────┐
│         netgraf                  │
│           │
│  ┌─────────────┐   poll/probe    ┌──────────────────────┐   │
│  │ 网络设备     │◄───SNMP/ICMP───│  inputs (网络插件)     │   │
│  │ Switch/AP   │───Trap(被动)──►│  snmp/ping/trap/...    │   │
│  │ Router/WLC  │              └──────────┬───────────┘   │
│  └─────────────┘                │ Samples       │
│            ▼       │
│               ┌──────────────────────┐  │
│   │  agent 调度 + 队列     │  │
│     └──────────┬───────────┘  │
│            ▼              │
│     ┌──────────────────────┐  │
│                │  writer (RemoteWrite) │  │
│└──────────┬───────────┘  │
└──────────────────────────────────────────────┼─────────────┘
               │ Prometheus Remote Write
                  ▼
    http://<o2>:5080/api/<org>/prometheus/api/v1/write
     │
                ▼
           ┌──────────────────────┐
   │     OpenObserve       │
  │  存储/查询/告警/AI     │
 └──────────────────────┘
```

- **主动采集**：snmp/ping/net_response 等按 interval 轮询设备。
- **被动接收**：snmp_trap 监听 UDP 162，接收设备上报。
- **统一输出**：所有插件产出的 `types.Sample` 汇入 agent 队列，由 writer 批量 Remote Write 到 OpenObserve。

---

## 6. 输出对接 OpenObserve

### 6.1 写入端点
```
http://<host>:5080/api/<org>/prometheus/api/v1/write
```
认证：HTTP Basic Auth（用户名=email，密码=password 或 token）。

### 6.2 writer 配置（conf/config.toml）
```toml
[[writers]]
url = "http://43.163.110.244:5080/api/default/prometheus/api/v1/write"
basic_auth_user = "admin@bench.com"
basic_auth_pass = "Bench123!"
timeout = 5000

[writer_opt]
batch = 1000
chan_size = 10000
```

### 6.3 指标命名与标签建议
- 统一前缀：`netgraf_` 或保留插件原生命名（如 `interface_ifHCInOctets`）。
- 关键标签：`ident`（设备IP）、`source`（sysName）、`ifName`/`ifDescr`（接口）、`vendor`、`role`（switch/router/ap/wlc）。
- 便于 OpenObserve 侧按设备/接口做大盘与告警。

---

## 7. 实施计划（分阶段）

### Phase 0：脚手架（本方案后第一步）
- fork Categraf → 改名 netgraf，改 module path、产品名、版本。
- 删除非网络插件目录 + logs/ibex/heartbeat/k8s。
- 裁剪插件注册，只留网络插件；`go mod tidy`。
- 保证 `go build` 通过、`./netgraf --test` 能跑。
- **产出**：可编译的最小骨架。

### Phase 1：MVP 端到端
- 保证 P0 插件（snmp/ping/snmp_trap/net_response）可用。
- writer 默认对接 OpenObserve，跑通"采一台交换机 → O2 能查到指标"。
- **产出**：端到端可用。

### Phase 2：完善
- 补齐 P1 插件（gnmi/snmp_zabbix/switch_legacy/http_response/dns_query）。
- 多设备并发、超时重试、表格容错策略（partial）。
- 内置常用 MIB、默认 OID 模板、设备厂商 profile。
- **产出**：生产可用。

### Phase 3：增强（可选）
- 设备自动发现（网段扫描 + SNMP 探活）。
- netgraf 自监控指标（self_metrics）。
- OpenObserve 大盘与告警规则模板随发行包提供。
- 容器化 + K8s 部署清单。

---

## 8. 风险与注意事项

| 风险 | 说明 | 应对 |
|---|---|---|
| 依赖耦合 | 部分 pkg 工具被非网络插件与网络插件共用，裁剪易误删 | 先删插件目录，再 `go mod tidy`+`go build` 迭代定位 |
| 框架隐式依赖 logs/ibex | agent 可能引用 logs 调度 | 编译报错处按需保留最小桩或裁掉引用 |
| License | Categraf 为 MIT | netgraf 需保留原始 MIT 许可与版权声明，注明 fork 来源 |
| MIB 依赖 | snmp 插件需 MIB 文件/gosmi | 发行包内置常用 MIB，或文档说明安装 net-snmp |
| SNMP Trap 端口 | 162/udp 需权限 | 文档说明 setcap 或 root/端口转发 |
| 上游同步 | Categraf 网络插件后续修 bug | 记录 fork 基线 commit，定期 cherry-pick 网络插件相关提交 |

---

## 9. 待确认事项

1. **module path / 组织名**：`github.com/<org>/netgraf` 里的 `<org>` 用什么？
2. **是否保留 heartbeat**：需要设备/采集器存活上报到 O2 吗？
3. **配置兼容**：是否要保持与 Categraf 配置格式 100% 兼容（方便老用户迁移）？
4. **发行形态**：单二进制 / Docker / K8s DaemonSet，优先哪个？
5. **fork 基线**：以 Categraf 哪个 tag/commit 为基线（建议用最新 release tag）？

---

## 10. 下一步

确认第 9 节的待确认事项后，进入 **Phase 0 脚手架**：
- 拉取 Categraf 源码到本地，执行改名与裁剪，产出可编译的 netgraf 骨架。
- 需要我在本机拉取 Categraf 仓库并实际开始裁剪吗？（会在你指定目录创建 netgraf/）
```
