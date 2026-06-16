from __future__ import annotations

from array import array
import ctypes
from dataclasses import dataclass
import os
from pathlib import Path
from typing import Any


EXPECTED_ABI_VERSION = 1

CLIP_SOURCE_KEY = "clip_source"
CLIP_MTIME_KEY = "clip_mtime"
CLIP_NATIVE_KEY = "clip_native_renderer"
CLIP_RENDERER_ABI_KEY = "clip_renderer_abi"
CLIP_CANVAS_WIDTH_KEY = "clip_canvas_width"
CLIP_CANVAS_HEIGHT_KEY = "clip_canvas_height"
CLIP_ROOT_LAYER_KEY = "clip_root_layer_id"
CLIP_LAYER_COUNT_KEY = "clip_layer_count"
CLIP_EXTERNAL_COUNT_KEY = "clip_external_data_count"
CLIP_RELOAD_STATUS_KEY = "clip_reload_status"
CLIP_RELOAD_ERROR_KEY = "clip_reload_error"
CLIP_SUPPORT_STATUS_KEY = "clip_support_status"
CLIP_SUPPORT_REPORT_KEY = "clip_support_report"
CLIP_SUPPORT_DETAILS_KEY = "clip_support_details"
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

RELOAD_STATUS_OK = "ok"
RELOAD_STATUS_STALE = "stale_source"
RELOAD_STATUS_MISSING = "missing_source"
RELOAD_STATUS_REFRESHING = "refreshing"
RELOAD_STATUS_ERROR = "error"

SUPPORT_STATUS_FULL = "full"
SUPPORT_STATUS_UNSUPPORTED = "unsupported"
SUPPORT_STATUS_UNKNOWN = "unknown"


class NativeBridgeError(RuntimeError):
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
    source_mtime: float | None
    pixels_rgba8: bytes
    support_summary: "NativeSupportSummary | None" = None


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
class NativeImageSourceState:
    clip_path: str
    stored_mtime: float | None
    current_mtime: float | None
    should_reload: bool
    status: str


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

            return NativeRenderResult(
                clip_path=source,
                width=int(info.width),
                height=int(info.height),
                root_layer_id=int(info.root_layer_id),
                layer_count=int(info.layer_count),
                external_data_count=int(info.external_data_count),
                renderer_abi=self.abi_version,
                source_mtime=source_mtime,
                pixels_rgba8=bytes(pixels),
                support_summary=support_summary,
            )
        finally:
            self._dll.clip_renderer_session_close(session)

    def _configure_signatures(self) -> None:
        self._dll.clip_renderer_abi_version.argtypes = []
        self._dll.clip_renderer_abi_version.restype = ctypes.c_uint32

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


def render_clip_rgba8(
    clip_path: str | os.PathLike[str],
    *,
    library_path: str | os.PathLike[str] | None = None,
    renderer: Any | None = None,
) -> NativeRenderResult:
    if renderer is not None:
        return renderer.render_rgba8(clip_path)
    return NativeRendererLibrary(resolve_renderer_library(library_path)).render_rgba8(clip_path)


def import_clip_as_image(
    clip_path: str | os.PathLike[str],
    *,
    bpy_module: Any,
    library_path: str | os.PathLike[str] | None = None,
    image_name: str | None = None,
    pack: bool = True,
    renderer: Any | None = None,
) -> Any:
    result = render_clip_rgba8(clip_path, library_path=library_path, renderer=renderer)
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
) -> Any:
    if len(result.pixels_rgba8) != result.width * result.height * 4:
        raise NativeBridgeError("native renderer returned an invalid RGBA buffer length")

    image = _ensure_image(bpy_module, result, image=image, image_name=image_name)
    image.source = "GENERATED"
    if hasattr(image, "colorspace_settings"):
        image.colorspace_settings.name = "sRGB"

    image.pixels.foreach_set(_rgba8_to_float_sequence(result.pixels_rgba8))
    image.update()
    _write_source_properties(image, result)
    if pack and hasattr(image, "pack"):
        image.pack()
    return image


def inspect_native_image_source(
    image: Any,
    *,
    exists: Any = os.path.exists,
    getmtime: Any = os.path.getmtime,
) -> NativeImageSourceState:
    clip_path = str(image.get(CLIP_SOURCE_KEY, "") or "")
    stored_mtime = _parse_mtime(image.get(CLIP_MTIME_KEY, ""))
    if not clip_path:
        return NativeImageSourceState(
            clip_path="",
            stored_mtime=stored_mtime,
            current_mtime=None,
            should_reload=False,
            status=RELOAD_STATUS_MISSING,
        )

    if not exists(clip_path):
        return NativeImageSourceState(
            clip_path=clip_path,
            stored_mtime=stored_mtime,
            current_mtime=None,
            should_reload=False,
            status=RELOAD_STATUS_MISSING,
        )

    try:
        current_mtime = float(getmtime(clip_path))
    except OSError:
        return NativeImageSourceState(
            clip_path=clip_path,
            stored_mtime=stored_mtime,
            current_mtime=None,
            should_reload=False,
            status=RELOAD_STATUS_MISSING,
        )

    should_reload = stored_mtime is None or current_mtime > stored_mtime + 1e-6
    return NativeImageSourceState(
        clip_path=clip_path,
        stored_mtime=stored_mtime,
        current_mtime=current_mtime,
        should_reload=should_reload,
        status=RELOAD_STATUS_STALE if should_reload else RELOAD_STATUS_OK,
    )


def write_reload_status(image: Any, status: str) -> None:
    image[CLIP_RELOAD_STATUS_KEY] = status
    if status != RELOAD_STATUS_ERROR:
        _clear_reload_error(image)


def write_reload_error(image: Any, message: str) -> None:
    image[CLIP_RELOAD_STATUS_KEY] = RELOAD_STATUS_ERROR
    image[CLIP_RELOAD_ERROR_KEY] = message


def resolve_renderer_library(
    library_path: str | os.PathLike[str] | None = None,
) -> str:
    if library_path:
        return str(Path(library_path).expanduser())

    env_path = os.environ.get("RIZUM_CLIP_RENDERER_DLL")
    if env_path:
        return env_path

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

    raise NativeBridgeError(
        "native renderer library not found; set RIZUM_CLIP_RENDERER_DLL or configure the add-on path"
    )


def _ensure_image(
    bpy_module: Any,
    result: NativeRenderResult,
    *,
    image: Any | None,
    image_name: str | None,
) -> Any:
    if image is not None:
        size = tuple(getattr(image, "size", (0, 0)))
        if size != (result.width, result.height):
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
    image[CLIP_NATIVE_KEY] = True
    image[CLIP_RENDERER_ABI_KEY] = result.renderer_abi
    image[CLIP_CANVAS_WIDTH_KEY] = result.width
    image[CLIP_CANVAS_HEIGHT_KEY] = result.height
    image[CLIP_ROOT_LAYER_KEY] = result.root_layer_id
    image[CLIP_LAYER_COUNT_KEY] = result.layer_count
    image[CLIP_EXTERNAL_COUNT_KEY] = result.external_data_count
    _write_support_properties(image, result.support_summary)
    write_reload_status(image, RELOAD_STATUS_OK)


def _write_support_properties(image: Any, summary: NativeSupportSummary | None) -> None:
    if summary is None:
        image[CLIP_SUPPORT_STATUS_KEY] = SUPPORT_STATUS_UNKNOWN
        image[CLIP_SUPPORT_REPORT_KEY] = "Native support summary unavailable."
        image[CLIP_SUPPORT_DETAILS_KEY] = ""
        return
    image[CLIP_SUPPORT_STATUS_KEY] = (
        SUPPORT_STATUS_FULL
        if summary.unsupported_count == 0
        else SUPPORT_STATUS_UNSUPPORTED
    )
    image[CLIP_SUPPORT_REPORT_KEY] = summary.report
    image[CLIP_SUPPORT_DETAILS_KEY] = "\n".join(summary.details)
    image[CLIP_SUPPORT_SOURCE_COUNT_KEY] = summary.source_count
    image[CLIP_SUPPORT_UNSUPPORTED_COUNT_KEY] = summary.unsupported_count
    image[CLIP_SUPPORT_RASTER_COUNT_KEY] = summary.raster_count
    image[CLIP_SUPPORT_RASTER_BYTES_KEY] = summary.raster_bytes
    image[CLIP_SUPPORT_MAX_RASTER_LAYER_KEY] = summary.max_raster_layer_id
    image[CLIP_SUPPORT_MAX_RASTER_WIDTH_KEY] = summary.max_raster_width
    image[CLIP_SUPPORT_MAX_RASTER_HEIGHT_KEY] = summary.max_raster_height
    image[CLIP_SUPPORT_MAX_RASTER_BYTES_KEY] = summary.max_raster_bytes
    image[CLIP_SUPPORT_MASK_COUNT_KEY] = summary.mask_count
    image[CLIP_SUPPORT_MASK_BYTES_KEY] = summary.mask_bytes
    image[CLIP_SUPPORT_MAX_MASK_LAYER_KEY] = summary.max_mask_layer_id
    image[CLIP_SUPPORT_MAX_MASK_WIDTH_KEY] = summary.max_mask_width
    image[CLIP_SUPPORT_MAX_MASK_HEIGHT_KEY] = summary.max_mask_height
    image[CLIP_SUPPORT_MAX_MASK_BYTES_KEY] = summary.max_mask_bytes


def _clear_reload_error(image: Any) -> None:
    try:
        keys = image.keys()
    except AttributeError:
        keys = ()
    if CLIP_RELOAD_ERROR_KEY not in keys:
        return
    try:
        del image[CLIP_RELOAD_ERROR_KEY]
    except Exception:
        image[CLIP_RELOAD_ERROR_KEY] = ""


def _parse_mtime(value: Any) -> float | None:
    if value is None or value == "":
        return None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def _rgba8_to_float_sequence(pixels: bytes) -> Any:
    try:
        import numpy as np

        return np.frombuffer(pixels, dtype=np.uint8).astype(np.float32) / np.float32(255.0)
    except Exception:
        return array("f", (value / 255.0 for value in pixels))


def _status_name(status: int) -> str:
    return {
        1: "NullArgument",
        2: "InvalidUtf8Path",
        3: "OpenFailed",
        4: "InvalidRegion",
        5: "ReadFailed",
        6: "BufferTooSmall",
    }.get(status, f"status {status}")
