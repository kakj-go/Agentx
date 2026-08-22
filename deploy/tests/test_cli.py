from __future__ import annotations

import json

import pytest
from agentx_deploy.cli import main, parser
from agentx_deploy.process import redact


def test_values_is_required() -> None:
    with pytest.raises(SystemExit):
        parser().parse_args(["install"])


def test_rollback_requires_revision() -> None:
    with pytest.raises(SystemExit):
        parser().parse_args(["rollback", "--values", "deploy/values/local.yaml", "--target", "runtime"])


def test_redaction_removes_sensitive_values() -> None:
    value = redact(
        'password=hello "token":"abc" private_key=xyz mysql://user:database-password@example.test/db '
        "-----BEGIN PRIVATE KEY-----\nkey-material\n-----END PRIVATE KEY----- safe=visible"
    )
    assert "hello" not in value
    assert "abc" not in value
    assert "xyz" not in value
    assert "database-password" not in value
    assert "key-material" not in value
    assert "visible" in value


def test_json_failure_is_machine_readable(capsys: pytest.CaptureFixture[str]) -> None:
    with pytest.raises(SystemExit):
        main(["status", "--values", "missing.yaml", "--output", "json"])
    payload = json.loads(capsys.readouterr().err)
    assert payload["status"] == "error"
    assert "missing.yaml" in payload["error"]


def test_render_json_is_machine_readable(monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]) -> None:
    monkeypatch.setattr("agentx_deploy.cli.execute", lambda _args: "apiVersion: v1\nkind: Service\n")
    main(["render", "--values", "deploy/values/local.yaml", "--output", "json"])
    payload = json.loads(capsys.readouterr().out)
    assert payload == {"status": "rendered", "manifest": "apiVersion: v1\nkind: Service\n"}
