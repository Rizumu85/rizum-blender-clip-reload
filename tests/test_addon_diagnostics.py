from __future__ import annotations

import importlib.util
import sys
import types
import unittest
from pathlib import Path


def _load_addon_module():
    package_dir = Path(__file__).resolve().parents[1] / "clip_studio_importer"

    bpy = types.ModuleType("bpy")
    bpy_app = types.ModuleType("bpy.app")
    bpy_app_handlers = types.ModuleType("bpy.app.handlers")
    bpy_props = types.ModuleType("bpy.props")
    bpy_types = types.ModuleType("bpy.types")
    bpy_extras = types.ModuleType("bpy_extras")
    bpy_extras_io = types.ModuleType("bpy_extras.io_utils")

    def persistent(func):
        return func

    def prop(**kwargs):
        return kwargs.get("default")

    for name in ("BoolProperty", "FloatProperty", "StringProperty"):
        setattr(bpy_props, name, prop)

    class AddonPreferences:
        pass

    class Operator:
        def report(self, levels, message):
            self.reported = (levels, message)

    class Panel:
        pass

    class ImportHelper:
        pass

    class FakeText:
        def __init__(self, name: str) -> None:
            self.name = name
            self.body = ""

        def clear(self) -> None:
            self.body = ""

        def write(self, text: str) -> None:
            self.body += text

    class FakeTexts(dict):
        def new(self, name: str):
            text = FakeText(name)
            self[name] = text
            return text

    class FakePixels:
        def __init__(self) -> None:
            self.values = None

        def foreach_set(self, values) -> None:
            self.values = list(values)

    class FakeColorSettings:
        def __init__(self) -> None:
            self.name = ""

    class FakeImage(dict):
        def __init__(self, name: str, width: int, height: int) -> None:
            super().__init__()
            self.name = name
            self.size = (width, height)
            self.source = ""
            self.colorspace_settings = FakeColorSettings()
            self.pixels = FakePixels()
            self.updated = False
            self.packed = False

        def update(self) -> None:
            self.updated = True

        def pack(self) -> None:
            self.packed = True

        def scale(self, width: int, height: int) -> None:
            self.size = (width, height)

    class FakeImages(list):
        def new(self, name: str, *, width: int, height: int, alpha: bool, float_buffer: bool):
            image = FakeImage(name, width, height)
            image.alpha = alpha
            image.float_buffer = float_buffer
            self.append(image)
            return image

        def get(self, name: str):
            for image in self:
                if image.name == name:
                    return image
            return None

    class FakeTimers:
        def __init__(self) -> None:
            self.callbacks = []

        def register(self, callback, **_kwargs):
            self.callbacks.append(callback)
            return None

        def is_registered(self, callback) -> bool:
            return callback in self.callbacks

        def unregister(self, callback) -> None:
            self.callbacks.remove(callback)

    bpy_app_handlers.persistent = persistent
    bpy_app_handlers.load_post = []
    bpy_app_handlers.save_pre = []
    bpy_app.handlers = bpy_app_handlers
    bpy_app.timers = FakeTimers()
    bpy.app = bpy_app
    bpy.props = bpy_props
    bpy_types.AddonPreferences = AddonPreferences
    bpy_types.Operator = Operator
    bpy_types.Panel = Panel
    bpy_types.TOPBAR_MT_file_import = types.SimpleNamespace()
    bpy.types = bpy_types
    bpy.utils = types.SimpleNamespace()
    bpy.data = types.SimpleNamespace(images=FakeImages(), texts=FakeTexts())
    bpy.context = types.SimpleNamespace(
        preferences=types.SimpleNamespace(
            addons={
                "clip_studio_importer": types.SimpleNamespace(
                    preferences=types.SimpleNamespace(
                        auto_reload=True,
                        debug=False,
                        developer_mode=False,
                    )
                )
            }
        )
    )
    bpy_extras_io.ImportHelper = ImportHelper

    sys.modules.update(
        {
            "bpy": bpy,
            "bpy.app": bpy_app,
            "bpy.app.handlers": bpy_app_handlers,
            "bpy.props": bpy_props,
            "bpy.types": bpy_types,
            "bpy_extras": bpy_extras,
            "bpy_extras.io_utils": bpy_extras_io,
        }
    )

    spec = importlib.util.spec_from_file_location(
        "clip_studio_importer",
        package_dir / "__init__.py",
        submodule_search_locations=[str(package_dir)],
    )
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


class FakeLayout:
    def __init__(self) -> None:
        self.labels: list[tuple[str, str | None]] = []
        self.operators: list[tuple[str, dict]] = []
        self.props: list[tuple[str, dict]] = []
        self.enabled = True

    def label(self, *, text: str, icon: str | None = None) -> None:
        self.labels.append((text, icon))

    def operator(self, operator_id: str, **kwargs) -> None:
        self.operators.append((operator_id, kwargs))

    def prop(self, _owner, property_name: str, **kwargs) -> None:
        self.props.append((property_name, kwargs))

    def row(self, **_kwargs):
        return self

    def box(self):
        return self


class AddonDiagnosticsTests(unittest.TestCase):
    def test_preferences_draw_hides_packaged_native_renderer_status(self) -> None:
        addon = _load_addon_module()
        original_worker = addon.native_bridge.packaged_renderer_worker_path
        addon.native_bridge.packaged_renderer_worker_path = (
            lambda: "C:/Blender/addons/clip_studio_importer/native/clip_cli.exe"
        )
        try:
            preferences = addon.CSI_AddonPreferences()
            preferences.layout = FakeLayout()
            preferences.auto_reload = True

            preferences.draw(types.SimpleNamespace())
        finally:
            addon.native_bridge.packaged_renderer_worker_path = original_worker

        labels = [label for label, _icon in preferences.layout.labels]
        self.assertNotIn("Packaged native renderer found.", labels)
        prop_names = [name for name, _kwargs in preferences.layout.props]
        self.assertIn("auto_reload", prop_names)
        self.assertIn("poll_interval", prop_names)
        self.assertIn("debug", prop_names)
        self.assertIn("developer_mode", prop_names)
        self.assertNotIn("native_library_path", prop_names)

    def test_panel_draws_error_diagnostic(self) -> None:
        addon = _load_addon_module()
        image = {
            addon.CLIP_SOURCE_KEY: "C:/art/sample.clip",
            addon.native_bridge.CLIP_CANVAS_WIDTH_KEY: 640,
            addon.native_bridge.CLIP_CANVAS_HEIGHT_KEY: 480,
            addon.native_bridge.CLIP_RENDERER_ABI_KEY: 1,
            addon.native_bridge.CLIP_RENDERER_VERSION_KEY: "0.1.0-test",
            addon.native_bridge.CLIP_ROOT_LAYER_KEY: 2,
            addon.native_bridge.CLIP_LAYER_COUNT_KEY: 12,
            addon.native_bridge.CLIP_EXTERNAL_COUNT_KEY: 9,
            addon.native_bridge.CLIP_RELOAD_STATUS_KEY: addon.native_bridge.RELOAD_STATUS_ERROR,
            addon.native_bridge.CLIP_RELOAD_ERROR_KEY: "native renderer failed loudly",
            addon.native_bridge.CLIP_RELOAD_LAST_SECONDS_KEY: 2.4,
            addon.native_bridge.CLIP_SUPPORT_STATUS_KEY: addon.native_bridge.SUPPORT_STATUS_UNSUPPORTED,
            addon.native_bridge.CLIP_SUPPORT_REPORT_KEY: "2 unsupported node(s).",
            addon.native_bridge.CLIP_SUPPORT_SOURCE_COUNT_KEY: 6,
            addon.native_bridge.CLIP_SUPPORT_UNSUPPORTED_COUNT_KEY: 2,
            addon.native_bridge.CLIP_SUPPORT_RASTER_COUNT_KEY: 3,
            addon.native_bridge.CLIP_SUPPORT_MAX_RASTER_LAYER_KEY: 9,
            addon.native_bridge.CLIP_SUPPORT_MAX_RASTER_WIDTH_KEY: 32,
            addon.native_bridge.CLIP_SUPPORT_MAX_RASTER_HEIGHT_KEY: 16,
            addon.native_bridge.CLIP_SUPPORT_MASK_COUNT_KEY: 1,
            addon.native_bridge.CLIP_SUPPORT_MAX_MASK_LAYER_KEY: 10,
            addon.native_bridge.CLIP_SUPPORT_MAX_MASK_WIDTH_KEY: 32,
            addon.native_bridge.CLIP_SUPPORT_MAX_MASK_HEIGHT_KEY: 16,
            addon.native_bridge.CLIP_SUPPORT_DETAILS_KEY: (
                "- layer 9 node 4 Filter\n"
                "- layer 10 node 5 Raster\n"
                "- layer 11 node 6 Filter\n"
                "- layer 12 node 7 Raster\n"
                "- layer 13 node 8 Filter\n"
                "- layer 14 node 9 Raster"
            ),
        }
        panel = addon.IMAGE_PT_clip_studio()
        panel.layout = FakeLayout()
        context = types.SimpleNamespace(space_data=types.SimpleNamespace(image=image))

        panel.draw(context)

        labels = [label for label, _icon in panel.layout.labels]
        self.assertIn("Render failed", labels)
        self.assertIn("Unsupported native nodes: 2", labels)
        self.assertNotIn("Mode: Native renderer", labels)
        self.assertNotIn("Native support: Unsupported nodes", labels)
        self.assertNotIn("Renderer version: 0.1.0-test", labels)
        self.assertNotIn("2 unsupported node(s).", labels)
        self.assertNotIn("Sources: 6; unsupported: 2", labels)
        self.assertNotIn("Raster resources: 3", labels)
        self.assertNotIn("Mask resources: 1", labels)
        self.assertNotIn("Largest raster: layer 9, 32x16", labels)
        self.assertNotIn("Largest mask: layer 10, 32x16", labels)
        self.assertIn(
            "Locations: layer 9/node 4 Filter, layer 10/node 5 Raster, "
            "layer 11/node 6 Filter, +3 more",
            labels,
        )
        self.assertIn("- layer 9 node 4 Filter", labels)
        self.assertIn("- layer 10 node 5 Raster", labels)
        self.assertIn("- layer 11 node 6 Filter", labels)
        self.assertIn("- layer 12 node 7 Raster", labels)
        self.assertNotIn("- layer 13 node 8 Filter", labels)
        self.assertNotIn("- layer 14 node 9 Raster", labels)
        self.assertIn("2 more unsupported item(s)", labels)
        self.assertIn("Error: native renderer failed loudly", labels)
        self.assertNotIn("Last render: 2.4s", labels)
        self.assertIn(
            (
                addon.IMAGE_OT_toggle_clip_support_details.bl_idname,
                {"text": "Show all unsupported details", "icon": "TRIA_RIGHT"},
            ),
            panel.layout.operators,
        )
        self.assertIn(
            (
                addon.IMAGE_OT_copy_clip_support_diagnostics.bl_idname,
                {"text": "Copy Diagnostic", "icon": "COPYDOWN"},
            ),
            panel.layout.operators,
        )
        self.assertIn(
            (
                addon.IMAGE_OT_copy_clip_support_locations.bl_idname,
                {"text": "Copy layer locations", "icon": "COPYDOWN"},
            ),
            panel.layout.operators,
        )
        self.assertNotIn(
            (
                addon.IMAGE_OT_open_clip_support_diagnostics.bl_idname,
                {"text": "Open Diagnostics", "icon": "TEXT"},
            ),
            panel.layout.operators,
        )
        self.assertNotIn(addon.CLIP_SUPPORT_DETAILS_EXPANDED_KEY, image)

    def test_panel_draws_refresh_elapsed_time(self) -> None:
        addon = _load_addon_module()
        image = {
            addon.CLIP_SOURCE_KEY: "C:/art/sample.clip",
            addon.native_bridge.CLIP_RELOAD_STATUS_KEY: addon.native_bridge.RELOAD_STATUS_REFRESHING,
            addon.native_bridge.CLIP_RELOAD_STARTED_AT_KEY: addon.time.time() - 1.2,
        }
        panel = addon.IMAGE_PT_clip_studio()
        panel.layout = FakeLayout()
        context = types.SimpleNamespace(space_data=types.SimpleNamespace(image=image))

        panel.draw(context)

        labels = [label for label, _icon in panel.layout.labels]
        self.assertTrue(any(label.startswith("Elapsed: ") for label in labels))

    def test_panel_hides_full_native_support_metadata(self) -> None:
        addon = _load_addon_module()
        image = {
            addon.CLIP_SOURCE_KEY: "C:/art/sample.clip",
            addon.CLIP_PACK_STATUS_KEY: addon.PACK_STATUS_NEEDS_PACK,
            addon.image_state.CLIP_RELOAD_STATUS_KEY: addon.image_state.RELOAD_STATUS_OK,
            addon.native_bridge.CLIP_RENDERER_VERSION_KEY: "0.1.0-test",
            addon.native_bridge.CLIP_SUPPORT_STATUS_KEY: addon.native_bridge.SUPPORT_STATUS_FULL,
            addon.native_bridge.CLIP_SUPPORT_REPORT_KEY: "Full native support for 4 source(s).",
            addon.native_bridge.CLIP_SUPPORT_SOURCE_COUNT_KEY: 4,
            addon.native_bridge.CLIP_SUPPORT_UNSUPPORTED_COUNT_KEY: 0,
            addon.native_bridge.CLIP_SUPPORT_RASTER_COUNT_KEY: 3,
            addon.native_bridge.CLIP_SUPPORT_MASK_COUNT_KEY: 1,
            addon.native_bridge.CLIP_SUPPORT_MAX_RASTER_LAYER_KEY: 9,
            addon.native_bridge.CLIP_SUPPORT_MAX_RASTER_WIDTH_KEY: 32,
            addon.native_bridge.CLIP_SUPPORT_MAX_RASTER_HEIGHT_KEY: 16,
            addon.native_bridge.CLIP_SUPPORT_MAX_MASK_LAYER_KEY: 10,
            addon.native_bridge.CLIP_SUPPORT_MAX_MASK_WIDTH_KEY: 32,
            addon.native_bridge.CLIP_SUPPORT_MAX_MASK_HEIGHT_KEY: 16,
        }
        panel = addon.IMAGE_PT_clip_studio()
        panel.layout = FakeLayout()
        context = types.SimpleNamespace(space_data=types.SimpleNamespace(image=image))

        panel.draw(context)

        labels = [label for label, _icon in panel.layout.labels]
        self.assertNotIn("Ready", labels)
        self.assertIn("Needs Pack", labels)
        self.assertNotIn("Mode: Native renderer", labels)
        self.assertNotIn("Will pack before saving the .blend.", labels)
        self.assertNotIn("Native support: Full native support", labels)
        self.assertNotIn("Renderer version: 0.1.0-test", labels)
        self.assertNotIn("Full native support for 4 source(s).", labels)
        self.assertNotIn("Sources: 4; unsupported: 0", labels)
        self.assertNotIn("Raster resources: 3", labels)
        self.assertNotIn("Mask resources: 1", labels)
        self.assertNotIn("Largest raster: layer 9, 32x16", labels)
        self.assertNotIn("Largest mask: layer 10, 32x16", labels)
        self.assertIn(
            (
                addon.IMAGE_OT_copy_clip_support_diagnostics.bl_idname,
                {"text": "Copy Diagnostic", "icon": "COPYDOWN"},
            ),
            panel.layout.operators,
        )

    def test_panel_tolerates_stale_native_bridge_without_timing_constants(self) -> None:
        addon = _load_addon_module()
        timing_attributes = [
            "CLIP_PHASE_WORKER_SECONDS_KEY",
            "CLIP_PHASE_OUTPUT_READ_SECONDS_KEY",
            "CLIP_PHASE_CONVERT_SECONDS_KEY",
            "CLIP_PHASE_FOREACH_SECONDS_KEY",
            "CLIP_PHASE_UPDATE_SECONDS_KEY",
            "CLIP_PHASE_PACK_SECONDS_KEY",
            "CLIP_PHASE_UPLOAD_SECONDS_KEY",
        ]
        originals = {
            name: getattr(addon.native_bridge, name)
            for name in timing_attributes
            if hasattr(addon.native_bridge, name)
        }
        try:
            for name in originals:
                delattr(addon.native_bridge, name)
            image = {
                addon.CLIP_SOURCE_KEY: "C:/art/sample.clip",
                addon.native_bridge.CLIP_RELOAD_STATUS_KEY: addon.native_bridge.RELOAD_STATUS_OK,
                "clip_phase_upload_seconds": 1.25,
            }
            panel = addon.IMAGE_PT_clip_studio()
            panel.layout = FakeLayout()
            addon.bpy.context.preferences.addons[
                "clip_studio_importer"
            ].preferences.developer_mode = True
            context = types.SimpleNamespace(space_data=types.SimpleNamespace(image=image))

            panel.draw(context)

            labels = [label for label, _icon in panel.layout.labels]
            self.assertIn("Blender upload total: 1.2s", labels)
        finally:
            for name, value in originals.items():
                setattr(addon.native_bridge, name, value)

    def test_panel_expands_all_support_details(self) -> None:
        addon = _load_addon_module()
        image = {
            addon.CLIP_SOURCE_KEY: "C:/art/sample.clip",
            addon.CLIP_SUPPORT_DETAILS_EXPANDED_KEY: True,
            addon.native_bridge.CLIP_RELOAD_STATUS_KEY: addon.native_bridge.RELOAD_STATUS_OK,
            addon.native_bridge.CLIP_SUPPORT_STATUS_KEY: addon.native_bridge.SUPPORT_STATUS_UNSUPPORTED,
            addon.native_bridge.CLIP_SUPPORT_REPORT_KEY: "5 unsupported node(s).",
            addon.native_bridge.CLIP_SUPPORT_DETAILS_KEY: "\n".join(
                f"- layer {index} node {index + 1} Filter" for index in range(1, 6)
            ),
        }
        panel = addon.IMAGE_PT_clip_studio()
        panel.layout = FakeLayout()
        context = types.SimpleNamespace(space_data=types.SimpleNamespace(image=image))

        panel.draw(context)

        labels = [label for label, _icon in panel.layout.labels]
        self.assertIn("- layer 1 node 2 Filter", labels)
        self.assertIn("- layer 5 node 6 Filter", labels)
        self.assertNotIn("1 more unsupported item(s)", labels)
        self.assertIn(
            (
                addon.IMAGE_OT_toggle_clip_support_details.bl_idname,
                {"text": "Show fewer unsupported details", "icon": "TRIA_DOWN"},
            ),
            panel.layout.operators,
        )

    def test_toggle_support_details_operator_flips_image_state(self) -> None:
        addon = _load_addon_module()
        image = {addon.CLIP_SOURCE_KEY: "C:/art/sample.clip"}
        context = types.SimpleNamespace(space_data=types.SimpleNamespace(image=image))
        operator = addon.IMAGE_OT_toggle_clip_support_details()

        self.assertEqual(operator.execute(context), {"FINISHED"})
        self.assertTrue(image[addon.CLIP_SUPPORT_DETAILS_EXPANDED_KEY])

        self.assertEqual(operator.execute(context), {"FINISHED"})
        self.assertFalse(image[addon.CLIP_SUPPORT_DETAILS_EXPANDED_KEY])

    def test_copy_support_diagnostics_operator_writes_clipboard(self) -> None:
        addon = _load_addon_module()
        image = {
            addon.CLIP_SOURCE_KEY: "C:/art/sample.clip",
            addon.CLIP_SIZE_KEY: "2048",
            addon.CLIP_SHA256_KEY: "abcd1234",
            addon.native_bridge.CLIP_RELOAD_STATUS_KEY: addon.native_bridge.RELOAD_STATUS_ERROR,
            addon.native_bridge.CLIP_RELOAD_ERROR_KEY: "native renderer failed loudly",
            addon.native_bridge.CLIP_RELOAD_LAST_SECONDS_KEY: 2.4,
            addon.native_bridge.CLIP_PHASE_WORKER_SECONDS_KEY: 1.1,
            addon.native_bridge.CLIP_PHASE_OUTPUT_READ_SECONDS_KEY: 0.2,
            addon.native_bridge.CLIP_PHASE_CONVERT_SECONDS_KEY: 0.3,
            addon.native_bridge.CLIP_PHASE_FOREACH_SECONDS_KEY: 0.4,
            addon.native_bridge.CLIP_PHASE_UPDATE_SECONDS_KEY: 0.5,
            addon.native_bridge.CLIP_PHASE_PACK_SECONDS_KEY: 0.6,
            addon.native_bridge.CLIP_PHASE_UPLOAD_SECONDS_KEY: 1.8,
            addon.native_bridge.CLIP_CANVAS_WIDTH_KEY: 640,
            addon.native_bridge.CLIP_CANVAS_HEIGHT_KEY: 480,
            addon.native_bridge.CLIP_RENDERER_ABI_KEY: 1,
            addon.native_bridge.CLIP_RENDERER_VERSION_KEY: "0.1.0-test",
            addon.native_bridge.CLIP_SUPPORT_STATUS_KEY: addon.native_bridge.SUPPORT_STATUS_UNSUPPORTED,
            addon.native_bridge.CLIP_SUPPORT_REPORT_KEY: "2 unsupported node(s).",
            addon.native_bridge.CLIP_SUPPORT_SOURCE_COUNT_KEY: 6,
            addon.native_bridge.CLIP_SUPPORT_UNSUPPORTED_COUNT_KEY: 2,
            addon.native_bridge.CLIP_SUPPORT_RASTER_COUNT_KEY: 3,
            addon.native_bridge.CLIP_SUPPORT_MASK_COUNT_KEY: 1,
            addon.native_bridge.CLIP_SUPPORT_DETAILS_KEY: "- layer 9 node 4 Filter",
        }
        context = types.SimpleNamespace(
            space_data=types.SimpleNamespace(image=image),
            window_manager=types.SimpleNamespace(clipboard=""),
        )
        operator = addon.IMAGE_OT_copy_clip_support_diagnostics()

        self.assertEqual(operator.execute(context), {"FINISHED"})

        clipboard = context.window_manager.clipboard
        self.assertIn("Clip Studio native render diagnostics", clipboard)
        self.assertIn("Source: C:/art/sample.clip", clipboard)
        self.assertIn("Source size: 2.0 KiB", clipboard)
        self.assertIn("Source SHA-256: abcd1234", clipboard)
        self.assertIn("Status: Render failed", clipboard)
        self.assertIn("Last render duration: 2.4s", clipboard)
        self.assertIn("Timing phases:", clipboard)
        self.assertIn("- Native worker: 1.1s", clipboard)
        self.assertIn("- Blender upload total: 1.8s", clipboard)
        self.assertIn("Canvas: 640x480", clipboard)
        self.assertIn("Renderer ABI: 1", clipboard)
        self.assertIn("Renderer version: 0.1.0-test", clipboard)
        self.assertIn("Unsupported native nodes: 2", clipboard)
        self.assertNotIn("Mode: Native renderer", clipboard)
        self.assertNotIn("Native support: Unsupported nodes", clipboard)
        self.assertNotIn("Support report:", clipboard)
        self.assertNotIn("2 unsupported node(s).", clipboard)
        self.assertNotIn("Raster resources: 3", clipboard)
        self.assertNotIn("Mask resources: 1", clipboard)
        self.assertIn("Unsupported locations:", clipboard)
        self.assertIn("- layer 9 node 4 Filter", clipboard)
        self.assertIn("Render error: native renderer failed loudly", clipboard)

    def test_copy_support_locations_operator_writes_clipboard(self) -> None:
        addon = _load_addon_module()
        image = {
            addon.CLIP_SOURCE_KEY: "C:/art/sample.clip",
            addon.native_bridge.CLIP_SUPPORT_DETAILS_KEY: (
                "- layer 9 [Tone curve] node 4 Filter: filter layer is not supported\n"
                "- layer 10 node 5 Raster: raster colour type None is not supported"
            ),
        }
        context = types.SimpleNamespace(
            space_data=types.SimpleNamespace(image=image),
            window_manager=types.SimpleNamespace(clipboard=""),
        )
        operator = addon.IMAGE_OT_copy_clip_support_locations()

        self.assertEqual(operator.execute(context), {"FINISHED"})

        clipboard = context.window_manager.clipboard
        self.assertIn("Clip Studio unsupported layer locations", clipboard)
        self.assertIn("Source: C:/art/sample.clip", clipboard)
        self.assertIn("- layer 9 [Tone curve] node 4 Filter", clipboard)
        self.assertIn("- layer 10 node 5 Raster", clipboard)
        self.assertNotIn("filter layer is not supported", clipboard)
        self.assertEqual(operator.reported[0], {"INFO"})

    def test_panel_summarizes_named_support_locations(self) -> None:
        addon = _load_addon_module()
        image = {
            addon.CLIP_SOURCE_KEY: "C:/art/sample.clip",
            addon.native_bridge.CLIP_RELOAD_STATUS_KEY: addon.native_bridge.RELOAD_STATUS_OK,
            addon.native_bridge.CLIP_SUPPORT_STATUS_KEY: addon.native_bridge.SUPPORT_STATUS_UNSUPPORTED,
            addon.native_bridge.CLIP_SUPPORT_REPORT_KEY: "1 unsupported node(s).",
            addon.native_bridge.CLIP_SUPPORT_DETAILS_KEY: (
                "- layer 9 [Tone curve] node 4 Filter: filter layer is not supported"
            ),
        }
        panel = addon.IMAGE_PT_clip_studio()
        panel.layout = FakeLayout()
        context = types.SimpleNamespace(space_data=types.SimpleNamespace(image=image))

        panel.draw(context)

        labels = [label for label, _icon in panel.layout.labels]
        self.assertIn("Locations: layer 9 [Tone curve]/node 4 Filter", labels)

    def test_open_support_diagnostics_operator_writes_text_block(self) -> None:
        addon = _load_addon_module()

        class FakeImage(dict):
            name = "sample image"

        image = FakeImage(
            {
                addon.CLIP_SOURCE_KEY: "C:/art/sample.clip",
                addon.CLIP_SIZE_KEY: "2048",
                addon.CLIP_SHA256_KEY: "abcd1234",
                addon.native_bridge.CLIP_RELOAD_STATUS_KEY: addon.native_bridge.RELOAD_STATUS_ERROR,
                addon.native_bridge.CLIP_RELOAD_ERROR_KEY: "native renderer failed loudly",
                addon.native_bridge.CLIP_RENDERER_VERSION_KEY: "0.1.0-test",
                addon.native_bridge.CLIP_SUPPORT_STATUS_KEY: addon.native_bridge.SUPPORT_STATUS_UNSUPPORTED,
                addon.native_bridge.CLIP_SUPPORT_REPORT_KEY: "1 unsupported node(s).",
                addon.native_bridge.CLIP_SUPPORT_DETAILS_KEY: "- layer 9 node 4 Filter",
            }
        )
        text_space = types.SimpleNamespace(type="TEXT_EDITOR", text=None)
        context = types.SimpleNamespace(
            space_data=types.SimpleNamespace(image=image),
            screen=types.SimpleNamespace(
                areas=[
                    types.SimpleNamespace(
                        type="TEXT_EDITOR",
                        spaces=[text_space],
                    )
                ]
            ),
        )
        operator = addon.IMAGE_OT_open_clip_support_diagnostics()

        self.assertEqual(operator.execute(context), {"FINISHED"})

        text = addon.bpy.data.texts["Clip Studio Diagnostics - sample image"]
        self.assertIs(text_space.text, text)
        self.assertIn("Clip Studio native render diagnostics", text.body)
        self.assertIn("Source: C:/art/sample.clip", text.body)
        self.assertIn("Source size: 2.0 KiB", text.body)
        self.assertIn("Source SHA-256: abcd1234", text.body)
        self.assertIn("Renderer version: 0.1.0-test", text.body)
        self.assertNotIn("Support report:", text.body)
        self.assertNotIn("1 unsupported node(s).", text.body)
        self.assertIn("- layer 9 node 4 Filter", text.body)
        self.assertIn("Render error: native renderer failed loudly", text.body)
        self.assertEqual(operator.reported[0], {"INFO"})
        self.assertIn("Opened Clip Studio Diagnostics - sample image", operator.reported[1])

    def test_import_operator_defers_image_creation_until_render_finishes(self) -> None:
        addon = _load_addon_module()
        original_image = types.SimpleNamespace(name="already-open")
        image_space = types.SimpleNamespace(type="IMAGE_EDITOR", image=original_image)
        uv_space = types.SimpleNamespace(type="IMAGE_EDITOR", ui_type="UV", image=original_image)
        context = types.SimpleNamespace(
            screen=types.SimpleNamespace(
                areas=[
                    types.SimpleNamespace(
                        type="IMAGE_EDITOR",
                        spaces=[image_space],
                    ),
                    types.SimpleNamespace(
                        type="IMAGE_EDITOR",
                        spaces=types.SimpleNamespace(active=uv_space),
                    ),
                    types.SimpleNamespace(
                        type="VIEW_3D",
                        spaces=[types.SimpleNamespace(type="VIEW_3D")],
                    )
                ]
            )
        )
        scheduled = []
        original_schedule = addon._schedule_async_decode

        def fake_schedule(
            path,
            name,
            *,
            create_on_success=False,
            show_on_success=False,
            auto_pack_on_success=False,
        ):
            scheduled.append(
                (
                    path,
                    name,
                    create_on_success,
                    show_on_success,
                    auto_pack_on_success,
                )
            )
            return True

        addon._schedule_async_decode = fake_schedule
        try:
            operator = addon.IMPORT_OT_clip_studio()
            operator.filepath = "C:/art/sample.clip"

            self.assertEqual(operator.execute(context), {"FINISHED"})
        finally:
            addon._schedule_async_decode = original_schedule

        self.assertIsNone(addon.bpy.data.images.get("sample.clip"))
        self.assertIs(image_space.image, original_image)
        self.assertIs(uv_space.image, original_image)
        self.assertEqual(
            scheduled,
            [
                (
                    addon.os.path.abspath("C:/art/sample.clip"),
                    "sample.clip",
                    True,
                    True,
                    True,
                )
            ],
        )
        self.assertEqual(operator.reported[0], {"INFO"})

    def test_import_operator_reserves_unique_name_without_overwriting_existing_image(self) -> None:
        addon = _load_addon_module()
        existing = addon.bpy.data.images.new(
            "sample.clip",
            width=1,
            height=1,
            alpha=True,
            float_buffer=False,
        )
        scheduled = []
        original_schedule = addon._schedule_async_decode

        def fake_schedule(
            path,
            name,
            *,
            create_on_success=False,
            show_on_success=False,
            auto_pack_on_success=False,
        ):
            scheduled.append(
                (
                    path,
                    name,
                    create_on_success,
                    show_on_success,
                    auto_pack_on_success,
                )
            )
            return True

        addon._schedule_async_decode = fake_schedule
        try:
            operator = addon.IMPORT_OT_clip_studio()
            operator.filepath = "C:/art/sample.clip"

            self.assertEqual(operator.execute(types.SimpleNamespace()), {"FINISHED"})
        finally:
            addon._schedule_async_decode = original_schedule

        self.assertIs(addon.bpy.data.images.get("sample.clip"), existing)
        self.assertIsNone(addon.bpy.data.images.get("sample.clip.001"))
        self.assertEqual(
            scheduled,
            [
                (
                    addon.os.path.abspath("C:/art/sample.clip"),
                    "sample.clip.001",
                    True,
                    True,
                    True,
                )
            ],
        )

    def test_initial_async_decode_shows_image_then_auto_packs(self) -> None:
        addon = _load_addon_module()
        original_image = types.SimpleNamespace(name="already-open")
        image_space = types.SimpleNamespace(type="IMAGE_EDITOR", image=original_image)
        addon.bpy.context.screen = types.SimpleNamespace(
            areas=[
                types.SimpleNamespace(
                    type="IMAGE_EDITOR",
                    spaces=[image_space],
                )
            ]
        )
        result = addon.native_bridge.NativeRenderResult(
            clip_path="C:/art/sample.clip",
            width=1,
            height=1,
            root_layer_id=2,
            layer_count=3,
            external_data_count=4,
            renderer_abi=addon.native_bridge.EXPECTED_ABI_VERSION,
            renderer_version="0.1.0-test",
            source_mtime=10.0,
            source_size=4,
            source_sha256="abcd",
            pixels_rgba8=bytes([255, 0, 0, 255]),
            support_summary=None,
            worker_seconds=1.2,
            output_read_seconds=0.1,
        )
        original_render = addon.native_bridge.render_clip_rgba8
        original_register = addon.bpy.app.timers.register
        timer_callbacks = []
        addon.native_bridge.render_clip_rgba8 = lambda _path, **_kwargs: result
        addon.bpy.app.timers.register = (
            lambda callback, **kwargs: timer_callbacks.append((callback, kwargs))
        )
        try:
            addon._async_decode(
                "C:/art/sample.clip",
                "sample.clip",
                create_on_success=True,
                show_on_success=True,
                auto_pack_on_success=True,
            )
            self.assertEqual(len(timer_callbacks), 1)
            self.assertEqual(timer_callbacks[0][1].get("first_interval"), 0.0)
            timer_callbacks.pop(0)[0]()
        finally:
            addon.native_bridge.render_clip_rgba8 = original_render
            addon.bpy.app.timers.register = original_register

        image = addon.bpy.data.images.get("sample.clip")
        self.assertIsNotNone(image)
        self.assertIs(image_space.image, image)
        self.assertFalse(image.packed)
        self.assertTrue(image.updated)
        self.assertEqual(image[addon.CLIP_SOURCE_KEY], "C:/art/sample.clip")
        self.assertEqual(image[addon.CLIP_PACK_STATUS_KEY], addon.PACK_STATUS_NEEDS_PACK)
        self.assertEqual(image[addon.native_bridge.CLIP_RELOAD_STATUS_KEY], "ok")
        self.assertEqual(len(timer_callbacks), 1)
        self.assertEqual(timer_callbacks[0][1].get("first_interval"), 0.1)

        timer_callbacks.pop(0)[0]()

        self.assertTrue(image.packed)
        self.assertEqual(image[addon.CLIP_PACK_STATUS_KEY], addon.PACK_STATUS_PACKED)
        self.assertIn(addon.CLIP_PACK_LAST_SECONDS_KEY, image)

    def test_reload_operator_schedules_background_render(self) -> None:
        addon = _load_addon_module()
        image = addon.bpy.data.images.new(
            "sample.clip",
            width=1,
            height=1,
            alpha=True,
            float_buffer=False,
        )
        image[addon.CLIP_SOURCE_KEY] = "C:/art/sample.clip"
        context = types.SimpleNamespace(space_data=types.SimpleNamespace(image=image))
        scheduled = []
        original_exists = addon.os.path.exists
        original_schedule = addon._schedule_async_decode
        addon.os.path.exists = lambda _path: True
        addon._schedule_async_decode = (
            lambda path, name: scheduled.append((path, name)) or True
        )
        try:
            operator = addon.IMAGE_OT_reload_clip_studio()

            self.assertEqual(operator.execute(context), {"FINISHED"})
        finally:
            addon.os.path.exists = original_exists
            addon._schedule_async_decode = original_schedule

        self.assertEqual(scheduled, [("C:/art/sample.clip", "sample.clip")])
        self.assertEqual(operator.reported[0], {"INFO"})

    def test_async_decode_updates_without_packing_and_marks_dirty(self) -> None:
        addon = _load_addon_module()
        image = addon.bpy.data.images.new(
            "sample.clip",
            width=1,
            height=1,
            alpha=True,
            float_buffer=False,
        )
        image[addon.CLIP_SOURCE_KEY] = "C:/art/sample.clip"
        result = addon.native_bridge.NativeRenderResult(
            clip_path="C:/art/sample.clip",
            width=1,
            height=1,
            root_layer_id=2,
            layer_count=3,
            external_data_count=4,
            renderer_abi=addon.native_bridge.EXPECTED_ABI_VERSION,
            renderer_version="0.1.0-test",
            source_mtime=10.0,
            source_size=4,
            source_sha256="abcd",
            pixels_rgba8=bytes([255, 0, 0, 255]),
            support_summary=None,
            worker_seconds=1.2,
            output_read_seconds=0.1,
        )
        original_render = addon.native_bridge.render_clip_rgba8
        original_register = addon.bpy.app.timers.register
        addon.native_bridge.render_clip_rgba8 = lambda _path, **_kwargs: result
        addon.bpy.app.timers.register = lambda callback, **_kwargs: callback()
        try:
            addon._async_decode("C:/art/sample.clip", "sample.clip")
        finally:
            addon.native_bridge.render_clip_rgba8 = original_render
            addon.bpy.app.timers.register = original_register

        self.assertFalse(image.packed)
        self.assertTrue(image.updated)
        self.assertEqual(image[addon.CLIP_PACK_STATUS_KEY], addon.PACK_STATUS_NEEDS_PACK)
        self.assertEqual(image[addon.native_bridge.CLIP_RELOAD_STATUS_KEY], "ok")

    def test_pack_now_operator_packs_current_pixels(self) -> None:
        addon = _load_addon_module()
        image = addon.bpy.data.images.new(
            "sample.clip",
            width=1,
            height=1,
            alpha=True,
            float_buffer=False,
        )
        image[addon.CLIP_SOURCE_KEY] = "C:/art/sample.clip"
        image[addon.CLIP_PACK_STATUS_KEY] = addon.PACK_STATUS_NEEDS_PACK
        context = types.SimpleNamespace(space_data=types.SimpleNamespace(image=image))
        operator = addon.IMAGE_OT_pack_clip_studio()

        self.assertEqual(operator.execute(context), {"FINISHED"})

        self.assertTrue(image.packed)
        self.assertEqual(image[addon.CLIP_PACK_STATUS_KEY], addon.PACK_STATUS_PACKED)
        self.assertIn(addon.CLIP_PACK_LAST_SECONDS_KEY, image)

    def test_save_pre_packs_dirty_native_images(self) -> None:
        addon = _load_addon_module()
        image = addon.bpy.data.images.new(
            "sample.clip",
            width=1,
            height=1,
            alpha=True,
            float_buffer=False,
        )
        image[addon.CLIP_SOURCE_KEY] = "C:/art/sample.clip"
        image[addon.CLIP_NATIVE_KEY] = True
        image[addon.CLIP_PACK_STATUS_KEY] = addon.PACK_STATUS_NEEDS_PACK

        addon._save_pre_pack_native_images(None)

        self.assertTrue(image.packed)
        self.assertEqual(image[addon.CLIP_PACK_STATUS_KEY], addon.PACK_STATUS_PACKED)

    def test_status_label_shortens_unknown_values(self) -> None:
        addon = _load_addon_module()

        self.assertEqual(
            addon._reload_status_label(addon.native_bridge.RELOAD_STATUS_MISSING),
            "Source missing",
        )
        self.assertEqual(addon._reload_status_label("future_status"), "Unknown")
        diagnostic = addon._short_diagnostic("x" * 200)
        self.assertLessEqual(len(diagnostic), 120)
        self.assertTrue(diagnostic.endswith("..."))
        self.assertTrue(diagnostic.isascii())


if __name__ == "__main__":
    unittest.main()
