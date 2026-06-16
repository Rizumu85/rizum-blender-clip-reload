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
        pass

    class Panel:
        pass

    class ImportHelper:
        pass

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
    bpy.data = types.SimpleNamespace(images=[])
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
            addon.native_bridge.CLIP_RELOAD_STATUS_KEY: addon.native_bridge.RELOAD_STATUS_ERROR,
            addon.native_bridge.CLIP_RELOAD_ERROR_KEY: "native renderer failed loudly",
            addon.native_bridge.CLIP_SUPPORT_STATUS_KEY: addon.native_bridge.SUPPORT_STATUS_UNSUPPORTED,
            addon.native_bridge.CLIP_SUPPORT_REPORT_KEY: "2 unsupported node(s).",
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
        self.assertIn("- layer 9 node 4 Filter", labels)
        self.assertIn("- layer 10 node 5 Raster", labels)
        self.assertIn("- layer 11 node 6 Filter", labels)
        self.assertIn("- layer 12 node 7 Raster", labels)
        self.assertNotIn("- layer 13 node 8 Filter", labels)
        self.assertNotIn("- layer 14 node 9 Raster", labels)
        self.assertIn("2 more unsupported item(s)", labels)
        self.assertIn("Error: native renderer failed loudly", labels)
        self.assertIn(
            (
                addon.IMAGE_OT_toggle_clip_support_details.bl_idname,
                {"text": "Show all unsupported details", "icon": "TRIA_RIGHT"},
            ),
            panel.layout.operators,
        )
        self.assertNotIn(addon.CLIP_SUPPORT_DETAILS_EXPANDED_KEY, image)

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
        self.assertLessEqual(len(addon._short_diagnostic("x" * 200)), 120)


if __name__ == "__main__":
    unittest.main()
