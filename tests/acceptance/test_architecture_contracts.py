from __future__ import annotations

import json
import re
import shutil
from pathlib import Path

import pytest
import yaml

from tests.e2e.support import render

ROOT = Path(__file__).resolve().parents[2]


def test_table_disposition_covers_catalog_exactly_once() -> None:
    catalog = (ROOT / "docs/reference/mysql-schema-catalog.md").read_text(encoding="utf-8")
    catalog_tables = set(re.findall(r"^### ([a-z0-9_]+)$", catalog, re.MULTILINE))
    records = json.loads((ROOT / "docs/planv2/contracts/table-disposition.json").read_text(encoding="utf-8"))
    names = [record["current_table"] for record in records]
    assert len(names) == len(set(names))
    assert set(names) == catalog_tables
    for record in records:
        assert record["decision"] in {"control", "runtime", "split", "delete"}
        assert record["authoritative_writer"]
        assert record["allowed_readers"]
        assert record["owning_task"]


def test_claim_lease_catalog_and_source_invariants() -> None:
    audit = json.loads((ROOT / "docs/planv2/contracts/claim-lease-audit.json").read_text(encoding="utf-8"))
    assert audit["apiVersion"] == "agentx.io/claim-lease-audit/v1"
    assert audit["defaults"] == {
        "leaseSeconds": 30,
        "heartbeatSeconds": 10,
        "maxBatchSize": 100,
        "clock": "UTC_TIMESTAMP(6)",
    }
    expected = {
        "control.publisher",
        "control.admission_outbox",
        "control.projector",
        "control.retention",
        "runtime.command",
        "runtime.execution_outbox",
        "runtime.event_sequencer",
        "runtime.trigger",
        "runtime.recovery",
        "runtime.artifact",
        "runtime.quota_leader",
        "runtime.bundle_gc",
        "runtime.retention",
        "runtime.trace_relay",
        "runtime.worker_attempt",
        "runtime.sandbox_reaper",
        "observability.trace_consumer",
    }
    roles = [entry["role"] for entry in audit["entries"]]
    assert len(roles) == len(set(roles))
    assert set(roles) == expected
    control_migration = (ROOT / "migrations/control/0006_horizontal_scalability.sql").read_text(encoding="utf-8")
    runtime_migration = (ROOT / "migrations/runtime/0006_horizontal_scalability.sql").read_text(encoding="utf-8")
    for entry in audit["entries"]:
        assert 1 <= int(entry["batchSize"]) <= 100
        assert entry["externalIoAfterCommit"] is True
        assert (ROOT / entry["source"]).is_file()
        index = entry.get("index", "")
        if index.startswith("idx_v206_"):
            migration = control_migration if entry["authority"].startswith("control_mysql.") else runtime_migration
            assert index in migration
    production_roots = (
        ROOT / "services/platform-control",
        ROOT / "services/agentx-v2-runtime",
        ROOT / "services/observability",
        ROOT / "crates/agentx-mysql-lease",
    )
    for source_path in (path for base in production_roots for path in base.rglob("*.rs")):
        if "tests" in source_path.parts or re.search(r"_tests?\.rs$", source_path.name):
            continue
        source = re.split(r"#\[cfg\(test\)\]", source_path.read_text(encoding="utf-8"), maxsplit=1)[0]
        assert not re.search(
            r"locked_until.{0,160}OffsetDateTime::now_utc\(\)|OffsetDateTime::now_utc\(\).{0,160}locked_until",
            source,
            re.DOTALL,
        )
        for line in source.splitlines():
            if "UPDATE " in line and "locked_by=NULL" in line and "locked_by=?" in line and "fencing_token=?" in line:
                assert "locked_until>UTC_TIMESTAMP(6)" in line


@pytest.mark.skipif(shutil.which("helm") is None, reason="Helm is not installed")
def test_backend_deployments_use_pod_uid_as_instance_id() -> None:
    for target in ("control", "runtime", "observability"):
        for document in yaml.safe_load_all(render("deploy/values/local.yaml", target)):
            if not isinstance(document, dict) or document.get("kind") != "Deployment":
                continue
            if document["metadata"]["name"] == "web-console":
                continue
            containers = document["spec"]["template"]["spec"]["containers"]
            environment = {item["name"] for container in containers for item in container.get("env", [])}
            assert "AGENTX_INSTANCE_ID" in environment


def test_openapi_routes_have_platform_control_handlers() -> None:
    openapi = json.loads((ROOT / "openapi/platform-api.json").read_text(encoding="utf-8"))
    source = "\n".join(
        path.read_text(encoding="utf-8") for path in sorted((ROOT / "services/platform-control/src").rglob("*.rs"))
    )
    route_matches = list(re.finditer(r'\.route\(\s*"(?P<path>/api/v1/[^"\s]+)"', source, re.DOTALL))
    implemented: set[str] = set()
    operations: set[str] = set()

    def normalize(path: str) -> str:
        return re.sub(r"\{[^}]+\}", "{}", path)

    for index, match in enumerate(route_matches):
        path = normalize(match.group("path"))
        implemented.add(path)
        end = route_matches[index + 1].start() if index + 1 < len(route_matches) else len(source)
        segment = source[match.start() : min(end, match.start() + 2000)]
        operations.update(
            f"{method} {path}" for method in re.findall(r"(?<![A-Za-z])(get|post|put|patch|delete)\s*\(", segment)
        )
    aliases = {
        "/api/v1/applications/{}/deployments/{}:rollback": "/api/v1/applications/{}/deployments/{}",
        "/api/v1/applications/{}/publish-attempts/{}:retry": "/api/v1/applications/{}/publish-attempts/{}",
    }
    unresolved: list[str] = []
    for path, item in openapi["paths"].items():
        normalized = normalize(path)
        lookup = aliases.get(normalized, normalized)
        methods = sorted(set(item) & {"get", "post", "put", "patch", "delete"})
        if lookup not in implemented or any(f"{method} {lookup}" not in operations for method in methods):
            unresolved.append(path)
    assert unresolved == []
