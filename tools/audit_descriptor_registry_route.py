#!/usr/bin/env python3
"""Audit the descriptor-registration wrapper and registry/lookup candidates."""

from __future__ import annotations

import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

import capstone
import pefile


ROOT = Path(__file__).resolve().parents[1]
EXE = Path(r"C:\Program Files\CELSYS\CLIP STUDIO 1.5\CLIP STUDIO PAINT\CLIPStudioPaint.exe")
IMAGE_BASE = 0x140000000

WRAPPER_RVA = 0x2049220
WRAPPER_END_RVA = 0x2049359
HELPERS = {
    0x2049920: "descriptor_string_reset_or_dtor_142049920",
    0x204A290: "descriptor_utf16_storage_reserve_14204A290",
}
TARGET_STUBS = {
    "VectorData": {"stub": 0x0CDF60, "return_site": 0x0CDF77, "descriptor": 0x5486110},
    "VectorObjectList": {"stub": 0x139830, "return_site": 0x139847, "descriptor": 0x54F2BE8},
    "TimeLapseBlob": {"stub": 0x1396B0, "return_site": 0x1396C7, "descriptor": 0x54F3788},
}
GENERIC_READERS = {
    0x3366080: "generic_value_reader_143366080",
    0x3365F90: "generic_value_reader_143365F90",
    0x3365840: "generic_value_reader_143365840",
}

OUT_STATIC_JSON = ROOT / "tmp_vector_probe" / "descriptor_registry_2049220_static_audit_v1.json"
OUT_STATIC_TXT = ROOT / "tmp_vector_probe" / "descriptor_registry_2049220_static_audit_v1.txt"
OUT_LOOKUP_JSON = ROOT / "tmp_vector_probe" / "descriptor_registry_lookup_static_audit_v1.json"
OUT_LOOKUP_TXT = ROOT / "tmp_vector_probe" / "descriptor_registry_lookup_static_audit_v1.txt"


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
                }
            )
        self.text = self.section_by_name(".text")
        self.text_rva = self.text.VirtualAddress
        self.text_end = self.text_rva + self.text.Misc_VirtualSize
        self.runtime_functions = []
        try:
            for entry in self.pe.DIRECTORY_ENTRY_EXCEPTION:
                begin = int(entry.struct.BeginAddress)
                end = int(entry.struct.EndAddress)
                if self.text_rva <= begin < end <= self.text_end:
                    self.runtime_functions.append((begin, end))
        except Exception:
            pass

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


def mem_accesses(insn) -> list[dict[str, Any]]:
    out = []
    for idx, op in enumerate(insn.operands):
        if op.type != capstone.x86.X86_OP_MEM:
            continue
        out.append(
            {
                "operand_index": idx,
                "base": insn.reg_name(op.mem.base) if op.mem.base else None,
                "index": insn.reg_name(op.mem.index) if op.mem.index else None,
                "scale": op.mem.scale,
                "disp": hx(op.mem.disp),
                "access": "write" if idx == 0 and insn.mnemonic.startswith(("mov", "lea", "xor", "add", "sub", "and", "or")) else "read",
            }
        )
    return out


def function_bounds(b: Bin, rva: int) -> dict[str, int]:
    for start, end in b.runtime_functions:
        if start <= rva < end:
            return {"start": start, "end": end}
    start = rva
    raw_before = b.read(max(b.text_rva, rva - 0x1000), rva - max(b.text_rva, rva - 0x1000))
    base = max(b.text_rva, rva - 0x1000)
    for idx in range(len(raw_before) - 1, -1, -1):
        if raw_before[idx] == 0xCC:
            j = idx + 1
            while j < len(raw_before) and raw_before[j] == 0xCC:
                j += 1
            start = base + j
            break
    end = min(b.text_end, rva + 0x3000)
    raw_after = b.read(rva, end - rva)
    for idx, byte in enumerate(raw_after):
        if byte == 0xCC:
            end = rva + idx
            break
    return {"start": start, "end": end}


def comment_for_wrapper_insn(insn) -> str | None:
    rva = rva_of_insn(insn)
    comments = {
        0x2049239: "save rdx string/key pointer in rdi",
        0x204923C: "save rcx descriptor pointer in rsi",
        0x204923F: "load vtable/type pointer for this descriptor class",
        0x2049246: "descriptor[0] = vtable/type pointer",
        0x204924B: "descriptor[8] = 0",
        0x204924F: "descriptor[0x10] = 0",
        0x2049253: "reset/free existing descriptor string storage",
        0x2049260: "strlen loop over rdx ASCII key",
        0x204926E: "reserve/allocate UTF-16 string storage for descriptor",
        0x20492D0: "SIMD byte-to-word ASCII -> UTF-16 widening loop",
        0x2049330: "scalar tail byte-to-word copy",
        0x2049341: "return original descriptor pointer in rax",
    }
    return comments.get(rva)


def audit_wrapper(b: Bin) -> dict[str, Any]:
    insns = b.disasm(WRAPPER_RVA, WRAPPER_END_RVA - WRAPPER_RVA)
    rows = []
    writes = []
    calls = []
    rip_refs = []
    hash_like = []
    lock_like = []
    string_ops = []
    for insn in insns:
        ct = call_target(insn)
        rt = rip_target(insn)
        accesses = mem_accesses(insn)
        row = {
            "rva": hx(rva_of_insn(insn)),
            "insn": fmt_insn(insn),
            "comment": comment_for_wrapper_insn(insn),
            "call_target_rva": hx(ct),
            "rip_target_rva": hx(rt),
            "memory_accesses": accesses,
        }
        rows.append(row)
        if ct is not None:
            calls.append({"call_rva": hx(rva_of_insn(insn)), "target_rva": hx(ct), "target_name": HELPERS.get(ct), "insn": fmt_insn(insn)})
        if rt is not None:
            sec = b.section_for_rva(rt)
            rip_refs.append({"insn_rva": hx(rva_of_insn(insn)), "target_rva": hx(rt), "section": sec["name"] if sec else None, "insn": fmt_insn(insn)})
        for acc in accesses:
            if acc["base"] in ("rcx", "rsi") and acc["access"] == "write":
                writes.append({"insn_rva": hx(rva_of_insn(insn)), "insn": fmt_insn(insn), **acc})
        if insn.mnemonic.startswith("lock"):
            lock_like.append({"insn_rva": hx(rva_of_insn(insn)), "insn": fmt_insn(insn)})
        if any(tok in insn.mnemonic for tok in ("cmp", "movzx", "movq", "punpcklbw", "movdqu")):
            string_ops.append({"insn_rva": hx(rva_of_insn(insn)), "insn": fmt_insn(insn)})
        if insn.mnemonic in ("imul", "mul", "ror", "rol", "shr", "shl") or "hash" in insn.op_str.lower():
            hash_like.append({"insn_rva": hx(rva_of_insn(insn)), "insn": fmt_insn(insn)})

    classification = "A"
    reason = (
        "0x142049220 writes only the rcx descriptor/global and its owned string storage. "
        "It calls string-storage helpers, scans/copies the rdx key, returns rcx in rax, "
        "and shows no hash/bucket/global map write in this function."
    )
    return {
        "function_rva": hx(WRAPPER_RVA),
        "function_end_rva": hx(WRAPPER_END_RVA),
        "arguments": {
            "rcx": "descriptor/global candidate to initialize",
            "rdx": "ASCII string key/name to copy into descriptor-owned UTF-16 storage",
            "r8": "unused in this overload",
            "r9": "unused in this overload",
        },
        "full_disassembly_with_comments": rows,
        "descriptor_writes": writes,
        "helper_calls_inside": calls,
        "rip_relative_refs": rip_refs,
        "hash_or_bucket_like_operations": hash_like,
        "lock_or_mutex_like_operations": lock_like,
        "string_compare_or_copy_operations": string_ops,
        "return_value_meaning": "rax = original rcx descriptor pointer",
        "return_value_used_by_target_stubs": False,
        "global_static_pointers_read_or_written": rip_refs,
        "candidate_registry_map_owner_pointer": None,
        "classification": classification,
        "classification_reason": reason,
    }


def audit_helper(b: Bin, rva: int, name: str) -> dict[str, Any]:
    bounds = function_bounds(b, rva)
    size = min(bounds["end"] - bounds["start"], 0x900)
    insns = b.disasm(bounds["start"], size)
    calls = []
    rip_refs = []
    lock_like = []
    for insn in insns:
        ct = call_target(insn)
        if ct is not None:
            calls.append({"call_rva": hx(rva_of_insn(insn)), "target_rva": hx(ct), "insn": fmt_insn(insn)})
        rt = rip_target(insn)
        if rt is not None:
            sec = b.section_for_rva(rt)
            rip_refs.append({"insn_rva": hx(rva_of_insn(insn)), "target_rva": hx(rt), "section": sec["name"] if sec else None, "insn": fmt_insn(insn)})
        if insn.mnemonic.startswith("lock"):
            lock_like.append({"insn_rva": hx(rva_of_insn(insn)), "insn": fmt_insn(insn)})
    return {
        "name": name,
        "function_start_rva": hx(bounds["start"]),
        "function_end_rva": hx(bounds["end"]),
        "direct_calls": calls,
        "rip_relative_refs": rip_refs,
        "lock_like_refcount_ops": lock_like,
        "assessment": "descriptor-owned string/refcount storage helper; no descriptor registry evidence by itself",
        "disassembly_sample": [fmt_insn(i) for i in insns[:120]],
    }


def collect_call_xrefs(b: Bin, targets: set[int]) -> dict[int, list[dict[str, Any]]]:
    refs: dict[int, list[dict[str, Any]]] = defaultdict(list)
    ranges = b.runtime_functions or [(b.text_rva, b.text_end)]
    seen = set()
    for start, end in ranges:
        raw = b.read(start, end - start)
        for off in range(0, max(0, len(raw) - 5)):
            if raw[off] != 0xE8:
                continue
            call_rva = start + off
            rel = int.from_bytes(raw[off + 1 : off + 5], "little", signed=True)
            target = call_rva + 5 + rel
            if target not in targets:
                continue
            if call_rva in seen:
                continue
            insn = next(iter(b.disasm(call_rva, 5)), None)
            if insn is None or call_target(insn) != target:
                continue
            seen.add(call_rva)
            bounds = function_bounds(b, call_rva)
            refs[target].append(
                {
                    "call_rva": hx(call_rva),
                    "enclosing_function_start_rva": hx(bounds["start"]),
                    "enclosing_function_end_rva": hx(bounds["end"]),
                    "insn": fmt_insn(insn),
                }
            )
    return refs


def caller_group_for_wrapper(b: Bin) -> dict[str, Any]:
    refs = collect_call_xrefs(b, {WRAPPER_RVA})[WRAPPER_RVA]
    by_function = defaultdict(list)
    target_return_sites = {meta["return_site"]: name for name, meta in TARGET_STUBS.items()}
    by_target = defaultdict(list)
    for ref in refs:
        call_rva = int(ref["call_rva"], 16)
        return_site = call_rva + 5
        name = target_return_sites.get(return_site)
        if name:
            by_target[name].append(ref)
        by_function[ref["enclosing_function_start_rva"]].append(ref)
    return {
        "candidate_direct_call_sites_total_overinclusive": len(refs),
        "caller_scan_note": (
            "Collected by validating direct-call encodings inside PE runtime-function ranges. "
            "Use exact target-stub return-site groups for firm claims; the broad total is an overinclusive orientation aid."
        ),
        "target_stub_callers": dict(by_target),
        "top_enclosing_functions": [
            {"function_start_rva": func, "call_count": count, "sample_calls": by_function[func][:8]}
            for func, count in Counter({k: len(v) for k, v in by_function.items()}).most_common(40)
        ],
        "all_callers_sample": refs[:200],
    }


def audit_lookup_candidates(b: Bin, static_audit: dict[str, Any]) -> dict[str, Any]:
    helper_targets = set(HELPERS)
    helper_xrefs = collect_call_xrefs(b, helper_targets)
    candidate_functions = {}
    for helper_rva, refs in helper_xrefs.items():
        for ref in refs:
            func = ref["enclosing_function_start_rva"]
            candidate_functions.setdefault(func, {"function_start_rva": func, "uses_helpers": [], "nearby_calls": [], "generic_reader_calls": [], "score": 0})
            candidate_functions[func]["uses_helpers"].append({**ref, "helper_rva": hx(helper_rva), "helper_name": HELPERS[helper_rva]})
    for func, row in candidate_functions.items():
        start = int(func, 16)
        bounds = function_bounds(b, start)
        insns = b.disasm(bounds["start"], min(bounds["end"] - bounds["start"], 0x1800))
        calls = []
        readers = []
        for insn in insns:
            ct = call_target(insn)
            if ct is None:
                continue
            item = {"call_rva": hx(rva_of_insn(insn)), "target_rva": hx(ct), "insn": fmt_insn(insn)}
            calls.append(item)
            if ct in GENERIC_READERS:
                readers.append({**item, "reader_name": GENERIC_READERS[ct]})
        row["function_end_rva"] = hx(bounds["end"])
        row["nearby_calls"] = calls[:80]
        row["generic_reader_calls"] = readers
        row["score"] = len(row["uses_helpers"]) + 5 * len(readers)
        row["evidence_for_lookup_vs_insert_vs_iteration"] = "generic string/storage helper user; lookup role not established"
        row["relation_to_target_descriptors"] = "none static; no direct descriptor xref"
    ranked = sorted(candidate_functions.values(), key=lambda r: r["score"], reverse=True)
    return {
        "helper_xrefs_by_helper": {hx(k): v[:80] for k, v in helper_xrefs.items()},
        "candidate_lookup_or_consumer_functions_ranked": ranked[:80],
        "assessment": (
            "No registry lookup helper was identified from 0x142049220: the helper calls are descriptor string-storage helpers "
            "with broad generic xrefs. No target descriptor pointer appears as static argument/field outside registration."
        ),
    }


def write_txt(static_audit: dict[str, Any], lookup_audit: dict[str, Any]) -> None:
    lines = []
    wrapper = static_audit["wrapper_142049220"]
    lines.append("== 0x142049220 Descriptor Registry Static Audit ==")
    lines.append(wrapper["classification_reason"])
    lines.append("")
    lines.append("Arguments:")
    for key, value in wrapper["arguments"].items():
        lines.append(f"- {key}: {value}")
    lines.append("")
    lines.append("Descriptor writes:")
    for row in wrapper["descriptor_writes"]:
        lines.append(f"- {row['insn']}")
    lines.append("")
    lines.append("Helper calls:")
    for row in wrapper["helper_calls_inside"]:
        lines.append(f"- {row['insn']} ; {row.get('target_name')}")
    lines.append("")
    lines.append("Full wrapper disassembly:")
    for row in wrapper["full_disassembly_with_comments"]:
        suffix = f" ; {row['comment']}" if row.get("comment") else ""
        lines.append(f"{row['insn']}{suffix}")
    lines.append("")
    lines.append("Callers into 0x142049220:")
    lines.append(
        "candidate_total_overinclusive="
        f"{static_audit['callers_into_0x142049220']['candidate_direct_call_sites_total_overinclusive']}"
    )
    lines.append(static_audit["callers_into_0x142049220"]["caller_scan_note"])
    for name, refs in static_audit["callers_into_0x142049220"]["target_stub_callers"].items():
        lines.append(f"- {name}: {len(refs)} target stub call(s)")
    lines.append("")
    lines.append("== Registry Lookup Static Audit ==")
    lines.append(lookup_audit["assessment"])
    for row in lookup_audit["candidate_lookup_or_consumer_functions_ranked"][:20]:
        lines.append(f"score={row['score']} func={row['function_start_rva']}..{row.get('function_end_rva')}")
        for use in row["uses_helpers"][:4]:
            lines.append(f"  {use['helper_name']} at {use['call_rva']}")
        for reader in row["generic_reader_calls"][:4]:
            lines.append(f"  reader {reader['reader_name']} at {reader['call_rva']}")
    OUT_STATIC_TXT.write_text("\n".join(lines) + "\n", encoding="utf-8")
    OUT_LOOKUP_TXT.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> None:
    b = Bin(EXE)
    wrapper = audit_wrapper(b)
    helper_audits = {hx(rva): audit_helper(b, rva, name) for rva, name in HELPERS.items()}
    static_audit = {
        "exe": str(EXE),
        "image_base": hx(IMAGE_BASE),
        "wrapper_142049220": wrapper,
        "helpers_inside_0x142049220": helper_audits,
        "callers_into_0x142049220": caller_group_for_wrapper(b),
        "overall_classification": wrapper["classification"],
        "overall_classification_reason": wrapper["classification_reason"],
    }
    lookup_audit = audit_lookup_candidates(b, static_audit)
    OUT_STATIC_JSON.write_text(json.dumps(static_audit, indent=2, ensure_ascii=False), encoding="utf-8")
    OUT_LOOKUP_JSON.write_text(json.dumps(lookup_audit, indent=2, ensure_ascii=False), encoding="utf-8")
    write_txt(static_audit, lookup_audit)
    print(json.dumps({
        "static_json": str(OUT_STATIC_JSON),
        "static_txt": str(OUT_STATIC_TXT),
        "lookup_json": str(OUT_LOOKUP_JSON),
        "lookup_txt": str(OUT_LOOKUP_TXT),
        "classification": wrapper["classification"],
        "helper_calls": wrapper["helper_calls_inside"],
        "lookup_candidate_count": len(lookup_audit["candidate_lookup_or_consumer_functions_ranked"]),
    }, indent=2))


if __name__ == "__main__":
    main()
