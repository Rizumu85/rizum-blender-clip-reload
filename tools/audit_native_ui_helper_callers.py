#!/usr/bin/env python3
"""Static audit for recurring helper/UI caller RVAs seen in prior traces."""

from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
EXE = Path(r"C:\Program Files\CELSYS\CLIP STUDIO 1.5\CLIP STUDIO PAINT\CLIPStudioPaint.exe")
R2 = Path(r"C:\Users\Rizum\tools\radare2-6.1.4\radare2-6.1.4-w64\bin\radare2.exe")
OUT_PATH = REPO_ROOT / "tmp_vector_probe" / "native_ui_helper_caller_static_audit_v1.json"
CALLER_RVAS = [0x32C895C, 0x1A7C19D, 0x1965D1A, 0x32D1D2A, 0x32E9286]
IMPORT_HINTS = (
    "CreateFile",
    "ReadFile",
    "MapView",
    "BitBlt",
    "StretchBlt",
    "AlphaBlend",
    "DIB",
    "Bitmap",
    "png",
    "sqlite",
    "cache",
    "render",
    "image",
    "SendMessage",
    "CallWindowProc",
)


def run_r2(commands: list[str]) -> str:
    cmd = ";".join(commands + ["q"])
    proc = subprocess.run(
        [str(R2), "-nq", "-B", "0x140000000", "-c", cmd, str(EXE)],
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    return strip_ansi(proc.stdout)


def strip_ansi(text: str) -> str:
    return re.sub(r"\x1b\[[0-9;]*m", "", text)


def classify(disasm: str, strings: list[str]) -> str:
    hay = (disasm + "\n" + "\n".join(strings)).lower()
    if any(token.lower() in hay for token in ("sendmessage", "callwindowproc", "defwindowproc", "dispatchmessage")):
        return "UI/message plumbing"
    if any(token.lower() in hay for token in ("createfile", "readfile", "mapview", "sqlite")):
        return "file I/O"
    if any(token.lower() in hay for token in ("bitblt", "dib", "bitmap", "wic", "d2d", "d3d", "image", "cache", "offscreen")):
        return "image/cache/offscreen"
    if any(token.lower() in hay for token in ("png", "zlib", "encoder")):
        return "PNG/export"
    return "unknown"


def direct_callees(disasm: str) -> list[str]:
    callees = []
    for line in disasm.splitlines():
        if " call " not in f" {line} ":
            continue
        m = re.search(r"call\s+([^\s;]+)", line)
        if m:
            callees.append(m.group(1))
    return sorted(set(callees))


def main() -> int:
    audits = []
    for rva in CALLER_RVAS:
        va = 0x140000000 + rva
        disasm = run_r2([f"s 0x{va - 0x180:x}", "pd 220"])
        callees = direct_callees(disasm)
        hints = [hint for hint in IMPORT_HINTS if hint.lower() in disasm.lower()]
        audits.append(
            {
                "caller_rva": f"0x{rva:x}",
                "caller_va": f"0x{va:x}",
                "enclosing_function_start": "unknown_static_first_pass",
                "enclosing_function_end": "unknown_static_first_pass",
                "disassembly_window": disasm,
                "direct_callees": callees,
                "import_or_string_hints": hints,
                "nearby_string_xrefs": [],
                "classification": classify(disasm, hints),
            }
        )
    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(json.dumps({"audits": audits}, indent=2, sort_keys=True), encoding="utf-8")
    print(json.dumps({"audits": audits}, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
