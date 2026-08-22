from __future__ import annotations

import json
from datetime import UTC, datetime
from pathlib import Path

import pytest
from agentx_deploy.config import load_values
from agentx_deploy.operations import backup_operation, image_reference
from agentx_deploy.process import Result


def test_image_reference_does_not_duplicate_repository_prefix() -> None:
    config = load_values("deploy/values/dockerhub-beta.yaml")
    assert image_reference(config, "agentx-migrate") == "kakj/agentx-migrate:v0.0.1-beta"
    assert image_reference(config, "platform-control") == "kakj/agentx-platform-control:v0.0.1-beta"


def test_python_backup_adapter_receipt_is_validated(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    config = load_values("deploy/values/production.example.yaml")
    adapter = tmp_path / "adapter.py"
    adapter.write_text("# fixture", encoding="utf-8")
    receipt = {
        "status": "passed",
        "recoveryPointUtc": datetime.now(UTC).isoformat(),
        "objectCount": 1,
        "contentSha256": "0" * 64,
        "schemaVersionObserved": "control-0006",
    }

    def fake_run(command: list[str], **_: object) -> Result:
        assert command[0].endswith(("python", "python.exe"))
        return Result(tuple(command), json.dumps(receipt), "", 0)

    monkeypatch.setattr("agentx_deploy.operations.run", fake_run)
    result = backup_operation(
        config,
        action="backup",
        target="control-mysql",
        backup_id="unit-backup",
        adapter=adapter,
        restore_target=None,
        artifact_dir=tmp_path / "artifacts",
        allow_in_place_restore=False,
    )
    assert result["status"] == "passed"
    assert Path(result["path"]).is_file()


def test_restore_requires_an_explicit_target(tmp_path: Path) -> None:
    config = load_values("deploy/values/production.example.yaml")
    adapter = tmp_path / "adapter.py"
    adapter.write_text("# fixture", encoding="utf-8")
    with pytest.raises(ValueError, match="restore-target"):
        backup_operation(
            config,
            action="restore",
            target="runtime-mysql",
            backup_id="unit-restore",
            adapter=adapter,
            restore_target=None,
            artifact_dir=tmp_path,
            allow_in_place_restore=False,
        )
