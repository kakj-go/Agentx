from __future__ import annotations

import subprocess
import uuid
from datetime import UTC, datetime, timedelta

import httpx
import pytest

from tests.e2e.support import run


@pytest.mark.cluster
@pytest.mark.runtime
def test_runtime_workloads_are_available(installed_agentx: dict[str, str]) -> None:
    namespace = installed_agentx["runtime_namespace"]
    for name in ("runtime-gateway", "workflow-runtime", "workflow-worker", "sandbox-manager"):
        subprocess.run(
            ["kubectl", "-n", namespace, "rollout", "status", f"deployment/{name}", "--timeout=300s"],
            check=True,
            shell=False,
        )


def _runtime_mysql(installed_agentx: dict[str, str], query: str) -> str:
    return run(
        (
            "kubectl",
            "-n",
            installed_agentx["runtime_namespace"],
            "exec",
            "statefulset/runtime-mysql",
            "--",
            "sh",
            "-ec",
            'MYSQL_PWD="$(cat /run/secrets/agentx/root-password)" mysql --ssl-mode=DISABLED '
            '--batch --skip-column-names -uroot agentx_runtime -e "$1"',
            "agentx-p3-04-query",
            query,
        ),
        timeout=60,
    ).stdout.strip()


def _deadline(seconds: int = 120) -> str:
    return (datetime.now(UTC) + timedelta(seconds=seconds)).isoformat().replace("+00:00", "Z")


def _proof(ids: dict[str, str], lease: dict[str, object], suffix: str) -> dict[str, object]:
    return {
        "apiVersion": 1,
        "tenantId": ids["tenant"],
        "executionId": ids["execution"],
        "nodeExecutionId": ids["node_execution"],
        "attemptId": ids["attempt"],
        "agentRunId": ids["agent_run"],
        "workerId": ids["worker"],
        "fencingToken": 1,
        "processSessionId": lease["processSessionId"],
        "leaseId": lease["leaseId"],
        "operationId": f"p3-04:{suffix}",
        "effectId": f"p3-04:{suffix}:effect",
        "idempotencyKey": f"p3-04:{ids['attempt']}:{suffix}",
        "deadline": _deadline(),
    }


def _mcp_process_source() -> str:
    return """import json,sys
sys.stderr.write('p3-04 diagnostic only\\n');sys.stderr.flush()
for line in sys.stdin:
    request=json.loads(line)
    method=request.get('method')
    if method=='initialize':
        result={'protocolVersion':'2025-03-26','capabilities':{'tools':{}},'serverInfo':{'name':'p3-04-stdio','version':'1'}}
    elif method=='tools/list':
        result={'tools':[{'name':'echo','description':'echo input','inputSchema':{'type':'object','properties':{'text':{'type':'string'}},'required':['text']}},{'name':'large','description':'large artifact','inputSchema':{'type':'object'}}]}
    elif method=='tools/call':
        arguments=request.get('params',{}).get('arguments',{})
        if request.get('params',{}).get('name')=='large':
            result={'content':[{'type':'text','text':'x'*70000}]}
        else:
            result={'content':[{'type':'text','text':arguments.get('text','')}]}
    else:
        continue
    sys.stdout.write(json.dumps({'jsonrpc':'2.0','id':request['id'],'result':result},separators=(',',':'))+'\\n');sys.stdout.flush()
"""


@pytest.mark.cluster
@pytest.mark.runtime
def test_stdio_mcp_process_session_uses_real_opensandbox_and_cleans_up(
    installed_agentx: dict[str, str], service_urls: dict[str, str]
) -> None:
    ids = {
        key: str(uuid.uuid4())
        for key in (
            "tenant",
            "workflow",
            "workflow_version",
            "execution",
            "trace",
            "node_execution",
            "attempt",
            "worker",
            "agent_run",
            "mcp_server_version",
            "sandbox_profile",
            "sandbox_profile_version",
        )
    }
    _runtime_mysql(
        installed_agentx,
        "INSERT INTO workflow_executions(id,tenant_id,workflow_id,workflow_version_id,trace_id,trigger_type,status,started_at) VALUES"
        f"(UUID_TO_BIN('{ids['execution']}'),UUID_TO_BIN('{ids['tenant']}'),UUID_TO_BIN('{ids['workflow']}'),"
        f"UUID_TO_BIN('{ids['workflow_version']}'),UUID_TO_BIN('{ids['trace']}'),'debug','running',UTC_TIMESTAMP(6));"
        "INSERT INTO node_executions(id,tenant_id,execution_id,node_id,node_key,node_name,node_type,node_version,generation,activation_slot,run_index,status,capability) VALUES"
        f"(UUID_TO_BIN('{ids['node_execution']}'),UUID_TO_BIN('{ids['tenant']}'),UUID_TO_BIN('{ids['execution']}'),"
        "'p3-04-stdio','p3_04_stdio','P3-04 stdio MCP','agent',2,0,0,0,'running','agent');"
        "INSERT INTO node_attempts(id,tenant_id,execution_id,node_execution_id,attempt_number,capability,compiler_version,manifest_version,status,idempotency_key,worker_instance_id,fencing_token,locked_until,deadline_at,started_at) VALUES"
        f"(UUID_TO_BIN('{ids['attempt']}'),UUID_TO_BIN('{ids['tenant']}'),UUID_TO_BIN('{ids['execution']}'),"
        f"UUID_TO_BIN('{ids['node_execution']}'),1,'agent','p3-04-e2e','2.0','running','p3-04:{ids['attempt']}','{ids['worker']}',1,"
        "DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 5 MINUTE),DATE_ADD(UTC_TIMESTAMP(6),INTERVAL 5 MINUTE),UTC_TIMESTAMP(6));",
    )
    manager = service_urls["sandbox_manager"]
    start = {
        "apiVersion": 1,
        "identity": {
            "tenantId": ids["tenant"],
            "agentRunId": ids["agent_run"],
            "mcpServerVersionId": ids["mcp_server_version"],
            "sandboxProfileVersionId": ids["sandbox_profile_version"],
        },
        "executionId": ids["execution"],
        "nodeExecutionId": ids["node_execution"],
        "attemptId": ids["attempt"],
        "workerId": ids["worker"],
        "fencingToken": 1,
        "operationId": "p3-04:start",
        "effectId": "p3-04:start:effect",
        "idempotencyKey": f"p3-04:{ids['attempt']}:start",
        "command": "/usr/bin/python3",
        "args": ["-u", "-c", _mcp_process_source()],
        "environmentCredentials": [],
        "profile": {
            "resourceKind": "sandbox_profile",
            "resourceId": ids["sandbox_profile"],
            "resourceVersion": ids["sandbox_profile_version"],
            "stateEpoch": 1,
            "contentHash": "sha256:" + "0" * 64,
            "configuration": {
                "kind": "sandbox_profile",
                "provider": "opensandbox",
                "image": "opensandbox/code-interpreter@sha256:64cd01f03f54ba347d1a1310dcbc18ac5cb17d01714e23b4ea4b840fbb0d6623",
                "cpuMillis": 500,
                "memoryBytes": 536870912,
                "diskBytes": 1073741824,
                "pidLimit": 256,
                "egressMode": "none",
                "maximumTtlSeconds": 120,
            },
            "objectIds": [],
        },
        "deadline": _deadline(180),
    }
    lease: dict[str, object] | None = None
    try:
        response = httpx.post(
            f"{manager}/internal/runtime/v1/sandbox-process-sessions:start",
            json=start,
            timeout=180,
        )
        assert response.status_code == 200, response.text
        started = response.json()
        lease = started["lease"]
        assert lease["status"] == "running"
        assert started["providerSandboxId"]

        def write(
            suffix: str, frame: dict[str, object], replay_policy: str = "safe"
        ) -> dict[str, object]:
            result = httpx.post(
                f"{manager}/internal/runtime/v1/sandbox-process-sessions/{lease['processSessionId']}:write",
                json={
                    **_proof(ids, lease, suffix),
                    "frame": frame,
                    "replayPolicy": replay_policy,
                },
                timeout=30,
            )
            result.raise_for_status()
            return result.json()

        initialize = write(
            "initialize",
            {
                "jsonrpc": "2.0",
                "id": "initialize-1",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": {"name": "agentx-e2e", "version": "1"},
                },
            },
        )
        assert any(frame["payload"].get("id") == "initialize-1" for frame in initialize["frames"])
        write("initialized", {"jsonrpc": "2.0", "method": "notifications/initialized"})
        tools = write("tools-list", {"jsonrpc": "2.0", "id": "tools-1", "method": "tools/list", "params": {}})
        tools_payload = next(frame["payload"] for frame in tools["frames"] if frame["payload"].get("id") == "tools-1")
        assert [tool["name"] for tool in tools_payload["result"]["tools"]] == ["echo", "large"]
        called = write(
            "tools-call",
            {
                "jsonrpc": "2.0",
                "id": "call-1",
                "method": "tools/call",
                "params": {"name": "echo", "arguments": {"text": "p3-04-opensandbox"}},
            },
        )
        call_payload = next(frame["payload"] for frame in called["frames"] if frame["payload"].get("id") == "call-1")
        assert call_payload["result"]["content"][0]["text"] == "p3-04-opensandbox"
        large = write(
            "large-call",
            {"jsonrpc": "2.0", "id": "large-1", "method": "tools/call", "params": {"name": "large", "arguments": {}}},
        )
        large_frame = next(frame for frame in large["frames"] if frame["payload"].get("id") == "large-1")
        assert large_frame["truncated"] is True
        assert len(large_frame["payload"]["artifactRefs"]) == 1

        read = httpx.post(
            f"{manager}/internal/runtime/v1/sandbox-process-sessions/{lease['processSessionId']}:read",
            json={**_proof(ids, lease, "read"), "afterSequence": 0, "maximumFrames": 100, "waitMillis": 100},
            timeout=30,
        )
        read.raise_for_status()
        frames = read.json()["frames"]
        assert any(frame["stream"] == "stderr" and frame["payload"].get("diagnostic") is True for frame in frames)
        assert all(frame["stream"] != "stderr" or "jsonrpc" not in frame["payload"] for frame in frames)
    finally:
        if lease is not None:
            terminate = httpx.post(
                f"{manager}/internal/runtime/v1/sandbox-process-sessions/{lease['processSessionId']}:terminate",
                json=_proof(ids, lease, "terminate"),
                timeout=60,
            )
            assert terminate.status_code == 200, terminate.text
        _runtime_mysql(
            installed_agentx,
            f"DELETE FROM artifact_references WHERE tenant_id=UUID_TO_BIN('{ids['tenant']}');"  # noqa: S608 -- UUID is generated by this test.
            f"DELETE FROM runtime_objects WHERE tenant_id=UUID_TO_BIN('{ids['tenant']}');"
            f"DELETE FROM sandbox_process_frames WHERE process_session_id IN (SELECT process_session_id FROM sandbox_process_sessions WHERE tenant_id=UUID_TO_BIN('{ids['tenant']}'));"
            f"DELETE FROM sandbox_process_sessions WHERE tenant_id=UUID_TO_BIN('{ids['tenant']}');"
            f"DELETE FROM node_attempts WHERE id=UUID_TO_BIN('{ids['attempt']}');"
            f"DELETE FROM node_executions WHERE id=UUID_TO_BIN('{ids['node_execution']}');"
            f"DELETE FROM workflow_executions WHERE id=UUID_TO_BIN('{ids['execution']}');",
        )
