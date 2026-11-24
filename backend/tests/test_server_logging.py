import json
import logging
from http.server import HTTPServer
from threading import Thread
from urllib.error import HTTPError
from urllib.request import urlopen

import pytest
from stellar_sdk import StrKey

from backend import server as backend_server


def test_summary_logs_include_escrow_id_for_every_outcome(monkeypatch, caplog):
    contract_id = StrKey.encode_contract(bytes(32))
    success_id = StrKey.encode_contract(bytes([1]) * 32)
    monkeypatch.setattr(
        backend_server, "_read_escrow_summary", lambda _contract_id: {"status": "open"}
    )
    caplog.set_level(logging.INFO, logger="karis_ky.backend")

    http_server = HTTPServer(("127.0.0.1", 0), backend_server.Handler)
    thread = Thread(target=http_server.serve_forever)
    thread.start()
    base_url = f"http://127.0.0.1:{http_server.server_port}"
    try:
        with urlopen(f"{base_url}/api/contracts/{success_id}/summary") as response:
            assert response.status == 200

        with pytest.raises(HTTPError) as invalid_error:
            urlopen(f"{base_url}/api/contracts/not-a-contract/summary")
        assert invalid_error.value.code == 400

        def fail_summary(_contract_id):
            raise RuntimeError("RPC unavailable")

        monkeypatch.setattr(
            backend_server, "_read_escrow_summary", fail_summary
        )
        with pytest.raises(HTTPError) as failed_error:
            urlopen(f"{base_url}/api/contracts/{contract_id}/summary")
        assert failed_error.value.code == 502
    finally:
        http_server.shutdown()
        thread.join()
        http_server.server_close()

    records = [json.loads(record.message) for record in caplog.records]
    assert [
        (record["event"], record["escrow_id"], record["status"], record["level"])
        for record in records
    ] == [
        ("escrow.summary", success_id, 200, "info"),
        ("escrow.summary", "not-a-contract", 400, "warn"),
        ("escrow.summary", contract_id, 502, "error"),
    ]
