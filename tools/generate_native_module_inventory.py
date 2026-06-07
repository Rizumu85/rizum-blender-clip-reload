#!/usr/bin/env python3
"""Generate a static module/import/export inventory for CSP route discovery."""

from __future__ import annotations

import json
import struct
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_EXE = Path(
    r"C:\Program Files\CELSYS\CLIP STUDIO 1.5\CLIP STUDIO PAINT\CLIPStudioPaint.exe"
)
OUT_PATH = REPO_ROOT / "tmp_vector_probe" / "native_module_inventory_v1.json"
KEYWORDS = ("png", "zlib", "sqlite", "wic", "bitmap", "image", "render", "cache", "offscreen", "d2d", "d3d", "gdi")


def rva_to_offset(sections: list[dict[str, int | str]], rva: int) -> int | None:
    for sec in sections:
        va = int(sec["virtual_address"])
        span = max(int(sec["virtual_size"]), int(sec["raw_size"]))
        if va <= rva < va + span:
            return int(sec["raw_ptr"]) + (rva - va)
    return None


def read_c_string(data: bytes, off: int) -> str:
    end = data.find(b"\0", off)
    if end < 0:
        end = min(len(data), off + 256)
    return data[off:end].decode("utf-8", "replace")


def parse_pe(path: Path) -> dict[str, object]:
    data = path.read_bytes()
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    number_of_sections = struct.unpack_from("<H", data, pe + 6)[0]
    opt_size = struct.unpack_from("<H", data, pe + 20)[0]
    opt = pe + 24
    magic = struct.unpack_from("<H", data, opt)[0]
    is_pe32_plus = magic == 0x20B
    image_base = struct.unpack_from("<Q" if is_pe32_plus else "<I", data, opt + (24 if is_pe32_plus else 28))[0]
    data_dir = opt + (112 if is_pe32_plus else 96)
    export_rva, export_size = struct.unpack_from("<II", data, data_dir)
    import_rva, import_size = struct.unpack_from("<II", data, data_dir + 8)

    sec_off = opt + opt_size
    sections = []
    for i in range(number_of_sections):
        off = sec_off + i * 40
        name = data[off : off + 8].rstrip(b"\0").decode("ascii", "replace")
        virtual_size, virtual_address, raw_size, raw_ptr = struct.unpack_from("<IIII", data, off + 8)
        sections.append(
            {
                "name": name,
                "virtual_address": virtual_address,
                "virtual_size": virtual_size,
                "raw_ptr": raw_ptr,
                "raw_size": raw_size,
            }
        )

    imports = []
    imp_off = rva_to_offset(sections, import_rva) if import_rva else None
    if imp_off is not None:
        cursor = imp_off
        while cursor + 20 <= len(data):
            original_first_thunk, _, _, name_rva, first_thunk = struct.unpack_from("<IIIII", data, cursor)
            if not any((original_first_thunk, name_rva, first_thunk)):
                break
            name_off = rva_to_offset(sections, name_rva)
            dll = read_c_string(data, name_off) if name_off is not None else f"<rva 0x{name_rva:x}>"
            imports.append(dll)
            cursor += 20

    exports = []
    exp_off = rva_to_offset(sections, export_rva) if export_rva else None
    if exp_off is not None and export_size:
        try:
            (_, _, _, _, _, _, number_of_functions, number_of_names, _, names_rva, ordinals_rva) = struct.unpack_from(
                "<IIHHIIIIIII", data, exp_off
            )
            names_off = rva_to_offset(sections, names_rva)
            ord_off = rva_to_offset(sections, ordinals_rva)
            if names_off is not None and ord_off is not None:
                for i in range(number_of_names):
                    name_rva = struct.unpack_from("<I", data, names_off + i * 4)[0]
                    name_off = rva_to_offset(sections, name_rva)
                    if name_off is not None:
                        exports.append(read_c_string(data, name_off))
        except struct.error:
            exports = []

    keyword_imports = sorted({dll for dll in imports if any(k in dll.lower() for k in KEYWORDS)})
    keyword_exports = sorted({sym for sym in exports if any(k in sym.lower() for k in KEYWORDS)})
    return {
        "path": str(path),
        "size": len(data),
        "image_base": f"0x{image_base:x}",
        "sections": sections,
        "imported_dll_names": sorted(imports, key=str.lower),
        "keyword_imports": keyword_imports,
        "keyword_exports": keyword_exports,
        "keyword_presence": {key: any(key in item.lower() for item in imports + exports) for key in KEYWORDS},
    }


def main() -> int:
    inventory = parse_pe(DEFAULT_EXE)
    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(json.dumps(inventory, indent=2, sort_keys=True), encoding="utf-8")
    print(json.dumps(inventory, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
