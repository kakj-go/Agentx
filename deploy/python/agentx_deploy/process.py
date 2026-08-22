from __future__ import annotations

import json
import os
import re
import shutil
import signal
import subprocess
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any

SENSITIVE = re.compile(
    r"(?i)((?:password|secret|token|private[_-]?key|authorization)[\"']?\s*[=:]\s*[\"']?)([^\"'\s,;}]+)"
)
URL_CREDENTIALS = re.compile(r"(?i)([a-z][a-z0-9+.-]*://)[^/@\s]+:[^/@\s]+@")
PRIVATE_PEM = re.compile(
    r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----.*?-----END (?:RSA |EC |OPENSSH )?PRIVATE KEY-----",
    re.DOTALL,
)


class CommandError(RuntimeError):
    def __init__(self, command: Sequence[str], returncode: int, output: str) -> None:
        safe_command = " ".join(redact(part) for part in command)
        super().__init__(f"command failed ({returncode}): {safe_command}\n{redact(output).strip()}")
        self.command = tuple(command)
        self.returncode = returncode


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
        if self.process.poll() is not None:
            self._close_logs()
            return
        try:
            if os.name == "nt":
                self.process.send_signal(signal.CTRL_BREAK_EVENT)
            else:
                os.killpg(self.process.pid, signal.SIGTERM)
        except (OSError, ProcessLookupError):
            self.process.terminate()
        try:
            self.process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            try:
                if os.name == "nt":
                    self.process.kill()
                else:
                    os.killpg(self.process.pid, signal.SIGKILL)
            except (OSError, ProcessLookupError):
                self.process.kill()
            self.process.wait(timeout=5)
        finally:
            self._close_logs()

    def _close_logs(self) -> None:
        self.stdout_handle.close()
        self.stderr_handle.close()


def start_process(command: Sequence[str | Path], *, stdout_path: Path, stderr_path: Path) -> ManagedProcess:
    stdout_path.parent.mkdir(parents=True, exist_ok=True)
    stderr_path.parent.mkdir(parents=True, exist_ok=True)
    stdout_handle = stdout_path.open("w", encoding="utf-8")
    stderr_handle = stderr_path.open("w", encoding="utf-8")
    kwargs: dict[str, Any] = {}
    if os.name == "nt":
        kwargs["creationflags"] = subprocess.CREATE_NEW_PROCESS_GROUP
    else:
        kwargs["start_new_session"] = True
    try:
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
    except Exception:
        stdout_handle.close()
        stderr_handle.close()
        raise
    return ManagedProcess(process, stdout_handle, stderr_handle)


def redact(value: str) -> str:
    redacted = URL_CREDENTIALS.sub(r"\1<redacted>@", value)
    redacted = PRIVATE_PEM.sub("<redacted-private-key>", redacted)
    return SENSITIVE.sub(lambda match: f"{match.group(1)}<redacted>", redacted)


def require_tool(name: str) -> str:
    executable = shutil.which(name)
    if not executable:
        raise RuntimeError(f"required tool is not installed or not on PATH: {name}")
    return executable


def run(
    command: Sequence[str | Path],
    *,
    cwd: Path | None = None,
    input_text: str | None = None,
    timeout: int = 600,
    check: bool = True,
    env: dict[str, str] | None = None,
) -> Result:
    args = tuple(str(part) for part in command)
    process_env = os.environ.copy()
    process_env.update({"PYTHONUTF8": "1"})
    if env:
        process_env.update(env)
    try:
        completed = subprocess.run(
            args,
            cwd=cwd,
            input=input_text,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
            check=False,
            shell=False,
            env=process_env,
        )
    except subprocess.TimeoutExpired as error:
        safe_command = " ".join(redact(part) for part in args)
        raise RuntimeError(f"command timed out after {timeout}s: {safe_command}") from error
    result = Result(args, completed.stdout, completed.stderr, completed.returncode)
    if check and completed.returncode != 0:
        raise CommandError(args, completed.returncode, completed.stdout + completed.stderr)
    return result
