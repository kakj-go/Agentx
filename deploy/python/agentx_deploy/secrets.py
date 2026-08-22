from __future__ import annotations

import base64
import json
import secrets as random
import uuid
from collections.abc import Iterable
from datetime import UTC, datetime, timedelta
from typing import Any

import yaml
from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ed25519, rsa
from cryptography.x509.oid import NameOID

from agentx_deploy.config import DeploymentConfig
from agentx_deploy.process import run


def _password() -> str:
    return random.token_urlsafe(36)


def _rsa_pair() -> tuple[str, str]:
    private = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    private_pem = private.private_bytes(
        serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()
    ).decode()
    public_pem = (
        private.public_key()
        .public_bytes(serialization.Encoding.PEM, serialization.PublicFormat.SubjectPublicKeyInfo)
        .decode()
    )
    return private_pem, public_pem


def _ed25519_pair() -> tuple[str, str]:
    private = ed25519.Ed25519PrivateKey.generate()
    private_pem = private.private_bytes(
        serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()
    ).decode()
    public_pem = (
        private.public_key()
        .public_bytes(serialization.Encoding.PEM, serialization.PublicFormat.SubjectPublicKeyInfo)
        .decode()
    )
    return private_pem, public_pem


def _certificate() -> tuple[str, str]:
    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "agentx-egress-gateway")])
    now = datetime.now(UTC)
    cert = (
        x509.CertificateBuilder()
        .subject_name(name)
        .issuer_name(name)
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - timedelta(minutes=5))
        .not_valid_after(now + timedelta(days=3650))
        .add_extension(x509.SubjectAlternativeName([x509.DNSName("agentx-egress-gateway")]), critical=False)
        .sign(key, hashes.SHA256())
    )
    private = key.private_bytes(
        serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()
    ).decode()
    return private, cert.public_bytes(serialization.Encoding.PEM).decode()


def _get_secret(namespace: str, name: str) -> dict[str, str] | None:
    result = run(("kubectl", "-n", namespace, "get", "secret", name, "-o", "json"), check=False, timeout=30)
    if result.returncode != 0:
        return None
    payload = json.loads(result.stdout)
    return {key: base64.b64decode(value).decode() for key, value in payload.get("data", {}).items()}


def _apply_secret(namespace: str, name: str, data: dict[str, str]) -> None:
    payload = {
        "apiVersion": "v1",
        "kind": "Secret",
        "metadata": {"name": name, "namespace": namespace, "labels": {"app.kubernetes.io/managed-by": "agentx-deploy"}},
        "type": "Opaque",
        "stringData": data,
    }
    run(("kubectl", "apply", "-f", "-"), input_text=json.dumps(payload), timeout=60)


def ensure_local_secrets(config: DeploymentConfig, targets: tuple[str, ...] | None = None) -> None:
    selected = set(targets or ("dependencies", "control", "runtime", "observability"))
    secret_config = config.values["global"]["secrets"]
    if secret_config["mode"] != "generated-local":
        ensure_existing_secrets(config)
        return
    namespaces = config.namespaces
    canonical_name = secret_config["dependencies"]
    existing = _get_secret(namespaces["dependencies"], canonical_name)
    if existing:
        publish_mirrors(config, existing, selected)
        return
    if "dependencies" not in selected:
        raise RuntimeError("generated-local targets require an installed authoritative Dependencies Secret")

    service_private, service_public = _rsa_pair()
    projector_private, projector_public = _rsa_pair()
    bff_private, bff_public = _rsa_pair()
    user_private, user_public = _rsa_pair()
    bundle_private, bundle_public = _ed25519_pair()
    work_private, work_public = _ed25519_pair()
    egress_private: dict[str, str] = {}
    egress_public: dict[str, str] = {}
    for role in ("runtime-gateway", "workflow-runtime", "workflow-worker", "sandbox-manager"):
        egress_private[role], egress_public[role] = _rsa_pair()
    tls_private, tls_cert = _certificate()
    canonical = {
        "MINIO_ROOT_USER": "agentx_admin",
        "MINIO_ROOT_PASSWORD": _password(),
        "CONTROL_OBJECT_PASSWORD": _password(),
        "RUNTIME_OBJECT_PASSWORD": _password(),
        "OBSERVABILITY_OBJECT_PASSWORD": _password(),
        "VAULT_DEV_ROOT_TOKEN_ID": _password(),
        "CONTROL_VAULT_TOKEN": _password(),
        "RUNTIME_VAULT_TOKEN": _password(),
        "OBSERVABILITY_REDIS_PASSWORD": _password(),
        "AGENTX_RUNTIME_REDIS_PASSWORD": _password(),
        "AGENTX_CONTROL_MYSQL_PASSWORD": _password(),
        "AGENTX_CONTROL_MYSQL_MIGRATE_PASSWORD": _password(),
        "AGENTX_CONTROL_MYSQL_ROOT_PASSWORD": _password(),
        "AGENTX_CONTROL_API_REFRESH_JWT_SIGNING_SECRET": _password(),
        "AGENTX_RUNTIME_MYSQL_PASSWORD": _password(),
        "AGENTX_RUNTIME_MYSQL_MIGRATE_PASSWORD": _password(),
        "AGENTX_RUNTIME_MYSQL_ROOT_PASSWORD": _password(),
        "AGENTX_CLICKHOUSE_QUERY_PASSWORD": _password(),
        "AGENTX_CLICKHOUSE_CONSUMER_PASSWORD": _password(),
        "AGENTX_CLICKHOUSE_MIGRATE_PASSWORD": _password(),
        "AGENTX_CONTROL_PUBLISHER_JWT_KID": "publisher-current",
        "AGENTX_CONTROL_PROJECTOR_JWT_KID": "projector-current",
        "AGENTX_CONTROL_BFF_JWT_KID": "bff-current",
        "AGENTX_CONTROL_USER_JWT_KID": "user-current",
        "AGENTX_CONTROL_BUNDLE_KEY_ID": "bundle-current",
        "AGENTX_CONTROL_WORK_PACKAGE_KEY_ID": "work-package-current",
        "AGENTX_CONTROL_PUBLISHER_JWT_PRIVATE_KEY_PEM": service_private,
        "AGENTX_CONTROL_PROJECTOR_JWT_PRIVATE_KEY_PEM": projector_private,
        "AGENTX_CONTROL_BFF_JWT_PRIVATE_KEY_PEM": bff_private,
        "AGENTX_CONTROL_USER_JWT_PRIVATE_KEY_PEM": user_private,
        "AGENTX_RUNTIME_SERVICE_JWT_PUBLIC_KEYS_JSON": json.dumps(
            {"publisher-current": service_public, "projector-current": projector_public}
        ),
        "AGENTX_OBSERVABILITY_BFF_JWT_PUBLIC_KEYS_JSON": json.dumps({"bff-current": bff_public}),
        "AGENTX_RUNTIME_USER_JWT_PUBLIC_KEYS_JSON": json.dumps({"user-current": user_public}),
        "AGENTX_CONTROL_BUNDLE_ED25519_PRIVATE_KEY_PEM": bundle_private,
        "AGENTX_RUNTIME_BUNDLE_PUBLIC_KEYS_JSON": json.dumps({"bundle-current": bundle_public}),
        "AGENTX_CONTROL_WORK_PACKAGE_ED25519_PRIVATE_KEY_PEM": work_private,
        "AGENTX_RUNTIME_WORK_PACKAGE_PUBLIC_KEYS_JSON": json.dumps({"work-package-current": work_public}),
        "AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM": egress_private["runtime-gateway"],
        "AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_KEY_ID": "runtime-gateway-current",
        "AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM": egress_private["workflow-runtime"],
        "AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_KEY_ID": "workflow-runtime-current",
        "AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM": egress_private["workflow-worker"],
        "AGENTX_WORKFLOW_WORKER_EGRESS_JWT_KEY_ID": "workflow-worker-current",
        "AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM": egress_private["sandbox-manager"],
        "AGENTX_SANDBOX_EGRESS_JWT_KEY_ID": "sandbox-manager-current",
        "AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON": json.dumps(
            {f"{role}-current": public for role, public in egress_public.items()}
        ),
        "AGENTX_EGRESS_TLS_CERTIFICATE_PEM": tls_cert,
        "AGENTX_EGRESS_TLS_PRIVATE_KEY_PEM": tls_private,
    }
    _apply_secret(namespaces["dependencies"], canonical_name, canonical)
    publish_mirrors(config, canonical, selected)


def publish_mirrors(config: DeploymentConfig, canonical: dict[str, str], targets: set[str]) -> None:
    namespaces = config.namespaces
    secret_config = config.values["global"]["secrets"]
    control = {
        "AGENTX_CONTROL_MYSQL_PASSWORD": canonical.get("AGENTX_CONTROL_MYSQL_PASSWORD", _password()),
        "AGENTX_CONTROL_MYSQL_MIGRATE_PASSWORD": canonical.get("AGENTX_CONTROL_MYSQL_MIGRATE_PASSWORD", _password()),
        "AGENTX_CONTROL_MYSQL_ROOT_PASSWORD": canonical.get("AGENTX_CONTROL_MYSQL_ROOT_PASSWORD", _password()),
        "AGENTX_CONTROL_S3_ACCESS_KEY": config.values["global"]["components"]["objectStorage"]["domains"]["control"][
            "user"
        ],
        "AGENTX_CONTROL_S3_SECRET_KEY": canonical["CONTROL_OBJECT_PASSWORD"],
        "AGENTX_CONTROL_API_REFRESH_JWT_SIGNING_SECRET": canonical.get(
            "AGENTX_CONTROL_API_REFRESH_JWT_SIGNING_SECRET", _password()
        ),
        "AGENTX_CONTROL_VAULT_TOKEN": canonical["CONTROL_VAULT_TOKEN"],
    }
    for key, value in canonical.items():
        if key.startswith("AGENTX_CONTROL_"):
            control[key] = value
    runtime = {
        "AGENTX_RUNTIME_MYSQL_PASSWORD": canonical.get("AGENTX_RUNTIME_MYSQL_PASSWORD", _password()),
        "AGENTX_RUNTIME_MYSQL_MIGRATE_PASSWORD": canonical.get("AGENTX_RUNTIME_MYSQL_MIGRATE_PASSWORD", _password()),
        "AGENTX_RUNTIME_MYSQL_ROOT_PASSWORD": canonical.get("AGENTX_RUNTIME_MYSQL_ROOT_PASSWORD", _password()),
        "AGENTX_RUNTIME_REDIS_PASSWORD": canonical["AGENTX_RUNTIME_REDIS_PASSWORD"],
        "AGENTX_RUNTIME_REDIS_ACL_FILE": f"user default on >{canonical['AGENTX_RUNTIME_REDIS_PASSWORD']} ~* &agentx:v2:invocation:wakeup:* +@all\nuser observability on >{canonical['OBSERVABILITY_REDIS_PASSWORD']} ~agentx:v2:trace:v1 ~agentx:v2:observability:jti:* +ping +xgroup +xreadgroup +xpending +xautoclaim +xack +set +get +del +exists",
        "AGENTX_RUNTIME_S3_ACCESS_KEY": config.values["global"]["components"]["objectStorage"]["domains"]["runtime"][
            "user"
        ],
        "AGENTX_RUNTIME_S3_SECRET_KEY": canonical["RUNTIME_OBJECT_PASSWORD"],
        "AGENTX_RUNTIME_VAULT_TOKEN": canonical["RUNTIME_VAULT_TOKEN"],
        "AGENTX_OPENSANDBOX_API_KEY": "agentx-local-opensandbox-key",
    }
    for key, value in canonical.items():
        if key.startswith("AGENTX_RUNTIME_") or key.startswith("AGENTX_WORKFLOW_") or key.startswith("AGENTX_SANDBOX_"):
            runtime[key] = value
    observability = {
        "AGENTX_CLICKHOUSE_QUERY_PASSWORD": canonical.get("AGENTX_CLICKHOUSE_QUERY_PASSWORD", _password()),
        "AGENTX_CLICKHOUSE_CONSUMER_PASSWORD": canonical.get("AGENTX_CLICKHOUSE_CONSUMER_PASSWORD", _password()),
        "AGENTX_CLICKHOUSE_MIGRATE_PASSWORD": canonical.get("AGENTX_CLICKHOUSE_MIGRATE_PASSWORD", _password()),
        "AGENTX_OBSERVABILITY_REDIS_PASSWORD": canonical["OBSERVABILITY_REDIS_PASSWORD"],
        "AGENTX_OBSERVABILITY_BFF_JWT_PUBLIC_KEYS_JSON": canonical["AGENTX_OBSERVABILITY_BFF_JWT_PUBLIC_KEYS_JSON"],
        "AGENTX_OBSERVABILITY_S3_ACCESS_KEY": config.values["global"]["components"]["objectStorage"]["domains"][
            "observability"
        ]["user"],
        "AGENTX_OBSERVABILITY_S3_SECRET_KEY": canonical["OBSERVABILITY_OBJECT_PASSWORD"],
    }
    if "control" in targets:
        _apply_secret(namespaces["control"], secret_config["control"], control)
    if "runtime" in targets:
        _apply_secret(namespaces["runtime"], secret_config["runtime"], runtime)
    if "observability" in targets:
        _apply_secret(namespaces["runtime"], secret_config["observability"], observability)
    tls_name = config.values["global"]["network"]["egressGateway"]["sandboxAccess"]["tlsSecretName"]
    if "dependencies" in targets:
        _apply_secret(
            namespaces["dependencies"],
            "agentx-egress-gateway-secrets",
            {"AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON": canonical["AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON"]},
        )
        _apply_secret(
            namespaces["dependencies"],
            tls_name,
            {
                "tls.crt": canonical["AGENTX_EGRESS_TLS_CERTIFICATE_PEM"],
                "tls.key": canonical["AGENTX_EGRESS_TLS_PRIVATE_KEY_PEM"],
                "ca.crt": canonical["AGENTX_EGRESS_TLS_CERTIFICATE_PEM"],
            },
        )
    if "runtime" in targets:
        ca_name = config.values["global"]["network"]["egressGateway"]["sandboxAccess"]["caSecretName"]
        _apply_secret(
            namespaces["runtime"],
            ca_name,
            {"ca.crt": canonical["AGENTX_EGRESS_TLS_CERTIFICATE_PEM"]},
        )


def ensure_existing_secrets(config: DeploymentConfig) -> None:
    secret_config = config.values["global"]["secrets"]
    for plane, namespace_key in (
        ("control", "control"),
        ("runtime", "runtime"),
        ("observability", "runtime"),
        ("dependencies", "dependencies"),
    ):
        name = secret_config[plane]
        if _get_secret(config.namespaces[namespace_key], name) is None:
            raise RuntimeError(f"required existing secret is missing: {config.namespaces[namespace_key]}/{name}")


def ensure_existing_secret_references(config: DeploymentConfig, manifests: Iterable[str]) -> dict[str, object]:
    if config.values["global"]["secrets"]["mode"] != "existing-kubernetes":
        return {"status": "not-required", "secrets": 0}

    required: dict[tuple[str, str], set[str]] = {}
    canonical = config.values["global"]["secrets"]["dependencies"]
    required[(config.namespaces["dependencies"], canonical)] = set()

    def add(namespace: str, name: object, key: object | None = None) -> None:
        if not isinstance(name, str) or not name:
            return
        keys = required.setdefault((namespace, name), set())
        if isinstance(key, str) and key:
            keys.add(key)

    def inspect(node: object, namespace: str) -> None:
        if isinstance(node, list):
            for item in node:
                inspect(item, namespace)
            return
        if not isinstance(node, dict):
            return
        secret_key_ref = node.get("secretKeyRef")
        if isinstance(secret_key_ref, dict) and not secret_key_ref.get("optional", False):
            add(namespace, secret_key_ref.get("name"), secret_key_ref.get("key"))
        secret_ref = node.get("secretRef")
        if isinstance(secret_ref, dict) and not secret_ref.get("optional", False):
            add(namespace, secret_ref.get("name"))
        secret_volume = node.get("secret")
        if isinstance(secret_volume, dict) and not secret_volume.get("optional", False):
            name = secret_volume.get("secretName")
            items = secret_volume.get("items", [])
            add(namespace, name)
            if isinstance(items, list):
                for item in items:
                    if isinstance(item, dict):
                        add(namespace, name, item.get("key"))
        for image_pull_secret in node.get("imagePullSecrets") or []:
            if isinstance(image_pull_secret, dict):
                add(namespace, image_pull_secret.get("name"))
        for entry in node.get("tls") or []:
            if isinstance(entry, dict):
                add(namespace, entry.get("secretName"))
        for value in node.values():
            inspect(value, namespace)

    for manifest in manifests:
        for document in yaml.safe_load_all(manifest):
            if not isinstance(document, dict):
                continue
            metadata = document.get("metadata", {})
            namespace = metadata.get("namespace") if isinstance(metadata, dict) else None
            if isinstance(namespace, str) and namespace:
                inspect(document, namespace)

    missing: list[str] = []
    for (namespace, name), keys in sorted(required.items()):
        result = run(("kubectl", "-n", namespace, "get", "secret", name, "-o", "json"), check=False, timeout=30)
        if result.returncode != 0:
            missing.append(f"{namespace}/{name}")
            continue
        payload: dict[str, Any] = json.loads(result.stdout)
        present = set(payload.get("data", {}))
        for key in sorted(keys - present):
            missing.append(f"{namespace}/{name}:{key}")
    if missing:
        raise RuntimeError(f"required existing Secret data is missing: {', '.join(missing)}")
    return {"status": "ready", "secrets": len(required)}


def sync_existing_mirrors(config: DeploymentConfig) -> list[str]:
    secret_config = config.values["global"]["secrets"]
    if secret_config["mode"] != "existing-kubernetes":
        ensure_local_secrets(config)
        return [secret_config["control"], secret_config["runtime"], secret_config["observability"]]
    canonical = _get_secret(config.namespaces["dependencies"], secret_config["dependencies"])
    if not canonical:
        raise RuntimeError("the authoritative Dependencies Secret is missing or empty")
    workload_namespaces = {
        "platformControl": "control",
        "controlMigration": "control",
        "controlBackup": "control",
        "runtimeGateway": "runtime",
        "workflowRuntime": "runtime",
        "workflowWorker": "runtime",
        "sandboxManager": "runtime",
        "runtimeMigration": "runtime",
        "runtimeBackup": "runtime",
        "observability": "runtime",
        "observabilityMigration": "runtime",
        "observabilityBackup": "runtime",
        "egressGateway": "dependencies",
    }
    updated: list[str] = []
    for field, name in secret_config.get("workloads", {}).items():
        namespace_key = workload_namespaces.get(field)
        if namespace_key is None:
            continue
        namespace = config.namespaces[namespace_key]
        mirror = _get_secret(namespace, name)
        if mirror is None:
            raise RuntimeError(f"workload Secret is missing: {namespace}/{name}")
        shared = set(mirror) & set(canonical)
        if shared:
            mirror.update({key: canonical[key] for key in shared})
            _apply_secret(namespace, name, mirror)
            updated.append(f"{namespace}/{name}")
    return updated


def rotate_egress_keys(config: DeploymentConfig, *, apply: bool) -> dict[str, object]:
    namespaces = config.namespaces
    secret_config = config.values["global"]["secrets"]
    workload_secrets = secret_config.get("workloads", {})
    gateway_name = workload_secrets.get("egressGateway", "agentx-egress-gateway-secrets")
    roles = {
        "runtime-gateway": (
            "runtimeGateway",
            "AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM",
            "AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_KEY_ID",
        ),
        "workflow-runtime": (
            "workflowRuntime",
            "AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM",
            "AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_KEY_ID",
        ),
        "workflow-worker": (
            "workflowWorker",
            "AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM",
            "AGENTX_WORKFLOW_WORKER_EGRESS_JWT_KEY_ID",
        ),
        "sandbox-manager": (
            "sandboxManager",
            "AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM",
            "AGENTX_SANDBOX_EGRESS_JWT_KEY_ID",
        ),
    }
    caller_secret_names = {
        deployment: workload_secrets.get(field, secret_config["runtime"]) for deployment, (field, _, _) in roles.items()
    }
    canonical = _get_secret(namespaces["dependencies"], secret_config["dependencies"])
    gateway = _get_secret(namespaces["dependencies"], gateway_name)
    caller_secrets = {
        name: _get_secret(namespaces["runtime"], name) for name in sorted(set(caller_secret_names.values()))
    }
    if not canonical or not gateway or any(secret is None for secret in caller_secrets.values()):
        raise RuntimeError("canonical, caller, and gateway secrets must exist before key rotation")
    required = {
        "AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON",
        "AGENTX_RUNTIME_GATEWAY_EGRESS_JWT_PRIVATE_KEY_PEM",
        "AGENTX_WORKFLOW_RUNTIME_EGRESS_JWT_PRIVATE_KEY_PEM",
        "AGENTX_WORKFLOW_WORKER_EGRESS_JWT_PRIVATE_KEY_PEM",
        "AGENTX_SANDBOX_EGRESS_JWT_PRIVATE_KEY_PEM",
    }
    missing = sorted(required - canonical.keys())
    if missing:
        raise RuntimeError(f"canonical secret is missing Egress key material: {', '.join(missing)}")
    phases = ["publish-overlap", "restart-gateway", "roll-callers", "remove-previous", "commit-canonical"]
    if not apply:
        public_keys = json.loads(canonical["AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON"])
        return {"status": "ready", "activeKids": sorted(public_keys), "phases": phases}

    lock_name = "agentx-egress-key-rotation-lock"
    lock = run(
        (
            "kubectl",
            "-n",
            namespaces["dependencies"],
            "create",
            "configmap",
            lock_name,
            f"--from-literal=owner={uuid.uuid4().hex}",
        ),
        check=False,
        timeout=30,
    )
    if lock.returncode != 0:
        raise RuntimeError("another Egress key rotation holds the cluster lock")
    old_canonical = dict(canonical)
    old_gateway = dict(gateway)
    old_caller_secrets = {name: dict(secret or {}) for name, secret in caller_secrets.items()}
    try:
        overlap = json.loads(canonical["AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON"])
        new_public: dict[str, str] = {}
        rotation = uuid.uuid4().hex[:10]
        generated: dict[str, tuple[str, str]] = {}
        for role, (_, private_key, kid_key) in roles.items():
            private, public = _rsa_pair()
            kid = f"{role}-{rotation}"
            canonical[private_key] = private
            canonical[kid_key] = kid
            overlap[kid] = public
            new_public[kid] = public
            generated[role] = (private, kid)
        _apply_secret(
            namespaces["dependencies"],
            gateway_name,
            {"AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON": json.dumps(overlap)},
        )
        _restart_and_wait(namespaces["dependencies"], "agentx-egress-gateway")
        for deployment, (_, private_key, kid_key) in roles.items():
            secret_name = caller_secret_names[deployment]
            caller_secret = caller_secrets[secret_name]
            if caller_secret is None:
                raise RuntimeError(f"caller Secret disappeared during rotation: {secret_name}")
            caller_secret[private_key], caller_secret[kid_key] = generated[deployment]
            _apply_secret(namespaces["runtime"], secret_name, caller_secret)
            _restart_and_wait(namespaces["runtime"], deployment)
        canonical["AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON"] = json.dumps(new_public)
        _apply_secret(namespaces["dependencies"], secret_config["dependencies"], canonical)
        _apply_secret(
            namespaces["dependencies"],
            gateway_name,
            {"AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON": canonical["AGENTX_EGRESS_JWT_PUBLIC_KEYS_JSON"]},
        )
        _restart_and_wait(namespaces["dependencies"], "agentx-egress-gateway")
        return {"status": "rotated", "rotationId": rotation, "activeKids": sorted(new_public)}
    except Exception:
        _apply_secret(namespaces["dependencies"], secret_config["dependencies"], old_canonical)
        for name, secret in old_caller_secrets.items():
            _apply_secret(namespaces["runtime"], name, secret)
        _apply_secret(namespaces["dependencies"], gateway_name, old_gateway)
        _restart_and_wait(namespaces["dependencies"], "agentx-egress-gateway", check=False)
        for deployment in roles:
            _restart_and_wait(namespaces["runtime"], deployment, check=False)
        raise
    finally:
        run(
            ("kubectl", "-n", namespaces["dependencies"], "delete", "configmap", lock_name, "--ignore-not-found"),
            check=False,
            timeout=30,
        )


def _restart_and_wait(namespace: str, deployment: str, *, check: bool = True) -> None:
    restarted = run(
        ("kubectl", "-n", namespace, "rollout", "restart", f"deployment/{deployment}"),
        timeout=60,
        check=check,
    )
    if restarted.returncode != 0:
        return
    run(
        ("kubectl", "-n", namespace, "rollout", "status", f"deployment/{deployment}", "--timeout=300s"),
        timeout=330,
        check=check,
    )
