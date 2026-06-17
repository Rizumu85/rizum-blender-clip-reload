"""
Clip Studio Paint (.clip) Importer for Blender.

The default importer path calls the Rust C ABI, uploads RGBA pixels into a
generated Blender Image, packs rendered pixels on save or explicit user
request, and stores .clip source-tracking properties for reload/watch updates.

No external auto-reload add-on is required.
"""

from __future__ import annotations

bl_info = {
    "name": "Clip Studio Paint (.clip) Importer",
    "author": "Rizum",
    "version": (0, 8, 62),
    "blender": (3, 0, 0),
    "location": "File > Import > Clip Studio (.clip)",
    "description": "Read .clip files as flattened image textures with non-blocking auto-reload.",
    "category": "Import-Export",
}

import os
import threading
import time

import bpy
from bpy.app.handlers import persistent
from bpy.props import BoolProperty, FloatProperty, StringProperty
from bpy.types import AddonPreferences, Operator, Panel
from bpy_extras.io_utils import ImportHelper

from . import native_bridge


CLIP_SOURCE_KEY = "clip_source"   # custom prop on Image: path to source .clip
CLIP_MTIME_KEY = "clip_mtime"     # custom prop on Image: last-seen mtime (str)
CLIP_SIZE_KEY = native_bridge.CLIP_SIZE_KEY
CLIP_SHA256_KEY = native_bridge.CLIP_SHA256_KEY
CLIP_NATIVE_KEY = native_bridge.CLIP_NATIVE_KEY
CLIP_SUPPORT_DETAILS_EXPANDED_KEY = "clip_support_details_expanded"
CLIP_PACK_STATUS_KEY = "clip_pack_status"
CLIP_PACK_LAST_SECONDS_KEY = "clip_pack_last_seconds"
CLIP_PACK_ERROR_KEY = "clip_pack_error"
PACK_STATUS_PACKED = "packed"
PACK_STATUS_NEEDS_PACK = "needs_pack"
PACK_STATUS_RENDERING = "rendering"
PACK_STATUS_PACKING = "packing"
PACK_STATUS_ERROR = "error"
SUPPORT_DETAIL_PREVIEW_LINES = 4
ADDON_PKG = __package__


# Module-level state for background-thread coordination.
# All access must go through `_state_lock`.
_state_lock = threading.Lock()
_in_flight: set = set()                 # clip paths currently being decoded


# --------------------------------------------------------------------------- #
# Helpers
# --------------------------------------------------------------------------- #

def _import_clip_as_image(clip_path: str) -> bpy.types.Image:
    started_at = time.time()
    image = native_bridge.import_clip_as_image(
        clip_path,
        bpy_module=bpy,
        pack=False,
    )
    image[native_bridge.CLIP_RELOAD_LAST_SECONDS_KEY] = time.time() - started_at
    _mark_image_needs_pack(image)
    return image


def _delete_image_key(image, key: str) -> None:
    try:
        del image[key]
    except (KeyError, TypeError):
        pass


def _set_pack_status(image, status: str, *, error: str = "") -> None:
    image[CLIP_PACK_STATUS_KEY] = status
    if error:
        image[CLIP_PACK_ERROR_KEY] = error
    elif status != PACK_STATUS_ERROR:
        _delete_image_key(image, CLIP_PACK_ERROR_KEY)


def _mark_image_needs_pack(image) -> None:
    _set_pack_status(image, PACK_STATUS_NEEDS_PACK)


def _resolve_clip_source_path(path: str | None) -> str:
    return native_bridge.resolve_source_path(str(path or ""), _blender_abspath())


def _blender_abspath():
    bpy_path = getattr(bpy, "path", None)
    return getattr(bpy_path, "abspath", None)


def _image_source_matches(img, clip_path: str) -> bool:
    stored_path = str(img.get(CLIP_SOURCE_KEY, "") or "")
    return stored_path == clip_path or _resolve_clip_source_path(stored_path) == clip_path


def _pack_status_label(status: str) -> str:
    return {
        PACK_STATUS_PACKED: "Packed",
        PACK_STATUS_NEEDS_PACK: "Needs Pack",
        PACK_STATUS_RENDERING: "Waiting for render",
        PACK_STATUS_PACKING: "Packing",
        PACK_STATUS_ERROR: "Pack Error",
    }.get(status, "Unknown")


def _pack_status_icon(status: str) -> str:
    return {
        PACK_STATUS_PACKED: "CHECKMARK",
        PACK_STATUS_NEEDS_PACK: "INFO",
        PACK_STATUS_RENDERING: "SORTTIME",
        PACK_STATUS_PACKING: "SORTTIME",
        PACK_STATUS_ERROR: "ERROR",
    }.get(status, "INFO")


def _pack_image_now(image) -> float:
    if not hasattr(image, "pack"):
        raise RuntimeError("Blender image does not support packing")
    _set_pack_status(image, PACK_STATUS_PACKING)
    started_at = time.perf_counter()
    try:
        image.pack()
    except Exception as exc:
        _set_pack_status(image, PACK_STATUS_ERROR, error=str(exc))
        raise
    seconds = time.perf_counter() - started_at
    image[CLIP_PACK_LAST_SECONDS_KEY] = float(seconds)
    _set_pack_status(image, PACK_STATUS_PACKED)
    return seconds


def _schedule_pack_after_initial_import(image_name: str, clip_path: str) -> None:
    def _on_pack():
        img = bpy.data.images.get(image_name)
        if img is None or not _image_source_matches(img, clip_path):
            return None
        with _state_lock:
            running = bool(clip_path and clip_path in _in_flight)
        if running or img.get(CLIP_PACK_STATUS_KEY) != PACK_STATUS_NEEDS_PACK:
            return None
        try:
            _pack_image_now(img)
        except Exception as exc:
            try:
                if _addon_prefs().debug:
                    print(
                        "[clip_studio_importer] "
                        f"initial pack failed for {clip_path}: {exc}"
                    )
            except Exception:
                pass
        return None

    bpy.app.timers.register(_on_pack, first_interval=0.1)


def _unique_image_name(name: str) -> str:
    base = name or "Clip Studio Image"
    if bpy.data.images.get(base) is None:
        return base
    index = 1
    while True:
        candidate = f"{base}.{index:03d}"
        if bpy.data.images.get(candidate) is None:
            return candidate
        index += 1


def _show_image_in_open_image_editors(context, image) -> int:
    screen = getattr(context, "screen", None)
    if screen is None:
        screen = getattr(getattr(bpy, "context", None), "screen", None)
    if screen is None:
        return 0

    shown = 0
    for area in getattr(screen, "areas", []):
        if getattr(area, "type", "") != "IMAGE_EDITOR":
            continue
        spaces = getattr(area, "spaces", None)
        active_space = getattr(spaces, "active", None)
        if active_space is None:
            try:
                active_space = spaces[0]
            except (TypeError, IndexError):
                active_space = None
        if active_space is None or not hasattr(active_space, "image"):
            continue
        active_space.image = image
        shown += 1
    return shown


def _addon_prefs():
    return bpy.context.preferences.addons[ADDON_PKG].preferences


def _reload_status_label(status: str) -> str:
    return {
        native_bridge.RELOAD_STATUS_OK: "Ready",
        native_bridge.RELOAD_STATUS_STALE: "Source changed",
        native_bridge.RELOAD_STATUS_MISSING: "Source missing",
        native_bridge.RELOAD_STATUS_REFRESHING: "Rendering",
        native_bridge.RELOAD_STATUS_ERROR: "Render failed",
    }.get(status, "Unknown")


def _reload_status_icon(status: str) -> str:
    return {
        native_bridge.RELOAD_STATUS_OK: "CHECKMARK",
        native_bridge.RELOAD_STATUS_STALE: "FILE_REFRESH",
        native_bridge.RELOAD_STATUS_MISSING: "ERROR",
        native_bridge.RELOAD_STATUS_REFRESHING: "SORTTIME",
        native_bridge.RELOAD_STATUS_ERROR: "ERROR",
    }.get(status, "INFO")


def _short_diagnostic(message: str, limit: int = 120) -> str:
    text = " ".join(str(message).split())
    if len(text) <= limit:
        return text
    if limit <= 3:
        return "." * max(limit, 0)
    return text[: limit - 3] + "..."


def _image_int_property(img, key: str, default: int = 0) -> int:
    try:
        return int(img.get(key, default))
    except (TypeError, ValueError):
        return default


def _image_float_property(img, key: str, default: float = 0.0) -> float:
    try:
        return float(img.get(key, default))
    except (TypeError, ValueError):
        return default


def _image_has_property(img, key: str) -> bool:
    try:
        return key in img.keys()
    except AttributeError:
        return key in img


def _native_bridge_key(attribute_name: str, fallback: str) -> str:
    return getattr(native_bridge, attribute_name, fallback)


def _format_seconds(value: float) -> str:
    if value < 0:
        value = 0.0
    if value < 10.0:
        return f"{value:.1f}s"
    return f"{value:.0f}s"


def _format_byte_count(value: int) -> str:
    if value <= 0:
        return "0 B"
    amount = float(value)
    for unit in ("B", "KiB", "MiB", "GiB"):
        if amount < 1024.0 or unit == "GiB":
            if unit == "B":
                return f"{int(amount)} {unit}"
            return f"{amount:.1f} {unit}"
        amount /= 1024.0
    return f"{value} B"


def _support_location_lines(img) -> list[str]:
    stored_locations = img.get(native_bridge.CLIP_SUPPORT_LOCATIONS_KEY, "")
    locations = [line for line in str(stored_locations).splitlines() if line]
    if locations:
        return locations
    support_details = img.get(native_bridge.CLIP_SUPPORT_DETAILS_KEY, "")
    return list(native_bridge.support_detail_locations(support_details))


def _short_support_location(location: str) -> str:
    text = str(location)
    if text.startswith("layer ") and " node " in text:
        layer_label, node_detail = text[len("layer "):].split(" node ", 1)
        parts = node_detail.split(maxsplit=1)
        if not parts:
            return text
        node_id = parts[0]
        kind = parts[1] if len(parts) > 1 else ""
        suffix = f" {kind}" if kind else ""
        return f"layer {layer_label}/node {node_id}{suffix}"
    return text


def _support_location_summary(img, *, limit: int = 3) -> str:
    locations = _support_location_lines(img)
    if not locations:
        return ""
    visible = [_short_support_location(line) for line in locations[:limit]]
    if len(locations) > limit:
        visible.append(f"+{len(locations) - limit} more")
    return "Locations: " + ", ".join(visible)


def _support_diagnostic_text(img) -> str:
    clip_path = img.get(CLIP_SOURCE_KEY, "")
    status = img.get(native_bridge.CLIP_RELOAD_STATUS_KEY, "unknown")
    width = _image_int_property(img, native_bridge.CLIP_CANVAS_WIDTH_KEY)
    height = _image_int_property(img, native_bridge.CLIP_CANVAS_HEIGHT_KEY)
    lines = [
        "Clip Studio native render diagnostics",
        f"Source: {clip_path}",
        f"Status: {_reload_status_label(status)}",
    ]
    source_size = _image_int_property(img, CLIP_SIZE_KEY)
    if source_size:
        lines.append(f"Source size: {_format_byte_count(source_size)}")
    source_sha256 = str(img.get(CLIP_SHA256_KEY, "") or "")
    if source_sha256:
        lines.append(f"Source SHA-256: {source_sha256}")
    if status == native_bridge.RELOAD_STATUS_REFRESHING:
        started_at = _image_float_property(img, native_bridge.CLIP_RELOAD_STARTED_AT_KEY)
        if started_at:
            lines.append(f"Render elapsed: {_format_seconds(time.time() - started_at)}")
    last_seconds = _image_float_property(img, native_bridge.CLIP_RELOAD_LAST_SECONDS_KEY)
    if last_seconds:
        lines.append(f"Last render duration: {_format_seconds(last_seconds)}")
    diff_mode = str(img.get(native_bridge.CLIP_RELOAD_DIFF_MODE_KEY, "") or "")
    if diff_mode:
        patch_count = _image_int_property(img, native_bridge.CLIP_RELOAD_PATCH_COUNT_KEY)
        if diff_mode == "patch":
            lines.append(f"Reload diff: patch ({patch_count} rects)")
        elif diff_mode == "no_change":
            lines.append("Reload diff: no changed tiles")
        else:
            lines.append("Reload diff: full")
    phase_lines = _timing_phase_lines(img)
    if phase_lines:
        lines.append("Timing phases:")
        lines.extend(f"- {line}" for line in phase_lines)
    pack_status = str(img.get(CLIP_PACK_STATUS_KEY, "") or "")
    if pack_status:
        lines.append(f"Pack status: {_pack_status_label(pack_status)}")
    pack_seconds = _image_float_property(img, CLIP_PACK_LAST_SECONDS_KEY)
    if pack_seconds:
        lines.append(f"Last pack duration: {_format_seconds(pack_seconds)}")
    pack_error = str(img.get(CLIP_PACK_ERROR_KEY, "") or "")
    if pack_error:
        lines.append(f"Pack error: {pack_error}")
    if width and height:
        lines.append(f"Canvas: {width}x{height}")
    renderer_abi = _image_int_property(img, native_bridge.CLIP_RENDERER_ABI_KEY)
    if renderer_abi:
        lines.append(f"Renderer ABI: {renderer_abi}")
    renderer_version = img.get(native_bridge.CLIP_RENDERER_VERSION_KEY, "")
    if renderer_version:
        lines.append(f"Renderer version: {renderer_version}")
    layer_count = _image_int_property(img, native_bridge.CLIP_LAYER_COUNT_KEY)
    if layer_count:
        lines.append(f"Layers: {layer_count}")
    unsupported_count = _image_int_property(
        img,
        native_bridge.CLIP_SUPPORT_UNSUPPORTED_COUNT_KEY,
    )
    if unsupported_count:
        lines.append(f"Unsupported native nodes: {unsupported_count}")
    location_lines = _support_location_lines(img)
    if location_lines:
        lines.append("Unsupported locations:")
        lines.extend(f"- {line}" for line in location_lines)
    support_details = img.get(native_bridge.CLIP_SUPPORT_DETAILS_KEY, "")
    detail_lines = [line for line in str(support_details).splitlines() if line]
    if detail_lines:
        lines.append("Unsupported details:")
        lines.extend(detail_lines)
    error = img.get(native_bridge.CLIP_RELOAD_ERROR_KEY, "")
    if error:
        lines.append(f"Render error: {error}")
    return "\n".join(lines)


def _timing_phase_lines(img) -> list[str]:
    phases = [
        (
            "Native worker",
            _native_bridge_key("CLIP_PHASE_WORKER_SECONDS_KEY", "clip_phase_worker_seconds"),
        ),
        (
            "Worker output read",
            _native_bridge_key(
                "CLIP_PHASE_OUTPUT_READ_SECONDS_KEY",
                "clip_phase_output_read_seconds",
            ),
        ),
        (
            "RGBA8 to Blender floats",
            _native_bridge_key("CLIP_PHASE_CONVERT_SECONDS_KEY", "clip_phase_convert_seconds"),
        ),
        (
            "Blender foreach_set",
            _native_bridge_key("CLIP_PHASE_FOREACH_SECONDS_KEY", "clip_phase_foreach_seconds"),
        ),
        (
            "Blender image update",
            _native_bridge_key("CLIP_PHASE_UPDATE_SECONDS_KEY", "clip_phase_update_seconds"),
        ),
        (
            "Blender image pack",
            _native_bridge_key("CLIP_PHASE_PACK_SECONDS_KEY", "clip_phase_pack_seconds"),
        ),
        (
            "Blender upload total",
            _native_bridge_key("CLIP_PHASE_UPLOAD_SECONDS_KEY", "clip_phase_upload_seconds"),
        ),
    ]
    lines = []
    for label, key in phases:
        if not _image_has_property(img, key):
            continue
        seconds = _image_float_property(img, key)
        lines.append(f"{label}: {_format_seconds(seconds)}")
    return lines


def _support_report_text_name(img) -> str:
    image_name = getattr(img, "name", "") or os.path.basename(img.get(CLIP_SOURCE_KEY, ""))
    image_name = str(image_name).strip() or "Clip Studio Image"
    return f"Clip Studio Diagnostics - {image_name}"


def _write_support_report_text(img):
    text_name = _support_report_text_name(img)
    texts = bpy.data.texts
    text = texts.get(text_name)
    if text is None:
        text = texts.new(text_name)
    text.clear()
    text.write(_support_diagnostic_text(img) + "\n")
    return text


def _show_text_in_editor(context, text) -> bool:
    screen = getattr(context, "screen", None)
    for area in getattr(screen, "areas", ()):
        if getattr(area, "type", None) != "TEXT_EDITOR":
            continue
        for space in getattr(area, "spaces", ()):
            if getattr(space, "type", None) == "TEXT_EDITOR":
                space.text = text
                return True
    return False


def _schedule_async_decode(
    clip_path: str,
    image_name: str | None,
    *,
    create_on_success: bool = False,
    show_on_success: bool = False,
    auto_pack_on_success: bool = False,
) -> bool:
    with _state_lock:
        if clip_path in _in_flight:
            return False
        _in_flight.add(clip_path)

    img = bpy.data.images.get(image_name) if image_name else None
    previous_manifest_json = ""
    if img is not None and _image_source_matches(img, clip_path):
        previous_manifest_json = str(
            img.get(native_bridge.CLIP_RELOAD_MANIFEST_KEY, "") or ""
        )
        img[native_bridge.CLIP_RELOAD_STARTED_AT_KEY] = time.time()
        native_bridge.write_reload_status(
            img,
            native_bridge.RELOAD_STATUS_REFRESHING,
        )
        _set_pack_status(img, PACK_STATUS_RENDERING)

    threading.Thread(
        target=_async_decode,
        args=(
            clip_path,
            image_name,
            create_on_success,
            show_on_success,
            auto_pack_on_success,
            previous_manifest_json,
        ),
        daemon=True,
    ).start()
    return True


# --------------------------------------------------------------------------- #
# Operators
# --------------------------------------------------------------------------- #

class IMPORT_OT_clip_studio(Operator, ImportHelper):
    """Import a Clip Studio Paint (.clip) file as a flattened Image."""
    bl_idname = "import_image.clip_studio"
    bl_label = "Import Clip Studio (.clip)"
    bl_options = {"REGISTER", "UNDO"}

    filename_ext = ".clip"
    filter_glob: StringProperty(default="*.clip", options={"HIDDEN"})

    def execute(self, context):
        clip_path = os.path.abspath(self.filepath)
        image_name = _unique_image_name(os.path.basename(clip_path))
        if not _schedule_async_decode(
            clip_path,
            image_name,
            create_on_success=True,
            show_on_success=True,
            auto_pack_on_success=True,
        ):
            self.report({"WARNING"}, f"Already rendering {os.path.basename(clip_path)}")
            return {"CANCELLED"}
        self.report({"INFO"},
                    f"Rendering {os.path.basename(clip_path)} in the background")
        return {"FINISHED"}


class IMAGE_OT_reload_clip_studio(Operator):
    """Re-render the .clip file this image was imported from."""
    bl_idname = "image.reload_clip_studio"
    bl_label = "Manual Reload"
    bl_options = {"REGISTER", "UNDO"}

    @classmethod
    def poll(cls, context):
        space = getattr(context, "space_data", None)
        img = getattr(space, "image", None) if space else None
        return img is not None and CLIP_SOURCE_KEY in img.keys()

    def execute(self, context):
        img = context.space_data.image
        stored_clip_path = img.get(CLIP_SOURCE_KEY)
        clip_path = _resolve_clip_source_path(stored_clip_path)
        if not clip_path or not os.path.exists(clip_path):
            native_bridge.write_reload_status(
                img,
                native_bridge.RELOAD_STATUS_MISSING,
            )
            self.report({"ERROR"}, f"Source .clip not found: {stored_clip_path!r}")
            return {"CANCELLED"}
        if not _schedule_async_decode(clip_path, img.name):
            self.report({"WARNING"}, f"Already rendering {os.path.basename(clip_path)}")
            return {"CANCELLED"}
        self.report({"INFO"}, f"Reloading {os.path.basename(clip_path)} in the background")
        return {"FINISHED"}


class IMAGE_OT_pack_clip_studio(Operator):
    """Pack current pixels now. Saving the .blend also packs images that need it."""
    bl_idname = "image.pack_clip_studio"
    bl_label = "Pack Clip Studio Image"
    bl_description = (
        "Pack current pixels into the .blend now. Saving the .blend also packs "
        "Needs Pack images automatically."
    )
    bl_options = {"REGISTER", "UNDO"}

    @classmethod
    def poll(cls, context):
        space = getattr(context, "space_data", None)
        img = getattr(space, "image", None) if space else None
        return img is not None and CLIP_SOURCE_KEY in img.keys()

    def execute(self, context):
        img = context.space_data.image
        clip_path = img.get(CLIP_SOURCE_KEY, "")
        with _state_lock:
            running = bool(clip_path and clip_path in _in_flight)
        if running:
            self.report({"WARNING"}, "Wait for the current render before packing")
            return {"CANCELLED"}
        try:
            seconds = _pack_image_now(img)
        except Exception as exc:
            self.report({"ERROR"}, f"Pack failed: {exc}")
            return {"CANCELLED"}
        self.report({"INFO"}, f"Packed {img.name} in {_format_seconds(seconds)}")
        return {"FINISHED"}


class IMAGE_OT_toggle_clip_support_details(Operator):
    """Toggle the support-detail list for the selected .clip image."""
    bl_idname = "image.toggle_clip_support_details"
    bl_label = "Toggle Support Details"
    bl_options = {"REGISTER"}

    @classmethod
    def poll(cls, context):
        space = getattr(context, "space_data", None)
        img = getattr(space, "image", None) if space else None
        return img is not None and CLIP_SOURCE_KEY in img.keys()

    def execute(self, context):
        img = context.space_data.image
        img[CLIP_SUPPORT_DETAILS_EXPANDED_KEY] = not bool(
            img.get(CLIP_SUPPORT_DETAILS_EXPANDED_KEY, False)
        )
        return {"FINISHED"}


class IMAGE_OT_copy_clip_support_diagnostics(Operator):
    """Copy the selected .clip image's native diagnostics to the clipboard."""
    bl_idname = "image.copy_clip_support_diagnostics"
    bl_label = "Copy Support Diagnostics"
    bl_options = {"REGISTER"}

    @classmethod
    def poll(cls, context):
        space = getattr(context, "space_data", None)
        img = getattr(space, "image", None) if space else None
        return img is not None and CLIP_SOURCE_KEY in img.keys()

    def execute(self, context):
        context.window_manager.clipboard = _support_diagnostic_text(
            context.space_data.image
        )
        self.report({"INFO"}, "Copied Clip Studio diagnostics")
        return {"FINISHED"}


class IMAGE_OT_copy_clip_support_locations(Operator):
    """Copy unsupported .clip layer/node locations to the clipboard."""
    bl_idname = "image.copy_clip_support_locations"
    bl_label = "Copy Layer Locations"
    bl_options = {"REGISTER"}

    @classmethod
    def poll(cls, context):
        space = getattr(context, "space_data", None)
        img = getattr(space, "image", None) if space else None
        return img is not None and CLIP_SOURCE_KEY in img.keys()

    def execute(self, context):
        img = context.space_data.image
        locations = _support_location_lines(img)
        if not locations:
            self.report({"WARNING"}, "No unsupported layer locations")
            return {"CANCELLED"}
        lines = [
            "Clip Studio unsupported layer locations",
            f"Source: {img.get(CLIP_SOURCE_KEY, '')}",
        ]
        lines.extend(f"- {location}" for location in locations)
        context.window_manager.clipboard = "\n".join(lines)
        self.report({"INFO"}, "Copied Clip Studio layer locations")
        return {"FINISHED"}


class IMAGE_OT_open_clip_support_diagnostics(Operator):
    """Open the selected .clip image's native diagnostics as a Blender text block."""
    bl_idname = "image.open_clip_support_diagnostics"
    bl_label = "Open Clip Studio Diagnostics"
    bl_options = {"REGISTER"}

    @classmethod
    def poll(cls, context):
        space = getattr(context, "space_data", None)
        img = getattr(space, "image", None) if space else None
        return img is not None and CLIP_SOURCE_KEY in img.keys()

    def execute(self, context):
        text = _write_support_report_text(context.space_data.image)
        shown = _show_text_in_editor(context, text)
        if shown:
            self.report({"INFO"}, f"Opened {text.name}")
        else:
            self.report({"INFO"}, f"Wrote {text.name}")
        return {"FINISHED"}


# --------------------------------------------------------------------------- #
# Background .clip watcher
#
# Threading model:
#   - `_watcher_tick` runs on the MAIN thread (it's a bpy.app.timers callback).
#     It only detects lightweight .clip freshness changes and spawns worker
#     threads.
#   - Worker threads run `_async_decode` through the native C ABI renderer.
#     On success they register `_on_main` with a timer to
#     apply Blender image updates on the main thread with sub-frame latency.
#   - bpy.* state mutation only happens on the main thread.
# --------------------------------------------------------------------------- #

def _async_decode(
    clip_path: str,
    image_name: str | None,
    create_on_success: bool = False,
    show_on_success: bool = False,
    auto_pack_on_success: bool = False,
    previous_manifest_json: str = "",
):
    """Worker-thread entry point for native render.

    Blender image mutation is scheduled back onto the main thread immediately,
    without waiting for the next watcher tick.
    """
    started_at = time.time()
    success = False
    native_result = None
    error_message = ""
    try:
        native_result = native_bridge.render_clip_rgba8(
            clip_path,
            previous_manifest_json=previous_manifest_json or None,
        )
        success = True
    except Exception as exc:
        error_message = str(exc)
        print(f"[clip_studio_importer] async decode failed for {clip_path}: {exc}")

    with _state_lock:
        _in_flight.discard(clip_path)

    if not success:
        def _on_error():
            img = bpy.data.images.get(image_name) if image_name else None
            if img is not None and _image_source_matches(img, clip_path):
                img[native_bridge.CLIP_RELOAD_LAST_SECONDS_KEY] = time.time() - started_at
                native_bridge.write_reload_error(img, error_message)
                if img.get(CLIP_PACK_STATUS_KEY) == PACK_STATUS_RENDERING:
                    _set_pack_status(
                        img,
                        PACK_STATUS_ERROR,
                        error="Render failed before pixels were updated",
                    )
            return None

        bpy.app.timers.register(_on_error, first_interval=0.0)
        return

    # Hop back to the main thread. bpy.app.timers.register is safe from worker
    # threads in modern Blender - internally it pushes into a thread-safe queue
    # processed at the next event loop tick (sub-frame latency, not poll
    # interval).
    def _on_main():
        img = bpy.data.images.get(image_name) if image_name else None
        if img is None and not create_on_success:
            return None
        if img is not None and not _image_source_matches(img, clip_path):
            return None
        if native_result is not None:
            img = native_bridge.create_or_update_image(
                bpy,
                native_result,
                image=img,
                image_name=image_name if create_on_success else None,
                pack=False,
                allow_resize=img is not None,
            )
            img[native_bridge.CLIP_RELOAD_LAST_SECONDS_KEY] = time.time() - started_at
            _mark_image_needs_pack(img)
            if show_on_success:
                _show_image_in_open_image_editors(bpy.context, img)
            if auto_pack_on_success:
                _schedule_pack_after_initial_import(img.name, clip_path)
        if img is not None:
            try:
                if _addon_prefs().debug:
                    print(f"[clip_studio_importer] reloaded {clip_path}")
            except Exception:
                pass
        return None  # one-shot

    bpy.app.timers.register(_on_main, first_interval=0.0)


def _watcher_tick():
    """Polled by bpy.app.timers on the main thread. Only detects lightweight
    mtime/size changes; actual reload is dispatched directly from the worker
    thread on completion.
    """
    try:
        prefs = _addon_prefs()
    except Exception:
        return 1.0

    interval = max(prefs.poll_interval, 0.1)
    if not prefs.auto_reload:
        return interval

    # Detect .clip freshness changes, spawn worker threads as needed.
    for img in bpy.data.images:
        clip_path = img.get(CLIP_SOURCE_KEY)
        if not clip_path:
            continue

        state = native_bridge.inspect_native_image_source(
            img,
            resolve_path=_blender_abspath(),
        )
        with _state_lock:
            running = bool(state.clip_path and state.clip_path in _in_flight)
        if running:
            if not _image_float_property(img, native_bridge.CLIP_RELOAD_STARTED_AT_KEY):
                img[native_bridge.CLIP_RELOAD_STARTED_AT_KEY] = time.time()
            native_bridge.write_reload_status(
                img,
                native_bridge.RELOAD_STATUS_REFRESHING,
            )
            continue
        native_bridge.write_reload_status(img, state.status)
        if state.status == native_bridge.RELOAD_STATUS_MISSING:
            continue
        if not state.should_reload or state.current_mtime is None:
            continue

        scheduled = _schedule_async_decode(
            state.clip_path,
            img.name,
        )
        if scheduled and prefs.debug:
            print(f"[clip_studio_importer] async-decoding {state.clip_path}")

    return interval


def _register_watcher():
    if not bpy.app.timers.is_registered(_watcher_tick):
        bpy.app.timers.register(_watcher_tick, persistent=True, first_interval=1.0)


def _unregister_watcher():
    if bpy.app.timers.is_registered(_watcher_tick):
        bpy.app.timers.unregister(_watcher_tick)
    # Drop any pending state - worker threads are daemon, will die with Blender.
    with _state_lock:
        _in_flight.clear()


@persistent
def _load_post_refresh_native_images(_dummy):
    """Refresh packed native images after a .blend is opened."""
    try:
        prefs = _addon_prefs()
        debug = bool(prefs.debug)
    except Exception:
        debug = False

    for img in bpy.data.images:
        if not img.get(CLIP_NATIVE_KEY):
            continue

        state = native_bridge.inspect_native_image_source(
            img,
            resolve_path=_blender_abspath(),
            check_hash=True,
        )
        with _state_lock:
            running = bool(state.clip_path and state.clip_path in _in_flight)
        if running:
            if not _image_float_property(img, native_bridge.CLIP_RELOAD_STARTED_AT_KEY):
                img[native_bridge.CLIP_RELOAD_STARTED_AT_KEY] = time.time()
            native_bridge.write_reload_status(
                img,
                native_bridge.RELOAD_STATUS_REFRESHING,
            )
            continue
        native_bridge.write_reload_status(img, state.status)
        if state.status == native_bridge.RELOAD_STATUS_MISSING:
            if debug:
                print(
                    "[clip_studio_importer] native source missing; "
                    f"keeping packed pixels for {img.name}"
                )
            continue
        if not state.should_reload:
            continue

        scheduled = _schedule_async_decode(
            state.clip_path,
            img.name,
        )
        if scheduled and debug:
            print(f"[clip_studio_importer] load-post native refresh {state.clip_path}")


def _register_load_post_handler():
    if _load_post_refresh_native_images not in bpy.app.handlers.load_post:
        bpy.app.handlers.load_post.append(_load_post_refresh_native_images)


def _unregister_load_post_handler():
    if _load_post_refresh_native_images in bpy.app.handlers.load_post:
        bpy.app.handlers.load_post.remove(_load_post_refresh_native_images)


@persistent
def _save_pre_pack_native_images(_dummy):
    """Pack dirty native images before saving the .blend."""
    for img in bpy.data.images:
        if not img.get(CLIP_NATIVE_KEY):
            continue
        if img.get(CLIP_PACK_STATUS_KEY) != PACK_STATUS_NEEDS_PACK:
            continue
        clip_path = img.get(CLIP_SOURCE_KEY, "")
        with _state_lock:
            running = bool(clip_path and clip_path in _in_flight)
        if running:
            continue
        try:
            _pack_image_now(img)
        except Exception:
            continue


def _register_save_pre_handler():
    if _save_pre_pack_native_images not in bpy.app.handlers.save_pre:
        bpy.app.handlers.save_pre.append(_save_pre_pack_native_images)


def _unregister_save_pre_handler():
    if _save_pre_pack_native_images in bpy.app.handlers.save_pre:
        bpy.app.handlers.save_pre.remove(_save_pre_pack_native_images)


# --------------------------------------------------------------------------- #
# Preferences + UI
# --------------------------------------------------------------------------- #

class CSI_AddonPreferences(AddonPreferences):
    bl_idname = ADDON_PKG

    auto_reload: BoolProperty(
        name="Autoreload .Clip",
        description="Watch every imported .clip's mtime and re-render "
                    "when it changes. Rendering runs on a background thread so "
                    "Blender's UI stays responsive.",
        default=True,
    )
    poll_interval: FloatProperty(
        name="Check Timer Frequency (s)",
        description="How often to check .clip mtimes and file sizes.",
        default=0.5,
        min=0.1,
        max=10.0,
    )
    debug: BoolProperty(
        name="Debug log",
        description="Print extra info to the system console.",
        default=False,
    )
    developer_mode: BoolProperty(
        name="Developer Mode",
        description="Show render timing and diagnostic actions in the image panel.",
        default=False,
    )

    def draw(self, context):
        layout = self.layout
        reload_box = layout.box()
        reload_box.prop(self, "auto_reload")
        row = reload_box.row()
        row.enabled = self.auto_reload
        row.prop(self, "poll_interval")

        developer_box = layout.box()
        developer_box.prop(self, "debug")
        developer_box.prop(self, "developer_mode")
        if not native_bridge.packaged_renderer_worker_path():
            developer_box.label(
                text="Packaged native renderer missing; rebuild the add-on package.",
                icon="ERROR",
            )


class IMAGE_PT_clip_studio(Panel):
    bl_space_type = "IMAGE_EDITOR"
    bl_region_type = "UI"
    bl_category = "Image"
    bl_label = "Clip Studio"

    @classmethod
    def poll(cls, context):
        space = getattr(context, "space_data", None)
        img = getattr(space, "image", None) if space else None
        return img is not None and CLIP_SOURCE_KEY in img.keys()

    def draw(self, context):
        img = context.space_data.image
        layout = self.layout
        clip_path = img.get(CLIP_SOURCE_KEY, "")
        status = img.get(native_bridge.CLIP_RELOAD_STATUS_KEY, "unknown")
        prefs = _addon_prefs()
        developer_mode = bool(getattr(prefs, "developer_mode", False))
        layout.label(text=f"Source: {os.path.basename(clip_path)}")
        if status != native_bridge.RELOAD_STATUS_OK:
            layout.label(
                text=_reload_status_label(status),
                icon=_reload_status_icon(status),
            )
        pack_status = str(img.get(CLIP_PACK_STATUS_KEY, "") or "")
        if pack_status:
            pack_row = layout.row(align=True)
            pack_row.label(
                text=f"Pack: {_pack_status_label(pack_status)}",
                icon=_pack_status_icon(pack_status),
            )
            pack_row.operator(
                IMAGE_OT_pack_clip_studio.bl_idname,
                text="Pack",
            )
            pack_error = img.get(CLIP_PACK_ERROR_KEY, "")
            if pack_error:
                layout.label(
                    text=f"Pack error: {_short_diagnostic(pack_error)}",
                    icon="ERROR",
                )
        else:
            layout.operator(
                IMAGE_OT_pack_clip_studio.bl_idname,
                text="Pack",
            )
        unsupported_count = _image_int_property(
            img,
            native_bridge.CLIP_SUPPORT_UNSUPPORTED_COUNT_KEY,
        )
        support_details = img.get(native_bridge.CLIP_SUPPORT_DETAILS_KEY, "")
        detail_lines = [line for line in str(support_details).splitlines() if line]
        if unsupported_count or detail_lines:
            layout.label(
                text=f"Unsupported native nodes: {unsupported_count or len(detail_lines)}",
                icon="ERROR",
            )
            location_summary = _support_location_summary(img)
            if location_summary:
                layout.label(
                    text=_short_diagnostic(location_summary),
                    icon="VIEWZOOM",
                )
            expanded = bool(img.get(CLIP_SUPPORT_DETAILS_EXPANDED_KEY, False))
            visible_details = (
                detail_lines
                if expanded
                else detail_lines[:SUPPORT_DETAIL_PREVIEW_LINES]
            )
            for detail in visible_details:
                layout.label(
                    text=_short_diagnostic(detail),
                    icon="DOT",
                )
            if len(detail_lines) > SUPPORT_DETAIL_PREVIEW_LINES:
                label = (
                    "Show fewer unsupported details"
                    if expanded
                    else "Show all unsupported details"
                )
                layout.operator(
                    IMAGE_OT_toggle_clip_support_details.bl_idname,
                    text=label,
                    icon="TRIA_DOWN" if expanded else "TRIA_RIGHT",
                )
            if not expanded and len(detail_lines) > SUPPORT_DETAIL_PREVIEW_LINES:
                layout.label(
                    text=(
                        f"{len(detail_lines) - SUPPORT_DETAIL_PREVIEW_LINES} "
                        "more unsupported item(s)"
                    ),
                    icon="INFO",
                )
            if _support_location_lines(img):
                layout.operator(
                    IMAGE_OT_copy_clip_support_locations.bl_idname,
                    text="Copy layer locations",
                    icon="COPYDOWN",
                )
        if status == native_bridge.RELOAD_STATUS_MISSING:
            layout.label(text="Packed pixels are still visible.", icon="INFO")
        elif status == native_bridge.RELOAD_STATUS_ERROR:
            message = img.get(native_bridge.CLIP_RELOAD_ERROR_KEY, "")
            if message:
                layout.label(
                    text=f"Error: {_short_diagnostic(message)}",
                    icon="ERROR",
                )
        layout.operator(
            IMAGE_OT_reload_clip_studio.bl_idname,
            text="Manual Reload",
            icon="FILE_REFRESH",
        )
        if status == native_bridge.RELOAD_STATUS_REFRESHING:
            started_at = _image_float_property(
                img,
                native_bridge.CLIP_RELOAD_STARTED_AT_KEY,
            )
            if started_at:
                layout.label(
                    text=f"Elapsed: {_format_seconds(time.time() - started_at)}",
                    icon="SORTTIME",
                )
        if developer_mode:
            last_seconds = _image_float_property(
                img,
                native_bridge.CLIP_RELOAD_LAST_SECONDS_KEY,
            )
            if last_seconds:
                layout.label(
                    text=f"Last render: {_format_seconds(last_seconds)}",
                    icon="TIME",
                )
            for line in _timing_phase_lines(img):
                layout.label(
                    text=_short_diagnostic(line),
                    icon="BLANK1",
                )
            layout.operator(
                IMAGE_OT_open_clip_support_diagnostics.bl_idname,
                text="Open Diagnostics",
                icon="TEXT",
            )
        layout.operator(
            IMAGE_OT_copy_clip_support_diagnostics.bl_idname,
            text="Copy Diagnostic",
            icon="COPYDOWN",
        )
        # If a render is currently running for this image's clip, show a hint.
        with _state_lock:
            running = clip_path in _in_flight
        if running:
            layout.label(text="Rendering in background", icon="SORTTIME")


def _menu_func_import(self, context):
    self.layout.operator(IMPORT_OT_clip_studio.bl_idname, text="Clip Studio (.clip)")


# --------------------------------------------------------------------------- #
# Registration
# --------------------------------------------------------------------------- #

_classes = (
    CSI_AddonPreferences,
    IMPORT_OT_clip_studio,
    IMAGE_OT_reload_clip_studio,
    IMAGE_OT_pack_clip_studio,
    IMAGE_OT_toggle_clip_support_details,
    IMAGE_OT_copy_clip_support_diagnostics,
    IMAGE_OT_copy_clip_support_locations,
    IMAGE_OT_open_clip_support_diagnostics,
    IMAGE_PT_clip_studio,
)


def register():
    for cls in _classes:
        bpy.utils.register_class(cls)
    bpy.types.TOPBAR_MT_file_import.append(_menu_func_import)
    _register_load_post_handler()
    _register_save_pre_handler()
    _register_watcher()


def unregister():
    _unregister_save_pre_handler()
    _unregister_load_post_handler()
    _unregister_watcher()
    native_bridge.shutdown_renderer_worker()
    bpy.types.TOPBAR_MT_file_import.remove(_menu_func_import)
    for cls in reversed(_classes):
        bpy.utils.unregister_class(cls)


if __name__ == "__main__":
    register()
