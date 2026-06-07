#!/usr/bin/env python3
"""Spawn CSP under Frida and run the plot-only fresh-open positive control."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

import frida


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_EXE = Path(
    r"C:\Program Files\CELSYS\CLIP STUDIO 1.5\CLIP STUDIO PAINT\CLIPStudioPaint.exe"
)
DEFAULT_CLIP = REPO_ROOT / "img" / "Vector_SizePressure.clip"
DEFAULT_SCRIPT = REPO_ROOT / "tmp_vector_probe" / "native_plot_only_fresh_open_control_v1.js"
SUMMARY_SCRIPT = REPO_ROOT / "tools" / "summarize_native_plot_only_control.py"
DEFAULT_TRACE_GLOB = "native_plot_only_fresh_open_control_*_pid{pid}.jsonl"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Spawn CLIPStudioPaint.exe suspended through Frida, load the plot-only "
            "trace, then resume with Vector_SizePressure.clip in argv."
        )
    )
    parser.add_argument("--exe", type=Path, default=DEFAULT_EXE)
    parser.add_argument("--clip", type=Path, default=DEFAULT_CLIP)
    parser.add_argument("--script", type=Path, default=DEFAULT_SCRIPT)
    parser.add_argument("--trace-glob", default=DEFAULT_TRACE_GLOB)
    parser.add_argument("--timeout", type=float, default=180.0)
    parser.add_argument("--min-wait", type=float, default=30.0)
    parser.add_argument("--quiet-window", type=float, default=8.0)
    parser.add_argument("--no-summarize", action="store_true")
    return parser.parse_args()


def read_latest_trace(process_id: int, started_at: float, trace_glob: str) -> Path | None:
    probe_dir = REPO_ROOT / "tmp_vector_probe"
    candidates = []
    for path in probe_dir.glob(trace_glob.format(pid=process_id)):
        try:
            stat = path.stat()
        except OSError:
            continue
        if stat.st_mtime >= started_at - 2:
            candidates.append((stat.st_mtime, path))
    if not candidates:
        return None
    return sorted(candidates)[-1][1]


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    try:
        with path.open("r", encoding="utf-8") as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                try:
                    row = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if isinstance(row, dict):
                    rows.append(row)
    except OSError:
        return rows
    return rows


def latest_counts(path: Path) -> tuple[int, int, bool, int, int]:
    rows = load_jsonl(path)
    plot_entries = sum(1 for row in rows if row.get("event") == "plot_entry")
    sizepressure = sum(
        1
        for row in rows
        if row.get("event") == "plot_entry"
        and isinstance(row.get("sizepressure_call_index"), int)
    )
    hooks_ready = any(row.get("event") == "ready_hooks_installed" for row in rows)
    signal_rows = sum(1 for row in rows if row.get("event") != "summary")
    return plot_entries, sizepressure, hooks_ready, len(rows), signal_rows


def run_summarizer(trace_path: Path) -> int:
    return subprocess.run(
        [sys.executable, str(SUMMARY_SCRIPT), str(trace_path)],
        cwd=REPO_ROOT,
        check=False,
    ).returncode


def main() -> int:
    args = parse_args()
    exe = args.exe.resolve()
    clip = args.clip.resolve()
    script_path = args.script.resolve()

    for path in (exe, clip, script_path):
        if not path.exists():
            print(f"missing path: {path}", file=sys.stderr)
            return 2

    script_source = script_path.read_text(encoding="utf-8")
    started_at = time.time()
    device = frida.get_local_device()
    argv = [str(exe), str(clip)]
    print(f"spawning: {exe}", flush=True)
    print(f"cwd: {exe.parent}", flush=True)
    print(f"argv[1]: {clip}", flush=True)
    pid = device.spawn(str(exe), argv=argv, cwd=str(exe.parent))
    print(f"spawned_pid: {pid}", flush=True)

    session = None
    script = None
    trace_path: Path | None = None
    try:
        session = device.attach(pid)
        script = session.create_script(script_source)
        script.load()

        deadline = time.time() + 10
        while time.time() < deadline:
            trace_path = read_latest_trace(pid, started_at, args.trace_glob)
            if trace_path is not None:
                _, _, hooks_ready, _, _ = latest_counts(trace_path)
                if hooks_ready:
                    break
            time.sleep(0.25)

        if trace_path is None:
            print("trace_path: <not created before resume>", file=sys.stderr, flush=True)
        else:
            print(f"trace_path: {trace_path}", flush=True)
            print(f"hooks_ready_before_resume: {latest_counts(trace_path)[2]}", flush=True)

        device.resume(pid)
        print("resumed: true", flush=True)

        last_plot_count = -1
        last_signal_count = -1
        last_change_at = time.time()
        resumed_at = time.time()
        deadline = time.time() + args.timeout
        while time.time() < deadline:
            if trace_path is None:
                trace_path = read_latest_trace(pid, started_at, args.trace_glob)
            if trace_path is not None:
                plot_count, sizepressure_count, hooks_ready, row_count, signal_count = latest_counts(trace_path)
                if signal_count != last_signal_count:
                    last_plot_count = plot_count
                    last_signal_count = signal_count
                    last_change_at = time.time()
                    print(
                        "trace_counts: "
                        f"plot_entries={plot_count} "
                        f"sizepressure={sizepressure_count} "
                        f"hooks_ready={hooks_ready} "
                        f"rows={row_count} "
                        f"signal_rows={signal_count}",
                        flush=True,
                    )
                if (
                    time.time() - resumed_at >= args.min_wait
                    and signal_count > 0
                    and time.time() - last_change_at >= args.quiet_window
                ):
                    break
            time.sleep(1)

        if trace_path is None:
            print("trace_path: <not found>", file=sys.stderr, flush=True)
            return 1

        print(f"final_trace_path: {trace_path}", flush=True)
        print(
            "final_counts: "
            f"plot_entries={latest_counts(trace_path)[0]} "
            f"sizepressure={latest_counts(trace_path)[1]} "
            f"hooks_ready={latest_counts(trace_path)[2]} "
            f"rows={latest_counts(trace_path)[3]} "
            f"signal_rows={latest_counts(trace_path)[4]}",
            flush=True,
        )
        if not args.no_summarize:
            return run_summarizer(trace_path)
        return 0
    finally:
        if script is not None:
            try:
                script.unload()
            except frida.InvalidOperationError:
                pass
        if session is not None:
            try:
                session.detach()
            except frida.InvalidOperationError:
                pass


if __name__ == "__main__":
    raise SystemExit(main())
