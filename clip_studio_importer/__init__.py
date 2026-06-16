"""
Clip Studio Paint (.clip) Importer for Blender.

The default importer path decodes a .clip file with the Python compositor,
writes a sidecar PNG cache, and loads that PNG as a file-backed Blender Image.
The optional native renderer path calls the Rust C ABI, uploads RGBA pixels
into a generated Blender Image, packs the latest render into the .blend, and
stores .clip source-tracking properties for reload/watch updates.

No external auto-reload add-on is required. The sidecar PNG remains compatible
with external image-reload tools when the Python path is selected.
"""

from __future__ import annotations

bl_info = {
    "name": "Clip Studio Paint (.clip) Importer",
    "author": "Rizum",
    "version": (0, 8, 25),
    "blender": (3, 0, 0),
    "location": "File > Import > Clip Studio (.clip)",
    "description": "Read .clip files as flattened image textures with non-blocking auto-reload.",
    "category": "Import-Export",
}

import os
import threading

import bpy
from bpy.app.handlers import persistent
from bpy.props import BoolProperty, FloatProperty, StringProperty
from bpy.types import AddonPreferences, Operator, Panel
from bpy_extras.io_utils import ImportHelper

from . import native_bridge
from .clip_loader import ClipFile, save_png


CLIP_SOURCE_KEY = "clip_source"   # custom prop on Image: path to source .clip
CLIP_MTIME_KEY = "clip_mtime"     # custom prop on Image: last-seen mtime (str)
CLIP_NATIVE_KEY = native_bridge.CLIP_NATIVE_KEY
ADDON_PKG = __package__


# Module-level state for background-thread coordination.
# All access must go through `_state_lock`.
_state_lock = threading.Lock()
_in_flight: set = set()                 # clip paths currently being decoded


# --------------------------------------------------------------------------- #
# Helpers
# --------------------------------------------------------------------------- #

def _sidecar_png_path(clip_path: str) -> str:
    """`foo.clip` → `foo.clip.png`. Double extension keeps it unambiguous."""
    return clip_path + ".png"


def _decode_clip_to_sidecar_png(clip_path: str) -> str:
    """Decode the .clip and write its PNG sidecar. Returns the PNG path.

    Pure Python+numpy+stdlib — safe to call from a worker thread.
    """
    clip = ClipFile(clip_path)
    try:
        rgba = clip.composite()
    finally:
        clip.close()
    png_path = _sidecar_png_path(clip_path)
    save_png(png_path, rgba)
    return png_path


def _native_library_path() -> str | None:
    path = getattr(_addon_prefs(), "native_library_path", "")
    return path.strip() or None


def _import_clip_as_sidecar_image(clip_path: str) -> bpy.types.Image:
    """Decode + sidecar + load. Returns a file-backed Blender Image.
    Synchronous (called from the import operator on the main thread).
    """
    png_path = _decode_clip_to_sidecar_png(clip_path)
    img = bpy.data.images.load(png_path, check_existing=True)
    img.reload()
    img.colorspace_settings.name = "sRGB"
    img[CLIP_SOURCE_KEY] = clip_path
    img[CLIP_MTIME_KEY] = str(os.path.getmtime(clip_path))
    return img


def _import_clip_as_native_image(clip_path: str) -> bpy.types.Image:
    return native_bridge.import_clip_as_image(
        clip_path,
        bpy_module=bpy,
        library_path=_native_library_path(),
        pack=True,
    )


def _import_clip_as_image(clip_path: str) -> bpy.types.Image:
    if _addon_prefs().use_native_renderer:
        return _import_clip_as_native_image(clip_path)
    return _import_clip_as_sidecar_image(clip_path)


def _addon_prefs():
    return bpy.context.preferences.addons[ADDON_PKG].preferences


def _schedule_async_decode(
    clip_path: str,
    image_name: str,
    *,
    use_native: bool,
    native_library_path: str | None,
    pre_stamp_mtime: float | None = None,
) -> bool:
    with _state_lock:
        if clip_path in _in_flight:
            return False
        _in_flight.add(clip_path)

    if pre_stamp_mtime is not None:
        img = bpy.data.images.get(image_name)
        if img is not None and img.get(CLIP_SOURCE_KEY) == clip_path:
            img[CLIP_MTIME_KEY] = str(pre_stamp_mtime)

    if use_native:
        img = bpy.data.images.get(image_name)
        if img is not None and img.get(CLIP_SOURCE_KEY) == clip_path:
            native_bridge.write_reload_status(
                img,
                native_bridge.RELOAD_STATUS_REFRESHING,
            )

    threading.Thread(
        target=_async_decode,
        args=(clip_path, image_name),
        kwargs={
            "use_native": use_native,
            "native_library_path": native_library_path,
        },
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

        for area in context.screen.areas:
            if area.type == "IMAGE_EDITOR":
                for space in area.spaces:
                    if space.type == "IMAGE_EDITOR":
                        space.image = img
                        break
                break

        self.report({"INFO"},
                    f"Imported {img.name} ({img.size[0]}x{img.size[1]})")
        return {"FINISHED"}


class IMAGE_OT_reload_clip_studio(Operator):
    """Re-decode the .clip file this image was imported from. Synchronous —
    user explicitly requested a refresh, so we wait."""
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
            self.report({"ERROR"}, f"Source .clip not found: {clip_path!r}")
            return {"CANCELLED"}
        try:
            if img.get(CLIP_NATIVE_KEY):
                result = native_bridge.render_clip_rgba8(
                    clip_path,
                    library_path=_native_library_path(),
                )
                native_bridge.create_or_update_image(
                    bpy,
                    result,
                    image=img,
                    pack=True,
                )
            else:
                _decode_clip_to_sidecar_png(clip_path)
                img[CLIP_MTIME_KEY] = str(os.path.getmtime(clip_path))
                img.reload()
        except Exception as exc:
            self.report({"ERROR"}, f"Reload failed: {exc}")
            return {"CANCELLED"}
        self.report({"INFO"}, f"Reloaded {os.path.basename(clip_path)}")
        return {"FINISHED"}


# --------------------------------------------------------------------------- #
# Background .clip watcher
#
# Threading model:
#   - `_watcher_tick` runs on the MAIN thread (it's a bpy.app.timers callback).
#     It only detects .clip mtime changes and spawns worker threads.
#   - Worker threads run `_async_decode`: either Python sidecar decode or
#     native C ABI render. On success they register `_on_main` with a timer to
#     apply Blender image updates on the main thread with sub-frame latency.
#   - bpy.* state mutation only happens on the main thread.
# --------------------------------------------------------------------------- #

def _async_decode(
    clip_path: str,
    image_name: str,
    *,
    use_native: bool,
    native_library_path: str | None,
):
    """Worker-thread entry point for sidecar decode or native render.

    Blender image mutation is scheduled back onto the main thread immediately,
    without waiting for the next watcher tick.
    """
    success = False
    native_result = None
    error_message = ""
    try:
        if use_native:
            native_result = native_bridge.render_clip_rgba8(
                clip_path,
                library_path=native_library_path,
            )
        else:
            _decode_clip_to_sidecar_png(clip_path)
        success = True
    except Exception as exc:
        error_message = str(exc)
        print(f"[clip_studio_importer] async decode failed for {clip_path}: {exc}")

    with _state_lock:
        _in_flight.discard(clip_path)

    if not success:
        if use_native:
            def _on_error():
                img = bpy.data.images.get(image_name)
                if img is not None and img.get(CLIP_SOURCE_KEY) == clip_path:
                    native_bridge.write_reload_status(
                        img,
                        native_bridge.RELOAD_STATUS_ERROR,
                    )
                    if error_message:
                        img[native_bridge.CLIP_RELOAD_ERROR_KEY] = error_message
                return None

            bpy.app.timers.register(_on_error, first_interval=0.0)
        return

    # Hop back to the main thread. bpy.app.timers.register is safe from worker
    # threads in modern Blender — internally it pushes into a thread-safe queue
    # processed at the next event loop tick (sub-frame latency, not poll
    # interval).
    def _on_main():
        img = bpy.data.images.get(image_name)
        if img is not None and img.get(CLIP_SOURCE_KEY) == clip_path:
            if use_native and native_result is not None:
                native_bridge.create_or_update_image(
                    bpy,
                    native_result,
                    image=img,
                    pack=True,
                )
            else:
                try:
                    img[CLIP_MTIME_KEY] = str(os.path.getmtime(clip_path))
                except OSError:
                    pass
                img.reload()
            try:
                if _addon_prefs().debug:
                    print(f"[clip_studio_importer] reloaded {clip_path}")
            except Exception:
                pass
        return None  # one-shot

    bpy.app.timers.register(_on_main, first_interval=0.0)


def _watcher_tick():
    """Polled by bpy.app.timers on the main thread. Only detects mtime changes;
    actual reload is dispatched directly from the worker thread on completion.
    """
    try:
        prefs = _addon_prefs()
    except Exception:
        return 1.0

    interval = max(prefs.poll_interval, 0.25)
    if not prefs.auto_reload:
        return interval

    # Detect .clip mtime changes, spawn worker threads as needed.
    for img in bpy.data.images:
        clip_path = img.get(CLIP_SOURCE_KEY)
        if not clip_path:
            continue

        if img.get(CLIP_NATIVE_KEY):
            state = native_bridge.inspect_native_image_source(img)
            with _state_lock:
                running = bool(state.clip_path and state.clip_path in _in_flight)
            if running:
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
                use_native=True,
                native_library_path=_native_library_path(),
            )
            if scheduled and prefs.debug:
                print(f"[clip_studio_importer] async-decoding {state.clip_path}")
            continue

        if not os.path.exists(clip_path):
            continue
        try:
            mtime = os.path.getmtime(clip_path)
        except OSError:
            continue

        prev_str = img.get(CLIP_MTIME_KEY, "")
        try:
            prev = float(prev_str) if prev_str else None
        except ValueError:
            prev = None

        if prev is None:
            # First sighting — record mtime, do not reload (matches Auto Reload).
            img[CLIP_MTIME_KEY] = str(mtime)
            continue

        if mtime > prev + 1e-6:
            scheduled = _schedule_async_decode(
                clip_path,
                img.name,
                use_native=False,
                native_library_path=_native_library_path(),
                pre_stamp_mtime=mtime,
            )
            if scheduled and prefs.debug:
                print(f"[clip_studio_importer] async-decoding {clip_path}")

    return interval


def _register_watcher():
    if not bpy.app.timers.is_registered(_watcher_tick):
        bpy.app.timers.register(_watcher_tick, persistent=True, first_interval=1.0)


def _unregister_watcher():
    if bpy.app.timers.is_registered(_watcher_tick):
        bpy.app.timers.unregister(_watcher_tick)
    # Drop any pending state — worker threads are daemon, will die with Blender.
    with _state_lock:
        _in_flight.clear()


@persistent
def _load_post_refresh_native_images(_dummy):
    """Refresh packed native images after a .blend is opened."""
    try:
        prefs = _addon_prefs()
        debug = bool(prefs.debug)
        native_library_path = _native_library_path()
    except Exception:
        debug = False
        native_library_path = None

    for img in bpy.data.images:
        if not img.get(CLIP_NATIVE_KEY):
            continue

        state = native_bridge.inspect_native_image_source(img)
        with _state_lock:
            running = bool(state.clip_path and state.clip_path in _in_flight)
        if running:
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
            use_native=True,
            native_library_path=native_library_path,
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
        description="Watch every imported .clip's mtime and re-decode + reload "
                    "when it changes. Decoding runs on a background thread so "
                    "Blender's UI stays responsive.",
        default=True,
    )
    poll_interval: FloatProperty(
        name="Poll interval (seconds)",
        description="How often to check .clip mtimes.",
        default=0.5,
        min=0.25,
        max=10.0,
    )
    debug: BoolProperty(
        name="Debug log",
        description="Print extra info to the system console.",
        default=False,
    )
    use_native_renderer: BoolProperty(
        name="Use native renderer",
        description="Import .clip files through the Rust C ABI without writing sidecar PNGs.",
        default=False,
    )
    native_library_path: StringProperty(
        name="Native renderer library",
        description="Optional override path to clip_capi.dll, libclip_capi.so, or libclip_capi.dylib.",
        default="",
        subtype="FILE_PATH",
    )

    def draw(self, context):
        layout = self.layout
        layout.prop(self, "auto_reload")
        row = layout.row()
        row.enabled = self.auto_reload
        row.prop(self, "poll_interval")
        layout.prop(self, "debug")
        layout.prop(self, "use_native_renderer")
        native_row = layout.row()
        native_row.enabled = self.use_native_renderer
        native_row.prop(self, "native_library_path")
        layout.label(
            text="Save a .clip in CSP — Blender's UI stays responsive while it decodes in the background.",
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
        is_native = bool(img.get(CLIP_NATIVE_KEY))
        layout.label(text=f"Source: {os.path.basename(clip_path)}")
        layout.label(text="Mode: Native renderer" if is_native else "Mode: Sidecar PNG")
        if is_native:
            layout.label(text=f"Status: {img.get(native_bridge.CLIP_RELOAD_STATUS_KEY, 'unknown')}")
        else:
            layout.label(text=f"PNG: {os.path.basename(img.filepath)}")
        layout.operator(IMAGE_OT_reload_clip_studio.bl_idname, icon="FILE_REFRESH")
        prefs = _addon_prefs()
        layout.prop(prefs, "auto_reload", text="Auto-reload on .clip change")
        # If a decode is currently running for this image's clip, show a hint.
        with _state_lock:
            running = clip_path in _in_flight
        if running:
            layout.label(text="Decoding in background…", icon="SORTTIME")


def _menu_func_import(self, context):
    self.layout.operator(IMPORT_OT_clip_studio.bl_idname, text="Clip Studio (.clip)")


# --------------------------------------------------------------------------- #
# Registration
# --------------------------------------------------------------------------- #

_classes = (
    CSI_AddonPreferences,
    IMPORT_OT_clip_studio,
    IMAGE_OT_reload_clip_studio,
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
