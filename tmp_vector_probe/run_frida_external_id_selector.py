"""Attach native_external_id_selector_trace_v1.js to a running CSP PID."""

from __future__ import annotations

import pathlib
import sys
import time

import frida


ROOT = pathlib.Path(__file__).resolve().parents[1]
TRACE_JS = ROOT / "tmp_vector_probe/native_external_id_selector_trace_v1.js"
READY_PATH = ROOT / "tmp_vector_probe/frida_external_id_selector_python_ready.txt"
MESSAGE_LOG = ROOT / "tmp_vector_probe/frida_external_id_selector_python_messages.log"


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print("usage: python tmp_vector_probe/run_frida_external_id_selector.py <pid> [seconds]", file=sys.stderr)
        return 2
    pid = int(argv[1])
    seconds = int(argv[2]) if len(argv) > 2 else 300
    source = TRACE_JS.read_text(encoding="utf-8")
    session = frida.attach(pid)

    def on_message(message, data):
        with MESSAGE_LOG.open("a", encoding="utf-8") as f:
            f.write(repr(message) + "\n")

    script = session.create_script(source)
    script.on("message", on_message)
    script.load()
    READY_PATH.write_text(str(pid), encoding="ascii")
    try:
        time.sleep(seconds)
    finally:
        session.detach()
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
