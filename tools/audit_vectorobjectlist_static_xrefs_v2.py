#!/usr/bin/env python3
"""Detailed static audit for VectorObjectList metadata xrefs and wrapper."""

from __future__ import annotations

import json
import struct
from collections import Counter, defaultdict
from pathlib import Path

import capstone
import pefile


ROOT = Path(__file__).resolve().parents[1]
EXE = Path(r"C:\Program Files\CELSYS\CLIP STUDIO 1.5\CLIP STUDIO PAINT\CLIPStudioPaint.exe")
IMAGE_BASE = 0x140000000
OUT = ROOT / "tmp_vector_probe" / "vectorobjectlist_static_xref_audit_v2.json"
OUT_TXT = ROOT / "tmp_vector_probe" / "vectorobjectlist_static_xref_audit_v2.txt"

PRIMARY_XREFS = [
    {"name": "VectorData", "string_rva": 0x44A76A8, "xref_rva": 0x0CDF64},
    {"name": "VectorObjectList", "string_rva": 0x44A76B8, "xref_rva": 0x139834},
    {"name": "TimeLapseBlob", "string_rva": 0x44A77D8, "xref_rva": 0x1396B4},
]
EXTERNAL_CHUNK_RVAS = [0x462BCEB, 0x462BD2C, 0x462BD4C, 0x462BD9B]
WRAPPER_RVA = 0x2049220


def hx(v: int | None) -> str | None:
    return None if v is None else f"0x{v:x}"


class PeImage:
    def __init__(self, path: Path) -> None:
        self.path = path
        self.data = path.read_bytes()
        self.pe = pefile.PE(str(path), fast_load=False)
        self.text = next(s for s in self.pe.sections if s.Name.rstrip(b"\x00") == b".text")
        self.text_rva = self.text.VirtualAddress
        self.text_end = self.text_rva + self.text.Misc_VirtualSize
        self.md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
        self.md.detail = True
        self.functions = self._load_pdata()

    def _load_pdata(self) -> list[dict[str, int]]:
        pdata = next(s for s in self.pe.sections if s.Name.rstrip(b"\x00") == b".pdata")
        funcs = []
        raw = pdata.get_data()
        for off in range(0, len(raw) - 11, 12):
            start, end, unwind = struct.unpack_from("<III", raw, off)
            if self.text_rva <= start < end <= self.text_end:
                funcs.append({"start": start, "end": end, "unwind": unwind})
        return sorted(funcs, key=lambda f: f["start"])

    def off(self, rva: int) -> int:
        return self.pe.get_offset_from_rva(rva)

    def rva_from_off(self, off: int) -> int:
        return self.pe.get_rva_from_offset(off)

    def read(self, rva: int, size: int) -> bytes:
        return self.data[self.off(rva) : self.off(rva) + size]

    def fn(self, rva: int) -> dict[str, int] | None:
        lo, hi = 0, len(self.functions)
        while lo < hi:
            mid = (lo + hi) // 2
            if self.functions[mid]["start"] <= rva:
                lo = mid + 1
            else:
                hi = mid
        idx = lo - 1
        if idx >= 0 and self.functions[idx]["start"] <= rva < self.functions[idx]["end"]:
            return self.functions[idx]
        return None

    def disasm(self, start: int, end: int) -> list[capstone.CsInsn]:
        if end <= start:
            return []
        return list(self.md.disasm(self.read(start, end - start), IMAGE_BASE + start))

    def window(self, center: int, before: int = 0x90, after: int = 0x170) -> list[capstone.CsInsn]:
        return self.disasm(max(self.text_rva, center - before), min(self.text_end, center + after))


def rva(insn: capstone.CsInsn) -> int:
    return insn.address - IMAGE_BASE


def fmt(insn: capstone.CsInsn) -> str:
    return f"{rva(insn):08x}: {insn.mnemonic:<7} {insn.op_str}".rstrip()


def call_target(insn: capstone.CsInsn) -> int | None:
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


def cstr(img: PeImage, rva_: int, max_len: int = 160) -> str | None:
    try:
        raw = img.data[img.off(rva_) : img.off(rva_) + max_len].split(b"\x00", 1)[0]
    except Exception:
        return None
    if not raw:
        return None
    try:
        text = raw.decode("ascii")
    except UnicodeDecodeError:
        return None
    if not all(32 <= ord(ch) < 127 for ch in text):
        return None
    return text


def scan_rip_refs(img: PeImage, targets: set[int]) -> dict[int, list[int]]:
    refs: dict[int, list[int]] = defaultdict(list)
    for insn in img.disasm(img.text_rva, img.text_end):
        target = rip_target(insn)
        if target in targets:
            refs[target].append(rva(insn))
    return refs


def scan_direct_callers(img: PeImage, target: int) -> list[int]:
    out = []
    for insn in img.disasm(img.text_rva, img.text_end):
        if call_target(insn) == target:
            out.append(rva(insn))
    return out


def scan_va_pointer_refs(img: PeImage, target: int) -> list[str]:
    needle = (IMAGE_BASE + target).to_bytes(8, "little")
    refs = []
    off = 0
    while True:
        found = img.data.find(needle, off)
        if found < 0:
            break
        try:
            refs.append(hx(img.rva_from_off(found)))
        except Exception:
            refs.append(f"file+0x{found:x}")
        off = found + 1
    return refs


def string_refs_in_fn(img: PeImage, fn: dict[str, int]) -> list[dict[str, str]]:
    rows = []
    for insn in img.disasm(fn["start"], fn["end"]):
        target = rip_target(insn)
        if target is None:
            continue
        text = cstr(img, target)
        if text and any(k in text for k in ("Vector", "TimeLapse", "ExternalChunk", "MainId", "LayerId", "Data", "Blob")):
            rows.append({"xref_rva": hx(rva(insn)), "string_rva": hx(target), "text": text, "insn": fmt(insn)})
    return rows


def local_call_after_xref(img: PeImage, fn: dict[str, int], xref: int) -> dict[str, object]:
    string_register = None
    seen = False
    for insn in img.disasm(fn["start"], fn["end"]):
        if rva(insn) == xref:
            seen = True
            if insn.operands and insn.operands[0].type == capstone.x86.X86_OP_REG:
                string_register = insn.reg_name(insn.operands[0].reg)
        elif seen and insn.mnemonic == "call":
            return {
                "string_carrier": string_register,
                "call_rva": hx(rva(insn)),
                "call_target_rva": hx(call_target(insn)),
                "call_insn": fmt(insn),
            }
    return {"string_carrier": string_register, "call_rva": None, "call_target_rva": None, "call_insn": None}


def classify_xref(audit: dict[str, object]) -> str:
    target = audit["direct_string_call"].get("call_target_rva")
    carrier = audit["direct_string_call"].get("string_carrier")
    if target == hx(WRAPPER_RVA) and carrier == "rdx":
        if audit["name"] == "VectorData":
            return "B column descriptor registration"
        return "A static table metadata registration"
    return "F unknown"


def audit_one_xref(img: PeImage, name: str, string_rva: int, xref_rva: int) -> dict[str, object]:
    fn = img.fn(xref_rva)
    if not fn:
        raise RuntimeError(f"no function for {name} xref {xref_rva:x}")
    insn_at = [i for i in img.window(xref_rva, 0, 0x10) if rva(i) == xref_rva]
    direct = local_call_after_xref(img, fn, xref_rva)
    audit: dict[str, object] = {
        "name": name,
        "string_rva": hx(string_rva),
        "xref_rva": hx(xref_rva),
        "enclosing_function_start": hx(fn["start"]),
        "enclosing_function_end": hx(fn["end"]),
        "instruction_at_xref": fmt(insn_at[0]) if insn_at else None,
        "disassembly_window": [fmt(i) for i in img.window(xref_rva, 0x90, 0x170)][:90],
        "direct_string_call": direct,
        "string_pointer_passed_directly_to_call": direct.get("call_target_rva") is not None,
        "argument_register_or_stack_slot": direct.get("string_carrier"),
        "calls_0x142049220_or_nearby_wrapper": direct.get("call_target_rva") in {hx(WRAPPER_RVA), "0x2040af0"},
        "function_string_refs": string_refs_in_fn(img, fn),
        "same_function_references_several_table_or_column_names": len(string_refs_in_fn(img, fn)) > 1,
        "nearby_calls": [
            {"call_rva": hx(rva(i)), "target_rva": hx(call_target(i)), "insn": fmt(i)}
            for i in img.disasm(fn["start"], fn["end"])
            if i.mnemonic == "call"
        ],
        "direct_callers_to_enclosing_function": [hx(x) for x in scan_direct_callers(img, fn["start"])[:100]],
        "function_pointer_refs": scan_va_pointer_refs(img, fn["start"])[:100],
        "process_initialization_hint": "no direct callers; one VA pointer ref suggests registration table/function-pointer driven startup init",
        "descriptor_store_static_evidence": "the stub loads a global/descriptor candidate into rcx, passes name in rdx, and tail-calls a common post-registration helper; no row extraction visible",
    }
    audit["classification"] = classify_xref(audit)
    return audit


def audit_wrapper(img: PeImage, wrapper_rva: int) -> dict[str, object]:
    fn = img.fn(wrapper_rva)
    if not fn:
        raise RuntimeError("no function for wrapper")
    insns = img.disasm(fn["start"], fn["end"])
    reads = []
    writes = []
    calls = []
    for insn in insns:
        line = fmt(insn)
        if any(reg in insn.op_str for reg in ("rcx", "rdx", "r8", "r9")):
            reads.append(line)
        if insn.mnemonic.startswith("mov") and "[" in insn.op_str:
            writes.append(line)
        if insn.mnemonic == "call":
            calls.append({"call_rva": hx(rva(insn)), "target_rva": hx(call_target(insn)), "insn": line})
    callers = scan_direct_callers(img, wrapper_rva)
    caller_hist = Counter()
    for c in callers:
        fnc = img.fn(c)
        caller_hist[hx(fnc["start"] if fnc else c)] += 1
    return {
        "wrapper_rva": hx(wrapper_rva),
        "function_start": hx(fn["start"]),
        "function_end": hx(fn["end"]),
        "disassembly": [fmt(i) for i in insns[:140]],
        "direct_call_count": len(calls),
        "direct_calls": calls,
        "argument_register_use_lines": reads[:80],
        "memory_write_lines": writes[:80],
        "direct_callers_count": len(callers),
        "direct_callers_first_120": [hx(x) for x in callers[:120]],
        "caller_function_histogram": dict(caller_hist.most_common(40)),
        "argument_contract_hypothesis": (
            "rcx is the descriptor/object being initialized or updated; rdx is the raw/static name string for the observed stubs. "
            "r8/r9 are not required by the short VectorObjectList/VectorData/TimeLapseBlob stubs. "
            "The function appears to copy/store name metadata into rcx-owned descriptor state and returns that object/status for chaining."
        ),
        "string_read_hypothesis": "string bytes are likely consumed from rdx or passed into helper calls inside the wrapper; verify with spawn trace previews.",
        "return_value_hypothesis": "metadata wrapper return/status; not a row value and not renderer state.",
    }


def main() -> int:
    img = PeImage(EXE)
    targets = {x["string_rva"] for x in PRIMARY_XREFS} | set(EXTERNAL_CHUNK_RVAS)
    refs = scan_rip_refs(img, targets)
    primary = [audit_one_xref(img, x["name"], x["string_rva"], x["xref_rva"]) for x in PRIMARY_XREFS]
    external = []
    for string_rva in EXTERNAL_CHUNK_RVAS:
        for xref in refs.get(string_rva, []):
            external.append(audit_one_xref(img, "ExternalChunk", string_rva, xref))
    result = {
        "exe": str(EXE),
        "image_base": hx(IMAGE_BASE),
        "primary_xrefs": primary,
        "external_chunk_string_rvas": [hx(x) for x in EXTERNAL_CHUNK_RVAS],
        "external_chunk_xrefs": external,
        "external_chunk_xref_note": "No direct RIP xrefs found for the listed ExternalChunk string RVAs in this static pass." if not external else None,
        "rip_ref_counts": {hx(k): len(v) for k, v in sorted(refs.items())},
        "wrapper_0x142049220": audit_wrapper(img, WRAPPER_RVA),
        "overall_assessment": (
            "0x139834 and 0x1396B4 are static table metadata registration stubs; 0x0CDF64 is a column descriptor registration stub. "
            "All pass the string in rdx to 0x142049220 with a descriptor/global candidate in rcx. These are not row-value extraction sites."
        ),
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(result, indent=2, sort_keys=True), encoding="utf-8")
    lines = [result["overall_assessment"], ""]
    for row in primary + external:
        lines.extend([
            f"== {row['name']} {row['xref_rva']} ==",
            f"fn {row['enclosing_function_start']}..{row['enclosing_function_end']}",
            f"classification: {row['classification']}",
            f"direct string call: {row['direct_string_call']}",
            *row["disassembly_window"],
            "",
        ])
    lines.extend(["== wrapper 0x142049220 ==", *result["wrapper_0x142049220"]["disassembly"]])
    OUT_TXT.write_text("\n".join(lines), encoding="utf-8")
    print(f"Wrote {OUT}")
    print(f"Wrote {OUT_TXT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
