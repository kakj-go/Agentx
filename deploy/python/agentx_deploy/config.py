from __future__ import annotations

import copy
import ipaddress
import json
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import jsonschema
import yaml

TARGETS = ("dependencies", "control", "runtime", "observability")
RELEASES = {
    "dependencies": "agentx-dependencies",
    "control": "agentx-control",
    "runtime": "agentx-runtime",
    "observability": "agentx-observability",
}
DNS_LABEL = re.compile(r"^[a-z0-9]([-a-z0-9]*[a-z0-9])?$")


@dataclass(frozen=True)
class DeploymentConfig:
    path: Path
    root: Path
    values: dict[str, Any]

    @property
    def environment(self) -> str:
        return str(self.values["global"]["environment"])

    @property
    def namespaces(self) -> dict[str, str]:
        return dict(self.values["global"]["namespaces"])

    def scoped(self, run_id: str | None) -> DeploymentConfig:
        if not run_id:
            return self
        if self.environment == "production":
            raise ValueError("--run-id is forbidden for production values")
        safe = re.sub(r"[^a-z0-9-]", "-", run_id.lower()).strip("-")
        if not safe or len(safe) > 36:
            raise ValueError("run id must form a non-empty DNS-safe suffix up to 36 characters")
        values = copy.deepcopy(self.values)
        values["global"]["namespaces"] = {
            "control": f"agentx-e2e-control-{safe}",
            "runtime": f"agentx-e2e-runtime-{safe}",
            "dependencies": f"agentx-e2e-deps-{safe}",
        }
        values["global"]["ingress"]["className"] = f"agentx-e2e-{safe}"
        return DeploymentConfig(self.path, self.root, values)


def repository_root(start: Path | None = None) -> Path:
    current = (start or Path(__file__)).resolve()
    for candidate in (current, *current.parents):
        if (candidate / "Cargo.toml").exists() and (candidate / "deploy").is_dir():
            return candidate
    raise RuntimeError("could not locate the Agentx repository root")


def load_values(path: str | Path, *, run_id: str | None = None) -> DeploymentConfig:
    root = repository_root()
    resolved = Path(path)
    if not resolved.is_absolute():
        resolved = root / resolved
    resolved = resolved.resolve()
    if not resolved.is_file():
        raise ValueError(f"values file does not exist: {resolved}")
    raw = yaml.safe_load(resolved.read_text(encoding="utf-8"))
    if not isinstance(raw, dict):
        raise ValueError("deployment values must be a YAML object")
    schema_path = root / "deploy" / "values" / "values.schema.json"
    schema = json.loads(schema_path.read_text(encoding="utf-8"))
    jsonschema.Draft202012Validator(schema).validate(raw)
    config = DeploymentConfig(resolved, root, raw).scoped(run_id)
    validate_semantics(config)
    return config


def validate_semantics(config: DeploymentConfig) -> None:
    namespaces = list(config.namespaces.values())
    if len(set(namespaces)) != 3:
        raise ValueError("control, runtime, and dependencies namespaces must be distinct")
    for namespace in namespaces:
        if len(namespace) > 63 or not DNS_LABEL.fullmatch(namespace):
            raise ValueError(f"invalid Kubernetes namespace: {namespace}")

    global_values = config.values["global"]
    components = global_values["components"]
    if config.environment == "production":
        if global_values["secrets"]["mode"] != "existing-kubernetes":
            raise ValueError("production requires existing-kubernetes secrets")
        if not global_values["ingress"].get("controlTlsSecretName") or not global_values["ingress"].get(
            "runtimeTlsSecretName"
        ):
            raise ValueError("production requires TLS secrets for both ingress hosts")
        workload_secrets = global_values["secrets"].get("workloads", {})
        required_workloads = {
            "platformControl",
            "controlMigration",
            "runtimeGateway",
            "workflowRuntime",
            "workflowWorker",
            "sandboxManager",
            "egressGateway",
            "runtimeMigration",
            "observability",
            "observabilityMigration",
            "controlBackup",
            "runtimeBackup",
            "observabilityBackup",
        }
        missing_workloads = sorted(required_workloads - set(workload_secrets))
        if missing_workloads:
            raise ValueError(f"production workload Secret mappings are missing: {', '.join(missing_workloads)}")
        if "backup" not in global_values:
            raise ValueError("production requires backup RPO/RTO settings")
        if any(
            components[name]["mode"] != "external"
            for name in ("controlMysql", "runtimeMysql", "runtimeRedis", "clickhouse")
        ):
            raise ValueError("production requires external MySQL, Redis, and ClickHouse")
        if components["objectStorage"]["mode"] != "external-s3":
            raise ValueError("production requires external S3")
        if components["objectStorage"]["allowHttp"]:
            raise ValueError("production object storage must use HTTPS")
        for name in ("controlMysql", "runtimeMysql"):
            mysql = components[name]
            if mysql["tlsMode"] != "verify_identity" or not mysql.get("caSecretName"):
                raise ValueError(f"production {name} requires verify_identity and caSecretName")
        if not components["runtimeRedis"]["url"].startswith("rediss://") or not components["runtimeRedis"].get(
            "caSecretName"
        ):
            raise ValueError("production Runtime Redis requires rediss:// and caSecretName")
        secure_endpoints = {
            "objectStorage": components["objectStorage"],
            "secretProvider": components["secretProvider"],
            "sandbox": components["sandbox"],
            "clickhouse": components["clickhouse"],
        }
        for name, component in secure_endpoints.items():
            endpoint = component.get("endpoint", component.get("url", ""))
            if not str(endpoint).startswith("https://") or not component.get("caSecretName"):
                raise ValueError(f"production {name} requires HTTPS and caSecretName")
        if not components["sandbox"]["secureAccess"]:
            raise ValueError("production OpenSandbox requires secure access")
        sandbox_access = global_values["network"]["egressGateway"]["sandboxAccess"]
        if sandbox_access["mode"] != "privateLoadBalancer" or not sandbox_access.get("caSecretName"):
            raise ValueError("production sandbox Egress requires privateLoadBalancer and caSecretName")
        annotations = sandbox_access.get("serviceAnnotations", {})
        private_annotations = {
            "service.beta.kubernetes.io/aws-load-balancer-internal": {"true"},
            "service.beta.kubernetes.io/azure-load-balancer-internal": {"true"},
            "networking.gke.io/load-balancer-type": {"Internal"},
            "cloud.google.com/load-balancer-type": {"Internal"},
        }
        if not any(annotations.get(key) in values for key, values in private_annotations.items()):
            raise ValueError("production sandbox Egress requires a supported internal load balancer annotation")
        digests = global_values["images"].get("digests", {})
        required = set(global_values["images"]["services"])
        if set(digests) != required:
            raise ValueError("production requires an immutable digest for every Agentx image")

    ports = global_values["network"]["egressGateway"]["allowedPublicPorts"]
    if 443 not in ports or len(ports) != len(set(ports)):
        raise ValueError("egress allowedPublicPorts must be unique and include 443")
    for target in global_values["network"].get("externalEgress", {}).values():
        for cidr in target["cidrs"]:
            network = ipaddress.ip_network(cidr, strict=False)
            if network.prefixlen == 0:
                raise ValueError("external egress cannot allow an unrestricted CIDR")


def selected_targets(target: str) -> tuple[str, ...]:
    if target == "all":
        return TARGETS
    if target not in TARGETS:
        raise ValueError(f"unsupported target: {target}")
    return (target,)
