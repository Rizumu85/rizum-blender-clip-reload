from __future__ import annotations

from dataclasses import dataclass
import hashlib
import os
import re
import time
from typing import Any


CLIP_SOURCE_KEY = "clip_source"
CLIP_MTIME_KEY = "clip_mtime"
CLIP_SIZE_KEY = "clip_size"
CLIP_SHA256_KEY = "clip_sha256"
CLIP_NATIVE_KEY = "clip_native_renderer"
CLIP_RENDERER_ABI_KEY = "clip_renderer_abi"
CLIP_RENDERER_VERSION_KEY = "clip_renderer_version"
CLIP_CANVAS_WIDTH_KEY = "clip_canvas_width"
CLIP_CANVAS_HEIGHT_KEY = "clip_canvas_height"
CLIP_ROOT_LAYER_KEY = "clip_root_layer_id"
CLIP_LAYER_COUNT_KEY = "clip_layer_count"
CLIP_EXTERNAL_COUNT_KEY = "clip_external_data_count"
CLIP_RELOAD_STATUS_KEY = "clip_reload_status"
CLIP_RELOAD_ERROR_KEY = "clip_reload_error"
CLIP_RELOAD_STARTED_AT_KEY = "clip_reload_started_at"
CLIP_RELOAD_LAST_SECONDS_KEY = "clip_reload_last_seconds"
CLIP_PHASE_WORKER_SECONDS_KEY = "clip_phase_worker_seconds"
CLIP_PHASE_OUTPUT_READ_SECONDS_KEY = "clip_phase_output_read_seconds"
CLIP_PHASE_CONVERT_SECONDS_KEY = "clip_phase_convert_seconds"
CLIP_PHASE_FOREACH_SECONDS_KEY = "clip_phase_foreach_seconds"
CLIP_PHASE_UPDATE_SECONDS_KEY = "clip_phase_update_seconds"
CLIP_PHASE_PACK_SECONDS_KEY = "clip_phase_pack_seconds"
CLIP_PHASE_UPLOAD_SECONDS_KEY = "clip_phase_upload_seconds"
CLIP_SUPPORT_STATUS_KEY = "clip_support_status"
CLIP_SUPPORT_REPORT_KEY = "clip_support_report"
CLIP_SUPPORT_DETAILS_KEY = "clip_support_details"
CLIP_SUPPORT_LOCATIONS_KEY = "clip_support_locations"
CLIP_SUPPORT_SOURCE_COUNT_KEY = "clip_support_source_count"
CLIP_SUPPORT_UNSUPPORTED_COUNT_KEY = "clip_support_unsupported_count"
CLIP_SUPPORT_RASTER_COUNT_KEY = "clip_support_raster_count"
CLIP_SUPPORT_RASTER_BYTES_KEY = "clip_support_raster_bytes"
CLIP_SUPPORT_MAX_RASTER_LAYER_KEY = "clip_support_max_raster_layer"
CLIP_SUPPORT_MAX_RASTER_WIDTH_KEY = "clip_support_max_raster_width"
CLIP_SUPPORT_MAX_RASTER_HEIGHT_KEY = "clip_support_max_raster_height"
CLIP_SUPPORT_MAX_RASTER_BYTES_KEY = "clip_support_max_raster_bytes"
CLIP_SUPPORT_MASK_COUNT_KEY = "clip_support_mask_count"
CLIP_SUPPORT_MASK_BYTES_KEY = "clip_support_mask_bytes"
CLIP_SUPPORT_MAX_MASK_LAYER_KEY = "clip_support_max_mask_layer"
CLIP_SUPPORT_MAX_MASK_WIDTH_KEY = "clip_support_max_mask_width"
CLIP_SUPPORT_MAX_MASK_HEIGHT_KEY = "clip_support_max_mask_height"
CLIP_SUPPORT_MAX_MASK_BYTES_KEY = "clip_support_max_mask_bytes"
CLIP_RELOAD_MANIFEST_KEY = "clip_reload_manifest_json"
CLIP_RELOAD_DIFF_MODE_KEY = "clip_reload_diff_mode"
CLIP_RELOAD_PATCH_COUNT_KEY = "clip_reload_patch_count"
CLIP_SUPPORT_DETAILS_EXPANDED_KEY = "clip_support_details_expanded"
CLIP_PACK_STATUS_KEY = "clip_pack_status"
CLIP_PACK_LAST_SECONDS_KEY = "clip_pack_last_seconds"
CLIP_PACK_ERROR_KEY = "clip_pack_error"

RELOAD_STATUS_OK = "ok"
RELOAD_STATUS_STALE = "stale_source"
RELOAD_STATUS_MISSING = "missing_source"
RELOAD_STATUS_REFRESHING = "refreshing"
RELOAD_STATUS_ERROR = "error"

SUPPORT_STATUS_FULL = "full"
SUPPORT_STATUS_UNSUPPORTED = "unsupported"
SUPPORT_STATUS_UNKNOWN = "unknown"

PACK_STATUS_PACKED = "packed"
PACK_STATUS_NEEDS_PACK = "needs_pack"
PACK_STATUS_RENDERING = "rendering"
PACK_STATUS_PACKING = "packing"
PACK_STATUS_ERROR = "error"

TIMING_PHASES: tuple[tuple[str, str], ...] = (
    ("Native worker", CLIP_PHASE_WORKER_SECONDS_KEY),
    ("Worker output read", CLIP_PHASE_OUTPUT_READ_SECONDS_KEY),
    ("RGBA8 to Blender floats", CLIP_PHASE_CONVERT_SECONDS_KEY),
    ("Blender foreach_set", CLIP_PHASE_FOREACH_SECONDS_KEY),
    ("Blender image update", CLIP_PHASE_UPDATE_SECONDS_KEY),
    ("Blender image pack", CLIP_PHASE_PACK_SECONDS_KEY),
    ("Blender upload total", CLIP_PHASE_UPLOAD_SECONDS_KEY),
)

_SUPPORT_DETAIL_LOCATION_RE = re.compile(
    r"^- layer (?P<layer_id>\d+)(?: \[(?P<name>[^\]]+)\])? "
    r"node (?P<node_id>\d+) (?P<kind>[^:]+?)(?:: (?P<reason>.*))?$"
)


@dataclass(frozen=True)
class SupportDetailRecord:
    layer_id: int
    layer_name: str
    node_id: int
    kind: str
    reason: str

    @property
    def location(self) -> str:
        layer = f"layer {self.layer_id}"
        if self.layer_name:
            layer = f"{layer} [{self.layer_name}]"
        return f"{layer} node {self.node_id} {self.kind}"


@dataclass(frozen=True)
class NativeImageSourceState:
    clip_path: str
    stored_mtime: float | None
    current_mtime: float | None
    stored_size: int | None
    current_size: int | None
    stored_sha256: str
    current_sha256: str | None
    should_reload: bool
    status: str


def delete_key(image: Any, key: str) -> None:
    try:
        keys = image.keys()
    except AttributeError:
        keys = ()
    if key not in keys:
        return
    try:
        del image[key]
    except Exception:
        image[key] = ""


def int_property(image: Any, key: str, default: int = 0) -> int:
    try:
        return int(image.get(key, default))
    except (TypeError, ValueError):
        return default


def float_property(image: Any, key: str, default: float = 0.0) -> float:
    try:
        return float(image.get(key, default))
    except (TypeError, ValueError):
        return default


def has_property(image: Any, key: str) -> bool:
    try:
        return key in image.keys()
    except AttributeError:
        return key in image


def resolve_source_path(path: str, resolve_path: Any | None = None) -> str:
    if not path or resolve_path is None:
        return path
    try:
        resolved = resolve_path(path)
    except Exception:
        return path
    return str(resolved or path)


def image_source_matches(image: Any, clip_path: str, resolve_path: Any | None = None) -> bool:
    stored_path = str(image.get(CLIP_SOURCE_KEY, "") or "")
    return stored_path == clip_path or resolve_source_path(stored_path, resolve_path) == clip_path


def source_file_sha256(path: str | os.PathLike[str]) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def inspect_native_image_source(
    image: Any,
    *,
    resolve_path: Any | None = None,
    exists: Any = os.path.exists,
    getmtime: Any = os.path.getmtime,
    getsize: Any = os.path.getsize,
    getsha256: Any | None = None,
    check_hash: bool = False,
) -> NativeImageSourceState:
    stored_clip_path = str(image.get(CLIP_SOURCE_KEY, "") or "")
    clip_path = resolve_source_path(stored_clip_path, resolve_path)
    stored_mtime = _parse_mtime(image.get(CLIP_MTIME_KEY, ""))
    stored_size = _parse_int(image.get(CLIP_SIZE_KEY, ""))
    stored_sha256 = str(image.get(CLIP_SHA256_KEY, "") or "")
    if not clip_path:
        return NativeImageSourceState(
            clip_path="",
            stored_mtime=stored_mtime,
            current_mtime=None,
            stored_size=stored_size,
            current_size=None,
            stored_sha256=stored_sha256,
            current_sha256=None,
            should_reload=False,
            status=RELOAD_STATUS_MISSING,
        )

    if not exists(clip_path):
        return NativeImageSourceState(
            clip_path=clip_path,
            stored_mtime=stored_mtime,
            current_mtime=None,
            stored_size=stored_size,
            current_size=None,
            stored_sha256=stored_sha256,
            current_sha256=None,
            should_reload=False,
            status=RELOAD_STATUS_MISSING,
        )

    try:
        current_mtime = float(getmtime(clip_path))
        current_size = int(getsize(clip_path))
    except OSError:
        return NativeImageSourceState(
            clip_path=clip_path,
            stored_mtime=stored_mtime,
            current_mtime=None,
            stored_size=stored_size,
            current_size=None,
            stored_sha256=stored_sha256,
            current_sha256=None,
            should_reload=False,
            status=RELOAD_STATUS_MISSING,
        )

    current_sha256: str | None = None
    hash_changed = False
    if check_hash:
        digest_fn = getsha256 or source_file_sha256
        try:
            current_sha256 = str(digest_fn(clip_path))
        except OSError:
            return NativeImageSourceState(
                clip_path=clip_path,
                stored_mtime=stored_mtime,
                current_mtime=None,
                stored_size=stored_size,
                current_size=None,
                stored_sha256=stored_sha256,
                current_sha256=None,
                should_reload=False,
                status=RELOAD_STATUS_MISSING,
            )
        hash_changed = not stored_sha256 or current_sha256 != stored_sha256

    mtime_changed = stored_mtime is None or abs(current_mtime - stored_mtime) > 1e-6
    size_changed = stored_size is not None and current_size != stored_size
    should_reload = mtime_changed or size_changed or hash_changed
    return NativeImageSourceState(
        clip_path=clip_path,
        stored_mtime=stored_mtime,
        current_mtime=current_mtime,
        stored_size=stored_size,
        current_size=current_size,
        stored_sha256=stored_sha256,
        current_sha256=current_sha256,
        should_reload=should_reload,
        status=RELOAD_STATUS_STALE if should_reload else RELOAD_STATUS_OK,
    )


def write_reload_status(image: Any, status: str) -> None:
    image[CLIP_RELOAD_STATUS_KEY] = status
    if status != RELOAD_STATUS_REFRESHING:
        delete_key(image, CLIP_RELOAD_STARTED_AT_KEY)
    if status != RELOAD_STATUS_ERROR:
        delete_key(image, CLIP_RELOAD_ERROR_KEY)


def write_reload_error(image: Any, message: str) -> None:
    image[CLIP_RELOAD_STATUS_KEY] = RELOAD_STATUS_ERROR
    image[CLIP_RELOAD_ERROR_KEY] = message
    delete_key(image, CLIP_RELOAD_STARTED_AT_KEY)


def set_render_started(image: Any, started_at: float | None = None) -> None:
    image[CLIP_RELOAD_STARTED_AT_KEY] = float(time.time() if started_at is None else started_at)
    write_reload_status(image, RELOAD_STATUS_REFRESHING)


def set_last_render_seconds(image: Any, seconds: float) -> None:
    image[CLIP_RELOAD_LAST_SECONDS_KEY] = float(seconds)


def set_pack_status(image: Any, status: str, *, error: str = "") -> None:
    image[CLIP_PACK_STATUS_KEY] = status
    if error:
        image[CLIP_PACK_ERROR_KEY] = error
    elif status != PACK_STATUS_ERROR:
        delete_key(image, CLIP_PACK_ERROR_KEY)


def mark_needs_pack(image: Any) -> None:
    set_pack_status(image, PACK_STATUS_NEEDS_PACK)


def pack_image_now(image: Any) -> float:
    if not hasattr(image, "pack"):
        raise RuntimeError("Blender image does not support packing")
    set_pack_status(image, PACK_STATUS_PACKING)
    started_at = time.perf_counter()
    try:
        image.pack()
    except Exception as exc:
        set_pack_status(image, PACK_STATUS_ERROR, error=str(exc))
        raise
    seconds = time.perf_counter() - started_at
    image[CLIP_PACK_LAST_SECONDS_KEY] = float(seconds)
    set_pack_status(image, PACK_STATUS_PACKED)
    return seconds


def write_render_result_properties(image: Any, result: Any) -> None:
    image[CLIP_SOURCE_KEY] = result.clip_path
    image[CLIP_MTIME_KEY] = "" if result.source_mtime is None else str(result.source_mtime)
    image[CLIP_SIZE_KEY] = "" if result.source_size is None else str(result.source_size)
    image[CLIP_SHA256_KEY] = result.source_sha256
    image[CLIP_NATIVE_KEY] = True
    image[CLIP_RENDERER_ABI_KEY] = result.renderer_abi
    image[CLIP_RENDERER_VERSION_KEY] = result.renderer_version
    image[CLIP_CANVAS_WIDTH_KEY] = result.width
    image[CLIP_CANVAS_HEIGHT_KEY] = result.height
    image[CLIP_ROOT_LAYER_KEY] = result.root_layer_id
    image[CLIP_LAYER_COUNT_KEY] = result.layer_count
    image[CLIP_EXTERNAL_COUNT_KEY] = result.external_data_count
    if result.reload_manifest_json:
        try:
            image[CLIP_RELOAD_MANIFEST_KEY] = result.reload_manifest_json
        except Exception:
            delete_key(image, CLIP_RELOAD_MANIFEST_KEY)
    else:
        delete_key(image, CLIP_RELOAD_MANIFEST_KEY)
    image[CLIP_RELOAD_DIFF_MODE_KEY] = result.reload_diff_mode
    image[CLIP_RELOAD_PATCH_COUNT_KEY] = len(result.patches)
    write_support_properties(image, result.support_summary)
    write_reload_status(image, RELOAD_STATUS_OK)


def write_phase_properties(
    image: Any,
    result: Any,
    *,
    convert_seconds: float,
    foreach_seconds: float,
    update_seconds: float,
    pack_seconds: float,
    upload_seconds: float,
) -> None:
    if result.worker_seconds is None:
        delete_key(image, CLIP_PHASE_WORKER_SECONDS_KEY)
    else:
        image[CLIP_PHASE_WORKER_SECONDS_KEY] = float(result.worker_seconds)
    if result.output_read_seconds is None:
        delete_key(image, CLIP_PHASE_OUTPUT_READ_SECONDS_KEY)
    else:
        image[CLIP_PHASE_OUTPUT_READ_SECONDS_KEY] = float(result.output_read_seconds)
    image[CLIP_PHASE_CONVERT_SECONDS_KEY] = float(convert_seconds)
    image[CLIP_PHASE_FOREACH_SECONDS_KEY] = float(foreach_seconds)
    image[CLIP_PHASE_UPDATE_SECONDS_KEY] = float(update_seconds)
    image[CLIP_PHASE_PACK_SECONDS_KEY] = float(pack_seconds)
    image[CLIP_PHASE_UPLOAD_SECONDS_KEY] = float(upload_seconds)


def write_support_properties(image: Any, summary: Any | None) -> None:
    if summary is None:
        image[CLIP_SUPPORT_STATUS_KEY] = SUPPORT_STATUS_UNKNOWN
        image[CLIP_SUPPORT_REPORT_KEY] = "Native support summary unavailable."
        image[CLIP_SUPPORT_DETAILS_KEY] = ""
        image[CLIP_SUPPORT_LOCATIONS_KEY] = ""
        return
    image[CLIP_SUPPORT_STATUS_KEY] = (
        SUPPORT_STATUS_FULL
        if summary.unsupported_count == 0
        else SUPPORT_STATUS_UNSUPPORTED
    )
    image[CLIP_SUPPORT_REPORT_KEY] = summary.report
    image[CLIP_SUPPORT_DETAILS_KEY] = "\n".join(summary.details)
    image[CLIP_SUPPORT_LOCATIONS_KEY] = "\n".join(
        support_detail_locations(summary.details)
    )
    image[CLIP_SUPPORT_SOURCE_COUNT_KEY] = summary.source_count
    image[CLIP_SUPPORT_UNSUPPORTED_COUNT_KEY] = summary.unsupported_count
    image[CLIP_SUPPORT_RASTER_COUNT_KEY] = summary.raster_count
    image[CLIP_SUPPORT_MAX_RASTER_LAYER_KEY] = summary.max_raster_layer_id
    image[CLIP_SUPPORT_MAX_RASTER_WIDTH_KEY] = summary.max_raster_width
    image[CLIP_SUPPORT_MAX_RASTER_HEIGHT_KEY] = summary.max_raster_height
    image[CLIP_SUPPORT_MASK_COUNT_KEY] = summary.mask_count
    image[CLIP_SUPPORT_MAX_MASK_LAYER_KEY] = summary.max_mask_layer_id
    image[CLIP_SUPPORT_MAX_MASK_WIDTH_KEY] = summary.max_mask_width
    image[CLIP_SUPPORT_MAX_MASK_HEIGHT_KEY] = summary.max_mask_height
    delete_key(image, CLIP_SUPPORT_RASTER_BYTES_KEY)
    delete_key(image, CLIP_SUPPORT_MAX_RASTER_BYTES_KEY)
    delete_key(image, CLIP_SUPPORT_MASK_BYTES_KEY)
    delete_key(image, CLIP_SUPPORT_MAX_MASK_BYTES_KEY)


def support_detail_records(details: Any) -> tuple[SupportDetailRecord, ...]:
    if isinstance(details, str):
        detail_lines = details.splitlines()
    else:
        try:
            detail_lines = list(details)
        except TypeError:
            detail_lines = ()
    records: list[SupportDetailRecord] = []
    for line in detail_lines:
        match = _SUPPORT_DETAIL_LOCATION_RE.match(str(line).strip())
        if not match:
            continue
        records.append(
            SupportDetailRecord(
                layer_id=int(match.group("layer_id")),
                layer_name=(match.group("name") or "").strip(),
                node_id=int(match.group("node_id")),
                kind=(match.group("kind") or "").strip(),
                reason=match.group("reason") or "",
            )
        )
    return tuple(records)


def support_detail_locations(details: Any) -> tuple[str, ...]:
    return tuple(record.location for record in support_detail_records(details))


def timing_phase_values(image: Any) -> tuple[tuple[str, float], ...]:
    values: list[tuple[str, float]] = []
    for label, key in TIMING_PHASES:
        if has_property(image, key):
            values.append((label, float_property(image, key)))
    return tuple(values)


def pack_status_label_raw(status: str) -> str:
    return {
        PACK_STATUS_PACKED: "Packed",
        PACK_STATUS_NEEDS_PACK: "Needs Pack",
        PACK_STATUS_RENDERING: "Waiting for render",
        PACK_STATUS_PACKING: "Packing",
        PACK_STATUS_ERROR: "Pack Error",
    }.get(status, "Unknown")


def pack_status_icon(status: str) -> str:
    return {
        PACK_STATUS_PACKED: "CHECKMARK",
        PACK_STATUS_NEEDS_PACK: "INFO",
        PACK_STATUS_RENDERING: "SORTTIME",
        PACK_STATUS_PACKING: "SORTTIME",
        PACK_STATUS_ERROR: "ERROR",
    }.get(status, "INFO")


def reload_status_label_raw(status: str) -> str:
    return {
        RELOAD_STATUS_OK: "Ready",
        RELOAD_STATUS_STALE: "Source changed",
        RELOAD_STATUS_MISSING: "Source missing",
        RELOAD_STATUS_REFRESHING: "Rendering",
        RELOAD_STATUS_ERROR: "Render failed",
    }.get(status, "Unknown")


def reload_status_icon(status: str) -> str:
    return {
        RELOAD_STATUS_OK: "CHECKMARK",
        RELOAD_STATUS_STALE: "FILE_REFRESH",
        RELOAD_STATUS_MISSING: "ERROR",
        RELOAD_STATUS_REFRESHING: "SORTTIME",
        RELOAD_STATUS_ERROR: "ERROR",
    }.get(status, "INFO")


def _parse_mtime(value: Any) -> float | None:
    if value is None or value == "":
        return None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def _parse_int(value: Any) -> int | None:
    if value is None or value == "":
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None
