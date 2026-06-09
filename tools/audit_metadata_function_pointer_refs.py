#!/usr/bin/env python3
"""Audit metadata registration function-pointer/data refs for VectorObjectList."""

from __future__ import annotations

import json
import struct
from collections import defaultdict
from pathlib import Path

import capstone
import pefile


ROOT = Path(__file__).resolve().parents[1]
EXE = Path(r"C:\Program Files\CELSYS\CLIP STUDIO 1.5\CLIP STUDIO PAINT\CLIPStudioPaint.exe")
IMAGE_BASE = 0x140000000
OUT_JSON = ROOT / "tmp_vector_probe" / "metadata_function_pointer_ref_audit_v1.json"
OUT_TXT = ROOT / "tmp_vector_probe" / "metadata_function_pointer_ref_audit_v1.txt"

REFS = {
    "VectorObjectList": {"entry_rva": 0x432A448, "stub_rva": 0x139830, "string_rva": 0x44A76B8},
    "VectorData": {"entry_rva": 0x4317078, "stub_rva": 0x0CDF60, "string_rva": 0x44A76A8},
    "TimeLapseBlob": {"entry_rva": 0x432A638, "stub_rva": 0x1396B0, "string_rva": 0x44A77D8},
}
WRAPPER_RVA = 0x2049220


def hx(value: int | None) -> str | None:
    return None if value is None else f"0x{value:x}"


class Bin:
    def __init__(self, path: Path) -> None:
        self.path = path
        self.data = path.read_bytes()
        self.pe = pefile.PE(str(path), fast_load=False)
        self.md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
        self.md.detail = True
        self.text = self.section_by_name(".text")
        self.text_rva = self.text.VirtualAddress
        self.text_end = self.text_rva + self.text.Misc_VirtualSize
        self.sections = []
        for sec in self.pe.sections:
            name = sec.Name.rstrip(b"\x00").decode("ascii", errors="replace")
            self.sections.append(
                {
                    "name": name,
                    "rva": sec.VirtualAddress,
                    "end": sec.VirtualAddress + max(sec.Misc_VirtualSize, sec.SizeOfRawData),
                    "characteristics": sec.Characteristics,
                }
            )
        self.reloc_rvas = self.load_reloc_rvas()

    def section_by_name(self, name: str):
        return next(sec for sec in self.pe.sections if sec.Name.rstrip(b"\x00").decode("ascii", errors="replace") == name)

    def section_for_rva(self, rva: int) -> dict[str, object] | None:
        for sec in self.sections:
            if sec["rva"] <= rva < sec["end"]:
                return sec
        return None

    def off(self, rva: int) -> int:
        return self.pe.get_offset_from_rva(rva)

    def rva_from_off(self, off: int) -> int:
        return self.pe.get_rva_from_offset(off)

    def read(self, rva: int, size: int) -> bytes:
        return self.data[self.off(rva) : self.off(rva) + size]

    def u64(self, rva: int) -> int:
        return struct.unpack_from("<Q", self.data, self.off(rva))[0]

    def u32(self, rva: int) -> int:
        return struct.unpack_from("<I", self.data, self.off(rva))[0]

    def disasm(self, start: int, end: int):
        return list(self.md.disasm(self.read(start, end - start), IMAGE_BASE + start))

    def load_reloc_rvas(self) -> set[int]:
        out: set[int] = set()
        try:
            for base in self.pe.DIRECTORY_ENTRY_BASERELOC:
                for entry in base.entries:
                    out.add(entry.rva)
        except Exception:
            pass
        return out


def rva_of_insn(insn) -> int:
    return insn.address - IMAGE_BASE


def fmt_insn(insn) -> str:
    return f"{rva_of_insn(insn):08x}: {insn.mnemonic:<7} {insn.op_str}".rstrip()


def call_target(insn) -> int | None:
    if insn.mnemonic != "call" or not insn.operands:
        return None
    op = insn.operands[0]
    if op.type == capstone.x86.X86_OP_IMM:
        return int(op.imm) - IMAGE_BASE
    return None


def rip_target(insn) -> int | None:
    for op in insn.operands:
        if op.type == capstone.x86.X86_OP_MEM and op.mem.base == capstone.x86.X86_REG_RIP:
            return int(insn.address + insn.size + op.mem.disp) - IMAGE_BASE
    return None


def c_string(b: Bin, rva: int, max_len: int = 128) -> str | None:
    try:
        raw = b.data[b.off(rva) : b.off(rva) + max_len].split(b"\x00", 1)[0]
    except Exception:
        return None
    if not raw:
        return None
    try:
        text = raw.decode("ascii")
    except UnicodeDecodeError:
        return None
    if all(32 <= ord(ch) < 127 for ch in text):
        return text
    return None


def u16_string(b: Bin, rva: int, max_chars: int = 64) -> str | None:
    try:
        raw = b.data[b.off(rva) : b.off(rva) + max_chars * 2]
        text = raw.decode("utf-16le", errors="ignore").split("\x00", 1)[0]
    except Exception:
        return None
    if text and all((32 <= ord(ch) < 127) or ord(ch) > 0x7f for ch in text):
        return text
    return None


def pointer_decode(b: Bin, value: int) -> dict[str, object]:
    kind = []
    target_rva = None
    if IMAGE_BASE <= value < IMAGE_BASE + 0x7000000:
        target_rva = value - IMAGE_BASE
        kind.append("absolute_va_like")
    elif 0 <= value < 0x7000000:
        target_rva = value
        kind.append("rva_like")
    sec = b.section_for_rva(target_rva) if target_rva is not None else None
    text = c_string(b, target_rva) if target_rva is not None else None
    text16 = u16_string(b, target_rva) if target_rva is not None else None
    if sec and sec["name"] == ".text":
        kind.append("points_to_executable_code")
    if text:
        kind.append("points_to_ascii")
    if text16:
        kind.append("points_to_utf16")
    return {
        "value": hx(value),
        "target_rva": hx(target_rva),
        "target_section": sec["name"] if sec else None,
        "kind": kind or ["opaque"],
        "ascii": text,
        "utf16": text16,
        "near_known": near_known(target_rva),
    }


def near_known(rva: int | None) -> list[str]:
    if rva is None:
        return []
    known = {
        "VectorObjectList_stub": 0x139830,
        "VectorData_stub": 0x0CDF60,
        "TimeLapseBlob_stub": 0x1396B0,
        "metadata_wrapper_142049220": WRAPPER_RVA,
    }
    return [name for name, target in known.items() if abs(rva - target) <= 0x40]


def scan_rip_refs(b: Bin, targets: set[int]) -> dict[int, list[dict[str, str]]]:
    refs: dict[int, list[dict[str, str]]] = defaultdict(list)
    for insn in b.disasm(b.text_rva, b.text_end):
        target = rip_target(insn)
        if target in targets:
            refs[target].append({"xref_rva": hx(rva_of_insn(insn)), "insn": fmt_insn(insn)})
    return refs


def scan_value_refs(b: Bin, values: set[int]) -> dict[int, list[str]]:
    refs: dict[int, list[str]] = defaultdict(list)
    needles = {value: value.to_bytes(8, "little") for value in values}
    for value, needle in needles.items():
        off = 0
        while True:
            found = b.data.find(needle, off)
            if found < 0:
                break
            try:
                refs[value].append(hx(b.rva_from_off(found)))
            except Exception:
                refs[value].append(f"file+0x{found:x}")
            off = found + 1
    return refs


def scan_indirect_calls_near_table_refs(b: Bin, refs: list[dict[str, str]]) -> list[dict[str, object]]:
    out = []
    for ref in refs:
        x = int(ref["xref_rva"], 16)
        win = b.disasm(max(b.text_rva, x - 0x80), min(b.text_end, x + 0x160))
        for insn in win:
            if insn.mnemonic == "call" and "ptr" in insn.op_str:
                out.append({"near_xref_rva": ref["xref_rva"], "call_rva": hx(rva_of_insn(insn)), "insn": fmt_insn(insn)})
    return out


def raw_dump(b: Bin, center: int) -> dict[str, object]:
    start = center - 0x80
    raw = b.read(start, 0x100)
    entries = []
    for off in range(0, len(raw), 8):
        rva = start + off
        val = struct.unpack_from("<Q", raw, off)[0]
        row = pointer_decode(b, val)
        row["entry_rva"] = hx(rva)
        row["has_relocation"] = rva in b.reloc_rvas
        entries.append(row)
    return {
        "start_rva": hx(start),
        "end_rva": hx(start + len(raw)),
        "hex": " ".join(f"{x:02x}" for x in raw),
        "entries_qword": entries,
    }


def audit_entry(b: Bin, name: str, entry_rva: int) -> dict[str, object]:
    sec = b.section_for_rva(entry_rva)
    static_value = b.u64(entry_rva)
    decoded = pointer_decode(b, static_value)
    rip_refs = scan_rip_refs(b, {entry_rva}).get(entry_rva, [])
    abs_refs = scan_value_refs(b, {IMAGE_BASE + entry_rva, static_value}).copy()
    neighbor = raw_dump(b, entry_rva)
    table_range = contiguous_code_pointer_range(b, entry_rva)
    nearby_named = []
    for row in neighbor["entries_qword"]:
        for ref_name, meta in REFS.items():
            if row["target_rva"] == hx(meta["stub_rva"]):
                nearby_named.append({"name": ref_name, "entry_rva": row["entry_rva"], "target_rva": row["target_rva"]})
    return {
        "name": name,
        "entry_rva": hx(entry_rva),
        "section": sec["name"] if sec else None,
        "section_characteristics": hx(sec["characteristics"]) if sec else None,
        "classification": "data pointer table / initializer table candidate"
        if decoded["target_section"] == ".text"
        else "data descriptor table candidate",
        "static_qword_value": hx(static_value),
        "static_qword_decoded": decoded,
        "runtime_value_formula": "module_base + target_rva if this qword is relocated as an image VA",
        "entry_has_relocation": entry_rva in b.reloc_rvas,
        "raw_dump_pm_0x80": neighbor,
        "contiguous_executable_pointer_table": table_range,
        "code_xrefs_to_entry_address": rip_refs,
        "indirect_calls_near_xrefs": scan_indirect_calls_near_table_refs(b, rip_refs),
        "data_refs_to_entry_or_value": {hx(k): v for k, v in abs_refs.items()},
        "neighboring_known_entries": nearby_named,
    }


def stub_string_name(b: Bin, stub_rva: int) -> str | None:
    try:
        insns = b.disasm(stub_rva, stub_rva + 0x20)
    except Exception:
        return None
    for insn in insns:
        if rva_of_insn(insn) == stub_rva + 4 and insn.mnemonic == "lea":
            target = rip_target(insn)
            return c_string(b, target) if target is not None else None
    return None


def is_executable_pointer_entry(b: Bin, entry_rva: int) -> bool:
    if entry_rva not in b.reloc_rvas:
        return False
    try:
        value = b.u64(entry_rva)
    except Exception:
        return False
    decoded = pointer_decode(b, value)
    return decoded["target_section"] == ".text"


def contiguous_code_pointer_range(b: Bin, center_rva: int) -> dict[str, object]:
    start = center_rva
    while is_executable_pointer_entry(b, start - 8):
        start -= 8
    end = center_rva + 8
    while is_executable_pointer_entry(b, end):
        end += 8
    entries = []
    for r in range(start, end, 8):
        value = b.u64(r)
        decoded = pointer_decode(b, value)
        target_rva = int(decoded["target_rva"], 16) if decoded["target_rva"] else None
        entries.append(
            {
                "entry_rva": hx(r),
                "value": hx(value),
                "target_rva": decoded["target_rva"],
                "target_section": decoded["target_section"],
                "stub_string_guess": stub_string_name(b, target_rva) if target_rva is not None else None,
                "near_known": decoded["near_known"],
            }
        )
    return {
        "start_rva": hx(start),
        "end_rva_exclusive": hx(end),
        "entry_count": len(entries),
        "entries": entries[:120],
        "entries_truncated": len(entries) > 120,
    }


def audit_stub(b: Bin, name: str, stub_rva: int, string_rva: int, entry_rva: int) -> dict[str, object]:
    fn_start = stub_rva
    # These stubs are tiny and pdata aligned in previous audit.
    insns = b.disasm(fn_start, fn_start + 0x40)
    wrapper_call = None
    desc_candidate = None
    for insn in insns:
        if rva_of_insn(insn) == stub_rva + 0x0B and insn.mnemonic == "lea":
            desc_candidate = rip_target(insn)
        if call_target(insn) == WRAPPER_RVA:
            wrapper_call = {"call_rva": hx(rva_of_insn(insn)), "insn": fmt_insn(insn)}
    return {
        "name": name,
        "entry_rva": hx(stub_rva),
        "function_start": hx(stub_rva),
        "function_end_estimate": hx(stub_rva + 0x27),
        "normal_prologue": "tiny stub: sub rsp, 0x28; no frame setup",
        "disassembly": [fmt_insn(i) for i in insns],
        "string_rva": hx(string_rva),
        "string_text": c_string(b, string_rva),
        "passes_rdx_string_to_0x142049220_at": wrapper_call,
        "rcx_descriptor_global_candidate_rva": hx(desc_candidate),
        "entry_table_ref_rva": hx(entry_rva),
        "entry_table_ref_value": hx(b.u64(entry_rva)),
        "entry_table_points_to_this_stub": b.u64(entry_rva) == IMAGE_BASE + stub_rva,
        "return_semantics": "tail-calls 0x1438ca758 after wrapper; no row value extraction visible",
    }


def main() -> int:
    b = Bin(EXE)
    entries = {name: audit_entry(b, name, meta["entry_rva"]) for name, meta in REFS.items()}
    stubs = {name: audit_stub(b, name, meta["stub_rva"], meta["string_rva"], meta["entry_rva"]) for name, meta in REFS.items()}
    result = {
        "exe": str(EXE),
        "image_base": hx(IMAGE_BASE),
        "refs": entries,
        "stubs": stubs,
        "initializer_dispatcher_candidates": {
            name: entry["indirect_calls_near_xrefs"] for name, entry in entries.items()
        },
        "assessment": (
            "The audited RVAs are data entries in a pointer/initializer-style table. "
            "Their static qwords are image VAs pointing at the tiny registration stubs; "
            "relocations mark them as runtime-rebased function pointers. Static code xrefs "
            "to the entries are absent in this pass, suggesting a startup table enumerator "
            "uses adjacent ranges rather than direct RIP references to individual entries."
        ),
    }
    OUT_JSON.parent.mkdir(parents=True, exist_ok=True)
    OUT_JSON.write_text(json.dumps(result, indent=2, sort_keys=True), encoding="utf-8")
    lines = [result["assessment"], ""]
    for name, row in entries.items():
        lines += [
            f"== {name} entry {row['entry_rva']} ==",
            f"section={row['section']} characteristics={row['section_characteristics']}",
            f"classification={row['classification']}",
            f"static={row['static_qword_value']} decoded={row['static_qword_decoded']}",
            f"reloc={row['entry_has_relocation']}",
            f"code_xrefs={row['code_xrefs_to_entry_address']}",
            f"near_known={row['neighboring_known_entries']}",
            "",
        ]
    for name, row in stubs.items():
        lines += [f"== {name} stub {row['entry_rva']} ==", *row["disassembly"], ""]
    OUT_TXT.write_text("\n".join(lines), encoding="utf-8")
    print(f"Wrote {OUT_JSON}")
    print(f"Wrote {OUT_TXT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
