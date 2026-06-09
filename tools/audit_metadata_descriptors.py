#!/usr/bin/env python3
"""Audit metadata descriptor globals and their static consumers.

This is intentionally static/read-only. It turns the known metadata registration
stubs into concrete descriptor/global RVAs, then searches for non-registration
uses of those descriptor globals.
"""

from __future__ import annotations

import json
import struct
from collections import defaultdict
from pathlib import Path
from typing import Any

import capstone
import pefile


ROOT = Path(__file__).resolve().parents[1]
EXE = Path(r"C:\Program Files\CELSYS\CLIP STUDIO 1.5\CLIP STUDIO PAINT\CLIPStudioPaint.exe")
IMAGE_BASE = 0x140000000
WRAPPER_RVA = 0x2049220
GENERIC_READERS = {
    0x3366080: "generic_value_reader_143366080",
    0x3365F90: "generic_value_reader_143365F90",
    0x3365840: "generic_value_reader_143365840",
}

TARGETS = {
    "VectorData": {"stub_rva": 0x0CDF60, "entry_ref_rva": 0x4317078},
    "VectorObjectList": {"stub_rva": 0x139830, "entry_ref_rva": 0x432A448},
    "TimeLapseBlob": {"stub_rva": 0x1396B0, "entry_ref_rva": 0x432A638},
}

OUT_STATIC_JSON = ROOT / "tmp_vector_probe" / "metadata_descriptor_static_audit_v1.json"
OUT_STATIC_TXT = ROOT / "tmp_vector_probe" / "metadata_descriptor_static_audit_v1.txt"
OUT_CONSUMER_JSON = ROOT / "tmp_vector_probe" / "vectorobjectlist_descriptor_consumer_static_audit_v1.json"
OUT_CONSUMER_TXT = ROOT / "tmp_vector_probe" / "vectorobjectlist_descriptor_consumer_static_audit_v1.txt"


def hx(value: int | None) -> str | None:
    return None if value is None else f"0x{value:x}"


class Bin:
    def __init__(self, path: Path) -> None:
        self.path = path
        self.data = path.read_bytes()
        self.pe = pefile.PE(str(path), fast_load=False)
        self.md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
        self.md.detail = True
        self.sections = []
        for sec in self.pe.sections:
            name = sec.Name.rstrip(b"\x00").decode("ascii", errors="replace")
            self.sections.append(
                {
                    "name": name,
                    "rva": sec.VirtualAddress,
                    "end": sec.VirtualAddress + max(sec.Misc_VirtualSize, sec.SizeOfRawData),
                    "characteristics": sec.Characteristics,
                    "raw_start": sec.PointerToRawData,
                    "raw_end": sec.PointerToRawData + sec.SizeOfRawData,
                }
            )
        self.text = self.section_by_name(".text")
        self.text_rva = self.text.VirtualAddress
        self.text_end = self.text_rva + self.text.Misc_VirtualSize

    def section_by_name(self, name: str):
        return next(sec for sec in self.pe.sections if sec.Name.rstrip(b"\x00").decode("ascii", errors="replace") == name)

    def section_for_rva(self, rva: int | None) -> dict[str, Any] | None:
        if rva is None:
            return None
        for sec in self.sections:
            if sec["rva"] <= rva < sec["end"]:
                return sec
        return None

    def off(self, rva: int) -> int:
        return self.pe.get_offset_from_rva(rva)

    def read(self, rva: int, size: int) -> bytes:
        return self.data[self.off(rva) : self.off(rva) + size]

    def u64(self, rva: int) -> int:
        return struct.unpack_from("<Q", self.data, self.off(rva))[0]

    def disasm(self, start: int, size: int):
        return list(self.md.disasm(self.read(start, size), IMAGE_BASE + start))


def rva_of_insn(insn) -> int:
    return int(insn.address - IMAGE_BASE)


def fmt_insn(insn) -> str:
    return f"{rva_of_insn(insn):08x}: {insn.mnemonic:<8} {insn.op_str}".rstrip()


def call_target(insn) -> int | None:
    if insn.mnemonic != "call" or not insn.operands:
        return None
    op = insn.operands[0]
    if op.type == capstone.x86.X86_OP_IMM:
        return int(op.imm - IMAGE_BASE)
    return None


def rip_target(insn) -> int | None:
    for op in insn.operands:
        if op.type == capstone.x86.X86_OP_MEM and op.mem.base == capstone.x86.X86_REG_RIP:
            return int(insn.address + insn.size + op.mem.disp - IMAGE_BASE)
    return None


def reg_name(insn, reg_id: int) -> str:
    return insn.reg_name(reg_id)


def write_kind(insn) -> str:
    if insn.mnemonic.startswith("call"):
        return "call"
    if insn.operands and insn.operands[0].type == capstone.x86.X86_OP_MEM:
        return "write" if insn.mnemonic.startswith(("mov", "lea", "xor", "and", "or", "add", "sub")) else "memory-destination"
    return "read-or-address"


def c_string(b: Bin, rva: int | None, max_len: int = 160) -> str | None:
    if rva is None:
        return None
    try:
        raw = b.data[b.off(rva) : b.off(rva) + max_len].split(b"\x00", 1)[0]
        text = raw.decode("ascii")
    except Exception:
        return None
    if text and all(32 <= ord(ch) < 127 for ch in text):
        return text
    return None


def function_bounds(b: Bin, rva: int) -> dict[str, Any]:
    start = rva
    # Most tiny registration stubs are padded by int3; for arbitrary xrefs this
    # gives a useful enclosing block, not a guaranteed compiler function.
    scan_start = max(b.text_rva, rva - 0x1000)
    raw = b.read(scan_start, rva - scan_start)
    for idx in range(len(raw) - 1, -1, -1):
        if raw[idx] == 0xCC:
            j = idx + 1
            while j < len(raw) and raw[j] == 0xCC:
                j += 1
            start = scan_start + j
            break
    end = min(b.text_end, rva + 0x1000)
    raw2 = b.read(rva, end - rva)
    for idx, byte in enumerate(raw2):
        if byte == 0xCC:
            end = rva + idx
            break
    return {"start_rva": start, "end_rva": end, "size": max(0, end - start)}


def find_stub_operands(b: Bin, name: str, stub_rva: int) -> dict[str, Any]:
    bounds = function_bounds(b, stub_rva)
    insns = b.disasm(bounds["start_rva"], bounds["end_rva"] - bounds["start_rva"])
    wrapper_calls = []
    last_rcx = None
    last_rdx = None
    for insn in insns:
        target = rip_target(insn)
        if insn.mnemonic == "lea" and target is not None and insn.operands:
            dst = reg_name(insn, insn.operands[0].reg)
            if dst == "rcx":
                last_rcx = {"insn_rva": hx(rva_of_insn(insn)), "insn": fmt_insn(insn), "target_rva": target}
            elif dst == "rdx":
                last_rdx = {"insn_rva": hx(rva_of_insn(insn)), "insn": fmt_insn(insn), "target_rva": target}
        ct = call_target(insn)
        if ct == WRAPPER_RVA:
            wrapper_calls.append(
                {
                    "call_rva": hx(rva_of_insn(insn)),
                    "call_insn": fmt_insn(insn),
                    "rcx_setup": last_rcx,
                    "rdx_setup": last_rdx,
                    "string_preview": c_string(b, last_rdx["target_rva"] if last_rdx else None),
                }
            )
    desc_rva = wrapper_calls[0]["rcx_setup"]["target_rva"] if wrapper_calls and wrapper_calls[0].get("rcx_setup") else None
    desc_sec = b.section_for_rva(desc_rva)
    return {
        "name": name,
        "stub_entry_rva": hx(stub_rva),
        "function_start_rva": hx(bounds["start_rva"]),
        "function_end_rva": hx(bounds["end_rva"]),
        "has_normal_prologue": bool(insns and insns[0].mnemonic == "sub" and "rsp" in insns[0].op_str),
        "calls_to_0x142049220": wrapper_calls,
        "descriptor_global_candidate_rva": hx(desc_rva),
        "descriptor_global_candidate_va": hx(IMAGE_BASE + desc_rva) if desc_rva is not None else None,
        "descriptor_candidate_section": desc_sec["name"] if desc_sec else None,
        "descriptor_candidate_section_characteristics": hx(desc_sec["characteristics"]) if desc_sec else None,
        "rcx_meaning": "RIP-relative global descriptor address" if desc_rva is not None else "unknown",
        "descriptor_stability_static_assessment": "module-owned static RVA; runtime pointer is module_base+rva and should be stable within one process",
        "return_value_used_by_stub": False,
        "return_value_note": "The call return is not stored; the stub immediately loads another rcx value, restores rsp, then tail-jumps.",
        "disassembly": [fmt_insn(i) for i in insns[:24]],
    }


def wrapper_static_audit(b: Bin) -> dict[str, Any]:
    insns = b.disasm(WRAPPER_RVA, 0x140)
    writes = []
    calls = []
    for insn in insns:
        if insn.mnemonic == "mov" and insn.operands and insn.operands[0].type == capstone.x86.X86_OP_MEM:
            mem = insn.operands[0].mem
            if mem.base in (capstone.x86.X86_REG_RCX, capstone.x86.X86_REG_RSI):
                writes.append(
                    {
                        "insn_rva": hx(rva_of_insn(insn)),
                        "insn": fmt_insn(insn),
                        "base": reg_name(insn, mem.base),
                        "offset": hx(mem.disp),
                    }
                )
        ct = call_target(insn)
        if ct is not None:
            calls.append({"call_rva": hx(rva_of_insn(insn)), "target_rva": hx(ct), "insn": fmt_insn(insn)})
    return {
        "function_rva": hx(WRAPPER_RVA),
        "known_end_rva": hx(0x2049359),
        "descriptor_field_writes_static": writes,
        "direct_calls": calls,
        "contract_assessment": (
            "rcx is copied to rsi and returned in rax. The wrapper writes a vtable-like pointer to [rcx], clears "
            "[rcx+8] and [rcx+0x10], calls 0x142049920, allocates/copies an UTF-16 expansion of the rdx ASCII name, "
            "and does not look like row extraction."
        ),
        "disassembly": [fmt_insn(i) for i in insns],
    }


def collect_text_insns(b: Bin):
    return b.disasm(b.text_rva, b.text_end - b.text_rva)


def audit_descriptor_consumers(b: Bin, descriptors: dict[str, int]) -> dict[str, Any]:
    targets = set(descriptors.values())
    refs: dict[str, list[dict[str, Any]]] = {name: [] for name in descriptors}
    function_groups: dict[str, dict[str, Any]] = {}
    insns = collect_text_insns(b)
    for insn in insns:
        target = rip_target(insn)
        if target not in targets:
            continue
        for name, desc_rva in descriptors.items():
            if target != desc_rva:
                continue
            xref_rva = rva_of_insn(insn)
            bounds = function_bounds(b, xref_rva)
            refs[name].append(
                {
                    "xref_rva": hx(xref_rva),
                    "insn": fmt_insn(insn),
                    "access_kind": write_kind(insn),
                    "enclosing_function_start_rva": hx(bounds["start_rva"]),
                    "enclosing_function_end_rva": hx(bounds["end_rva"]),
                    "is_registration_stub": abs(bounds["start_rva"] - TARGETS[name]["stub_rva"]) <= 0x20,
                }
            )
            key = hx(bounds["start_rva"])
            if key not in function_groups:
                function_groups[key] = {
                    "function_start_rva": hx(bounds["start_rva"]),
                    "function_end_rva": hx(bounds["end_rva"]),
                    "uses": [],
                    "nearby_calls": [],
                    "mentions_generic_readers": [],
                    "strings_nearby": [],
                    "classification": "unknown",
                }
            function_groups[key]["uses"].append({"descriptor": name, "xref_rva": hx(xref_rva), "insn": fmt_insn(insn)})

    for key, group in function_groups.items():
        start = int(group["function_start_rva"], 16)
        end = int(group["function_end_rva"], 16)
        if end <= start or end - start > 0x4000:
            end = min(b.text_end, start + 0x1000)
        win = b.disasm(start, end - start)
        calls = []
        readers = []
        for insn in win:
            ct = call_target(insn)
            if ct is None:
                continue
            row = {"call_rva": hx(rva_of_insn(insn)), "target_rva": hx(ct), "insn": fmt_insn(insn)}
            calls.append(row)
            if ct in GENERIC_READERS:
                readers.append({**row, "name": GENERIC_READERS[ct]})
        group["nearby_calls"] = calls[:80]
        group["mentions_generic_readers"] = readers
        if any(not use.get("is_registration_stub") for use in refs.get("VectorObjectList", []) + refs.get("VectorData", [])):
            group["classification"] = "descriptor user candidate"
        if any(use["descriptor"] == "TimeLapseBlob" for use in group["uses"]) and not any(
            use["descriptor"] in ("VectorObjectList", "VectorData") for use in group["uses"]
        ):
            group["classification"] = "TimeLapseBlob-only comparison user"

    non_registration = {
        name: [row for row in rows if not row["is_registration_stub"]]
        for name, rows in refs.items()
    }
    ranked = []
    for group in function_groups.values():
        score = 0
        score += 5 * sum(1 for use in group["uses"] if use["descriptor"] in ("VectorObjectList", "VectorData"))
        score += 3 * len(group["mentions_generic_readers"])
        score += min(5, len(group["nearby_calls"]))
        ranked.append({**group, "score": score})
    ranked.sort(key=lambda item: item["score"], reverse=True)
    return {
        "descriptor_rvas": {name: hx(rva) for name, rva in descriptors.items()},
        "xrefs_by_descriptor": refs,
        "non_registration_xrefs_by_descriptor": non_registration,
        "candidate_consumers_ranked": ranked[:80],
        "assessment": (
            "Direct RIP-relative descriptor xrefs are registration-heavy if non_registration_xrefs_by_descriptor is empty. "
            "That points toward registry/map insertion and lookup rather than direct static descriptor consumers."
        ),
    }


def write_txt(static_audit: dict[str, Any], consumer_audit: dict[str, Any]) -> None:
    lines = []
    lines.append("== Metadata Descriptor Static Audit ==")
    lines.append(static_audit["assessment"])
    lines.append("")
    for name, row in static_audit["stubs"].items():
        lines.append(f"== {name} stub {row['stub_entry_rva']} ==")
        lines.append(f"function={row['function_start_rva']}..{row['function_end_rva']}")
        lines.append(f"descriptor={row['descriptor_global_candidate_rva']} section={row['descriptor_candidate_section']}")
        for call in row["calls_to_0x142049220"]:
            lines.append(f"wrapper call {call['call_rva']}: {call['call_insn']}")
            lines.append(f"  rcx: {call['rcx_setup']['insn']} -> {hex(call['rcx_setup']['target_rva'])}")
            lines.append(f"  rdx: {call['rdx_setup']['insn']} -> {hex(call['rdx_setup']['target_rva'])} '{call['string_preview']}'")
        lines.extend("  " + insn for insn in row["disassembly"][:10])
        lines.append("")
    lines.append("== 0x142049220 wrapper writes ==")
    for write in static_audit["wrapper_142049220"]["descriptor_field_writes_static"]:
        lines.append(f"{write['insn']} ; base={write['base']} offset={write['offset']}")
    lines.append("")
    lines.append("== Descriptor Consumer Static Audit ==")
    lines.append(consumer_audit["assessment"])
    for name, rows in consumer_audit["non_registration_xrefs_by_descriptor"].items():
        lines.append(f"{name}: non-registration xrefs={len(rows)}")
        for row in rows[:20]:
            lines.append(f"  {row['xref_rva']} {row['insn']} in {row['enclosing_function_start_rva']}..{row['enclosing_function_end_rva']}")
    lines.append("")
    lines.append("Top candidate consumers:")
    for row in consumer_audit["candidate_consumers_ranked"][:20]:
        lines.append(f"score={row['score']} func={row['function_start_rva']}..{row['function_end_rva']} class={row['classification']}")
        for use in row["uses"][:6]:
            lines.append(f"  {use['descriptor']} {use['xref_rva']} {use['insn']}")
        for reader in row["mentions_generic_readers"][:4]:
            lines.append(f"  reader {reader['name']} {reader['call_rva']}")
    OUT_STATIC_TXT.write_text("\n".join(lines) + "\n", encoding="utf-8")
    OUT_CONSUMER_TXT.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> None:
    b = Bin(EXE)
    stubs = {name: find_stub_operands(b, name, meta["stub_rva"]) for name, meta in TARGETS.items()}
    descriptors = {
        name: int(row["descriptor_global_candidate_rva"], 16)
        for name, row in stubs.items()
        if row.get("descriptor_global_candidate_rva")
    }
    static_audit = {
        "exe": str(EXE),
        "image_base": hx(IMAGE_BASE),
        "assessment": (
            "The registration stubs pass RIP-relative module globals in rcx and ASCII table/column names in rdx "
            "to 0x142049220. The wrapper returns the same descriptor pointer in rax; the stubs do not store it."
        ),
        "stubs": stubs,
        "wrapper_142049220": wrapper_static_audit(b),
    }
    consumer_audit = audit_descriptor_consumers(b, descriptors)
    OUT_STATIC_JSON.write_text(json.dumps(static_audit, indent=2, ensure_ascii=False), encoding="utf-8")
    OUT_CONSUMER_JSON.write_text(json.dumps(consumer_audit, indent=2, ensure_ascii=False), encoding="utf-8")
    write_txt(static_audit, consumer_audit)
    print(json.dumps({
        "static_json": str(OUT_STATIC_JSON),
        "static_txt": str(OUT_STATIC_TXT),
        "consumer_json": str(OUT_CONSUMER_JSON),
        "consumer_txt": str(OUT_CONSUMER_TXT),
        "descriptors": {name: hx(rva) for name, rva in descriptors.items()},
        "non_registration_xrefs": {
            name: len(rows) for name, rows in consumer_audit["non_registration_xrefs_by_descriptor"].items()
        },
    }, indent=2))


if __name__ == "__main__":
    main()
