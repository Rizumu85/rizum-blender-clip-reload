from __future__ import annotations

from dataclasses import dataclass
import json
import os
from pathlib import Path
from typing import Any

from . import image_state


@dataclass(frozen=True)
class NativeRenderPatch:
    x: int
    y: int
    width: int
    height: int
    byte_offset: int


@dataclass(frozen=True)
class NativeSupportSummary:
    source_count: int
    unsupported_count: int
    raster_count: int
    raster_bytes: int
    max_raster_layer_id: int
    max_raster_width: int
    max_raster_height: int
    max_raster_bytes: int
    mask_count: int
    mask_bytes: int
    max_mask_layer_id: int
    max_mask_width: int
    max_mask_height: int
    max_mask_bytes: int
    report: str
    details: tuple[str, ...] = ()


@dataclass(frozen=True)
class NativeRenderResult:
    clip_path: str
    width: int
    height: int
    root_layer_id: int
    layer_count: int
    external_data_count: int
    renderer_abi: int
    renderer_version: str
    source_mtime: float | None
    source_size: int | None
    source_sha256: str
    pixels_rgba8: bytes
    support_summary: NativeSupportSummary | None = None
    worker_seconds: float | None = None
    output_read_seconds: float | None = None
    reload_manifest_json: str = ""
    reload_diff_mode: str = "full"
    reload_diff_reason: str = ""
    patches: tuple[NativeRenderPatch, ...] = ()


@dataclass(frozen=True)
class WorkerRenderFiles:
    rgba_path: Path
    json_path: Path
    previous_manifest_path: Path | None = None


class WorkerProtocolError(RuntimeError):
    pass


def prepare_render_files(temp_dir: Path, previous_manifest_json: str | None) -> WorkerRenderFiles:
    rgba_path = temp_dir / "render.rgba"
    json_path = temp_dir / "render.json"
    previous_manifest_path = None
    if previous_manifest_json:
        previous_manifest_path = temp_dir / "old_manifest.json"
        previous_manifest_path.write_text(previous_manifest_json, encoding="utf-8")
    return WorkerRenderFiles(
        rgba_path=rgba_path,
        json_path=json_path,
        previous_manifest_path=previous_manifest_path,
    )


def one_shot_command(executable_path: str, source: str, files: WorkerRenderFiles) -> list[str]:
    command = [
        executable_path,
        source,
        "--blender-render-rgba",
        str(files.rgba_path),
        "--blender-render-json",
        str(files.json_path),
    ]
    if files.previous_manifest_path is not None:
        command.extend(["--blender-reload-old-json", str(files.previous_manifest_path)])
    return command


def persistent_request(source: str, files: WorkerRenderFiles) -> dict[str, Any]:
    request: dict[str, Any] = {
        "clip_path": source,
        "rgba_path": str(files.rgba_path),
        "json_path": str(files.json_path),
    }
    if files.previous_manifest_path is not None:
        request["previous_manifest_path"] = str(files.previous_manifest_path)
    return request


def persistent_request_line(request: dict[str, Any]) -> str:
    return json.dumps(request, separators=(",", ":")) + "\n"


def read_render_output(files: WorkerRenderFiles) -> tuple[dict[str, Any], bytes]:
    try:
        metadata = json.loads(files.json_path.read_text(encoding="utf-8"))
        pixels = files.rgba_path.read_bytes()
    except OSError as exc:
        raise WorkerProtocolError(f"native renderer worker output missing: {exc}") from exc
    except json.JSONDecodeError as exc:
        raise WorkerProtocolError(
            f"native renderer worker returned invalid JSON: {exc}"
        ) from exc
    if not isinstance(metadata, dict):
        raise WorkerProtocolError("native renderer worker returned non-object JSON")
    return metadata, pixels


def render_result_from_worker_output(
    source: str,
    metadata: dict[str, Any],
    pixels: bytes,
    *,
    expected_abi_version: int,
    worker_seconds: float,
    output_read_seconds: float,
) -> NativeRenderResult:
    width = int(metadata["width"])
    height = int(metadata["height"])
    reload_diff = metadata.get("reload_diff", {}) or {}
    reload_mode = str(reload_diff.get("mode", "full") or "full")
    reload_reason = str(reload_diff.get("reason", "") or "")
    reload_manifest_json = _compact_json(reload_diff.get("manifest"))
    patches = _worker_reload_patches(reload_diff.get("rects", []) or [])
    if reload_mode == "patch":
        expected_len = sum(patch.width * patch.height * 4 for patch in patches)
    elif reload_mode == "no_change":
        expected_len = 0
    else:
        expected_len = width * height * 4
        reload_mode = "full"
        patches = ()
    if len(pixels) != expected_len:
        raise WorkerProtocolError("native renderer worker returned an invalid RGBA buffer length")

    support = metadata.get("support", {})
    resources = metadata.get("resources", {})
    unsupported = support.get("unsupported", []) or []
    details = tuple(_worker_unsupported_detail(item) for item in unsupported)
    support_summary = NativeSupportSummary(
        source_count=int(support.get("source_count", 0) or 0),
        unsupported_count=int(support.get("unsupported_count", len(details)) or 0),
        raster_count=int(resources.get("raster_count", 0) or 0),
        raster_bytes=int(resources.get("raster_bytes", 0) or 0),
        max_raster_layer_id=int(resources.get("max_raster_layer_id") or 0),
        max_raster_width=int(resources.get("max_raster_width", 0) or 0),
        max_raster_height=int(resources.get("max_raster_height", 0) or 0),
        max_raster_bytes=int(resources.get("max_raster_bytes", 0) or 0),
        mask_count=int(resources.get("mask_count", 0) or 0),
        mask_bytes=int(resources.get("mask_bytes", 0) or 0),
        max_mask_layer_id=int(resources.get("max_mask_layer_id") or 0),
        max_mask_width=int(resources.get("max_mask_width", 0) or 0),
        max_mask_height=int(resources.get("max_mask_height", 0) or 0),
        max_mask_bytes=int(resources.get("max_mask_bytes", 0) or 0),
        report=str(support.get("report", "") or ""),
        details=details,
    )
    try:
        source_mtime = os.path.getmtime(source)
    except OSError:
        source_mtime = None
    try:
        source_size = os.path.getsize(source)
    except OSError:
        source_size = None
    try:
        source_sha256 = image_state.source_file_sha256(source)
    except OSError:
        source_sha256 = ""
    return NativeRenderResult(
        clip_path=source,
        width=width,
        height=height,
        root_layer_id=int(metadata["root_layer_id"]),
        layer_count=int(metadata["layer_count"]),
        external_data_count=int(metadata["external_data_count"]),
        renderer_abi=expected_abi_version,
        renderer_version=str(metadata.get("renderer_version", "") or ""),
        source_mtime=source_mtime,
        source_size=source_size,
        source_sha256=source_sha256,
        pixels_rgba8=pixels,
        support_summary=support_summary,
        worker_seconds=worker_seconds,
        output_read_seconds=output_read_seconds,
        reload_manifest_json=reload_manifest_json,
        reload_diff_mode=reload_mode,
        reload_diff_reason=reload_reason,
        patches=patches,
    )


def _compact_json(value: Any) -> str:
    if value is None:
        return ""
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True)


def _worker_unsupported_detail(item: Any) -> str:
    layer_id = int(item.get("layer_id", 0) or 0)
    layer_name = str(item.get("layer_name", "") or "").strip()
    node_id = int(item.get("node_id", 0) or 0)
    kind = str(item.get("kind", "") or "").strip()
    reason = str(item.get("reason", "") or "").strip()
    layer = f"layer {layer_id}"
    if layer_name:
        layer = f"{layer} [{layer_name}]"
    detail = f"- {layer} node {node_id} {kind}".rstrip()
    if reason:
        detail = f"{detail}: {reason}"
    return detail


def _worker_reload_patches(rects: Any) -> tuple[NativeRenderPatch, ...]:
    patches: list[NativeRenderPatch] = []
    offset = 0
    for item in rects:
        x = int(item.get("x", 0) or 0)
        y = int(item.get("y", 0) or 0)
        width = int(item.get("width", 0) or 0)
        height = int(item.get("height", 0) or 0)
        if width <= 0 or height <= 0:
            continue
        patches.append(
            NativeRenderPatch(
                x=x,
                y=y,
                width=width,
                height=height,
                byte_offset=offset,
            )
        )
        offset += width * height * 4
    return tuple(patches)
