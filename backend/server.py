import json
from http.server import BaseHTTPRequestHandler, HTTPServer

HOST = "127.0.0.1"
PORT = 8000


class Handler(BaseHTTPRequestHandler):
    def _send_json(self, status, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/api/health":
            self._send_json(200, {
                "status": "ok",
                "service": "karis-ky-backend",
                "contracts": ["escrow"]
            })
            return

        if self.path == "/api/contract-summary":
            self._send_json(200, {
                "name": "karis-ky Escrow Contracts",
                "description": "Soroban smart contracts for invoice liquidity on Stellar.",
                "entrypoints": ["init", "fund", "settle", "withdraw"]
            })
            return

        self._send_json(404, {"error": "not found"})


if __name__ == "__main__":
    server = HTTPServer((HOST, PORT), Handler)
    print(f"Backend listening on http://{HOST}:{PORT}")
    server.serve_forever()
