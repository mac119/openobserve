#!/usr/bin/env python3
"""
Fake AI Agent — 用于抓包分析 OpenObserve 主服务发给 o2-sre-agent 的请求格式。

它做两件事：
1. 把收到的任何请求（method / path / headers / body）完整打印到控制台
2. 返回一个简单的 SSE 流响应，让 UI 上的 AI Chat 能显示出内容

用法：
    python3 fake_ai_agent.py --port 8080

然后在 OpenObserve 主服务配置：
    O2_AI_ENABLED=true
    O2_AI_AGENT_URL=http://<本机IP>:8080
    重启 openobserve

在 UI 点击 AI Chat 发一条消息，观察本脚本控制台打印的请求内容。

仅依赖 Python 标准库，无需 pip install。
"""

import argparse
import json
import datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def ts():
    return datetime.datetime.now().strftime("%H:%M:%S.%f")[:-3]


def pretty(data: bytes) -> str:
    """尝试格式化 JSON，失败则原样返回。"""
    try:
        obj = json.loads(data)
        return json.dumps(obj, indent=2, ensure_ascii=False)
    except Exception:
        return data.decode("utf-8", errors="replace")


class Handler(BaseHTTPRequestHandler):
    # 关闭默认的访问日志（我们自己打印）
    def log_message(self, fmt, *args):
        pass

    def _dump_request(self):
        print("\n" + "=" * 80)
        print(f"[{ts()}] {self.command} {self.path}")
        print("-" * 80)
        print("HEADERS:")
        for k, v in self.headers.items():
            # 认证头只打印前缀，避免泄露
            if k.lower() == "authorization":
                print(f"  {k}: {v[:20]}...(truncated)")
            else:
                print(f"  {k}: {v}")

        length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(length) if length > 0 else b""
        if body:
            print("-" * 80)
            print("BODY:")
            print(pretty(body))
        print("=" * 80 + "\n")
        return body

    def _send_sse_response(self):
        """返回一个简单的 SSE 流，模拟 agent 的流式回答。"""
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "keep-alive")
        self.end_headers()

        # 尝试多种可能的 SSE 事件格式，方便观察前端能识别哪种
        events = [
            {"type": "content", "content": "Hello from "},
            {"type": "content", "content": "fake agent! "},
            {"type": "content", "content": "This confirms the protocol works."},
            {"type": "done"},
        ]
        for ev in events:
            line = f"data: {json.dumps(ev)}\n\n"
            try:
                self.wfile.write(line.encode())
                self.wfile.flush()
            except Exception as e:
                print(f"[{ts()}] write error: {e}")
                break

    def _send_json_response(self):
        """非流式响应（QueryResponse 格式）。"""
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        resp = {"response": "Hello from fake agent (non-stream)!"}
        self.wfile.write(json.dumps(resp).encode())

    def do_POST(self):
        body = self._dump_request()
        # 根据 path 判断是流式还是非流式
        if "stream" in self.path.lower():
            self._send_sse_response()
        else:
            # 也用 SSE 兜底（很多 agent 端点都是流式）
            self._send_sse_response()

    def do_GET(self):
        self._dump_request()
        # 健康检查等
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(b'{"status":"ok","service":"fake-agent"}')

    def do_PUT(self):
        self._dump_request()
        self.send_response(200)
        self.end_headers()

    def do_DELETE(self):
        self._dump_request()
        self.send_response(200)
        self.end_headers()


def main():
    parser = argparse.ArgumentParser(description="Fake AI Agent for protocol sniffing")
    parser.add_argument("--port", type=int, default=8080, help="Listen port")
    parser.add_argument("--host", default="0.0.0.0", help="Listen host")
    args = parser.parse_args()

    server = ThreadingHTTPServer((args.host, args.port), Handler)
    print("=" * 80)
    print(f"  Fake AI Agent listening on http://{args.host}:{args.port}")
    print(f"  Started at {ts()}")
    print("=" * 80)
    print("Configure OpenObserve:")
    print(f"  O2_AI_ENABLED=true")
    print(f"  O2_AI_AGENT_URL=http://<this-host-ip>:{args.port}")
    print("Then restart openobserve and click AI Chat in the UI.")
    print("=" * 80)
    print("Waiting for requests...\n")

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\nShutting down.")
        server.shutdown()


if __name__ == "__main__":
    main()
