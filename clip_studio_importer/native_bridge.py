from __future__ import annotations

from array import array
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile
import threading
import time
from typing import Any

from .image_state import (
    CLIP_CANVAS_HEIGHT_KEY,
    CLIP_CANVAS_WIDTH_KEY,
    CLIP_EXTERNAL_COUNT_KEY,
    CLIP_LAYER_COUNT_KEY,
    CLIP_MTIME_KEY,
    CLIP_NATIVE_KEY,
    CLIP_PHASE_CONVERT_SECONDS_KEY,
    CLIP_PHASE_FOREACH_SECONDS_KEY,
    CLIP_PHASE_OUTPUT_READ_SECONDS_KEY,
    CLIP_PHASE_PACK_SECONDS_KEY,
    CLIP_PHASE_UPDATE_SECONDS_KEY,
    CLIP_PHASE_UPLOAD_SECONDS_KEY,
    CLIP_PHASE_WORKER_SECONDS_KEY,
    CLIP_RELOAD_DIFF_MODE_KEY,
    CLIP_RELOAD_ERROR_KEY,
    CLIP_RELOAD_LAST_SECONDS_KEY,
    CLIP_RELOAD_MANIFEST_KEY,
    CLIP_RELOAD_PATCH_COUNT_KEY,
    CLIP_RELOAD_STARTED_AT_KEY,
    CLIP_RELOAD_STATUS_KEY,
    CLIP_RENDERER_ABI_KEY,
    CLIP_RENDERER_VERSION_KEY,
    CLIP_ROOT_LAYER_KEY,
    CLIP_SHA256_KEY,
    CLIP_SIZE_KEY,
    CLIP_SOURCE_KEY,
    CLIP_SUPPORT_DETAILS_KEY,
    CLIP_SUPPORT_LOCATIONS_KEY,
    CLIP_SUPPORT_MASK_COUNT_KEY,
    CLIP_SUPPORT_MASK_BYTES_KEY,
    CLIP_SUPPORT_MAX_MASK_HEIGHT_KEY,
    CLIP_SUPPORT_MAX_MASK_LAYER_KEY,
    CLIP_SUPPORT_MAX_MASK_WIDTH_KEY,
    CLIP_SUPPORT_MAX_MASK_BYTES_KEY,
    CLIP_SUPPORT_MAX_RASTER_HEIGHT_KEY,
    CLIP_SUPPORT_MAX_RASTER_LAYER_KEY,
    CLIP_SUPPORT_MAX_RASTER_WIDTH_KEY,
    CLIP_SUPPORT_MAX_RASTER_BYTES_KEY,
    CLIP_SUPPORT_RASTER_COUNT_KEY,
    CLIP_SUPPORT_RASTER_BYTES_KEY,
    CLIP_SUPPORT_REPORT_KEY,
    CLIP_SUPPORT_SOURCE_COUNT_KEY,
    CLIP_SUPPORT_STATUS_KEY,
    CLIP_SUPPORT_UNSUPPORTED_COUNT_KEY,
    NativeImageSourceState,
    SupportDetailRecord,
    delete_key,
    inspect_native_image_source,
    resolve_source_path,
    source_file_sha256,
    support_detail_locations,
    support_detail_records,
    write_phase_properties,
    write_reload_error,
    write_reload_status,
    write_render_result_properties,
    RELOAD_STATUS_ERROR,
    RELOAD_STATUS_MISSING,
    RELOAD_STATUS_OK,
    RELOAD_STATUS_REFRESHING,
    RELOAD_STATUS_STALE,
    SUPPORT_STATUS_FULL,
    SUPPORT_STATUS_UNKNOWN,
    SUPPORT_STATUS_UNSUPPORTED,
)
from . import worker_protocol
from .worker_protocol import (
    NativeRenderPatch,
    NativeRenderResult,
    NativeSupportSummary,
    WorkerProtocolError,
)


EXPECTED_ABI_VERSION = 1


class NativeBridgeError(RuntimeError):
    pass


class NativeWorkerTransportError(NativeBridgeError):
    pass


_PERSISTENT_WORKER_LOCK = threading.Lock()
_PERSISTENT_WORKER: "PersistentNativeRendererWorker | None" = None


def render_clip_rgba8(
    clip_path: str | os.PathLike[str],
    *,
    previous_manifest_json: str | None = None,
) -> NativeRenderResult:
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

    write_render_result_properties(image, result)
    pack_seconds = 0.0
    if pack and hasattr(image, "pack"):
        pack_started = time.perf_counter()
        image.pack()
        pack_seconds = time.perf_counter() - pack_started
    upload_seconds = time.perf_counter() - upload_started
    write_phase_properties(
        image,
        result,
        convert_seconds=convert_seconds,
        foreach_seconds=foreach_seconds,
        update_seconds=update_seconds,
        pack_seconds=pack_seconds,
        upload_seconds=upload_seconds,
    )
    return image


def packaged_renderer_worker_path() -> str | None:
    module_dir = Path(__file__).resolve().parent
    for candidate in _packaged_renderer_worker_candidates(module_dir):
        if candidate.exists():
            return str(candidate)
    return None


def _runtime_platform_id() -> str | None:
    machine = platform.machine().lower()
    system = platform.system()
    is_x64 = machine in {"amd64", "x86_64"}
    is_arm64 = machine in {"arm64", "aarch64"}
    if os.name == "nt" and is_x64:
        return "windows-x64"
    if system == "Linux" and is_x64:
        return "linux-x64"
    if system == "Darwin" and is_x64:
        return "macos-x64"
    if system == "Darwin" and is_arm64:
        return "macos-arm64"
    return None


def _runtime_worker_name() -> str:
    if _runtime_platform_id() in {"linux-x64", "macos-x64", "macos-arm64"}:
        return "clip_cli"
    return "clip_cli.exe"


def _packaged_renderer_worker_candidates(module_dir: Path) -> list[Path]:
    platform_id = _runtime_platform_id()
    worker_name = _runtime_worker_name()
    candidates: list[Path] = []
    if platform_id:
        candidates.append(module_dir / "native" / platform_id / worker_name)
    candidates.extend(
        [
            module_dir / worker_name,
            module_dir / "native" / worker_name,
            module_dir / "clip_cli.exe",
            module_dir / "clip_cli",
            module_dir / "native" / "clip_cli.exe",
            module_dir / "native" / "clip_cli",
        ]
    )
    return list(dict.fromkeys(candidates))


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
            files = worker_protocol.prepare_render_files(temp, previous_manifest_json)
            command = worker_protocol.one_shot_command(
                self.executable_path,
                source,
                files,
            )
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
                metadata, pixels = worker_protocol.read_render_output(files)
                output_read_seconds = time.perf_counter() - read_started
            except WorkerProtocolError as exc:
                raise NativeBridgeError(str(exc)) from exc

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
                files = worker_protocol.prepare_render_files(temp, previous_manifest_json)
                request = worker_protocol.persistent_request(source, files)

                worker_started = time.perf_counter()
                self._send_request_locked(request)
                worker_seconds = time.perf_counter() - worker_started

                try:
                    read_started = time.perf_counter()
                    metadata, pixels = worker_protocol.read_render_output(files)
                    output_read_seconds = time.perf_counter() - read_started
                except WorkerProtocolError as exc:
                    raise NativeBridgeError(str(exc)) from exc

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
            process.stdin.write(worker_protocol.persistent_request_line(request))
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
    try:
        return worker_protocol.render_result_from_worker_output(
            source,
            metadata,
            pixels,
            expected_abi_version=EXPECTED_ABI_VERSION,
            worker_seconds=worker_seconds,
            output_read_seconds=output_read_seconds,
        )
    except WorkerProtocolError as exc:
        raise NativeBridgeError(str(exc)) from exc


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
