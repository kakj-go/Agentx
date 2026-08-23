from __future__ import annotations

import json
import os
import re
import signal
import subprocess
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]


def agentxctl() -> str:
    configured = os.getenv("AGENTXCTL_BIN")
    if configured:
        return configured
    suffix = ".exe" if os.name == "nt" else ""
    return str(ROOT / "target" / "debug" / f"agentxctl{suffix}")


def backup_adapter() -> str:
    configured = os.getenv("AGENTX_BACKUP_ADAPTER_BIN")
    if configured:
        return configured
    suffix = ".exe" if os.name == "nt" else ""
    return str(ROOT / "target" / "debug" / f"agentx-backup-test-adapter{suffix}")


@dataclass(frozen=True)
class Result:
    command: tuple[str, ...]
    stdout: str
    stderr: str
    returncode: int

    def json(self) -> Any:
        return json.loads(self.stdout)


@dataclass
class ManagedProcess:
    process: subprocess.Popen[str]
    stdout_handle: Any
    stderr_handle: Any

    def stop(self, timeout: int = 15) -> None:
        if self.process.poll() is None:
            try:
                if os.name == "nt":
                    self.process.send_signal(signal.CTRL_BREAK_EVENT)
                else:
                    os.killpg(self.process.pid, signal.SIGTERM)
                self.process.wait(timeout=timeout)
            except (OSError, ProcessLookupError, subprocess.TimeoutExpired):
                self.process.kill()
                self.process.wait(timeout=5)
        self.stdout_handle.close()
        self.stderr_handle.close()


def start_process(command: Sequence[str | Path], *, stdout_path: Path, stderr_path: Path) -> ManagedProcess:
    stdout_path.parent.mkdir(parents=True, exist_ok=True)
    stdout_handle = stdout_path.open("w", encoding="utf-8")
    stderr_handle = stderr_path.open("w", encoding="utf-8")
    kwargs: dict[str, Any] = (
        {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP} if os.name == "nt" else {"start_new_session": True}
    )
    process = subprocess.Popen(
        tuple(str(part) for part in command),
        stdout=stdout_handle,
        stderr=stderr_handle,
        text=True,
        encoding="utf-8",
        errors="replace",
        shell=False,
        **kwargs,
    )
    return ManagedProcess(process, stdout_handle, stderr_handle)


def redact(value: str) -> str:
    value = re.sub(r"(?i)([a-z][a-z0-9+.-]*://)[^/@\s]+:[^/@\s]+@", r"\1<redacted>@", value)
    value = re.sub(
        r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----.*?-----END (?:RSA |EC |OPENSSH )?PRIVATE KEY-----",
        "<redacted-private-key>",
        value,
        flags=re.DOTALL,
    )
    return re.sub(
        r"(?i)((?:password|secret|token|private[_-]?key|authorization)[\"']?\s*[=:]\s*[\"']?)([^\"'\s,;}]+)",
        r"\1<redacted>",
        value,
    )


def run(
    command: Sequence[str | Path],
    *,
    input_text: str | None = None,
    timeout: int = 600,
    check: bool = True,
    env: dict[str, str] | None = None,
) -> Result:
    completed = subprocess.run(
        tuple(str(part) for part in command),
        cwd=ROOT,
        input=input_text,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        check=False,
        shell=False,
        env={**os.environ, **(env or {})},
    )
    result = Result(tuple(str(part) for part in command), completed.stdout, completed.stderr, completed.returncode)
    if check and result.returncode != 0:
        raise RuntimeError(
            f"command failed ({result.returncode}): {' '.join(result.command)}\n{redact(result.stdout + result.stderr)}"
        )
    return result


def deployment_config(values: Path, run_id: str | None = None) -> dict[str, Any]:
    command = [agentxctl(), "validate", "--values", str(values), "--output", "json"]
    if run_id:
        command.extend(("--run-id", run_id))
    return run(command, timeout=300).json()


def render(values: Path | str, target: str, run_id: str | None = None) -> str:
    command = [agentxctl(), "render", "--values", str(values), "--target", target]
    if run_id:
        command.extend(("--run-id", run_id))
    return run(command, timeout=300).stdout
