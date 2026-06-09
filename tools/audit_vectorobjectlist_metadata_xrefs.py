#!/usr/bin/env python3
"""Audit VectorObjectList/VectorData metadata string xrefs in CLIPStudioPaint.exe."""

from __future__ import annotations

import json
import struct
from collections import Counter, defaultdict
from pathlib import Path

import capstone
import pefile


REPO_ROOT = Path(__file__).resolve().parents[1]
EXE = Path(r"C:\Program Files\CELSYS\CLIP STUDIO 1.5\CLIP STUDIO PAINT\CLIPStudioPaint.exe")
IMAGE_BASE = 0x140000000
OUT_JSON = REPO_ROOT / "tmp_vector_probe" / "vectorobjectlist_metadata_xref_audit_v1.json"
OUT_TXT = REPO_ROOT / "tmp_vector_probe" / "capstone_vectorobjectlist_metadata_xref_audit.txt"

PRIMARY_REFS = {
    "VectorData": {"string_rva": 0x44A76A8, "xref_rva": 0x0CDF64},
    "VectorObjectList": {"string_rva": 0x44A76B8, "xref_rva": 0x139834},
    "TimeLapseBlob": {"string_rva": 0x44A77D8, "xref_rva": 0x1396B4},
}
EXTERNAL_CHUNK_STRING_RVAS = [0x462BCEB, 0x462BD2C, 0x462BD4C, 0x462BD9B]
INTERESTING_NAMES = {
    "VectorData",
    "VectorObjectList",
    "TimeLapseBlob",
    "ExternalChunk",
    "MainId",
    "LayerId",
    "Object",
    "Id",
}


def hx(value: int | None) -> str | None:
    return None if value is None else f"0x{value:x}"


def read_c_string(data: bytes, off: int, max_len: int = 96) -> str | None:
    if off < 0 or off >= len(data):
        return None
    raw = data[off : off + max_len].split(b"\x00", 1)[0]
    if not raw:
        return None
    try:
        text = raw.decode("ascii")
    except UnicodeDecodeError:
        return None
    if not all((32 <= ord(ch) < 127) for ch in text):
        return None
    return text


class Binary:
    def __init__(self, path: Path) -> None:
        self.path = path
        self.pe = pefile.PE(str(path), fast_load=False)
        self.image = path.read_bytes()
        self.text = next(sec for sec in self.pe.sections if sec.Name.rstrip(b"\x00") == b".text")
        self.text_rva = self.text.VirtualAddress
        self.text_bytes = self.text.get_data()
        self.text_end = self.text_rva + self.text.Misc_VirtualSize
        self.md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
        self.md.detail = True
        self.functions = self._load_pdata_functions()
        self.function_by_start = {fn["start"]: fn for fn in self.functions}

    def _load_pdata_functions(self) -> list[dict[str, int]]:
        pdata = next(sec for sec in self.pe.sections if sec.Name.rstrip(b"\x00") == b".pdata")
        raw = pdata.get_data()
        funcs: list[dict[str, int]] = []
        for off in range(0, len(raw) - 11, 12):
            start, end, unwind = struct.unpack_from("<III", raw, off)
            if start == 0 and end == 0:
                continue
            if self.text_rva <= start < end <= self.text_end:
                funcs.append({"start": start, "end": end, "unwind": unwind})
        funcs.sort(key=lambda row: row["start"])
        return funcs

    def rva_to_file_offset(self, rva: int) -> int:
        return self.pe.get_offset_from_rva(rva)

    def function_for_rva(self, rva: int) -> dict[str, int] | None:
        lo, hi = 0, len(self.functions)
        while lo < hi:
            mid = (lo + hi) // 2
            if self.functions[mid]["start"] <= rva:
                lo = mid + 1
            else:
                hi = mid
        idx = lo - 1
        if idx >= 0:
            fn = self.functions[idx]
            if fn["start"] <= rva < fn["end"]:
                return fn
        return None

    def bytes_for_rva(self, rva: int, size: int) -> bytes:
        off = self.rva_to_file_offset(rva)
        return self.image[off : off + size]

    def disasm_range(self, start_rva: int, end_rva: int) -> list[capstone.CsInsn]:
        code = self.bytes_for_rva(start_rva, max(0, end_rva - start_rva))
        return list(self.md.disasm(code, IMAGE_BASE + start_rva))

    def disasm_window(self, center_rva: int, before: int = 0x80, after: int = 0x100) -> list[capstone.CsInsn]:
        start = max(self.text_rva, center_rva - before)
        end = min(self.text_end, center_rva + after)
        return self.disasm_range(start, end)


def insn_rva(insn: capstone.CsInsn) -> int:
    return insn.address - IMAGE_BASE


def fmt_insn(insn: capstone.CsInsn) -> str:
    return f"{insn_rva(insn):08x}: {insn.mnemonic:<6} {insn.op_str}".rstrip()


def direct_call_target(insn: capstone.CsInsn) -> int | None:
    if insn.mnemonic != "call" or not insn.operands:
        return None
    op = insn.operands[0]
    if op.type == capstone.x86.X86_OP_IMM:
        return int(op.imm) - IMAGE_BASE
    return None


def rip_target(insn: capstone.CsInsn) -> int | None:
    for op in insn.operands:
        if op.type == capstone.x86.X86_OP_MEM and op.mem.base == capstone.x86.X86_REG_RIP:
            return int(insn.address + insn.size + op.mem.disp) - IMAGE_BASE
    return None


def scan_direct_callers(binary: Binary, target_rva: int) -> list[int]:
    callers: list[int] = []
    for insn in binary.disasm_range(binary.text_rva, binary.text_end):
        if direct_call_target(insn) == target_rva:
            callers.append(insn_rva(insn))
    return callers


def scan_rip_refs(binary: Binary, target_rvas: set[int]) -> dict[int, list[int]]:
    refs: dict[int, list[int]] = defaultdict(list)
    for insn in binary.disasm_range(binary.text_rva, binary.text_end):
        target = rip_target(insn)
        if target in target_rvas:
            refs[target].append(insn_rva(insn))
    return refs


def strings_referenced_in_function(binary: Binary, fn: dict[str, int]) -> list[dict[str, str]]:
    out = []
    for insn in binary.disasm_range(fn["start"], fn["end"]):
        target = rip_target(insn)
        if target is None:
            continue
        try:
            text = read_c_string(binary.image, binary.rva_to_file_offset(target))
        except Exception:
            text = None
        if not text:
            continue
        if text in INTERESTING_NAMES or text.startswith(("Vector", "TimeLapse", "External", "Layer", "Main")):
            out.append({"xref_rva": hx(insn_rva(insn)), "string_rva": hx(target), "text": text, "insn": fmt_insn(insn)})
    return out


def local_string_call(binary: Binary, xref_rva: int) -> dict[str, object]:
    """Track the RIP-loaded string register to the next nearby call."""
    fn = binary.function_for_rva(xref_rva)
    if fn is not None:
        window = binary.disasm_range(fn["start"], fn["end"])
    else:
        window = binary.disasm_window(xref_rva, before=0x10, after=0x80)
    string_reg = None
    after = False
    for insn in window:
        if insn_rva(insn) == xref_rva:
            after = True
            if insn.operands and insn.operands[0].type == capstone.x86.X86_OP_REG:
                string_reg = insn.reg_name(insn.operands[0].reg)
        elif after and insn.mnemonic == "call":
            return {
                "string_register": string_reg,
                "call_rva": hx(insn_rva(insn)),
                "call_target_rva": hx(direct_call_target(insn)),
                "call_insn": fmt_insn(insn),
            }
    return {"string_register": string_reg, "call_rva": None, "call_target_rva": None, "call_insn": None}


def classify_usage(strings: list[dict[str, str]], calls: list[dict[str, str]], local: dict[str, object]) -> str:
    names = {row["text"] for row in strings}
    target = local.get("call_target_rva")
    call_count = len(calls)
    if {"VectorObjectList", "TimeLapseBlob"} & names and len(names) >= 4:
        return "A table/schema registration"
    if "VectorData" in names and ("MainId" in names or "LayerId" in names):
        return "A table/schema registration"
    if target == "0x2049220" and call_count <= 2:
        return "A table/schema registration"
    if target and call_count < 8:
        return "F unknown metadata-like wrapper"
    return "F unknown"


def audit_xref(binary: Binary, label: str, xref_rva: int, string_rva: int) -> dict[str, object]:
    fn = binary.function_for_rva(xref_rva)
    if fn is None:
        raise RuntimeError(f"no function for RVA 0x{xref_rva:x}")
    window = [fmt_insn(insn) for insn in binary.disasm_window(xref_rva, before=0x90, after=0x120)]
    fn_insns = binary.disasm_range(fn["start"], fn["end"])
    calls = [
        {"call_rva": hx(insn_rva(insn)), "target_rva": hx(direct_call_target(insn)), "insn": fmt_insn(insn)}
        for insn in fn_insns
        if insn.mnemonic == "call"
    ]
    string_refs = strings_referenced_in_function(binary, fn)
    local = local_string_call(binary, xref_rva)
    callers = scan_direct_callers(binary, fn["start"])
    function_va_le = (IMAGE_BASE + fn["start"]).to_bytes(8, "little")
    function_pointer_offsets = []
    search_at = 0
    while True:
        found = binary.image.find(function_va_le, search_at)
        if found < 0:
            break
        try:
            function_pointer_offsets.append(hx(binary.pe.get_rva_from_offset(found)))
        except Exception:
            function_pointer_offsets.append(f"file+0x{found:x}")
        search_at = found + 1
    return {
        "label": label,
        "string_rva": hx(string_rva),
        "xref_rva": hx(xref_rva),
        "function_start": hx(fn["start"]),
        "function_end": hx(fn["end"]),
        "local_string_call": local,
        "nearby_calls": calls,
        "function_string_refs": string_refs,
        "same_function_references_table_and_column_names": any(row["text"] == "VectorObjectList" for row in string_refs)
        and any(row["text"] == "VectorData" for row in string_refs),
        "direct_callers_to_function": [hx(rva) for rva in callers[:100]],
        "direct_caller_count": len(callers),
        "function_pointer_references": function_pointer_offsets[:100],
        "function_pointer_reference_count": len(function_pointer_offsets),
        "disassembly_window": window,
        "usage_classification": classify_usage(string_refs, calls, local),
        "run_frequency_hypothesis": "likely database/schema initialization, not per-row extraction"
        if len(string_refs) >= 3
        else "unknown",
        "metadata_or_consumer_hypothesis": "generic ORM/table metadata setup"
        if len(string_refs) >= 3
        else "unknown",
    }


def summarize_common_wrappers(audits: list[dict[str, object]]) -> dict[str, object]:
    local_targets = Counter()
    function_starts = Counter()
    call_targets = Counter()
    for audit in audits:
        if audit["local_string_call"].get("call_target_rva"):
            local_targets[audit["local_string_call"]["call_target_rva"]] += 1
        function_starts[audit["function_start"]] += 1
        for call in audit["nearby_calls"]:
            if call["target_rva"]:
                call_targets[call["target_rva"]] += 1
    return {
        "common_local_string_wrapper_targets": dict(local_targets.most_common()),
        "shared_enclosing_functions": dict(function_starts.most_common()),
        "common_call_targets_inside_enclosing_functions": dict(call_targets.most_common(20)),
        "assessment": (
            "VectorObjectList/TimeLapseBlob table strings and VectorData column string are all used in "
            "short metadata-like wrappers that pass the string in RDX to a common callee; descriptor "
            "consumption is not proven by these static xrefs."
        ),
    }


def main() -> int:
    binary = Binary(EXE)
    target_rvas = {row["string_rva"] for row in PRIMARY_REFS.values()} | set(EXTERNAL_CHUNK_STRING_RVAS)
    refs = scan_rip_refs(binary, target_rvas)

    audits: list[dict[str, object]] = []
    for label, row in PRIMARY_REFS.items():
        audits.append(audit_xref(binary, label, row["xref_rva"], row["string_rva"]))

    external_audits = []
    for string_rva in EXTERNAL_CHUNK_STRING_RVAS:
        for xref_rva in refs.get(string_rva, []):
            external_audits.append(audit_xref(binary, "ExternalChunk", xref_rva, string_rva))

    result = {
        "exe": str(EXE),
        "image_base": hx(IMAGE_BASE),
        "primary_audits": audits,
        "external_chunk_audits": external_audits,
        "rip_ref_counts": {hx(k): len(v) for k, v in sorted(refs.items())},
        "common_metadata_path": summarize_common_wrappers(audits + external_audits),
    }

    OUT_JSON.parent.mkdir(parents=True, exist_ok=True)
    OUT_JSON.write_text(json.dumps(result, indent=2, sort_keys=True), encoding="utf-8")

    lines = [
        "VectorObjectList / VectorData / TimeLapseBlob metadata xref audit",
        f"EXE: {EXE}",
        "",
    ]
    for audit in audits + external_audits:
        lines.extend(
            [
                f"== {audit['label']} xref {audit['xref_rva']} ==",
                f"function: {audit['function_start']}..{audit['function_end']}",
                f"usage: {audit['usage_classification']}",
                f"local string call: {audit['local_string_call']}",
                f"function strings: {[row['text'] for row in audit['function_string_refs']]}",
                f"direct callers ({audit['direct_caller_count']}): {audit['direct_callers_to_function'][:20]}",
                "disassembly:",
                *audit["disassembly_window"],
                "",
            ]
        )
    lines.append(json.dumps(result["common_metadata_path"], indent=2, sort_keys=True))
    OUT_TXT.write_text("\n".join(lines), encoding="utf-8")
    print(f"Wrote {OUT_JSON}")
    print(f"Wrote {OUT_TXT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
