from __future__ import annotations

import argparse
import hashlib
import json
from datetime import UTC, datetime


def main() -> None:
    parser = argparse.ArgumentParser(description="Deterministic E2E backup provider receipt fixture")
    parser.add_argument("--action", choices=("backup", "restore"), required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--backup-id", required=True)
    parser.add_argument("--values", required=True)
    parser.add_argument("--restore-target")
    args = parser.parse_args()
    identity = f"{args.backup_id}:{args.target}:{args.action}:{args.restore_target or ''}"
    print(
        json.dumps(
            {
                "status": "passed",
                "recoveryPointUtc": datetime.now(UTC).isoformat(),
                "objectCount": 1,
                "contentSha256": hashlib.sha256(identity.encode()).hexdigest(),
                "schemaVersionObserved": "agentx-e2e-fixture-v1",
            }
        )
    )


if __name__ == "__main__":
    main()
