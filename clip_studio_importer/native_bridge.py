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

RELOAD_STATUS_OK = "ok"
RELOAD_STATUS_STALE = "stale_source"
RELOAD_STATUS_MISSING = "missing_source"
RELOAD_STATUS_REFRESHING = "refreshing"
RELOAD_STATUS_ERROR = "error"


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
    write_reload_status(image, RELOAD_STATUS_OK)


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
    }.get(status, f"status {status}")
