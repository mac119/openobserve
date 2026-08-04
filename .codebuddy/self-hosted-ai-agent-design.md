# 自研 AI Agent（替代 o2-sre-agent）设计方案

## 可行性结论

**完全可以自研。** `o2-sre-agent` 本质上就是一个 HTTP 服务：
- 接收 OpenObserve 主服务转发的查询请求（固定 JSON 格式）
- 调用 LLM + 工具（查询 OpenObserve 数据）
- 返回 SSE 流式响应

只要我们的 agent **实现相同的接口协议**，OpenObserve 主服务无感知，直接对接。

---

## 一、接口协议（从主服务源码逆向）

### 主服务如何调用 agent

代码位置：`src/handler/http/request/ai/chat.rs`

主服务通过企业版的 `get_agent_client()` 创建 client，调用：
```rust
client.query_stream_with_headers(agent_type, query_req, &auth_str, headers)
```

### 关键配置

```bash
O2_AI_ENABLED=true
O2_AI_AGENT_URL=http://your-agent:8080   # 指向我们自研的 agent
```

### 请求格式（QueryRequest）

主服务发给 agent 的 JSON 结构（从源码 `QueryRequest` 推断）：

```json
{
  "query": "帮我看最近的错误日志",           // 最后一条用户消息
  "context": {
    "org_id": "default",
    "page_type": "logs",                     // 当前页面上下文
    "incident_id": "xxx"                     // 可选，有则用 SRE/RCA agent
  },
  "model": "gpt-4o-mini",                    // 可选
  "history": [                               // 历史对话
    {"role": "user", "content": "..."},
    {"role": "assistant", "content": "..."}
  ],
  "images": [                                // 可选，多模态
    {"data": "base64...", "mime_type": "image/png", "filename": "x.png"}
  ],
  "mcps": [...],                             // 可选，MCP 工具集
  "clis": [...],                             // 可选，CLI 工具
  "skills": [...]                            // 可选，技能
}
```

### agent_type 判断逻辑

```rust
// src/handler/http/request/ai/chat.rs:58
fn get_agent_type(context) -> &str {
    if context 包含 incident_id {
        RCA_AGENT_TYPE      // "rca" - 根因分析 agent
    } else {
        DEFAULT_AGENT_TYPE  // "o2-ai" - 默认助手
    }
}
```

### 响应格式（SSE 流）

主服务直接**透传** agent 的 SSE 流给前端。格式为标准 SSE：

```
data: {"type": "content", "content": "根据日志分析..."}\n\n
data: {"type": "content", "content": "发现 3 个错误..."}\n\n
data: {"type": "tool_call", "tool": "query_logs", "args": {...}}\n\n
data: {"type": "done"}\n\n
```

错误格式：
```
data: {"type": "error", "error": "错误信息"}\n\n
```

### 非流式响应（QueryResponse）

```json
{
  "response": "完整的回答文本"
}
```

---

## 二、自研 Agent 架构

```
┌─────────────────────────────────────────────────────┐
│              自研 AI Agent (tianji-agent)             │
│                                                      │
│  POST /query          (非流式)                        │
│  POST /query_stream   (SSE 流式)  ← 主要                │
│                                                      │
│  ┌────────────────────────────────────────────┐    │
│  │  1. 解析 QueryRequest                        │    │
│  │  2. 构建 LLM prompt (query + context + history) │
│  │  3. Agent 循环 (ReAct):                       │    │
│  │     - LLM 决定调用哪个工具                     │    │
│  │     - 执行工具 (查 OpenObserve API)           │    │
│  │     - 结果喂回 LLM                            │    │
│  │     - 重复直到得出答案                         │    │
│  │  4. SSE 流式返回                              │    │
│  └────────────────────────────────────────────┘    │
│              │                    │                  │
│      ┌───────▼──────┐    ┌────────▼────────┐        │
│      │  LLM 客户端   │    │  工具集 (Tools)  │        │
│      │  (LiteLLM)   │    │                 │        │
│      └──────────────┘    └────────┬────────┘        │
└───────────────────────────────────┼─────────────────┘
                                    │
                          ┌─────────▼─────────┐
                          │  OpenObserve API  │
                          │  (查询日志/指标)   │
                          └───────────────────┘
```

---

## 三、开源方案选型

### 方案 A：基于开源 Agent 框架自研（推荐）

用成熟的 LLM Agent 框架，只需实现工具 + 接口适配：

| 框架 | 语言 | 优势 |
|------|------|------|
| **LangChain / LangGraph** | Python | 生态最全，工具/Agent 抽象成熟 |
| **LlamaIndex** | Python | RAG + Agent，适合数据密集 |
| **Vercel AI SDK** | TS/Node | 流式响应原生支持，轻量 |
| **Rig** | Rust | 与 OpenObserve 同语言，性能好 |
| **Eino** (字节开源) | Go | 国产，Agent 编排强 |

**推荐组合**：Python + LangGraph + FastAPI（生态成熟、开发快）

### 方案 B：直接用开源 SRE/可观测性 Agent

已有的开源项目可以参考或改造：

| 项目 | 说明 |
|------|------|
| **HolmesGPT**（Robusta） | 开源 K8s/可观测性 AI 诊断 agent，支持接入 Prometheus/Loki 等 |
| **k8sgpt** | K8s 诊断，可扩展 |
| **Ollama + 自定义工具** | 本地 LLM + 自研工具 |

**HolmesGPT 最接近** o2-sre-agent 的定位（可观测性 RCA），可以研究它的工具设计。

---

## 四、MVP 实现（Python + FastAPI + LangGraph）

### 项目结构
```
tianji-agent/
├── main.py              # FastAPI 服务，实现 /query_stream
├── agent.py             # LangGraph Agent 定义
├── tools/
│   ├── query_logs.py    # 查询日志工具
│   ├── query_metrics.py # 查询指标工具
│   └── query_traces.py  # 查询 traces 工具
├── llm.py               # LiteLLM 客户端
└── config.py
```

### 核心接口实现（示意）

```python
# main.py
from fastapi import FastAPI, Request
from fastapi.responses import StreamingResponse
import json

app = FastAPI()

@app.post("/query_stream")
async def query_stream(request: Request):
    body = await request.json()
    query = body["query"]
    context = body.get("context", {})
    history = body.get("history", [])
    auth = request.headers.get("Authorization")  # 转发给 OpenObserve API

    async def event_generator():
        # 运行 Agent（ReAct 循环）
        async for chunk in run_agent(query, context, history, auth):
            if chunk["type"] == "content":
                yield f'data: {json.dumps(chunk)}\n\n'
            elif chunk["type"] == "tool_call":
                yield f'data: {json.dumps(chunk)}\n\n'
        yield f'data: {json.dumps({"type": "done"})}\n\n'

    return StreamingResponse(event_generator(), media_type="text/event-stream")
```

### Agent + 工具（示意）

```python
# agent.py
from langgraph.prebuilt import create_react_agent
from langchain_openai import ChatOpenAI
from tools.query_logs import query_logs_tool
from tools.query_metrics import query_metrics_tool

async def run_agent(query, context, history, auth):
    # LLM 通过 LiteLLM 网关
    llm = ChatOpenAI(
        base_url="https://litellm.ai.levelinfinite.com",
        api_key="sk-xxx",
        model=context.get("model", "gpt-4o-mini"),
    )

    tools = [
        query_logs_tool(auth, context["org_id"]),
        query_metrics_tool(auth, context["org_id"]),
    ]

    agent = create_react_agent(llm, tools)

    async for event in agent.astream_events(
        {"messages": history + [("user", query)]},
        version="v2",
    ):
        # 转换 LangGraph 事件为 OpenObserve SSE 格式
        if event["event"] == "on_chat_model_stream":
            content = event["data"]["chunk"].content
            if content:
                yield {"type": "content", "content": content}
        elif event["event"] == "on_tool_start":
            yield {"type": "tool_call", "tool": event["name"]}
```

### 工具实现（示意）

```python
# tools/query_logs.py
from langchain_core.tools import tool
import httpx

def query_logs_tool(auth: str, org_id: str):
    @tool
    async def query_logs(stream: str, sql: str, start_time: int, end_time: int) -> str:
        """查询 OpenObserve 日志。sql 为 SQL 查询语句，时间为微秒时间戳。"""
        async with httpx.AsyncClient() as client:
            resp = await client.post(
                f"http://openobserve:5080/api/{org_id}/_search",
                headers={"Authorization": auth},
                json={
                    "query": {
                        "sql": sql,
                        "start_time": start_time,
                        "end_time": end_time,
                    }
                },
            )
            return resp.text
    return query_logs
```

---

## 五、部署对接

```bash
# 1. 启动自研 agent
docker run -d --name tianji-agent -p 8080:8080 \
  -e LLM_GATEWAY=https://litellm.ai.levelinfinite.com \
  -e LLM_API_KEY=sk-xxx \
  -e OPENOBSERVE_URL=http://openobserve:5080 \
  tianji-agent:latest

# 2. 配置 OpenObserve 主服务指向它
O2_AI_ENABLED=true
O2_AI_AGENT_URL=http://tianji-agent:8080

# 3. 重启 OpenObserve
sudo systemctl restart openobserve
```

---

## 六、待验证的细节

自研前需要确认（因为闭源部分看不到完整协议）：

1. **确切的 URL 路径** — agent 的 endpoint 是 `/query_stream` 还是别的？（需抓包或看企业版 client 源码）
2. **agent_type 如何传递** — 是 URL path、header 还是 body 字段？
3. **SSE event 的确切格式** — `type` 字段的取值集合（content/tool_call/done/error）
4. **认证方式** — auth_str 如何传给 agent

### 如何获取这些细节

- **方法 1（推荐）**：抓包。启动一个假 agent（只打印收到的请求），配置 `O2_AI_AGENT_URL` 指向它，在 UI 点 AI Chat，看主服务发来的完整请求
- **方法 2**：反编译/分析企业版二进制中的 `o2_enterprise::ai::client` 模块
- **方法 3**：直接问 OpenObserve 官方要接口文档

---

## 七、下一步建议

1. **先抓包摸清协议**：写一个 10 行的 Python HTTP 服务，把主服务发来的请求原样打印，搞清楚确切格式
2. **实现最小 echo agent**：能返回固定 SSE 响应，让 UI 的 AI Chat 能显示出来
3. **接入 LLM**：用 LiteLLM 网关，实现基础对话
4. **加工具**：实现 query_logs / query_metrics，让 AI 能查数据
5. **加 RCA 逻辑**：针对 incident 的根因分析
