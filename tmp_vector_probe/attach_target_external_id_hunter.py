#!/usr/bin/env python3
"""Attach the target external-id hunter to an already-running CSP process."""

from __future__ import annotations

import argparse
import json
import time
from pathlib import Path

import frida


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tmp_vector_probe" / "native_target_external_id_hunter_v1.js"
RUNNER_LOG = ROOT / "tmp_vector_probe" / "target_external_id_hunter_attach_runner_stdout.log"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("pid", type=int)
    parser.add_argument("--seconds", type=float, default=120.0)
    args = parser.parse_args()

    device = frida.get_local_device()
    session = device.attach(args.pid)
    messages = []

    def on_message(message, data):
        messages.append(message)

    script = session.create_script(SCRIPT.read_text(encoding="utf-8"))
    script.on("message", on_message)
    script.load()
    time.sleep(args.seconds)
    try:
        script.unload()
    except Exception as exc:  # pragma: no cover - diagnostic path
        messages.append({"type": "unload_error", "description": repr(exc)})
    session.detach()
    RUNNER_LOG.write_text(json.dumps({"pid": args.pid, "messages": messages}, indent=2), encoding="utf-8")
    print(json.dumps({"pid": args.pid, "messages": len(messages), "runner_log": str(RUNNER_LOG)}, indent=2))


if __name__ == "__main__":
    main()
