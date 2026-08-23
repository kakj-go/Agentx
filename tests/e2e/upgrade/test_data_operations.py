from __future__ import annotations

import json
from pathlib import Path

import pytest

from tests.e2e.support import ROOT, agentxctl, backup_adapter, run


@pytest.mark.upgrade
def test_backup_and_restore_use_the_five_field_adapter_contract(tmp_path: Path) -> None:
    values = ROOT / "deploy" / "values" / "production.example.yaml"
    adapter = backup_adapter()
    backup = run(
        (
            agentxctl(),
            "backup",
            "--values",
            values,
            "--data-target",
            "control-mysql",
            "--backup-id",
            "e2e-adapter",
            "--adapter",
            adapter,
            "--artifact-dir",
            tmp_path,
            "--output",
            "json",
        )
    ).json()
    assert backup["status"] == "passed"
    assert set(backup["providerReceipt"]) == {
        "status",
        "recoveryPointUtc",
        "objectCount",
        "contentSha256",
        "schemaVersionObserved",
    }
    assert json.loads(Path(backup["path"]).read_text(encoding="utf-8"))["operation"] == "backup"

    restored = run(
        (
            agentxctl(),
            "restore",
            "--values",
            values,
            "--data-target",
            "control-mysql",
            "--backup-id",
            "e2e-adapter",
            "--adapter",
            adapter,
            "--restore-target",
            "restored-control.example.internal",
            "--artifact-dir",
            tmp_path,
            "--output",
            "json",
        )
    ).json()
    assert restored["status"] == "passed"
    assert restored["restoreTarget"] == "restored-control.example.internal"
