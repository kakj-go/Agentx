from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from agentx_deploy.config import load_values, selected_targets
from agentx_deploy.operations import (
    backup_operation,
    build_images,
    doctor,
    emit,
    install,
    migrate,
    render,
    rollback,
    rotate_keys,
    status,
    sync_secrets,
    uninstall,
    validate,
)
from agentx_deploy.process import redact

TARGET_CHOICES = ("control", "runtime", "observability", "dependencies", "all")
BACKUP_TARGET_CHOICES = (
    "control-mysql",
    "runtime-mysql",
    "control-objects",
    "runtime-objects",
    "observability-objects",
    "clickhouse",
)


def _common(parser: argparse.ArgumentParser, *, target: bool = True) -> None:
    parser.add_argument("--values", required=True, help="deployment values YAML")
    if target:
        parser.add_argument("--target", choices=TARGET_CHOICES, default="all")
    parser.add_argument("--run-id", help="E2E-only namespace suffix")
    parser.add_argument("--output", choices=("text", "json"), default="text")


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(prog="agentx-deploy", description="Cross-platform Agentx Helm deployment CLI")
    root.add_argument("--version", action="version", version="agentx-deploy 0.1.0")
    commands = root.add_subparsers(dest="command", required=True)
    for name in ("validate", "render", "install", "upgrade", "status", "doctor", "sync-secrets"):
        command = commands.add_parser(name)
        _common(command)
        if name == "validate":
            command.add_argument("--cluster", action="store_true", help="also verify cluster connectivity")
        if name in ("install", "upgrade"):
            command.add_argument("--skip-doctor", action="store_true")
    rollback_parser = commands.add_parser("rollback")
    _common(rollback_parser)
    rollback_parser.add_argument("--revision", type=int, required=True)
    uninstall_parser = commands.add_parser("uninstall")
    _common(uninstall_parser)
    uninstall_parser.add_argument("--purge-data", action="store_true")
    uninstall_parser.add_argument("--yes", action="store_true")
    migrate_parser = commands.add_parser("migrate")
    _common(migrate_parser)
    migrate_parser.add_argument("--phase", choices=("expand", "contract"), default="expand")
    migrate_parser.add_argument("--timeout", type=int, default=600)
    build_parser = commands.add_parser("build-images")
    _common(build_parser, target=False)
    build_parser.add_argument("--service", action="append", default=[])
    build_parser.add_argument("--push", action="store_true")
    build_parser.add_argument("--skip-kubernetes-import", action="store_true")
    for name in ("backup", "restore"):
        backup_parser = commands.add_parser(name)
        _common(backup_parser, target=False)
        backup_parser.add_argument("--data-target", choices=BACKUP_TARGET_CHOICES, required=True)
        backup_parser.add_argument("--backup-id", required=True)
        backup_parser.add_argument("--adapter", type=Path, required=True)
        backup_parser.add_argument("--restore-target")
        backup_parser.add_argument("--allow-in-place-restore", action="store_true")
        backup_parser.add_argument("--artifact-dir", type=Path, default=Path("artifacts/data-operations"))
    rotate = commands.add_parser("rotate-egress-keys")
    _common(rotate, target=False)
    rotate.add_argument("--action", choices=("plan", "rotate"), default="plan")
    return root


def execute(args: argparse.Namespace) -> object:
    config = load_values(args.values, run_id=args.run_id)
    targets = selected_targets(getattr(args, "target", "all"))
    match args.command:
        case "validate":
            return validate(config, targets, cluster=args.cluster)
        case "render":
            return render(config, targets)
        case "install":
            return install(config, targets, run_doctor=not args.skip_doctor)
        case "upgrade":
            return install(config, targets, run_doctor=not args.skip_doctor, preserve_replicas=True)
        case "status":
            return status(config, targets)
        case "doctor":
            return doctor(config, targets)
        case "rollback":
            return rollback(config, args.target, args.revision)
        case "uninstall":
            return uninstall(config, targets, purge_data=args.purge_data, confirmed=args.yes)
        case "migrate":
            return migrate(config, args.target, args.phase, args.timeout)
        case "sync-secrets":
            if args.target != "all":
                raise ValueError("sync-secrets requires --target all")
            return sync_secrets(config)
        case "build-images":
            return build_images(
                config,
                args.service,
                push=args.push,
                skip_kubernetes_import=args.skip_kubernetes_import,
            )
        case "backup" | "restore":
            return backup_operation(
                config,
                action=args.command,
                target=args.data_target,
                backup_id=args.backup_id,
                adapter=args.adapter.resolve(),
                restore_target=args.restore_target,
                artifact_dir=args.artifact_dir.resolve(),
                allow_in_place_restore=args.allow_in_place_restore,
            )
        case "rotate-egress-keys":
            return rotate_keys(config, apply=args.action == "rotate")
        case _:
            raise ValueError(f"unsupported command: {args.command}")


def main(argv: list[str] | None = None) -> None:
    args = parser().parse_args(argv)
    try:
        result = execute(args)
        if isinstance(result, str) and args.command == "render" and args.output == "text":
            print(result, end="")
        elif isinstance(result, str) and args.command == "render":
            emit({"status": "rendered", "manifest": result}, args.output)
        else:
            emit(result, args.output)
    except Exception as error:
        message = redact(str(error))
        if args.output == "json":
            print(json.dumps({"status": "error", "error": message}, ensure_ascii=False), file=sys.stderr)
        else:
            print(f"agentx-deploy: {message}", file=sys.stderr)
        raise SystemExit(1) from error


if __name__ == "__main__":
    main()
