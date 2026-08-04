# 「天机」AI 诊断能力设计方案

## 定位

「天机」是一款**基础运维监控系统**，聚焦**网络设备 + 网络质量监控**。差异化竞争力在于**内置的 AI 诊断能力**——不同于 OpenObserve 把 AI 全放在企业版闭源，天机将 AI 诊断作为核心开放能力。

核心理念：**从"被动看图"到"主动诊断"** —— 系统不仅展示指标，还能自动回答"网络为什么慢了？故障在哪个设备？根因是什么？"

---

## 一、竞品 AI 能力对比

| 能力 | OpenObserve（企业版） | 天机（规划） |
|------|:---:|:---:|
| AI Chat 助手 | ✅ 闭源 | ✅ 开放 |
| NL→查询 | ✅ 闭源 | ✅ 开放 |
| **自动根因分析（RCA）** | ✅ 闭源（o2-sre-agent） | ✅ **核心能力** |
| **网络拓扑感知诊断** | ❌ | ✅ **差异化** |
| **异常自动检测** | ❌ | ✅ |
| **故障链路定位** | ❌ | ✅ **差异化** |
| 智能告警降噪 | 部分 | ✅ |

天机的独特优势：**网络领域专精**（设备拓扑 + 链路质量），OpenObserve 是通用可观测性，缺乏网络专业诊断。

---

## 二、天机 AI 诊断的四大能力

### 能力 1：异常自动检测（Anomaly Detection）

不需要人工设阈值，基于历史 baseline 自动发现异常。

```
输入：设备指标时序（丢包率、延迟、带宽利用率、CPU、接口错误计数）
       ↓
算法：动态基线（移动平均 + 标准差）/ 季节性分解（STL）/ 孤立森林（Isolation Forest）
       ↓
输出：异常事件（哪个设备、哪个指标、偏离程度、时间点）
```

**网络场景示例**：
- 交换机端口 Gi0/1 丢包率突然从 0.01% 升到 5%
- 某链路延迟从 2ms 抖动到 50ms（jitter 异常）
- 核心路由器 CPU 从 30% 飙到 90%

### 能力 2：自动根因分析（RCA）

告警触发后，AI 自动关联多维信号，推断根因。

```
故障现象：用户反馈"访问某业务慢"
       ↓
AI 诊断链路：
  1. 关联时间窗口内的所有异常（拓扑相关设备）
  2. 沿网络拓扑路径回溯（源→目标经过哪些设备/链路）
  3. 关联变更事件（配置变更、设备重启、链路切换）
  4. 大模型综合分析 → 生成根因假设 + 置信度
       ↓
输出：
  "根因：核心交换机 SW-Core-01 的上行链路 Te1/0/1 出现拥塞
   （利用率 98%），导致经过该链路的业务延迟上升。
   触发时间与 14:32 的 BGP 路由收敛事件吻合。
   建议：检查该链路流量突增来源 / 启用备用链路。
   置信度：85%"
```

### 能力 3：网络拓扑感知诊断（差异化核心）

结合网络拓扑图，做**故障传播分析**和**影响范围评估**。

```
        ┌─────────┐
        │ Core-01 │ ← 根因设备（红）
        └────┬────┘
       ┌─────┴─────┐
   ┌───▼───┐   ┌───▼───┐
   │Agg-01 │   │Agg-02 │ ← 受影响（橙）
   └───┬───┘   └───────┘
   ┌───▼───┐
   │Acc-01 │ ← 受影响业务（橙）
   └───────┘
```

AI 能力：
- **故障定界**：区分"根因设备"vs"受影响设备"（避免告警风暴中误判）
- **影响面评估**：这个故障影响了哪些下游设备/业务/用户
- **路径分析**：源到目标的网络路径上，哪一跳是瓶颈

### 能力 4：智能告警降噪与聚合

```
100 条告警（告警风暴）
       ↓
AI 聚合：识别同源告警 → 归并为 1 个"事件"
       ↓
"事件：Core-01 故障，衍生 100 条下游告警。
 根因告警：Core-01 上行链路 down。
 其余 99 条为衍生告警，已自动抑制。"
```

---

## 三、技术架构

```
┌────────────────────────────────────────────────────────────┐
│                      天机 Web UI                             │
│  ┌────────────┐  ┌─────────────┐  ┌──────────────────┐     │
│  │ 诊断对话框  │  │ 拓扑诊断视图 │  │ 事件时间线        │     │
│  └─────┬──────┘  └──────┬──────┘  └────────┬─────────┘     │
└────────┼────────────────┼──────────────────┼───────────────┘
         │                │                  │
┌────────▼────────────────▼──────────────────▼───────────────┐
│                  天机 AI 诊断引擎（tianji-ai-agent）          │
│                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌─────────────────┐  │
│  │ 异常检测器    │  │ RCA 推理引擎  │  │ 拓扑分析器       │  │
│  │ (统计/ML)    │  │ (LLM + 规则) │  │ (图算法)         │  │
│  └──────────────┘  └──────┬───────┘  └─────────────────┘  │
│                           │                                │
│                    ┌──────▼───────┐                        │
│                    │ Tool/MCP 层  │  (查询指标/日志/拓扑)   │
│                    └──────┬───────┘                        │
└───────────────────────────┼────────────────────────────────┘
                           │
         ┌─────────────────┼─────────────────┐
         │                 │                 │
   ┌─────▼─────┐    ┌──────▼──────┐   ┌──────▼──────┐
   │ 时序库     │    │ 拓扑库       │   │ LLM 网关     │
   │ (指标)     │    │ (设备/链路)  │   │ (LiteLLM)   │
   └───────────┘    └─────────────┘   └─────────────┘
```

### 核心组件

#### 1. 异常检测器（无 LLM，纯算法，实时）
- 轻量级统计方法（EWMA、3-sigma）用于实时检测
- ML 模型（Isolation Forest / Prophet）用于周期性 baseline
- 输出结构化异常事件，喂给 RCA 引擎

#### 2. RCA 推理引擎（LLM + 规则）
- **规则层**：网络专家规则（如"链路 down → 下游全部不可达"）快速定界
- **LLM 层**：综合异常、拓扑、变更、日志，生成自然语言根因分析
- 采用 **ReAct / Agent** 模式，能主动调用工具查询更多数据

#### 3. 拓扑分析器（图算法）
- 网络拓扑存为图（设备=节点，链路=边）
- 故障传播用图遍历（BFS/DFS）计算影响面
- 最短路径分析定位瓶颈跳

#### 4. Tool/MCP 层（Agent 的工具集）
Agent 可调用的工具：
- `query_metrics(device, metric, time_range)` — 查设备指标
- `query_logs(device, keyword, time_range)` — 查设备日志
- `get_topology(device)` — 获取拓扑关系
- `get_change_events(time_range)` — 查配置变更/事件
- `ping_test(src, dst)` — 主动探测（可选）

---

## 四、数据模型扩展

### 网络设备表
```sql
CREATE TABLE devices (
    device_id TEXT PRIMARY KEY,
    hostname TEXT,
    device_type TEXT,       -- switch/router/firewall/ap
    ip_address TEXT,
    location TEXT,
    layer TEXT,             -- core/aggregation/access
    vendor TEXT,            -- cisco/huawei/h3c/juniper
    model TEXT
);
```

### 拓扑关系表
```sql
CREATE TABLE topology_links (
    link_id TEXT PRIMARY KEY,
    src_device_id TEXT,
    src_interface TEXT,
    dst_device_id TEXT,
    dst_interface TEXT,
    link_type TEXT,         -- fiber/copper/wireless
    bandwidth_mbps INTEGER
);
```

### 异常事件表
```sql
CREATE TABLE anomaly_events (
    event_id TEXT PRIMARY KEY,
    device_id TEXT,
    metric_name TEXT,
    detected_at TIMESTAMP,
    severity TEXT,
    baseline_value REAL,
    actual_value REAL,
    deviation_pct REAL,
    status TEXT             -- open/investigating/resolved
);
```

### 诊断记录表
```sql
CREATE TABLE diagnosis_records (
    diagnosis_id TEXT PRIMARY KEY,
    trigger_event_id TEXT,
    root_cause TEXT,
    confidence REAL,
    affected_devices JSON,
    recommendation TEXT,
    llm_reasoning TEXT,
    created_at TIMESTAMP
);
```

---

## 五、诊断流程（端到端）

```
1. 数据采集（SNMP/Telemetry/Syslog/NetFlow）
   → 设备指标、接口状态、流量、日志持续入库
        ↓
2. 异常检测（后台定时任务，每 30s）
   → 检测出异常 → 写入 anomaly_events
        ↓
3. 触发诊断（异常达到阈值 或 用户手动触发）
   → RCA 引擎启动
        ↓
4. RCA 推理（Agent 循环）
   a. 拉取异常事件 + 拓扑关系
   b. 规则层快速定界（是否是链路 down 类明确故障）
   c. LLM 分析：调用工具查更多数据（logs/metrics/changes）
   d. 生成根因假设 + 置信度 + 建议
        ↓
5. 输出诊断报告
   → 拓扑图高亮根因设备
   → 自然语言解释
   → 修复建议
   → 写入 diagnosis_records
        ↓
6. 反馈闭环
   → 用户确认根因是否正确 → 优化规则/prompt
```

---

## 六、MVP 分阶段实现

### Phase 1：基础监控 + 异常检测（不依赖 LLM）
- 网络设备采集（SNMP）
- 指标时序存储
- 统计法异常检测（EWMA + 3-sigma）
- 异常事件展示

### Phase 2：拓扑管理 + 图分析
- 拓扑自动发现（LLDP/CDP）或手动录入
- 拓扑可视化
- 故障传播分析（受影响面计算）

### Phase 3：LLM RCA 引擎
- 接入 LLM（复用 LiteLLM 网关）
- Agent + 工具集（查指标/日志/拓扑）
- 自然语言根因分析

### Phase 4：智能降噪 + 反馈优化
- 告警聚合归并
- 诊断反馈闭环
- 规则/prompt 持续优化

---

## 七、与 LiteLLM 的整合

天机的 LLM 调用统一走 LiteLLM 网关（你已有的 `litellm.ai.levelinfinite.com`）：

```yaml
# 天机 AI 引擎配置
llm:
  gateway: "https://litellm.ai.levelinfinite.com"
  api_key: "sk-xxx"
  models:
    rca: "vertex_ai/gemini-3.1-pro-preview"    # RCA 用强模型
    summary: "gpt-4o-mini"                       # 摘要用小模型
  tracing:
    enabled: true
    otlp_endpoint: "http://tianji:5080/api/default/"  # LLM 调用回传天机自监控
```

好处：天机自己的 AI 诊断调用，也能被天机自己监控（吃自己的狗粮）。

---

## 八、差异化总结

| 维度 | 天机的独特价值 |
|------|---------------|
| **网络专精** | 不是通用可观测性，深耕网络设备/链路诊断 |
| **拓扑感知** | 结合网络拓扑做故障传播与定界（竞品缺失） |
| **AI 开放** | 核心诊断能力开放，不像 OpenObserve 锁在企业版 |
| **闭环诊断** | 从异常检测→根因分析→修复建议→反馈优化全链路 |
| **中文运维场景** | 面向国内网络设备（华为/H3C/锐捷）和运维习惯优化 |
