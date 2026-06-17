"""
Clip Studio Paint (.clip) Importer for Blender.

The default importer path calls the Rust C ABI, uploads RGBA pixels into a
generated Blender Image, packs the latest render into the .blend, and stores
.clip source-tracking properties for reload/watch updates.

No external auto-reload add-on is required.
"""

from __future__ import annotations

bl_info = {
    "name": "Clip Studio Paint (.clip) Importer",
    "author": "Rizum",
    "version": (0, 8, 48),
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
        pack=True,
    )
    image[native_bridge.CLIP_RELOAD_LAST_SECONDS_KEY] = time.time() - started_at
    return image


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


def _support_status_label(status: str) -> str:
    return {
        native_bridge.SUPPORT_STATUS_FULL: "Full native support",
        native_bridge.SUPPORT_STATUS_UNSUPPORTED: "Unsupported nodes",
        native_bridge.SUPPORT_STATUS_UNKNOWN: "Support unknown",
    }.get(status, "Support unknown")


def _support_status_icon(status: str) -> str:
    return {
        native_bridge.SUPPORT_STATUS_FULL: "CHECKMARK",
        native_bridge.SUPPORT_STATUS_UNSUPPORTED: "ERROR",
        native_bridge.SUPPORT_STATUS_UNKNOWN: "INFO",
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


def _support_resource_lines(img) -> list[str]:
    source_count = _image_int_property(img, native_bridge.CLIP_SUPPORT_SOURCE_COUNT_KEY)
    unsupported_count = _image_int_property(
        img,
        native_bridge.CLIP_SUPPORT_UNSUPPORTED_COUNT_KEY,
    )
    raster_count = _image_int_property(img, native_bridge.CLIP_SUPPORT_RASTER_COUNT_KEY)
    mask_count = _image_int_property(img, native_bridge.CLIP_SUPPORT_MASK_COUNT_KEY)
    lines = [
        f"Sources: {source_count}; unsupported: {unsupported_count}",
        f"Raster resources: {raster_count}",
        f"Mask resources: {mask_count}",
    ]
    max_raster_layer = _image_int_property(
        img,
        native_bridge.CLIP_SUPPORT_MAX_RASTER_LAYER_KEY,
    )
    if max_raster_layer:
        width = _image_int_property(img, native_bridge.CLIP_SUPPORT_MAX_RASTER_WIDTH_KEY)
        height = _image_int_property(img, native_bridge.CLIP_SUPPORT_MAX_RASTER_HEIGHT_KEY)
        lines.append(
            "Largest raster: "
            f"layer {max_raster_layer}, {width}x{height}"
        )
    max_mask_layer = _image_int_property(img, native_bridge.CLIP_SUPPORT_MAX_MASK_LAYER_KEY)
    if max_mask_layer:
        width = _image_int_property(img, native_bridge.CLIP_SUPPORT_MAX_MASK_WIDTH_KEY)
        height = _image_int_property(img, native_bridge.CLIP_SUPPORT_MAX_MASK_HEIGHT_KEY)
        lines.append(f"Largest mask: layer {max_mask_layer}, {width}x{height}")
    return lines


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
    support_status = img.get(native_bridge.CLIP_SUPPORT_STATUS_KEY, "")
    width = _image_int_property(img, native_bridge.CLIP_CANVAS_WIDTH_KEY)
    height = _image_int_property(img, native_bridge.CLIP_CANVAS_HEIGHT_KEY)
    lines = [
        "Clip Studio native render diagnostics",
        f"Source: {clip_path}",
        f"Status: {_reload_status_label(status)}",
        "Mode: Native renderer",
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
    phase_lines = _timing_phase_lines(img)
    if phase_lines:
        lines.append("Timing phases:")
        lines.extend(f"- {line}" for line in phase_lines)
    if width and height:
        lines.append(f"Canvas: {width}x{height}")
    renderer_abi = _image_int_property(img, native_bridge.CLIP_RENDERER_ABI_KEY)
    if renderer_abi:
        lines.append(f"Renderer ABI: {renderer_abi}")
    renderer_version = img.get(native_bridge.CLIP_RENDERER_VERSION_KEY, "")
    if renderer_version:
        lines.append(f"Renderer version: {renderer_version}")
    root_layer = _image_int_property(img, native_bridge.CLIP_ROOT_LAYER_KEY)
    layer_count = _image_int_property(img, native_bridge.CLIP_LAYER_COUNT_KEY)
    external_count = _image_int_property(img, native_bridge.CLIP_EXTERNAL_COUNT_KEY)
    if root_layer or layer_count or external_count:
        lines.append(
            f"Root layer: {root_layer}; layers: {layer_count}; "
            f"external chunks: {external_count}"
        )
    if support_status:
        lines.append(f"Native support: {_support_status_label(support_status)}")
    report = img.get(native_bridge.CLIP_SUPPORT_REPORT_KEY, "")
    if report:
        lines.append(f"Support report: {report}")
    lines.extend(_support_resource_lines(img))
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
    return f"Clip Studio Support - {image_name}"


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
    image_name: str,
) -> bool:
    with _state_lock:
        if clip_path in _in_flight:
            return False
        _in_flight.add(clip_path)

    img = bpy.data.images.get(image_name)
    if img is not None and img.get(CLIP_SOURCE_KEY) == clip_path:
        img[native_bridge.CLIP_RELOAD_STARTED_AT_KEY] = time.time()
        native_bridge.write_reload_status(
            img,
            native_bridge.RELOAD_STATUS_REFRESHING,
        )

    threading.Thread(
        target=_async_decode,
        args=(clip_path, image_name),
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
        clip_path = self.filepath
        try:
            img = _import_clip_as_image(clip_path)
        except Exception as exc:
            self.report({"ERROR"}, f"Failed to read .clip: {exc}")
            return {"CANCELLED"}

        _show_image_in_open_image_editors(context, img)
        self.report({"INFO"},
                    f"Imported {img.name} ({img.size[0]}x{img.size[1]})")
        return {"FINISHED"}


class IMAGE_OT_reload_clip_studio(Operator):
    """Re-render the .clip file this image was imported from."""
    bl_idname = "image.reload_clip_studio"
    bl_label = "Reload from .clip"
    bl_options = {"REGISTER", "UNDO"}

    @classmethod
    def poll(cls, context):
        space = getattr(context, "space_data", None)
        img = getattr(space, "image", None) if space else None
        return img is not None and CLIP_SOURCE_KEY in img.keys()

    def execute(self, context):
        img = context.space_data.image
        clip_path = img.get(CLIP_SOURCE_KEY)
        if not clip_path or not os.path.exists(clip_path):
            native_bridge.write_reload_status(
                img,
                native_bridge.RELOAD_STATUS_MISSING,
            )
            self.report({"ERROR"}, f"Source .clip not found: {clip_path!r}")
            return {"CANCELLED"}
        started_at = time.time()
        img[native_bridge.CLIP_RELOAD_STARTED_AT_KEY] = started_at
        native_bridge.write_reload_status(
            img,
            native_bridge.RELOAD_STATUS_REFRESHING,
        )
        try:
            result = native_bridge.render_clip_rgba8(
                clip_path,
            )
            native_bridge.create_or_update_image(
                bpy,
                result,
                image=img,
                pack=True,
            )
            img[native_bridge.CLIP_RELOAD_LAST_SECONDS_KEY] = time.time() - started_at
        except Exception as exc:
            img[native_bridge.CLIP_RELOAD_LAST_SECONDS_KEY] = time.time() - started_at
            native_bridge.write_reload_error(img, str(exc))
            self.report({"ERROR"}, f"Reload failed: {exc}")
            return {"CANCELLED"}
        self.report({"INFO"}, f"Reloaded {os.path.basename(clip_path)}")
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
    bl_label = "Open Support Report"
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
    image_name: str,
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
        )
        success = True
    except Exception as exc:
        error_message = str(exc)
        print(f"[clip_studio_importer] async decode failed for {clip_path}: {exc}")

    with _state_lock:
        _in_flight.discard(clip_path)

    if not success:
        def _on_error():
            img = bpy.data.images.get(image_name)
            if img is not None and img.get(CLIP_SOURCE_KEY) == clip_path:
                img[native_bridge.CLIP_RELOAD_LAST_SECONDS_KEY] = time.time() - started_at
                native_bridge.write_reload_error(img, error_message)
            return None

        bpy.app.timers.register(_on_error, first_interval=0.0)
        return

    # Hop back to the main thread. bpy.app.timers.register is safe from worker
    # threads in modern Blender - internally it pushes into a thread-safe queue
    # processed at the next event loop tick (sub-frame latency, not poll
    # interval).
    def _on_main():
        img = bpy.data.images.get(image_name)
        if img is not None and img.get(CLIP_SOURCE_KEY) == clip_path:
            if native_result is not None:
                native_bridge.create_or_update_image(
                    bpy,
                    native_result,
                    image=img,
                    pack=True,
                )
                img[native_bridge.CLIP_RELOAD_LAST_SECONDS_KEY] = time.time() - started_at
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

    interval = max(prefs.poll_interval, 0.25)
    if not prefs.auto_reload:
        return interval

    # Detect .clip freshness changes, spawn worker threads as needed.
    for img in bpy.data.images:
        clip_path = img.get(CLIP_SOURCE_KEY)
        if not clip_path:
            continue

        state = native_bridge.inspect_native_image_source(img)
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

        state = native_bridge.inspect_native_image_source(img, check_hash=True)
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


# --------------------------------------------------------------------------- #
# Preferences + UI
# --------------------------------------------------------------------------- #

class CSI_AddonPreferences(AddonPreferences):
    bl_idname = ADDON_PKG

    auto_reload: BoolProperty(
        name="Auto-reload on .clip change",
        description="Watch every imported .clip's mtime and re-render "
                    "when it changes. Rendering runs on a background thread so "
                    "Blender's UI stays responsive.",
        default=True,
    )
    poll_interval: FloatProperty(
        name="Poll interval (seconds)",
        description="How often to check .clip mtimes and file sizes.",
        default=0.5,
        min=0.25,
        max=10.0,
    )
    debug: BoolProperty(
        name="Debug log",
        description="Print extra info to the system console.",
        default=False,
    )

    def draw(self, context):
        layout = self.layout
        layout.prop(self, "auto_reload")
        row = layout.row()
        row.enabled = self.auto_reload
        row.prop(self, "poll_interval")
        layout.prop(self, "debug")
        packaged_worker_path = native_bridge.packaged_renderer_worker_path()
        if packaged_worker_path:
            layout.label(
                text="Packaged native renderer found.",
                icon="CHECKMARK",
            )
        else:
            layout.label(
                text="Packaged native renderer missing; rebuild the add-on package.",
                icon="ERROR",
            )
        layout.label(
            text="Save a .clip in CSP - Blender's UI stays responsive while it renders in the background.",
            icon="INFO",
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
        layout.label(text=f"Source: {os.path.basename(clip_path)}")
        layout.label(text="Mode: Native renderer")
        layout.label(
            text=f"Status: {_reload_status_label(status)}",
            icon=_reload_status_icon(status),
        )
        support_status = img.get(native_bridge.CLIP_SUPPORT_STATUS_KEY, "")
        if support_status:
            layout.label(
                text=f"Native support: {_support_status_label(support_status)}",
                icon=_support_status_icon(support_status),
            )
            renderer_version = img.get(native_bridge.CLIP_RENDERER_VERSION_KEY, "")
            if renderer_version:
                layout.label(
                    text=f"Renderer version: {_short_diagnostic(renderer_version)}",
                    icon="INFO",
                )
            support_report = img.get(native_bridge.CLIP_SUPPORT_REPORT_KEY, "")
            if support_report:
                layout.label(
                    text=_short_diagnostic(support_report),
                    icon="INFO",
                )
            for line in _support_resource_lines(img):
                layout.label(
                    text=_short_diagnostic(line),
                    icon="BLANK1",
                )
            location_summary = _support_location_summary(img)
            if location_summary:
                layout.label(
                    text=_short_diagnostic(location_summary),
                    icon="VIEWZOOM",
                )
            support_details = img.get(native_bridge.CLIP_SUPPORT_DETAILS_KEY, "")
            detail_lines = [line for line in str(support_details).splitlines() if line]
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
            layout.operator(
                IMAGE_OT_copy_clip_support_diagnostics.bl_idname,
                text="Copy support diagnostics",
                icon="COPYDOWN",
            )
            if _support_location_lines(img):
                layout.operator(
                    IMAGE_OT_copy_clip_support_locations.bl_idname,
                    text="Copy layer locations",
                    icon="COPYDOWN",
                )
            layout.operator(
                IMAGE_OT_open_clip_support_diagnostics.bl_idname,
                text="Open support report",
                icon="TEXT",
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
        layout.operator(IMAGE_OT_reload_clip_studio.bl_idname, icon="FILE_REFRESH")
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
        prefs = _addon_prefs()
        layout.prop(prefs, "auto_reload", text="Auto-reload on .clip change")
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
    _register_watcher()


def unregister():
    _unregister_load_post_handler()
    _unregister_watcher()
    bpy.types.TOPBAR_MT_file_import.remove(_menu_func_import)
    for cls in reversed(_classes):
        bpy.utils.unregister_class(cls)


if __name__ == "__main__":
    register()
