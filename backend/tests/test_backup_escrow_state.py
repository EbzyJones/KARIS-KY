import json

import pytest
from backend import backup_escrow_state as backup
from stellar_sdk.contract import ContractClient


class FakeServer:
    server_url = "https://rpc.example.test"


def test_build_snapshot_with_successful_rpc_responses(monkeypatch):
    calls = []

    def invoke(self, function_name, args, source=None):
        calls.append(function_name)
        if function_name == "get_escrow":
            return {"status": 0, "funded_amount": 125}
        if function_name == "get_unique_funder_count":
            return 2
        return {"entrypoint": function_name}

    monkeypatch.setattr(ContractClient, "invoke", invoke)

    snapshot = backup.build_snapshot(
        FakeServer(), "C" + "A" * 55, "test network", []
    )

    assert snapshot["escrow"] == {"status": 0, "funded_amount": 125}
    assert snapshot["unique_funder_count"] == 2
    assert snapshot["funding_token"] == {"entrypoint": "get_funding_token"}
    assert len(calls) == 16


def test_build_snapshot_records_rpc_errors(monkeypatch):
    def invoke(self, function_name, args, source=None):
        if function_name == "get_escrow":
            raise RuntimeError("RPC unavailable")
        return function_name

    monkeypatch.setattr(ContractClient, "invoke", invoke)

    snapshot = backup.build_snapshot(
        FakeServer(), "C" + "A" * 55, "test network", []
    )

    assert snapshot["escrow"] == {"__error": "RPC unavailable"}
    assert snapshot["schema_version"] == "get_version"


def test_build_snapshot_records_malformed_rpc_responses(monkeypatch):
    def invoke(self, function_name, args, source=None):
        if function_name == "get_escrow":
            raise ValueError("malformed simulation response")
        return function_name

    monkeypatch.setattr(ContractClient, "invoke", invoke)

    snapshot = backup.build_snapshot(
        FakeServer(), "C" + "A" * 55, "test network", []
    )

    assert snapshot["escrow"] == {"__error": "malformed simulation response"}


def test_snapshot_round_trip_export_import_export(tmp_path):
    original = {
        "escrow": {"status": 1, "funded_amount": 125},
        "investors": {"GTEST": {"contribution": 125}},
    }

    exported = backup.export_snapshot(
        original,
        "C" + "A" * 55,
        123456,
        "20261122T033000Z",
    )
    backup_path = tmp_path / "snapshot.json"
    backup_path.write_text(json.dumps(exported), encoding="utf-8")

    imported = backup.import_snapshot(str(backup_path))
    re_exported = backup.export_snapshot(
        imported,
        "C" + "A" * 55,
        123456,
        "20261122T033000Z",
    )

    assert re_exported["state"] == original
    assert re_exported == exported


def test_corrupted_snapshot_is_rejected(tmp_path):
    exported = backup.export_snapshot(
        {"escrow": {"status": 1, "funded_amount": 125}},
        "C" + "A" * 55,
        123456,
    )
    exported["state"]["escrow"]["funded_amount"] = 999

    backup_path = tmp_path / "corrupted.json"
    backup_path.write_text(json.dumps(exported), encoding="utf-8")

    with pytest.raises(ValueError, match="checksum mismatch"):
        backup.import_snapshot(str(backup_path))