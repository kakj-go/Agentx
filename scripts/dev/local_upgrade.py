"""One-shot local build + upgrade for the agentx dev cluster.

Orchestrates the three manual steps (and the two easy-to-miss ones) into a
single command:

1. rebuild agentxctl so embedded charts / values schemas are current;
2. cargo xtask images (build + import into the docker-desktop containerd);
3. agentxctl validate + upgrade (runs SQL migrations via the migrate jobs);
4. rollout-restart workloads whose pod templates did not change (the image tag
   stays ``dev``, so Helm alone never rotates them);
5. wait for rollouts and summarize pod health.

Usage:
    uv run scripts/dev/local_upgrade.py                     # all services
    uv run scripts/dev/local_upgrade.py --service platform-control --service runtime-gateway
    uv run scripts/dev/local_upgrade.py --skip-build        # upgrade + restart only
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VALUES = ROOT / "deploy" / "values" / "local.yaml"

# service -> (namespace, deployment); jobs (migrate/bootstrap/doctor) are omitted.
DEPLOYMENTS: dict[str, tuple[str, str]] = {
    "platform-control": ("agentx-control", "platform-control"),
    "web-console": ("agentx-control", "web-console"),
    "runtime-gateway": ("agentx-runtime", "runtime-gateway"),
    "workflow-runtime": ("agentx-runtime", "workflow-runtime"),
    "workflow-worker": ("agentx-runtime", "workflow-worker"),
    "sandbox-manager": ("agentx-runtime", "sandbox-manager"),
    "observability": ("agentx-runtime", "observability"),
    "agentx-egress-gateway": ("agentx-deps", "agentx-egress-gateway"),
}

ALL_SERVICES = [
    "agentx-migrate",
    "platform-control",
    "web-console",
    "runtime-gateway",
    "workflow-runtime",
    "workflow-worker",
    "sandbox-manager",
    "agentx-egress-gateway",
    "observability",
]


def step(name: str) -> None:
    print(f"\n==> {name}", flush=True)


def run(command: list[str], *, timeout: int = 3600, check: bool = True) -> subprocess.CompletedProcess[str]:
    print(f"    $ {' '.join(command)}", flush=True)
    completed = subprocess.run(
        command,
        cwd=ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
    )
    if completed.stdout.strip():
        print(completed.stdout.rstrip()[-4000:])
    if completed.stderr.strip():
        print(completed.stderr.rstrip()[-4000:], file=sys.stderr)
    if check and completed.returncode != 0:
        raise SystemExit(f"command failed ({completed.returncode}): {' '.join(command)}")
    return completed


def agentxctl_path() -> str:
    suffix = ".exe" if os.name == "nt" else ""
    return str(ROOT / "target" / "debug" / f"agentxctl{suffix}")


def kubectl(*args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    return run(["kubectl", *args], timeout=600, check=check)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument(
        "--service", action="append", default=[], help="image service to rebuild (repeatable; default: all)"
    )
    parser.add_argument("--values", type=Path, default=VALUES, help="values file (default: deploy/values/local.yaml)")
    parser.add_argument(
        "--target",
        default="all",
        choices=["all", "control", "runtime", "observability", "dependencies"],
        help="upgrade target",
    )
    parser.add_argument("--skip-build", action="store_true", help="skip image build/import, only upgrade + restart")
    parser.add_argument(
        "--skip-restart", action="store_true", help="skip rollout restart of template-unchanged workloads"
    )
    parser.add_argument(
        "--skip-ctl-build", action="store_true", help="skip rebuilding agentxctl (embedded charts stay as-is)"
    )
    args = parser.parse_args()

    services = args.service or ALL_SERVICES
    unknown = [
        service
        for service in services
        if service not in ALL_SERVICES and service not in ("agentx-bootstrap", "agentx-doctor", "echo-node", "echo-mcp")
    ]
    if unknown:
        raise SystemExit(f"unknown services: {', '.join(unknown)}")

    started = time.time()
    if not args.skip_ctl_build:
        step("Rebuild agentxctl (embedded charts and values schemas are compile-time assets)")
        run(["cargo", "build", "-q", "-p", "agentxctl"], timeout=1200)

    if not args.skip_build:
        step(f"Build and import images: {', '.join(services)}")
        command = ["cargo", "xtask", "images", "--values", str(args.values)]
        for service in services:
            command += ["--service", service]
        run(command, timeout=3600)

    step(f"Validate values ({args.values.name})")
    ctl = agentxctl_path()
    run([ctl, "validate", "--values", str(args.values)], timeout=600)

    step(f"Upgrade target '{args.target}' (runs SQL migrations, waits for jobs)")
    upgrade = [ctl, "upgrade", "--values", str(args.values), "--target", args.target]
    run(upgrade, timeout=3600)

    if not args.skip_restart:
        step("Rollout-restart workloads so unchanged pod templates pick up the new dev images")
        for service in services:
            deployment = DEPLOYMENTS.get(service)
            if not deployment:
                continue
            namespace, name = deployment
            kubectl("rollout", "restart", f"deployment/{name}", "-n", namespace, check=False)
        for service in services:
            deployment = DEPLOYMENTS.get(service)
            if not deployment:
                continue
            namespace, name = deployment
            kubectl("rollout", "status", f"deployment/{name}", "-n", namespace, "--timeout=300s", check=False)

    step(
        "Verify ingress network-policy trust label (a missing agentx.io/ingress=allowed on the dependencies namespace makes every ingress request return 504)"
    )
    deps_namespace = kubectl(
        "get", "namespace", "agentx-deps", "-o", r"jsonpath={.metadata.labels.agentx\.io/ingress}", check=False
    )
    if (deps_namespace.stdout or "").strip() != "allowed":
        kubectl("label", "namespace", "agentx-deps", "agentx.io/ingress=allowed", "--overwrite")
        print("    labeled agentx-deps (was missing)")

    step("Cluster health summary")
    for namespace in ("agentx-control", "agentx-runtime", "agentx-deps"):
        kubectl("get", "pods", "-n", namespace, "--field-selector=status.phase!=Succeeded")
    jobs = kubectl("get", "jobs", "-A", "--no-headers", check=False)
    bad = []
    for line in (jobs.stdout or "").splitlines():
        if "migrate" not in line:
            continue
        columns = line.split()
        # columns: NAMESPACE NAME STATUS COMPLETIONS DURATION AGE
        if len(columns) < 4 or columns[2] != "Complete":
            bad.append(line)
            continue
        done, _, total = columns[3].partition("/")
        if done != total:
            bad.append(line)
    if bad:
        raise SystemExit("a migrate job has not succeeded; inspect `kubectl get jobs -A`")

    print(f"\nDone in {time.time() - started:.0f}s.")


if __name__ == "__main__":
    main()
