from __future__ import annotations

from array import array
import ctypes
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import threading
import time
from typing import Any


EXPECTED_ABI_VERSION = 1

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

RELOAD_STATUS_OK = "ok"
RELOAD_STATUS_STALE = "stale_source"
RELOAD_STATUS_MISSING = "missing_source"
RELOAD_STATUS_REFRESHING = "refreshing"
RELOAD_STATUS_ERROR = "error"

SUPPORT_STATUS_FULL = "full"
SUPPORT_STATUS_UNSUPPORTED = "unsupported"
SUPPORT_STATUS_UNKNOWN = "unknown"

_SUPPORT_DETAIL_LOCATION_RE = re.compile(
    r"^- layer (?P<layer_id>\d+)(?: \[(?P<name>[^\]]+)\])? "
    r"node (?P<node_id>\d+) (?P<kind>[^:]+?)(?:: (?P<reason>.*))?$"
)


class NativeBridgeError(RuntimeError):
    pass


class NativeWorkerTransportError(NativeBridgeError):
    pass


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
    support_summary: "NativeSupportSummary | None" = None
    worker_seconds: float | None = None
    output_read_seconds: float | None = None
    reload_manifest_json: str = ""
    reload_diff_mode: str = "full"
    reload_diff_reason: str = ""
    patches: tuple["NativeRenderPatch", ...] = ()


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


_PERSISTENT_WORKER_LOCK = threading.Lock()
_PERSISTENT_WORKER: "PersistentNativeRendererWorker | None" = None


class _ClipRendererImageInfo(ctypes.Structure):
    _fields_ = [
        ("width", ctypes.c_uint32),
        ("height", ctypes.c_uint32),
        ("root_layer_id", ctypes.c_uint32),
        ("layer_count", ctypes.c_size_t),
        ("external_data_count", ctypes.c_size_t),
    ]


class _ClipRendererSupportInfo(ctypes.Structure):
    _fields_ = [
        ("source_count", ctypes.c_size_t),
        ("unsupported_count", ctypes.c_size_t),
        ("raster_count", ctypes.c_size_t),
        ("raster_bytes", ctypes.c_uint64),
        ("max_raster_layer_id", ctypes.c_uint32),
        ("max_raster_width", ctypes.c_uint32),
        ("max_raster_height", ctypes.c_uint32),
        ("max_raster_bytes", ctypes.c_uint64),
        ("mask_count", ctypes.c_size_t),
        ("mask_bytes", ctypes.c_uint64),
        ("max_mask_layer_id", ctypes.c_uint32),
        ("max_mask_width", ctypes.c_uint32),
        ("max_mask_height", ctypes.c_uint32),
        ("max_mask_bytes", ctypes.c_uint64),
    ]


class NativeRendererLibrary:
    def __init__(self, library_path: str | os.PathLike[str]):
        self.library_path = str(Path(library_path).resolve())
        self._dll = ctypes.CDLL(self.library_path)
        self._configure_signatures()
        abi = int(self._dll.clip_renderer_abi_version())
        if abi != EXPECTED_ABI_VERSION:
            raise NativeBridgeError(
                f"native renderer ABI {abi} does not match expected {EXPECTED_ABI_VERSION}"
            )
        self.abi_version = abi
        self.renderer_version = self._read_renderer_version()

    def render_rgba8(self, clip_path: str | os.PathLike[str]) -> NativeRenderResult:
        source = str(Path(clip_path).resolve())
        session = ctypes.c_void_p()
        status = self._dll.clip_renderer_session_open(
            source.encode("utf-8"),
            ctypes.byref(session),
        )
        self._raise_if_error(status, "open native clip session")
        if not session.value:
            raise NativeBridgeError("native renderer returned a null session")

        try:
            info = _ClipRendererImageInfo()
            status = self._dll.clip_renderer_session_info(session, ctypes.byref(info))
            self._raise_if_error(status, "read native clip info")
            support_summary = self._support_summary(session)

            pixel_len = int(info.width) * int(info.height) * 4
            pixels = (ctypes.c_uint8 * pixel_len)()
            status = self._dll.clip_renderer_session_read_rgba8(
                session,
                0,
                0,
                info.width,
                info.height,
                pixels,
                pixel_len,
            )
            self._raise_if_error(status, "read native clip pixels")

            try:
                source_mtime = os.path.getmtime(source)
            except OSError:
                source_mtime = None
            try:
                source_size = os.path.getsize(source)
            except OSError:
                source_size = None
            try:
                source_sha256 = source_file_sha256(source)
            except OSError:
                source_sha256 = ""

            return NativeRenderResult(
                clip_path=source,
                width=int(info.width),
                height=int(info.height),
                root_layer_id=int(info.root_layer_id),
                layer_count=int(info.layer_count),
                external_data_count=int(info.external_data_count),
                renderer_abi=self.abi_version,
                renderer_version=self.renderer_version,
                source_mtime=source_mtime,
                source_size=source_size,
                source_sha256=source_sha256,
                pixels_rgba8=bytes(pixels),
                support_summary=support_summary,
                worker_seconds=None,
                output_read_seconds=None,
            )
        finally:
            self._dll.clip_renderer_session_close(session)

    def _configure_signatures(self) -> None:
        self._dll.clip_renderer_abi_version.argtypes = []
        self._dll.clip_renderer_abi_version.restype = ctypes.c_uint32

        self._dll.clip_renderer_version.argtypes = []
        self._dll.clip_renderer_version.restype = ctypes.c_char_p

        self._dll.clip_renderer_last_error.argtypes = []
        self._dll.clip_renderer_last_error.restype = ctypes.c_char_p

        self._dll.clip_renderer_session_open.argtypes = [
            ctypes.c_char_p,
            ctypes.POINTER(ctypes.c_void_p),
        ]
        self._dll.clip_renderer_session_open.restype = ctypes.c_int

        self._dll.clip_renderer_session_close.argtypes = [ctypes.c_void_p]
        self._dll.clip_renderer_session_close.restype = None

        self._dll.clip_renderer_session_info.argtypes = [
            ctypes.c_void_p,
            ctypes.POINTER(_ClipRendererImageInfo),
        ]
        self._dll.clip_renderer_session_info.restype = ctypes.c_int

        self._dll.clip_renderer_session_read_rgba8.argtypes = [
            ctypes.c_void_p,
            ctypes.c_uint32,
            ctypes.c_uint32,
            ctypes.c_uint32,
            ctypes.c_uint32,
            ctypes.POINTER(ctypes.c_uint8),
            ctypes.c_size_t,
        ]
        self._dll.clip_renderer_session_read_rgba8.restype = ctypes.c_int

        try:
            self._support_info_fn = self._dll.clip_renderer_session_support_info
        except AttributeError:
            self._support_info_fn = None
        if self._support_info_fn is not None:
            self._support_info_fn.argtypes = [
                ctypes.c_void_p,
                ctypes.POINTER(_ClipRendererSupportInfo),
                ctypes.c_char_p,
                ctypes.c_size_t,
                ctypes.POINTER(ctypes.c_size_t),
            ]
            self._support_info_fn.restype = ctypes.c_int

    def _support_summary(self, session: ctypes.c_void_p) -> NativeSupportSummary | None:
        if self._support_info_fn is None:
            return None
        info = _ClipRendererSupportInfo()
        required_len = ctypes.c_size_t(0)
        buffer_len = 4096
        report = ctypes.create_string_buffer(buffer_len)
        status = self._support_info_fn(
            session,
            ctypes.byref(info),
            report,
            buffer_len,
            ctypes.byref(required_len),
        )
        if status == 6 and required_len.value > buffer_len:
            buffer_len = int(required_len.value)
            report = ctypes.create_string_buffer(buffer_len)
            status = self._support_info_fn(
                session,
                ctypes.byref(info),
                report,
                buffer_len,
                ctypes.byref(required_len),
            )
        self._raise_if_error(status, "read native support info")
        decoded_report = report.value.decode("utf-8", errors="replace")
        report_lines = decoded_report.splitlines()
        summary_line = report_lines[0] if report_lines else ""
        detail_lines = tuple(line for line in report_lines[1:] if line)
        return NativeSupportSummary(
            source_count=int(info.source_count),
            unsupported_count=int(info.unsupported_count),
            raster_count=int(info.raster_count),
            raster_bytes=int(info.raster_bytes),
            max_raster_layer_id=int(info.max_raster_layer_id),
            max_raster_width=int(info.max_raster_width),
            max_raster_height=int(info.max_raster_height),
            max_raster_bytes=int(info.max_raster_bytes),
            mask_count=int(info.mask_count),
            mask_bytes=int(info.mask_bytes),
            max_mask_layer_id=int(info.max_mask_layer_id),
            max_mask_width=int(info.max_mask_width),
            max_mask_height=int(info.max_mask_height),
            max_mask_bytes=int(info.max_mask_bytes),
            report=summary_line,
            details=detail_lines,
        )

    def _raise_if_error(self, status: int, action: str) -> None:
        if status == 0:
            return
        message = self._last_error_message()
        detail = f": {message}" if message else ""
        raise NativeBridgeError(f"could not {action} ({_status_name(status)}){detail}")

    def _last_error_message(self) -> str:
        message = self._dll.clip_renderer_last_error()
        if not message:
            return ""
        return message.decode("utf-8", errors="replace")

    def _read_renderer_version(self) -> str:
        version = self._dll.clip_renderer_version()
        if not version:
            return ""
        return version.decode("utf-8", errors="replace")


def render_clip_rgba8(
    clip_path: str | os.PathLike[str],
    *,
    renderer: Any | None = None,
    previous_manifest_json: str | None = None,
) -> NativeRenderResult:
    if renderer is not None:
        return renderer.render_rgba8(clip_path)
    worker_path = packaged_renderer_worker_path()
    if worker_path:
        if _persistent_worker_enabled():
            try:
                return _shared_renderer_worker(worker_path).render_rgba8(
                    clip_path,
                    previous_manifest_json=previous_manifest_json,
                )
            except NativeWorkerTransportError:
                shutdown_renderer_worker()
        return NativeRendererWorker(worker_path).render_rgba8(
            clip_path,
            previous_manifest_json=previous_manifest_json,
        )
    raise NativeBridgeError("packaged native renderer worker not found; rebuild the add-on package")


def _persistent_worker_enabled() -> bool:
    disabled = os.environ.get("RIZUM_CLIP_DISABLE_PERSISTENT_WORKER", "")
    return disabled.lower() not in {"1", "true", "yes", "on"}


def _shared_renderer_worker(executable_path: str) -> "PersistentNativeRendererWorker":
    global _PERSISTENT_WORKER
    resolved = str(Path(executable_path).resolve())
    with _PERSISTENT_WORKER_LOCK:
        if (
            _PERSISTENT_WORKER is None
            or _PERSISTENT_WORKER.executable_path != resolved
            or not _PERSISTENT_WORKER.is_alive()
        ):
            if _PERSISTENT_WORKER is not None:
                _PERSISTENT_WORKER.shutdown()
            _PERSISTENT_WORKER = PersistentNativeRendererWorker(resolved)
        return _PERSISTENT_WORKER


def shutdown_renderer_worker() -> None:
    global _PERSISTENT_WORKER
    with _PERSISTENT_WORKER_LOCK:
        worker = _PERSISTENT_WORKER
        _PERSISTENT_WORKER = None
    if worker is not None:
        worker.shutdown()


def import_clip_as_image(
    clip_path: str | os.PathLike[str],
    *,
    bpy_module: Any,
    image_name: str | None = None,
    pack: bool = True,
    renderer: Any | None = None,
) -> Any:
    result = render_clip_rgba8(clip_path, renderer=renderer)
    return create_or_update_image(
        bpy_module,
        result,
        image_name=image_name,
        pack=pack,
    )


def create_or_update_image(
    bpy_module: Any,
    result: NativeRenderResult,
    *,
    image: Any | None = None,
    image_name: str | None = None,
    pack: bool = True,
    allow_resize: bool = False,
) -> Any:
    if result.reload_diff_mode == "patch":
        expected_len = sum(patch.width * patch.height * 4 for patch in result.patches)
        if len(result.pixels_rgba8) != expected_len:
            raise NativeBridgeError("native renderer returned an invalid patch RGBA buffer length")
        if image is None:
            raise NativeBridgeError("native renderer returned a patch without an existing image")
    elif result.reload_diff_mode == "no_change":
        if result.pixels_rgba8:
            raise NativeBridgeError("native renderer returned pixels for an unchanged reload")
        if image is None:
            raise NativeBridgeError("native renderer returned no-change without an existing image")
    elif len(result.pixels_rgba8) != result.width * result.height * 4:
        raise NativeBridgeError("native renderer returned an invalid RGBA buffer length")

    upload_started = time.perf_counter()
    image = _ensure_image(
        bpy_module,
        result,
        image=image,
        image_name=image_name,
        allow_resize=allow_resize,
    )
    image.source = "GENERATED"
    if hasattr(image, "colorspace_settings"):
        image.colorspace_settings.name = "sRGB"

    if result.reload_diff_mode == "patch":
        convert_seconds, foreach_seconds = _apply_rgba8_patches_to_image(image, result)
        update_started = time.perf_counter()
        image.update()
        update_seconds = time.perf_counter() - update_started
    elif result.reload_diff_mode == "no_change":
        convert_seconds = 0.0
        foreach_seconds = 0.0
        update_seconds = 0.0
    else:
        convert_started = time.perf_counter()
        pixels = _rgba8_to_blender_float_sequence(
            result.pixels_rgba8,
            result.width,
            result.height,
        )
        convert_seconds = time.perf_counter() - convert_started

        foreach_started = time.perf_counter()
        image.pixels.foreach_set(pixels)
        foreach_seconds = time.perf_counter() - foreach_started

        update_started = time.perf_counter()
        image.update()
        update_seconds = time.perf_counter() - update_started

    _write_source_properties(image, result)
    pack_seconds = 0.0
    if pack and hasattr(image, "pack"):
        pack_started = time.perf_counter()
        image.pack()
        pack_seconds = time.perf_counter() - pack_started
    upload_seconds = time.perf_counter() - upload_started
    _write_phase_properties(
        image,
        result,
        convert_seconds=convert_seconds,
        foreach_seconds=foreach_seconds,
        update_seconds=update_seconds,
        pack_seconds=pack_seconds,
        upload_seconds=upload_seconds,
    )
    return image


def inspect_native_image_source(
    image: Any,
    *,
    exists: Any = os.path.exists,
    getmtime: Any = os.path.getmtime,
    getsize: Any = os.path.getsize,
    getsha256: Any | None = None,
    check_hash: bool = False,
) -> NativeImageSourceState:
    clip_path = str(image.get(CLIP_SOURCE_KEY, "") or "")
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
        _delete_image_key(image, CLIP_RELOAD_STARTED_AT_KEY)
    if status != RELOAD_STATUS_ERROR:
        _clear_reload_error(image)


def write_reload_error(image: Any, message: str) -> None:
    image[CLIP_RELOAD_STATUS_KEY] = RELOAD_STATUS_ERROR
    image[CLIP_RELOAD_ERROR_KEY] = message
    _delete_image_key(image, CLIP_RELOAD_STARTED_AT_KEY)


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


def resolve_renderer_library() -> str:
    env_path = os.environ.get("RIZUM_CLIP_RENDERER_DLL")
    if env_path:
        return env_path

    packaged_path = packaged_renderer_library_path()
    if packaged_path:
        return packaged_path

    raise NativeBridgeError(
        "native renderer library not found; set RIZUM_CLIP_RENDERER_DLL or rebuild the add-on package"
    )


def packaged_renderer_library_path() -> str | None:
    module_dir = Path(__file__).resolve().parent
    candidates = [
        module_dir / "clip_capi.dll",
        module_dir / "libclip_capi.so",
        module_dir / "libclip_capi.dylib",
        module_dir / "native" / "clip_capi.dll",
        module_dir / "native" / "libclip_capi.so",
        module_dir / "native" / "libclip_capi.dylib",
    ]
    for candidate in candidates:
        if candidate.exists():
            return str(candidate)
    return None


def packaged_renderer_worker_path() -> str | None:
    module_dir = Path(__file__).resolve().parent
    candidates = [
        module_dir / "clip_cli.exe",
        module_dir / "clip_cli",
        module_dir / "native" / "clip_cli.exe",
        module_dir / "native" / "clip_cli",
    ]
    for candidate in candidates:
        if candidate.exists():
            return str(candidate)
    return None


class NativeRendererWorker:
    def __init__(self, executable_path: str | os.PathLike[str]):
        self.executable_path = str(Path(executable_path).resolve())

    def render_rgba8(
        self,
        clip_path: str | os.PathLike[str],
        *,
        previous_manifest_json: str | None = None,
    ) -> NativeRenderResult:
        source = str(Path(clip_path).resolve())
        with tempfile.TemporaryDirectory(prefix="rizum_clip_render_") as temp_dir:
            temp = Path(temp_dir)
            rgba_path = temp / "render.rgba"
            json_path = temp / "render.json"
            command = [
                self.executable_path,
                source,
                "--blender-render-rgba",
                str(rgba_path),
                "--blender-render-json",
                str(json_path),
            ]
            if previous_manifest_json:
                old_manifest_path = temp / "old_manifest.json"
                old_manifest_path.write_text(previous_manifest_json, encoding="utf-8")
                command.extend(["--blender-reload-old-json", str(old_manifest_path)])
            creationflags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
            worker_started = time.perf_counter()
            completed = subprocess.run(
                command,
                capture_output=True,
                text=True,
                creationflags=creationflags,
            )
            worker_seconds = time.perf_counter() - worker_started
            if completed.returncode != 0:
                message = completed.stderr.strip() or completed.stdout.strip()
                raise NativeBridgeError(
                    f"native renderer worker failed with exit code {completed.returncode}: {message}"
                )
            try:
                read_started = time.perf_counter()
                metadata = json.loads(json_path.read_text(encoding="utf-8"))
                pixels = rgba_path.read_bytes()
                output_read_seconds = time.perf_counter() - read_started
            except OSError as exc:
                raise NativeBridgeError(f"native renderer worker output missing: {exc}") from exc
            except json.JSONDecodeError as exc:
                raise NativeBridgeError(f"native renderer worker returned invalid JSON: {exc}") from exc

        return _render_result_from_worker_output(
            source,
            metadata,
            pixels,
            worker_seconds=worker_seconds,
            output_read_seconds=output_read_seconds,
        )


class PersistentNativeRendererWorker:
    def __init__(self, executable_path: str | os.PathLike[str]):
        self.executable_path = str(Path(executable_path).resolve())
        self._process: subprocess.Popen[str] | None = None
        self._lock = threading.Lock()

    def is_alive(self) -> bool:
        return self._process is not None and self._process.poll() is None

    def render_rgba8(
        self,
        clip_path: str | os.PathLike[str],
        *,
        previous_manifest_json: str | None = None,
    ) -> NativeRenderResult:
        source = str(Path(clip_path).resolve())
        with self._lock:
            with tempfile.TemporaryDirectory(prefix="rizum_clip_render_") as temp_dir:
                temp = Path(temp_dir)
                rgba_path = temp / "render.rgba"
                json_path = temp / "render.json"
                request: dict[str, Any] = {
                    "clip_path": source,
                    "rgba_path": str(rgba_path),
                    "json_path": str(json_path),
                }
                if previous_manifest_json:
                    old_manifest_path = temp / "old_manifest.json"
                    old_manifest_path.write_text(previous_manifest_json, encoding="utf-8")
                    request["previous_manifest_path"] = str(old_manifest_path)

                worker_started = time.perf_counter()
                self._send_request_locked(request)
                worker_seconds = time.perf_counter() - worker_started

                try:
                    read_started = time.perf_counter()
                    metadata = json.loads(json_path.read_text(encoding="utf-8"))
                    pixels = rgba_path.read_bytes()
                    output_read_seconds = time.perf_counter() - read_started
                except OSError as exc:
                    raise NativeBridgeError(
                        f"native renderer worker output missing: {exc}"
                    ) from exc
                except json.JSONDecodeError as exc:
                    raise NativeBridgeError(
                        f"native renderer worker returned invalid JSON: {exc}"
                    ) from exc

        return _render_result_from_worker_output(
            source,
            metadata,
            pixels,
            worker_seconds=worker_seconds,
            output_read_seconds=output_read_seconds,
        )

    def shutdown(self) -> None:
        with self._lock:
            process = self._process
            self._process = None
            if process is None:
                return
            if process.poll() is None:
                try:
                    if process.stdin:
                        process.stdin.write('{"shutdown":true}\n')
                        process.stdin.flush()
                    process.wait(timeout=2.0)
                except Exception:
                    process.terminate()
                    try:
                        process.wait(timeout=2.0)
                    except Exception:
                        process.kill()

    def _ensure_process_locked(self) -> subprocess.Popen[str]:
        if self._process is not None and self._process.poll() is None:
            return self._process
        creationflags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
        try:
            self._process = subprocess.Popen(
                [self.executable_path, "--blender-render-server"],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                text=True,
                creationflags=creationflags,
            )
        except OSError as exc:
            raise NativeWorkerTransportError(
                f"failed to start persistent native renderer worker: {exc}"
            ) from exc
        return self._process

    def _send_request_locked(self, request: dict[str, Any]) -> None:
        process = self._ensure_process_locked()
        if process.stdin is None or process.stdout is None:
            raise NativeWorkerTransportError("persistent native renderer worker has no stdio")
        try:
            process.stdin.write(json.dumps(request, separators=(",", ":")) + "\n")
            process.stdin.flush()
            response_line = process.stdout.readline()
        except OSError as exc:
            self._process = None
            raise NativeWorkerTransportError(
                f"persistent native renderer worker pipe failed: {exc}"
            ) from exc
        if not response_line:
            returncode = process.poll()
            self._process = None
            raise NativeWorkerTransportError(
                f"persistent native renderer worker stopped unexpectedly"
                f"{'' if returncode is None else f' with exit code {returncode}'}"
            )
        try:
            response = json.loads(response_line)
        except json.JSONDecodeError as exc:
            self._process = None
            raise NativeWorkerTransportError(
                f"persistent native renderer worker returned invalid JSON: {exc}"
            ) from exc
        if not response.get("ok", False):
            error = str(response.get("error", "") or "unknown native renderer worker error")
            raise NativeBridgeError(error)


def _render_result_from_worker_output(
    source: str,
    metadata: dict[str, Any],
    pixels: bytes,
    *,
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
        raise NativeBridgeError("native renderer worker returned an invalid RGBA buffer length")

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
        source_sha256 = source_file_sha256(source)
    except OSError:
        source_sha256 = ""
    return NativeRenderResult(
        clip_path=source,
        width=width,
        height=height,
        root_layer_id=int(metadata["root_layer_id"]),
        layer_count=int(metadata["layer_count"]),
        external_data_count=int(metadata["external_data_count"]),
        renderer_abi=EXPECTED_ABI_VERSION,
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


def _compact_json(value: Any) -> str:
    if value is None:
        return ""
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True)


def source_file_sha256(path: str | os.PathLike[str]) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _ensure_image(
    bpy_module: Any,
    result: NativeRenderResult,
    *,
    image: Any | None,
    image_name: str | None,
    allow_resize: bool,
) -> Any:
    if image is not None:
        size = tuple(getattr(image, "size", (0, 0)))
        if size != (result.width, result.height):
            if allow_resize and hasattr(image, "scale"):
                image.scale(result.width, result.height)
                return image
            raise NativeBridgeError(
                f"existing image size {size} does not match native render {result.width}x{result.height}"
            )
        return image

    name = image_name or Path(result.clip_path).name
    existing = bpy_module.data.images.get(name)
    if existing is not None and tuple(getattr(existing, "size", (0, 0))) == (
        result.width,
        result.height,
    ):
        return existing
    return bpy_module.data.images.new(
        name,
        width=result.width,
        height=result.height,
        alpha=True,
        float_buffer=False,
    )


def _write_source_properties(image: Any, result: NativeRenderResult) -> None:
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
            _delete_image_key(image, CLIP_RELOAD_MANIFEST_KEY)
    else:
        _delete_image_key(image, CLIP_RELOAD_MANIFEST_KEY)
    image[CLIP_RELOAD_DIFF_MODE_KEY] = result.reload_diff_mode
    image[CLIP_RELOAD_PATCH_COUNT_KEY] = len(result.patches)
    _write_support_properties(image, result.support_summary)
    write_reload_status(image, RELOAD_STATUS_OK)


def _write_phase_properties(
    image: Any,
    result: NativeRenderResult,
    *,
    convert_seconds: float,
    foreach_seconds: float,
    update_seconds: float,
    pack_seconds: float,
    upload_seconds: float,
) -> None:
    if result.worker_seconds is None:
        _delete_image_key(image, CLIP_PHASE_WORKER_SECONDS_KEY)
    else:
        image[CLIP_PHASE_WORKER_SECONDS_KEY] = float(result.worker_seconds)
    if result.output_read_seconds is None:
        _delete_image_key(image, CLIP_PHASE_OUTPUT_READ_SECONDS_KEY)
    else:
        image[CLIP_PHASE_OUTPUT_READ_SECONDS_KEY] = float(result.output_read_seconds)
    image[CLIP_PHASE_CONVERT_SECONDS_KEY] = float(convert_seconds)
    image[CLIP_PHASE_FOREACH_SECONDS_KEY] = float(foreach_seconds)
    image[CLIP_PHASE_UPDATE_SECONDS_KEY] = float(update_seconds)
    image[CLIP_PHASE_PACK_SECONDS_KEY] = float(pack_seconds)
    image[CLIP_PHASE_UPLOAD_SECONDS_KEY] = float(upload_seconds)


def _write_support_properties(image: Any, summary: NativeSupportSummary | None) -> None:
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
    _delete_image_key(image, CLIP_SUPPORT_RASTER_BYTES_KEY)
    _delete_image_key(image, CLIP_SUPPORT_MAX_RASTER_BYTES_KEY)
    _delete_image_key(image, CLIP_SUPPORT_MASK_BYTES_KEY)
    _delete_image_key(image, CLIP_SUPPORT_MAX_MASK_BYTES_KEY)


def _clear_reload_error(image: Any) -> None:
    _delete_image_key(image, CLIP_RELOAD_ERROR_KEY)


def _delete_image_key(image: Any, key: str) -> None:
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


def _rgba8_to_blender_float_sequence(pixels: bytes, width: int, height: int) -> Any:
    row_len = int(width) * 4
    try:
        import numpy as np

        rows = np.frombuffer(pixels, dtype=np.uint8).reshape((int(height), row_len))
        return rows[::-1].copy().astype(np.float32).ravel() / np.float32(255.0)
    except Exception:
        values = array("f")
        for y in range(int(height) - 1, -1, -1):
            start = y * row_len
            values.extend(value / 255.0 for value in pixels[start : start + row_len])
        return values


def _rgba8_row_to_blender_float_sequence(pixels: bytes) -> Any:
    try:
        import numpy as np

        return np.frombuffer(pixels, dtype=np.uint8).astype(np.float32) / np.float32(255.0)
    except Exception:
        return array("f", (value / 255.0 for value in pixels))


def _apply_rgba8_patches_to_image(image: Any, result: NativeRenderResult) -> tuple[float, float]:
    convert_seconds = 0.0
    foreach_seconds = 0.0
    for patch in result.patches:
        row_len = patch.width * 4
        patch_len = row_len * patch.height
        patch_bytes = result.pixels_rgba8[
            patch.byte_offset : patch.byte_offset + patch_len
        ]
        if len(patch_bytes) != patch_len:
            raise NativeBridgeError("native renderer patch buffer is truncated")
        for row in range(patch.height):
            row_start = row * row_len
            row_end = row_start + row_len
            convert_started = time.perf_counter()
            values = _rgba8_row_to_blender_float_sequence(patch_bytes[row_start:row_end])
            convert_seconds += time.perf_counter() - convert_started

            blender_y = result.height - 1 - (patch.y + row)
            pixel_start = (blender_y * result.width + patch.x) * 4
            pixel_end = pixel_start + row_len
            foreach_started = time.perf_counter()
            image.pixels[pixel_start:pixel_end] = values
            foreach_seconds += time.perf_counter() - foreach_started
    return convert_seconds, foreach_seconds


def _status_name(status: int) -> str:
    return {
        1: "NullArgument",
        2: "InvalidUtf8Path",
        3: "OpenFailed",
        4: "InvalidRegion",
        5: "ReadFailed",
        6: "BufferTooSmall",
    }.get(status, f"status {status}")
