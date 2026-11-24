import json
import logging
import os
import re
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from urllib.parse import urlsplit

HOST = "127.0.0.1"
PORT = 8000
DEFAULT_ALLOWED_ORIGINS = {"http://127.0.0.1:5500", "http://localhost:5500"}
ALLOWED_ORIGINS = {
    origin.strip()
    for origin in os.environ.get(
        "ALLOWED_ORIGINS", ",".join(DEFAULT_ALLOWED_ORIGINS)
    ).split(",")
    if origin.strip()
}
CONTRACT_ID_PATTERN = re.compile(r"^C[A-Z2-7]{55}$")
SPEC_PATH = Path(__file__).resolve().parent.parent / "sdk-ts" / "spec.json"
with SPEC_PATH.open(encoding="utf-8") as spec_file:
    ENTRYPOINTS = [entrypoint["name"] for entrypoint in json.load(spec_file)["entrypoints"]]


def _log_escrow_summary(contract_id, status):
    level = "error" if status >= 500 else "warn" if status >= 400 else "info"
    record = {
        "timestamp": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "level": level,
        "event": "escrow.summary",
        "escrow_id": contract_id,
        "status": status,
    }
    log_level = (
        logging.ERROR
        if status >= 500
        else logging.WARNING
        if status >= 400
        else logging.INFO
    )
    logging.getLogger("karis_ky.backend").log(
        log_level, json.dumps(record, separators=(",", ":"))
    )


def _read_escrow_summary(contract_id):
    from stellar_sdk import Keypair, Network
    from stellar_sdk.contract import ContractClient

    rpc_url = os.environ.get("SOROBAN_RPC_URL", "https://soroban-testnet.stellar.org")
    network_passphrase = os.environ.get(
        "SOROBAN_NETWORK_PASSPHRASE", Network.TESTNET_NETWORK_PASSPHRASE
    )
    source = Keypair.random().public_key
    client = ContractClient(contract_id, rpc_url, network_passphrase)
    return client.invoke("get_escrow_summary", [], source=source)


def _is_unknown_contract_error(error):
    return re.search(
        r"contract.*(?:not found|does not exist|missing)|"
        r"(?:not found|does not exist).*contract|missingvalue",
        str(error),
        re.IGNORECASE,
    ) is not None


def _is_valid_contract_id(contract_id):
    if not CONTRACT_ID_PATTERN.fullmatch(contract_id):
        return False
    from stellar_sdk import StrKey

    return StrKey.is_valid_contract(contract_id)


class Handler(BaseHTTPRequestHandler):
    def _send_json(self, status, payload):
        summary_match = re.fullmatch(
            r"/api/contracts/([^/]+)/summary", urlsplit(self.path).path
        )
        if summary_match:
            _log_escrow_summary(summary_match.group(1), status)
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Vary", "Origin")
        origin = self.headers.get("Origin")
        if origin in ALLOWED_ORIGINS:
            self.send_header("Access-Control-Allow-Origin", origin)
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        path = urlsplit(self.path).path
        if path == "/api/health":
            self._send_json(200, {
                "status": "ok",
                "service": "karis-ky-backend",
                "contracts": ["escrow"]
            })
            return

        if path == "/api/contract-summary":
            self._send_json(200, {
                "name": "karis-ky Escrow Contracts",
                "description": "Soroban smart contracts for invoice liquidity on Stellar.",
                "entrypoints": ENTRYPOINTS
            })
            return

        summary_match = re.fullmatch(r"/api/contracts/([^/]+)/summary", path)
        if summary_match:
            contract_id = summary_match.group(1)
            if not _is_valid_contract_id(contract_id):
                self._send_json(400, {"error": "invalid contract ID"})
                return
            try:
                summary = _read_escrow_summary(contract_id)
                self._send_json(200, {"contractId": contract_id, "summary": summary})
            except Exception as error:
                status = 404 if _is_unknown_contract_error(error) else 502
                message = "contract not found" if status == 404 else "Soroban RPC request failed"
                self._send_json(status, {"error": message})
            return

        self._send_json(404, {"error": "not found"})

    def do_OPTIONS(self):
        self.send_response(204)
        self.send_header("Vary", "Origin")
        origin = self.headers.get("Origin")
        if origin in ALLOWED_ORIGINS:
            self.send_header("Access-Control-Allow-Origin", origin)
        self.send_header("Access-Control-Allow-Methods", "GET, OPTIONS")
        self.send_header("Content-Length", "0")
        self.end_headers()


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(message)s")
    server = HTTPServer((HOST, PORT), Handler)
    print(f"Backend listening on http://{HOST}:{PORT}")
    server.serve_forever()
