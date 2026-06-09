#!/usr/bin/env python3
"""Run the target body ownership Frida trace until the target body appears.

This is a wrapper around the existing read-only trace:

    tmp_vector_probe/native_target_body_ownership_trace_v1.js

It intentionally adds no hook targets and makes no CSP/importer changes. Each
attempt attaches Frida to the newest CLIPStudioPaint.exe process, optionally
triggers opening Vector_SizePressure.clip, waits for a bounded interval, stops
Frida, and runs tools/correlate_target_body_ownership.py on the trace JSONL.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass, asdict
from datetime import datetime
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
OUT_DIR = REPO_ROOT / "tmp_vector_probe"
TRACE_SCRIPT = OUT_DIR / "native_target_body_ownership_trace_v1.js"
CORRELATOR = REPO_ROOT / "tools/correlate_target_body_ownership.py"
DEFAULT_CLIP_PATH = REPO_ROOT / "img/Vector_SizePressure.clip"
CORRELATION_PATH = OUT_DIR / "native_target_body_ownership_correlation_v1.json"
SUMMARY_PATH = OUT_DIR / "target_body_ownership_until_detected_summary_v1.json"

TARGET_ID = "extrnlid62D15CB4395245648869B4AEBAD8FBCE"
TARGET_SIZE = 2644
TARGET_HASH = "fnv1a32:7bece4ac"


@dataclass
class AttemptResult:
    attempt: int
    pid: int | None
    started_at: str
    finished_at: str | None = None
    trace_path: str | None = None
    stdout_log: str | None = None
    stderr_log: str | None = None
    correlation_path: str | None = None
    success: bool = False
    success_reason: str | None = None
    classification: str | None = None
    classification_reason: str | None = None
    dest_ptr: str | None = None
    dest_hash: str | None = None
    body_size: int | None = None
    error: str | None = None


def timestamp() -> str:
    return datetime.now().strftime("%Y%m%d_%H%M%S_%f")[:-3]


def run_text(cmd: list[str], *, cwd: Path = REPO_ROOT, timeout: int = 30) -> str:
    proc = subprocess.run(
        cmd,
        cwd=str(cwd),
        text=True,
        capture_output=True,
        timeout=timeout,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"command failed ({proc.returncode}): {' '.join(cmd)}\n"
            f"stdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )
    return proc.stdout.strip()


def find_frida() -> str:
    appdata = os.environ.get("APPDATA")
    if appdata:
        candidate = Path(appdata) / "Python/Python311/Scripts/frida.exe"
        if candidate.exists():
            return str(candidate)
    found = shutil.which("frida")
    if found:
        return found
    raise RuntimeError("frida.exe was not found on PATH or in the user Python Scripts directory")


def latest_csp_pid() -> int:
    command = (
        "$p = Get-Process CLIPStudioPaint -ErrorAction SilentlyContinue | "
        "Sort-Object StartTime -Descending | Select-Object -First 1; "
        "if ($null -eq $p) { exit 3 }; $p.Id"
    )
    out = run_text(["powershell", "-NoProfile", "-Command", command], timeout=10)
    try:
        return int(out.splitlines()[-1].strip())
    except (ValueError, IndexError) as exc:
        raise RuntimeError(f"could not parse CLIPStudioPaint pid from: {out!r}") from exc


def existing_trace_files() -> set[Path]:
    return set(OUT_DIR.glob("native_target_body_ownership_*_pid*.jsonl"))


def newest_new_trace(before: set[Path], since: float) -> Path | None:
    candidates = [
        path
        for path in OUT_DIR.glob("native_target_body_ownership_*_pid*.jsonl")
        if path not in before and path.stat().st_mtime >= since - 2.0
    ]
    if not candidates:
        return None
    return max(candidates, key=lambda path: path.stat().st_mtime)


def trigger_none(_: Path) -> None:
    print("trigger=none: open the target .clip manually while Frida is attached.", flush=True)


def trigger_shell_open(clip_path: Path) -> None:
    os.startfile(str(clip_path))  # type: ignore[attr-defined]


def ps_quote(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def trigger_sendkeys_open(clip_path: Path) -> None:
    script = f"""
Add-Type -AssemblyName System.Windows.Forms
$clipPath = {ps_quote(str(clip_path))}
Set-Clipboard -Value $clipPath
$ws = New-Object -ComObject WScript.Shell
$activated = $ws.AppActivate('CLIP STUDIO PAINT')
Start-Sleep -Milliseconds 800
if (-not $activated) {{ Write-Error 'CLIP STUDIO PAINT window was not activated'; exit 2 }}
[System.Windows.Forms.SendKeys]::SendWait('^o')
Start-Sleep -Seconds 2
[System.Windows.Forms.SendKeys]::SendWait('^v')
Start-Sleep -Milliseconds 300
[System.Windows.Forms.SendKeys]::SendWait('{{ENTER}}')
"""
    run_text(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script],
        timeout=15,
    )


def trigger_open_dialog_paste(clip_path: Path) -> None:
    trigger_sendkeys_open(clip_path)


def start_frida(pid: int, frida: str, attempt: int) -> tuple[subprocess.Popen[str], Path, Path]:
    stamp = timestamp()
    stdout_path = OUT_DIR / f"target_body_ownership_until_detected_{stamp}_attempt{attempt:03d}_stdout.log"
    stderr_path = OUT_DIR / f"target_body_ownership_until_detected_{stamp}_attempt{attempt:03d}_stderr.log"
    stdout_fh = stdout_path.open("w", encoding="utf-8", errors="replace")
    stderr_fh = stderr_path.open("w", encoding="utf-8", errors="replace")
    proc = subprocess.Popen(
        [frida, "-p", str(pid), "-l", str(TRACE_SCRIPT)],
        cwd=str(REPO_ROOT),
        stdin=subprocess.DEVNULL,
        stdout=stdout_fh,
        stderr=stderr_fh,
        text=True,
    )
    # Keep handles on the process object so Windows does not close them early.
    proc._codex_stdout_fh = stdout_fh  # type: ignore[attr-defined]
    proc._codex_stderr_fh = stderr_fh  # type: ignore[attr-defined]
    return proc, stdout_path, stderr_path


def stop_process(proc: subprocess.Popen[str]) -> None:
    try:
        if proc.poll() is None:
            if os.name == "nt":
                subprocess.run(
                    ["taskkill", "/PID", str(proc.pid), "/T", "/F"],
                    text=True,
                    capture_output=True,
                    timeout=10,
                )
                proc.wait(timeout=8)
            else:
                proc.terminate()
                try:
                    proc.wait(timeout=8)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait(timeout=8)
    finally:
        for attr in ("_codex_stdout_fh", "_codex_stderr_fh"):
            fh = getattr(proc, attr, None)
            if fh and not fh.closed:
                fh.close()


def run_correlator(trace_path: Path) -> dict[str, Any]:
    proc = subprocess.run(
        [sys.executable, str(CORRELATOR), str(trace_path)],
        cwd=str(REPO_ROOT),
        text=True,
        capture_output=True,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"correlator failed ({proc.returncode}) for {trace_path}\n"
            f"stdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )
    return json.loads(CORRELATION_PATH.read_text(encoding="utf-8"))


def target_body_observed(correlation: dict[str, Any]) -> tuple[bool, str]:
    if correlation.get("target_external_id") == TARGET_ID:
        return True, "target external id observed"
    if correlation.get("body_size") == TARGET_SIZE:
        return True, "target body size 2644 observed"
    if correlation.get("dest_hash") == TARGET_HASH:
        return True, "target body hash observed"
    return False, "target body not observed"


def r13_model_validated(correlation: dict[str, Any]) -> bool:
    candidates = correlation.get("owner_candidates")
    if not isinstance(candidates, list):
        return False
    has_dest = False
    has_size = False
    for item in candidates:
        if not isinstance(item, dict):
            continue
        label = str(item.get("object_label") or "")
        if label not in {"r13", "reserve_owner", "entry_rdx", "reserve_return_owner"}:
            continue
        if item.get("equals_dest_ptr") or item.get("contains_dest_ptr"):
            has_dest = True
        if item.get("equals_body_size"):
            has_size = True
    return has_dest and has_size


def write_summary(
    attempts: list[AttemptResult],
    *,
    success: bool,
    final_correlation: dict[str, Any] | None,
    args: argparse.Namespace,
) -> None:
    result = {
        "output_path": str(SUMMARY_PATH),
        "success": success,
        "success_reason": attempts[-1].success_reason if attempts else None,
        "target_external_id": TARGET_ID,
        "target_body_size": TARGET_SIZE,
        "target_hash": TARGET_HASH,
        "trace_script": str(TRACE_SCRIPT),
        "correlator": str(CORRELATOR),
        "clip_path": str(args.clip_path),
        "trigger": args.trigger,
        "attempt_seconds": args.attempt_seconds,
        "max_attempts": args.max_attempts,
        "attempts": [asdict(attempt) for attempt in attempts],
        "final_correlation_path": str(CORRELATION_PATH) if final_correlation else None,
        "final_trace_path": attempts[-1].trace_path if attempts else None,
        "r13_ownership_model_validated": bool(final_correlation and r13_model_validated(final_correlation)),
        "r13_parent_ownership": "unresolved",
        "no_new_hook_targets": True,
    }
    SUMMARY_PATH.write_text(json.dumps(result, indent=2, ensure_ascii=False), encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Repeatedly run the existing target body ownership Frida trace until it observes the target body.",
    )
    parser.add_argument("--clip-path", type=Path, default=DEFAULT_CLIP_PATH)
    parser.add_argument("--attempt-seconds", type=float, default=60.0)
    parser.add_argument(
        "--max-attempts",
        type=int,
        default=0,
        help="maximum attempts; 0 means keep trying until detected or interrupted",
    )
    parser.add_argument(
        "--sleep-between-attempts",
        type=float,
        default=3.0,
    )
    parser.add_argument(
        "--trigger",
        choices=("none", "sendkeys-open", "open-dialog-paste", "shell-open"),
        default="sendkeys-open",
        help="how to trigger opening the clip after attaching Frida",
    )
    parser.add_argument(
        "--attach-delay-seconds",
        type=float,
        default=2.0,
        help="seconds to wait after frida starts before triggering the open action",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    args.clip_path = args.clip_path.resolve()
    if not TRACE_SCRIPT.exists():
        raise SystemExit(f"missing trace script: {TRACE_SCRIPT}")
    if not CORRELATOR.exists():
        raise SystemExit(f"missing correlator: {CORRELATOR}")
    if not args.clip_path.exists():
        raise SystemExit(f"missing clip file: {args.clip_path}")

    frida = find_frida()
    triggers = {
        "none": trigger_none,
        "sendkeys-open": trigger_sendkeys_open,
        "open-dialog-paste": trigger_open_dialog_paste,
        "shell-open": trigger_shell_open,
    }
    trigger = triggers[args.trigger]
    attempts: list[AttemptResult] = []
    final_correlation: dict[str, Any] | None = None
    attempt = 0

    print(f"frida={frida}", flush=True)
    print(f"trace_script={TRACE_SCRIPT}", flush=True)
    print(f"clip_path={args.clip_path}", flush=True)
    print(f"summary_path={SUMMARY_PATH}", flush=True)

    try:
        while args.max_attempts == 0 or attempt < args.max_attempts:
            attempt += 1
            started = timestamp()
            result = AttemptResult(attempt=attempt, pid=None, started_at=started)
            attempts.append(result)
            before = existing_trace_files()
            start_time = time.time()

            try:
                pid = latest_csp_pid()
                result.pid = pid
                proc, stdout_log, stderr_log = start_frida(pid, frida, attempt)
                result.stdout_log = str(stdout_log)
                result.stderr_log = str(stderr_log)
                print(f"attempt {attempt}: attached to pid {pid}", flush=True)
                time.sleep(args.attach_delay_seconds)
                trigger(args.clip_path)
                time.sleep(args.attempt_seconds)
                stop_process(proc)

                trace_path = newest_new_trace(before, start_time)
                if not trace_path:
                    result.error = "trace JSONL was not produced"
                    print(f"attempt {attempt}: {result.error}", flush=True)
                else:
                    result.trace_path = str(trace_path)
                    final_correlation = run_correlator(trace_path)
                    result.correlation_path = str(CORRELATION_PATH)
                    result.classification = final_correlation.get("classification")
                    result.classification_reason = final_correlation.get("classification_reason")
                    result.dest_ptr = final_correlation.get("dest_ptr")
                    result.dest_hash = final_correlation.get("dest_hash")
                    body_size = final_correlation.get("body_size")
                    result.body_size = body_size if isinstance(body_size, int) else None
                    result.success, result.success_reason = target_body_observed(final_correlation)
                    print(
                        f"attempt {attempt}: classification={result.classification} "
                        f"body_size={result.body_size} dest_hash={result.dest_hash} "
                        f"success={result.success}",
                        flush=True,
                    )
                    if result.success:
                        result.finished_at = timestamp()
                        write_summary(attempts, success=True, final_correlation=final_correlation, args=args)
                        print(f"detected target body; wrote {SUMMARY_PATH}", flush=True)
                        return 0
            except KeyboardInterrupt:
                raise
            except Exception as exc:
                result.error = str(exc)
                print(f"attempt {attempt}: error: {exc}", flush=True)
            finally:
                result.finished_at = timestamp()
                write_summary(attempts, success=False, final_correlation=final_correlation, args=args)

            if args.max_attempts != 0 and attempt >= args.max_attempts:
                break
            time.sleep(args.sleep_between_attempts)
    except KeyboardInterrupt:
        print("interrupted; wrote latest summary", flush=True)
        write_summary(attempts, success=False, final_correlation=final_correlation, args=args)
        return 130

    print(f"target body not detected after {attempt} attempt(s); wrote {SUMMARY_PATH}", flush=True)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
