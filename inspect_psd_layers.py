from __future__ import annotations

import argparse
import json
import struct
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import numpy as np
from PIL import Image


@dataclass
class ChannelInfo:
    channel_id: int
    length: int
    data_offset: int | None = None


@dataclass
class LayerInfo:
    index: int
    name: str
    top: int
    left: int
    bottom: int
    right: int
    blend_mode: str
    opacity: int
    clipping: int
    flags: int
    channels: list[ChannelInfo]
    tags: dict[str, Any]


class Reader:
    def __init__(self, data: bytes):
        self.data = data
        self.pos = 0

    def read(self, size: int) -> bytes:
        if self.pos + size > len(self.data):
            raise ValueError("Unexpected end of PSD data.")
        out = self.data[self.pos : self.pos + size]
        self.pos += size
        return out

    def skip(self, size: int) -> None:
        self.read(size)

    def u8(self) -> int:
        return self.read(1)[0]

    def i16(self) -> int:
        return struct.unpack(">h", self.read(2))[0]

    def u16(self) -> int:
        return struct.unpack(">H", self.read(2))[0]

    def i32(self) -> int:
        return struct.unpack(">i", self.read(4))[0]

    def u32(self) -> int:
        return struct.unpack(">I", self.read(4))[0]


def _padded(size: int, multiple: int) -> int:
    return (size + multiple - 1) // multiple * multiple


def _read_pascal_name(reader: Reader) -> str:
    start = reader.pos
    size = reader.u8()
    raw = reader.read(size)
    consumed = 1 + size
    pad = _padded(consumed, 4) - consumed
    if pad:
        reader.skip(pad)
    try:
        return raw.decode("macroman").rstrip("\x00")
    except UnicodeDecodeError:
        return raw.decode("latin1", errors="replace").rstrip("\x00")


def _read_unicode_string(payload: bytes) -> str | None:
    if len(payload) < 4:
        return None
    count = struct.unpack_from(">I", payload, 0)[0]
    raw = payload[4 : 4 + count * 2]
    if len(raw) != count * 2:
        return None
    return raw.decode("utf-16-be", errors="replace").rstrip("\x00")


def _read_layer_record(reader: Reader, index: int) -> LayerInfo:
    top = reader.i32()
    left = reader.i32()
    bottom = reader.i32()
    right = reader.i32()
    channel_count = reader.u16()
    channels = [
        ChannelInfo(channel_id=reader.i16(), length=reader.u32())
        for _ in range(channel_count)
    ]
    blend_sig = reader.read(4)
    blend_mode = reader.read(4).decode("latin1", errors="replace")
    if blend_sig not in (b"8BIM", b"8B64"):
        blend_mode = f"?{blend_mode}"
    opacity = reader.u8()
    clipping = reader.u8()
    flags = reader.u8()
    reader.u8()

    extra_len = reader.u32()
    extra_end = reader.pos + extra_len
    mask_len = reader.u32()
    if mask_len:
        reader.skip(mask_len)
    ranges_len = reader.u32()
    if ranges_len:
        reader.skip(ranges_len)
    name = _read_pascal_name(reader)

    tags: dict[str, Any] = {}
    while reader.pos + 12 <= extra_end:
        sig = reader.read(4)
        key = reader.read(4).decode("latin1", errors="replace")
        size = reader.u32()
        payload = reader.read(size)
        if size % 2:
            reader.skip(1)
        if sig not in (b"8BIM", b"8B64"):
            tags.setdefault("_bad_sig", []).append(sig.decode("latin1", errors="replace"))
        if key == "luni":
            uni = _read_unicode_string(payload)
            if uni:
                name = uni
                tags[key] = uni
            else:
                tags[key] = {"length": size}
        elif key in ("lsct", "lsdk") and size >= 4:
            tags[key] = {"section_type": struct.unpack_from(">I", payload, 0)[0]}
        else:
            tags[key] = {"length": size}

    if reader.pos != extra_end:
        reader.pos = extra_end

    return LayerInfo(
        index=index,
        name=name,
        top=top,
        left=left,
        bottom=bottom,
        right=right,
        blend_mode=blend_mode,
        opacity=opacity,
        clipping=clipping,
        flags=flags,
        channels=channels,
        tags=tags,
    )


def _packbits_decode(payload: bytes, expected: int) -> bytes:
    out = bytearray()
    i = 0
    while i < len(payload) and len(out) < expected:
        n = struct.unpack("b", payload[i : i + 1])[0]
        i += 1
        if 0 <= n <= 127:
            count = n + 1
            out.extend(payload[i : i + count])
            i += count
        elif -127 <= n <= -1:
            count = 1 - n
            if i >= len(payload):
                break
            out.extend(payload[i : i + 1] * count)
            i += 1
        else:
            pass
    if len(out) < expected:
        out.extend(b"\x00" * (expected - len(out)))
    return bytes(out[:expected])


def _decode_channel(data: bytes, layer: LayerInfo, channel: ChannelInfo) -> np.ndarray:
    width = max(0, layer.right - layer.left)
    height = max(0, layer.bottom - layer.top)
    expected = width * height
    if expected == 0 or channel.data_offset is None:
        return np.zeros((height, width), dtype=np.uint8)

    start = channel.data_offset
    payload = data[start : start + channel.length]
    if len(payload) < 2:
        return np.zeros((height, width), dtype=np.uint8)

    compression = struct.unpack_from(">H", payload, 0)[0]
    body = payload[2:]
    if compression == 0:
        raw = body[:expected].ljust(expected, b"\x00")
    elif compression == 1:
        table_size = height * 2
        if len(body) < table_size:
            raw = b"\x00" * expected
        else:
            counts = struct.unpack(f">{height}H", body[:table_size]) if height else ()
            pos = table_size
            rows = []
            for count in counts:
                rows.append(_packbits_decode(body[pos : pos + count], width))
                pos += count
            raw = b"".join(rows)
    else:
        raw = b"\x00" * expected
    return np.frombuffer(raw[:expected], dtype=np.uint8).reshape((height, width)).copy()


def parse_psd(path: Path) -> tuple[dict[str, Any], list[LayerInfo], bytes]:
    data = path.read_bytes()
    reader = Reader(data)
    if reader.read(4) != b"8BPS":
        raise ValueError(f"{path} is not a PSD/PSB file.")
    version = reader.u16()
    reader.skip(6)
    channels = reader.u16()
    height = reader.u32()
    width = reader.u32()
    depth = reader.u16()
    color_mode = reader.u16()
    header = {
        "path": str(path),
        "version": version,
        "channels": channels,
        "width": width,
        "height": height,
        "depth": depth,
        "color_mode": color_mode,
    }

    reader.skip(reader.u32())
    reader.skip(reader.u32())
    layer_mask_len = reader.u32()
    layer_mask_end = reader.pos + layer_mask_len
    layers: list[LayerInfo] = []
    if layer_mask_len:
        layer_info_len = reader.u32()
        layer_info_end = reader.pos + layer_info_len
        if layer_info_len:
            raw_count = reader.i16()
            layer_count = abs(raw_count)
            header["layer_count_raw"] = raw_count
            for idx in range(layer_count):
                layers.append(_read_layer_record(reader, idx))
            channel_pos = reader.pos
            for layer in layers:
                for channel in layer.channels:
                    channel.data_offset = channel_pos
                    channel_pos += channel.length
            reader.pos = layer_info_end
    reader.pos = layer_mask_end
    return header, layers, data


def _layer_rgba(data: bytes, layer: LayerInfo) -> np.ndarray:
    width = max(0, layer.right - layer.left)
    height = max(0, layer.bottom - layer.top)
    rgba = np.zeros((height, width, 4), dtype=np.uint8)
    decoded = {ch.channel_id: _decode_channel(data, layer, ch) for ch in layer.channels}
    for channel_id, rgba_idx in ((0, 0), (1, 1), (2, 2)):
        if channel_id in decoded:
            rgba[..., rgba_idx] = decoded[channel_id]
    if -1 in decoded:
        rgba[..., 3] = decoded[-1]
    elif width and height:
        rgba[..., 3] = 255
    return rgba


def _channel_summary(data: bytes, layer: LayerInfo, channel: ChannelInfo) -> dict[str, Any]:
    arr = _decode_channel(data, layer, channel)
    if arr.size == 0:
        return {"id": channel.channel_id, "length": channel.length, "shape": list(arr.shape)}
    nonzero = arr > 0
    return {
        "id": channel.channel_id,
        "length": channel.length,
        "shape": list(arr.shape),
        "min": int(arr.min()),
        "max": int(arr.max()),
        "mean": round(float(arr.mean()), 6),
        "nonzero": int(nonzero.sum()),
    }


def inspect(path: Path, export_dir: Path | None = None) -> dict[str, Any]:
    header, layers, data = parse_psd(path)
    out_layers = []
    if export_dir is not None:
        export_dir.mkdir(parents=True, exist_ok=True)
    for layer in layers:
        rgba = _layer_rgba(data, layer)
        alpha = rgba[..., 3] if rgba.size else np.zeros((0, 0), dtype=np.uint8)
        entry = {
            "index": layer.index,
            "name": layer.name,
            "bbox": [layer.left, layer.top, layer.right, layer.bottom],
            "blend_mode": layer.blend_mode,
            "opacity": layer.opacity,
            "clipping": layer.clipping,
            "flags": layer.flags,
            "tags": layer.tags,
            "channels": [_channel_summary(data, layer, ch) for ch in layer.channels],
            "alpha_nonzero": int((alpha > 0).sum()) if alpha.size else 0,
        }
        if rgba.size:
            visible = alpha > 0
            entry["rgba_mean_visible"] = (
                [round(float(v), 6) for v in rgba[visible].mean(axis=0)]
                if int(visible.sum())
                else None
            )
        if export_dir is not None and rgba.size and int((alpha > 0).sum()):
            safe_name = "".join(c if c.isalnum() or c in "._- " else "_" for c in layer.name)
            out_path = export_dir / f"{layer.index:02d}_{safe_name or 'layer'}.png"
            Image.fromarray(rgba, "RGBA").save(out_path)
            entry["export"] = str(out_path)
        out_layers.append(entry)
    return {**header, "layers": out_layers}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("psd", nargs="+")
    parser.add_argument("--out", help="Write JSON to this path.")
    parser.add_argument("--export-dir", help="Export decoded layer PNGs.")
    args = parser.parse_args()

    export_dir = Path(args.export_dir) if args.export_dir else None
    results = [inspect(Path(psd), export_dir) for psd in args.psd]
    payload: Any = results[0] if len(results) == 1 else results
    text = json.dumps(payload, ensure_ascii=False, indent=2)
    if args.out:
        Path(args.out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.out).write_text(text + "\n", encoding="utf-8")
    else:
        print(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
