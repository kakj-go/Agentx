from __future__ import annotations

import hashlib
import json
import os
import subprocess
import tempfile
from pathlib import Path
from zipfile import ZipFile

import httpx
import pytest


@pytest.mark.cluster
@pytest.mark.product
def test_downloaded_canvas_plugin_template_builds_outside_repository(
    service_urls: dict[str, str],
    installed_agentx: dict[str, str],
) -> None:
    with httpx.Client(base_url=service_urls["web"], timeout=60) as client:
        status = client.get("/api/v1/bootstrap/status")
        status.raise_for_status()
        if status.json()["required"]:
            login = client.post(
                "/api/v1/bootstrap",
                json={
                    "companyName": "Agentx E2E",
                    "adminUsername": "admin",
                    "adminDisplayName": "Agentx E2E Admin",
                    "password": "agentx-e2e-admin-password",
                    "locale": "zh-CN",
                    "timezone": "Asia/Shanghai",
                },
            )
        else:
            login = client.post(
                "/api/v1/auth/login",
                json={"username": "admin", "password": "agentx-e2e-admin-password"},
            )
        login.raise_for_status()
        response = client.get(
            "/api/v1/canvas-plugin-sdk/template",
            headers={"Authorization": f"Bearer {login.json()['accessToken']}"},
        )
        response.raise_for_status()

    pnpm = ("corepack.cmd", "pnpm") if os.name == "nt" else ("corepack", "pnpm")
    with tempfile.TemporaryDirectory(prefix="agentx-plugin-template-") as directory:
        root = Path(directory)
        archive = root / "template.zip"
        archive.write_bytes(response.content)
        project = root / "project"
        project.mkdir()
        with ZipFile(archive) as package:
            package.extractall(project)
        assert (project / "AGENTS.md").is_file()
        assert (project / "vendor" / "plugin-runner" / "runner.mjs").is_file()
        manifest_path = project / "manifest.json"
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        manifest.update(
            packageId="acme/ai-variant",
            packageVersion="1.1.0",
            displayName="AI Variant Mapper",
        )
        manifest["traceRenderers"][0]["contentType"] = "acme.ai-variant/summary"
        manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        node_path = project / "nodes" / "json_mapper.json"
        node = json.loads(node_path.read_text(encoding="utf-8"))
        node.update(nodeType="acme.ai_variant", displayName="AI Variant Mapper")
        node_path.write_text(json.dumps(node, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        runtime_path = project / "src" / "runtime" / "entry.ts"
        runtime = runtime_path.read_text(encoding="utf-8")
        runtime = runtime.replace("acme.json-mapper/summary", "acme.ai-variant/summary")
        runtime = runtime.replace(
            "const outputs = (context.inputs.main ?? []).map(",
            "const outputs = (context.inputs.main ?? []).filter((item) => (item.json as Record<string, Json>).enabled !== false).map(",
        )
        runtime = runtime.replace(
            "label: context.parameters.label",
            "label: context.parameters.label, variant: true",
        )
        runtime_path.write_text(runtime, encoding="utf-8")
        runtime_test_path = project / "tests" / "runtime.test.mjs"
        runtime_test = runtime_test_path.read_text(encoding="utf-8")
        runtime_test = runtime_test.replace("acme.json_mapper", "acme.ai_variant")
        runtime_test = runtime_test.replace("acme/json-mapper", "acme/ai-variant")
        runtime_test = runtime_test.replace("acme.json-mapper", "acme.ai-variant")
        runtime_test = runtime_test.replace("packageVersion: '1.0.0'", "packageVersion: '1.1.0'")
        runtime_test = runtime_test.replace(
            "{ id: 1, label: 'customer' }",
            "{ id: 1, label: 'customer', variant: true }",
        )
        runtime_test_path.write_text(runtime_test, encoding="utf-8")
        ui_path = project / "src" / "ui" / "entry.ts"
        ui_path.write_text(
            ui_path.read_text(encoding="utf-8").replace("Label: ${", "Variant label: ${"), encoding="utf-8"
        )
        for command in (
            (*pnpm, "install", "--frozen-lockfile"),
            (*pnpm, "check"),
            (*pnpm, "test"),
            (*pnpm, "build"),
            (*pnpm, "pack:plugin"),
        ):
            subprocess.run(command, cwd=project, check=True, shell=False, timeout=180)
        output = project / "dist" / "acme-json-mapper.agentx-plugin"
        assert output.stat().st_size > 1_000
        digest = hashlib.sha256(output.read_bytes()).hexdigest()
        subprocess.run((*pnpm, "pack:plugin"), cwd=project, check=True, shell=False, timeout=180)
        assert hashlib.sha256(output.read_bytes()).hexdigest() == digest
        lock = (project / "pnpm-lock.yaml").read_text(encoding="utf-8")
        assert "workspace:" not in lock
        assert str(Path(__file__).resolve().parents[3]) not in lock
        environment = {
            **os.environ,
            "AGENTX_E2E_BASE_URL": service_urls["web"],
            "AGENTX_E2E_STAGE": "helm-agentxctl",
            "AGENTX_E2E_RUN_ID": installed_agentx["run_id"],
            "AGENTX_E2E_SUITE": "canvas-plugin-template-variant",
            "AGENTX_PLUGIN_VARIANT_PATH": str(output),
            "AGENTX_PLUGIN_VARIANT_PACKAGE_ID": "acme/ai-variant",
            "AGENTX_PLUGIN_VARIANT_NODE_TYPE": "acme.ai_variant",
            "AGENTX_PLUGIN_VARIANT_DISPLAY_NAME": "AI Variant Mapper",
        }
        subprocess.run(
            (
                *pnpm,
                "--filter",
                "@agentx/e2e",
                "exec",
                "playwright",
                "test",
                "tests/canvas-plugin-template-variant.spec.ts",
            ),
            cwd=installed_agentx["root"],
            check=True,
            shell=False,
            env=environment,
            timeout=600,
        )
