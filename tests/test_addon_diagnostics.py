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

    bpy_app_handlers.persistent = persistent
    bpy_app_handlers.load_post = []
    bpy_app.handlers = bpy_app_handlers
    bpy_app.timers = types.SimpleNamespace()
    bpy.app = bpy_app
    bpy.props = bpy_props
    bpy_types.AddonPreferences = AddonPreferences
    bpy_types.Operator = Operator
    bpy_types.Panel = Panel
    bpy_types.TOPBAR_MT_file_import = types.SimpleNamespace()
    bpy.types = bpy_types
    bpy.utils = types.SimpleNamespace()
    bpy.data = types.SimpleNamespace(images=[], texts=FakeTexts())
    bpy.context = types.SimpleNamespace(
        preferences=types.SimpleNamespace(
            addons={
                "clip_studio_importer": types.SimpleNamespace(
                    preferences=types.SimpleNamespace(auto_reload=True)
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

    def label(self, *, text: str, icon: str | None = None) -> None:
        self.labels.append((text, icon))

    def operator(self, operator_id: str, **kwargs) -> None:
        self.operators.append((operator_id, kwargs))

    def prop(self, _owner, property_name: str, **kwargs) -> None:
        self.props.append((property_name, kwargs))


class AddonDiagnosticsTests(unittest.TestCase):
    def test_panel_draws_error_diagnostic(self) -> None:
        addon = _load_addon_module()
        image = {
            addon.CLIP_SOURCE_KEY: "C:/art/sample.clip",
            addon.native_bridge.CLIP_CANVAS_WIDTH_KEY: 640,
            addon.native_bridge.CLIP_CANVAS_HEIGHT_KEY: 480,
            addon.native_bridge.CLIP_RENDERER_ABI_KEY: 1,
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
            addon.native_bridge.CLIP_SUPPORT_RASTER_BYTES_KEY: 2048,
            addon.native_bridge.CLIP_SUPPORT_MAX_RASTER_LAYER_KEY: 9,
            addon.native_bridge.CLIP_SUPPORT_MAX_RASTER_WIDTH_KEY: 32,
            addon.native_bridge.CLIP_SUPPORT_MAX_RASTER_HEIGHT_KEY: 16,
            addon.native_bridge.CLIP_SUPPORT_MAX_RASTER_BYTES_KEY: 2048,
            addon.native_bridge.CLIP_SUPPORT_MASK_COUNT_KEY: 1,
            addon.native_bridge.CLIP_SUPPORT_MASK_BYTES_KEY: 512,
            addon.native_bridge.CLIP_SUPPORT_MAX_MASK_LAYER_KEY: 10,
            addon.native_bridge.CLIP_SUPPORT_MAX_MASK_WIDTH_KEY: 32,
            addon.native_bridge.CLIP_SUPPORT_MAX_MASK_HEIGHT_KEY: 16,
            addon.native_bridge.CLIP_SUPPORT_MAX_MASK_BYTES_KEY: 512,
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
        self.assertIn("Status: Render failed", labels)
        self.assertIn("Native support: Unsupported nodes", labels)
        self.assertIn("2 unsupported node(s).", labels)
        self.assertIn("Sources: 6; unsupported: 2", labels)
        self.assertIn("Raster resources: 3, 2.0 KiB", labels)
        self.assertIn("Mask resources: 1, 512 B", labels)
        self.assertIn("Largest raster: layer 9, 32x16, 2.0 KiB", labels)
        self.assertIn("Largest mask: layer 10, 32x16, 512 B", labels)
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
        self.assertIn("Last render: 2.4s", labels)
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
                {"text": "Copy support diagnostics", "icon": "COPYDOWN"},
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
        self.assertIn(
            (
                addon.IMAGE_OT_open_clip_support_diagnostics.bl_idname,
                {"text": "Open support report", "icon": "TEXT"},
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
            addon.native_bridge.CLIP_RELOAD_STATUS_KEY: addon.native_bridge.RELOAD_STATUS_ERROR,
            addon.native_bridge.CLIP_RELOAD_ERROR_KEY: "native renderer failed loudly",
            addon.native_bridge.CLIP_RELOAD_LAST_SECONDS_KEY: 2.4,
            addon.native_bridge.CLIP_CANVAS_WIDTH_KEY: 640,
            addon.native_bridge.CLIP_CANVAS_HEIGHT_KEY: 480,
            addon.native_bridge.CLIP_SUPPORT_STATUS_KEY: addon.native_bridge.SUPPORT_STATUS_UNSUPPORTED,
            addon.native_bridge.CLIP_SUPPORT_REPORT_KEY: "2 unsupported node(s).",
            addon.native_bridge.CLIP_SUPPORT_SOURCE_COUNT_KEY: 6,
            addon.native_bridge.CLIP_SUPPORT_UNSUPPORTED_COUNT_KEY: 2,
            addon.native_bridge.CLIP_SUPPORT_RASTER_COUNT_KEY: 3,
            addon.native_bridge.CLIP_SUPPORT_RASTER_BYTES_KEY: 2048,
            addon.native_bridge.CLIP_SUPPORT_MASK_COUNT_KEY: 1,
            addon.native_bridge.CLIP_SUPPORT_MASK_BYTES_KEY: 512,
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
        self.assertIn("Status: Render failed", clipboard)
        self.assertIn("Last render duration: 2.4s", clipboard)
        self.assertIn("Canvas: 640x480", clipboard)
        self.assertIn("Native support: Unsupported nodes", clipboard)
        self.assertIn("Raster resources: 3, 2.0 KiB", clipboard)
        self.assertIn("Unsupported locations:", clipboard)
        self.assertIn("- layer 9 node 4 Filter", clipboard)
        self.assertIn("Render error: native renderer failed loudly", clipboard)

    def test_copy_support_locations_operator_writes_clipboard(self) -> None:
        addon = _load_addon_module()
        image = {
            addon.CLIP_SOURCE_KEY: "C:/art/sample.clip",
            addon.native_bridge.CLIP_SUPPORT_DETAILS_KEY: (
                "- layer 9 node 4 Filter: filter layer is not supported\n"
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
        self.assertIn("- layer 9 node 4 Filter", clipboard)
        self.assertIn("- layer 10 node 5 Raster", clipboard)
        self.assertNotIn("filter layer is not supported", clipboard)
        self.assertEqual(operator.reported[0], {"INFO"})

    def test_open_support_diagnostics_operator_writes_text_block(self) -> None:
        addon = _load_addon_module()

        class FakeImage(dict):
            name = "sample image"

        image = FakeImage(
            {
                addon.CLIP_SOURCE_KEY: "C:/art/sample.clip",
                addon.native_bridge.CLIP_RELOAD_STATUS_KEY: addon.native_bridge.RELOAD_STATUS_ERROR,
                addon.native_bridge.CLIP_RELOAD_ERROR_KEY: "native renderer failed loudly",
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

        text = addon.bpy.data.texts["Clip Studio Support - sample image"]
        self.assertIs(text_space.text, text)
        self.assertIn("Clip Studio native render diagnostics", text.body)
        self.assertIn("Source: C:/art/sample.clip", text.body)
        self.assertIn("1 unsupported node(s).", text.body)
        self.assertIn("- layer 9 node 4 Filter", text.body)
        self.assertIn("Render error: native renderer failed loudly", text.body)
        self.assertEqual(operator.reported[0], {"INFO"})
        self.assertIn("Opened Clip Studio Support - sample image", operator.reported[1])

    def test_status_label_shortens_unknown_values(self) -> None:
        addon = _load_addon_module()

        self.assertEqual(
            addon._reload_status_label(addon.native_bridge.RELOAD_STATUS_MISSING),
            "Source missing",
        )
        self.assertEqual(addon._reload_status_label("future_status"), "Unknown")
        self.assertEqual(
            addon._support_status_label(addon.native_bridge.SUPPORT_STATUS_FULL),
            "Full native support",
        )
        diagnostic = addon._short_diagnostic("x" * 200)
        self.assertLessEqual(len(diagnostic), 120)
        self.assertTrue(diagnostic.endswith("..."))
        self.assertTrue(diagnostic.isascii())


if __name__ == "__main__":
    unittest.main()
