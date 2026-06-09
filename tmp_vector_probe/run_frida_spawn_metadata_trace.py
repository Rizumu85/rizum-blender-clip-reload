"""Spawn CSP under Frida with native_sqlite_metadata_spawn_trace_v1.js loaded."""

from __future__ import annotations

import pathlib
import sys
import time

import frida


ROOT = pathlib.Path(__file__).resolve().parents[1]
EXE = pathlib.Path(r"C:\Program Files\CELSYS\CLIP STUDIO 1.5\CLIP STUDIO PAINT\CLIPStudioPaint.exe")
TRACE_JS = ROOT / "tmp_vector_probe" / "native_sqlite_metadata_spawn_trace_v1.js"
READY_PATH = ROOT / "tmp_vector_probe" / "frida_spawn_metadata_ready.json"
MESSAGE_LOG = ROOT / "tmp_vector_probe" / "frida_spawn_metadata_messages.log"


def main(argv: list[str]) -> int:
    seconds = int(argv[1]) if len(argv) > 1 else 60
    device = frida.get_local_device()
    pid = device.spawn([str(EXE)])
    session = device.attach(pid)
    source = TRACE_JS.read_text(encoding="utf-8")

    def on_message(message, data):
        with MESSAGE_LOG.open("a", encoding="utf-8") as f:
            f.write(repr(message) + "\n")

    script = session.create_script(source)
    script.on("message", on_message)
    script.load()
    READY_PATH.write_text(f'{{"pid": {pid}, "seconds": {seconds}}}', encoding="ascii")
    device.resume(pid)
    try:
        time.sleep(seconds)
    finally:
        session.detach()
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
