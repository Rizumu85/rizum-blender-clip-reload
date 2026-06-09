#!/usr/bin/env python3
"""Spawn CSP with Vector_SizePressure.clip under the target external-id hunter."""

from __future__ import annotations

import argparse
import json
import subprocess
import time
from pathlib import Path

import frida


ROOT = Path(__file__).resolve().parents[1]
EXE = Path(r"C:\Program Files\CELSYS\CLIP STUDIO 1.5\CLIP STUDIO PAINT\CLIPStudioPaint.exe")
CLIP = ROOT / "img" / "Vector_SizePressure.clip"
SCRIPT = ROOT / "tmp_vector_probe" / "native_target_external_id_hunter_v1.js"
RUNNER_LOG = ROOT / "tmp_vector_probe" / "target_external_id_hunter_runner_stdout.log"


def stop_csp() -> None:
    subprocess.run(
        [
            "powershell",
            "-NoProfile",
            "-Command",
            "Get-Process CLIPStudioPaint -ErrorAction SilentlyContinue | Stop-Process -Force",
        ],
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--seconds", type=float, default=55.0)
    parser.add_argument("--clip", type=Path, default=CLIP)
    parser.add_argument("--keep-process", action="store_true")
    args = parser.parse_args()

    stop_csp()
    time.sleep(1.5)

    device = frida.get_local_device()
    pid = device.spawn([str(EXE), str(args.clip)])
    session = device.attach(pid)
    messages = []

    def on_message(message, data):
        messages.append(message)

    script = session.create_script(SCRIPT.read_text(encoding="utf-8"))
    script.on("message", on_message)
    script.load()
    device.resume(pid)
    time.sleep(args.seconds)
    try:
        script.unload()
    except Exception as exc:  # pragma: no cover - diagnostic path
        messages.append({"type": "unload_error", "description": repr(exc)})
    session.detach()
    if not args.keep_process:
        subprocess.run(
            [
                "powershell",
                "-NoProfile",
                "-Command",
                f"Get-Process -Id {pid} -ErrorAction SilentlyContinue | Stop-Process -Force",
            ],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    RUNNER_LOG.write_text(json.dumps({"pid": pid, "messages": messages}, indent=2), encoding="utf-8")
    print(json.dumps({"pid": pid, "messages": len(messages), "runner_log": str(RUNNER_LOG)}, indent=2))


if __name__ == "__main__":
    main()
