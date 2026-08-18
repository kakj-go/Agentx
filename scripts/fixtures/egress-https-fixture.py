import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, pattern, *args):
        sys.stdout.write((pattern % args) + "\n")
        sys.stdout.flush()

    def body(self):
        length = int(self.headers.get("content-length", "0"))
        raw = self.rfile.read(length) if length else b"{}"
        try:
            return json.loads(raw)
        except json.JSONDecodeError:
            return {}

    def send_bytes(self, status, content_type, payload):
        self.send_response(status)
        self.send_header("content-type", content_type)
        self.send_header("content-length", str(len(payload)))
        self.send_header("connection", "close")
        self.end_headers()
        self.wfile.write(payload)

    def send_json(self, value, status=200):
        self.send_bytes(status, "application/json", json.dumps(value).encode())

    def do_GET(self):
        if self.path == "/health":
            self.send_json({"ok": True, "route": "health"})
        else:
            self.send_json({"ok": False}, 404)

    def do_POST(self):
        value = self.body()
        if self.path == "/mcp":
            result = {
                "protocolVersion": "2025-03-26",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "agentx-egress-fixture", "version": "1"},
            }
            envelope = {"jsonrpc": "2.0", "id": value.get("id"), "result": result}
            if "text/event-stream" in self.headers.get("accept", ""):
                payload = ("data: " + json.dumps(envelope) + "\n\n").encode()
                self.send_bytes(200, "text/event-stream", payload)
            else:
                self.send_json(envelope)
            return
        allowed = {
            "/v1/chat/completions",
            "/rag/query",
            "/rag/documents/text",
            "/memory/search",
            "/memory/memories",
            "/http",
            "/remote",
            "/poll",
            "/lifecycle",
        }
        if self.path in allowed:
            self.send_json({"ok": True, "route": self.path, "input": value})
        else:
            self.send_json({"ok": False}, 404)


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 18090
    ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
