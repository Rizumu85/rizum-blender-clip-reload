from __future__ import annotations

import unittest
import importlib.util
import sys
from pathlib import Path


def _load_native_bridge():
    path = Path(__file__).resolve().parents[1] / "clip_studio_importer" / "native_bridge.py"
    spec = importlib.util.spec_from_file_location("native_bridge_under_test", path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


native_bridge = _load_native_bridge()


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


class FakeImages(dict):
    def new(self, name: str, *, width: int, height: int, alpha: bool, float_buffer: bool):
        image = FakeImage(name, width, height)
        image.alpha = alpha
        image.float_buffer = float_buffer
        self[name] = image
        return image


class FakeBpy:
    class Data:
        def __init__(self) -> None:
            self.images = FakeImages()

    def __init__(self) -> None:
        self.data = self.Data()


class FakeRenderer:
    def render_rgba8(self, clip_path):
        return native_bridge.NativeRenderResult(
            clip_path=str(clip_path),
            width=1,
            height=1,
            root_layer_id=2,
            layer_count=3,
            external_data_count=4,
            renderer_abi=native_bridge.EXPECTED_ABI_VERSION,
            source_mtime=123.5,
            pixels_rgba8=bytes([0, 128, 255, 255]),
            support_summary=native_bridge.NativeSupportSummary(
                source_count=2,
                unsupported_count=0,
                raster_count=1,
                raster_bytes=4,
                max_raster_layer_id=10,
                max_raster_width=1,
                max_raster_height=1,
                max_raster_bytes=4,
                mask_count=0,
                mask_bytes=0,
                max_mask_layer_id=0,
                max_mask_width=0,
                max_mask_height=0,
                max_mask_bytes=0,
                report="Full native support for 2 source(s).",
                details=(),
            ),
        )


class NativeBridgeTests(unittest.TestCase):
    def test_import_clip_as_image_uploads_pixels_and_tracks_source(self) -> None:
        bpy = FakeBpy()

        image = native_bridge.import_clip_as_image(
            "sample.clip",
            bpy_module=bpy,
            image_name="sample",
            renderer=FakeRenderer(),
        )

        self.assertEqual(image.name, "sample")
        self.assertEqual(image.size, (1, 1))
        self.assertEqual(image.source, "GENERATED")
        self.assertEqual(image.colorspace_settings.name, "sRGB")
        self.assertEqual(image.pixels.values, [0.0, 128 / 255.0, 1.0, 1.0])
        self.assertTrue(image.updated)
        self.assertTrue(image.packed)
        self.assertEqual(image[native_bridge.CLIP_SOURCE_KEY], "sample.clip")
        self.assertEqual(image[native_bridge.CLIP_MTIME_KEY], "123.5")
        self.assertTrue(image[native_bridge.CLIP_NATIVE_KEY])
        self.assertEqual(image[native_bridge.CLIP_RENDERER_ABI_KEY], 1)
        self.assertEqual(image[native_bridge.CLIP_CANVAS_WIDTH_KEY], 1)
        self.assertEqual(image[native_bridge.CLIP_CANVAS_HEIGHT_KEY], 1)
        self.assertEqual(image[native_bridge.CLIP_RELOAD_STATUS_KEY], "ok")
        self.assertEqual(
            image[native_bridge.CLIP_SUPPORT_STATUS_KEY],
            native_bridge.SUPPORT_STATUS_FULL,
        )
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_SOURCE_COUNT_KEY], 2)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_UNSUPPORTED_COUNT_KEY], 0)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_RASTER_COUNT_KEY], 1)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_RASTER_BYTES_KEY], 4)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_MAX_RASTER_LAYER_KEY], 10)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_MAX_RASTER_WIDTH_KEY], 1)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_MAX_RASTER_HEIGHT_KEY], 1)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_MAX_RASTER_BYTES_KEY], 4)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_MASK_COUNT_KEY], 0)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_MASK_BYTES_KEY], 0)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_MAX_MASK_LAYER_KEY], 0)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_MAX_MASK_WIDTH_KEY], 0)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_MAX_MASK_HEIGHT_KEY], 0)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_MAX_MASK_BYTES_KEY], 0)
        self.assertIn("Full native support", image[native_bridge.CLIP_SUPPORT_REPORT_KEY])
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_DETAILS_KEY], "")

    def test_update_records_unsupported_support_summary(self) -> None:
        bpy = FakeBpy()
        result = FakeRenderer().render_rgba8("sample.clip")
        result = native_bridge.NativeRenderResult(
            clip_path=result.clip_path,
            width=result.width,
            height=result.height,
            root_layer_id=result.root_layer_id,
            layer_count=result.layer_count,
            external_data_count=result.external_data_count,
            renderer_abi=result.renderer_abi,
            source_mtime=result.source_mtime,
            pixels_rgba8=result.pixels_rgba8,
            support_summary=native_bridge.NativeSupportSummary(
                source_count=3,
                unsupported_count=2,
                raster_count=1,
                raster_bytes=4,
                max_raster_layer_id=9,
                max_raster_width=2,
                max_raster_height=2,
                max_raster_bytes=16,
                mask_count=1,
                mask_bytes=2,
                max_mask_layer_id=10,
                max_mask_width=1,
                max_mask_height=2,
                max_mask_bytes=2,
                report="2 unsupported node(s).",
                details=(
                    "- layer 9 node 4 Filter: filter layer is not supported",
                    "- layer 10 node 5 Raster: raster colour type None is not supported",
                ),
            ),
        )

        image = native_bridge.create_or_update_image(bpy, result)

        self.assertEqual(
            image[native_bridge.CLIP_SUPPORT_STATUS_KEY],
            native_bridge.SUPPORT_STATUS_UNSUPPORTED,
        )
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_UNSUPPORTED_COUNT_KEY], 2)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_REPORT_KEY], "2 unsupported node(s).")
        self.assertIn("layer 9", image[native_bridge.CLIP_SUPPORT_DETAILS_KEY])
        self.assertIn("layer 10", image[native_bridge.CLIP_SUPPORT_DETAILS_KEY])
        self.assertEqual(
            image[native_bridge.CLIP_SUPPORT_LOCATIONS_KEY],
            "layer 9 node 4 Filter\nlayer 10 node 5 Raster",
        )

    def test_update_records_unknown_support_when_summary_unavailable(self) -> None:
        bpy = FakeBpy()
        result = FakeRenderer().render_rgba8("sample.clip")
        result = native_bridge.NativeRenderResult(
            clip_path=result.clip_path,
            width=result.width,
            height=result.height,
            root_layer_id=result.root_layer_id,
            layer_count=result.layer_count,
            external_data_count=result.external_data_count,
            renderer_abi=result.renderer_abi,
            source_mtime=result.source_mtime,
            pixels_rgba8=result.pixels_rgba8,
        )

        image = native_bridge.create_or_update_image(bpy, result)

        self.assertEqual(
            image[native_bridge.CLIP_SUPPORT_STATUS_KEY],
            native_bridge.SUPPORT_STATUS_UNKNOWN,
        )
        self.assertIn("unavailable", image[native_bridge.CLIP_SUPPORT_REPORT_KEY])
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_DETAILS_KEY], "")
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_LOCATIONS_KEY], "")

    def test_support_detail_locations_extracts_layer_node_kind(self) -> None:
        locations = native_bridge.support_detail_locations(
            (
                "- layer 9 node 4 Filter: filter layer is not supported",
                "- layer 10 node 5 Raster: raster colour type None is not supported",
                "plain summary line",
            )
        )

        self.assertEqual(
            locations,
            ("layer 9 node 4 Filter", "layer 10 node 5 Raster"),
        )

    def test_successful_update_clears_previous_reload_error(self) -> None:
        bpy = FakeBpy()
        image = FakeImage("sample", 1, 1)
        image[native_bridge.CLIP_RELOAD_STATUS_KEY] = native_bridge.RELOAD_STATUS_ERROR
        image[native_bridge.CLIP_RELOAD_ERROR_KEY] = "old failure"
        image[native_bridge.CLIP_RELOAD_STARTED_AT_KEY] = "10.0"

        native_bridge.create_or_update_image(
            bpy,
            FakeRenderer().render_rgba8("sample.clip"),
            image=image,
        )

        self.assertEqual(image[native_bridge.CLIP_RELOAD_STATUS_KEY], "ok")
        self.assertNotIn(native_bridge.CLIP_RELOAD_ERROR_KEY, image)
        self.assertNotIn(native_bridge.CLIP_RELOAD_STARTED_AT_KEY, image)

    def test_write_reload_error_records_message(self) -> None:
        image = FakeImage("sample", 1, 1)
        image[native_bridge.CLIP_RELOAD_STARTED_AT_KEY] = "10.0"

        native_bridge.write_reload_error(image, "render failed")

        self.assertEqual(
            image[native_bridge.CLIP_RELOAD_STATUS_KEY],
            native_bridge.RELOAD_STATUS_ERROR,
        )
        self.assertEqual(image[native_bridge.CLIP_RELOAD_ERROR_KEY], "render failed")
        self.assertNotIn(native_bridge.CLIP_RELOAD_STARTED_AT_KEY, image)

    def test_write_reload_status_clears_previous_error(self) -> None:
        image = FakeImage("sample", 1, 1)
        native_bridge.write_reload_error(image, "render failed")
        image[native_bridge.CLIP_RELOAD_STARTED_AT_KEY] = "10.0"

        native_bridge.write_reload_status(image, native_bridge.RELOAD_STATUS_MISSING)

        self.assertEqual(
            image[native_bridge.CLIP_RELOAD_STATUS_KEY],
            native_bridge.RELOAD_STATUS_MISSING,
        )
        self.assertNotIn(native_bridge.CLIP_RELOAD_ERROR_KEY, image)
        self.assertNotIn(native_bridge.CLIP_RELOAD_STARTED_AT_KEY, image)

    def test_write_reload_status_keeps_started_time_while_refreshing(self) -> None:
        image = FakeImage("sample", 1, 1)
        image[native_bridge.CLIP_RELOAD_STARTED_AT_KEY] = "10.0"

        native_bridge.write_reload_status(image, native_bridge.RELOAD_STATUS_REFRESHING)

        self.assertEqual(
            image[native_bridge.CLIP_RELOAD_STATUS_KEY],
            native_bridge.RELOAD_STATUS_REFRESHING,
        )
        self.assertEqual(image[native_bridge.CLIP_RELOAD_STARTED_AT_KEY], "10.0")

    def test_update_existing_image_rejects_size_mismatch(self) -> None:
        bpy = FakeBpy()
        result = FakeRenderer().render_rgba8("sample.clip")
        image = FakeImage("sample", 2, 2)

        with self.assertRaises(native_bridge.NativeBridgeError):
            native_bridge.create_or_update_image(bpy, result, image=image)

    def test_inspect_native_image_source_marks_fresh_source_ok(self) -> None:
        image = FakeImage("sample", 1, 1)
        image[native_bridge.CLIP_SOURCE_KEY] = "sample.clip"
        image[native_bridge.CLIP_MTIME_KEY] = "10.0"

        state = native_bridge.inspect_native_image_source(
            image,
            exists=lambda path: True,
            getmtime=lambda path: 10.0,
        )

        self.assertEqual(state.clip_path, "sample.clip")
        self.assertEqual(state.stored_mtime, 10.0)
        self.assertEqual(state.current_mtime, 10.0)
        self.assertFalse(state.should_reload)
        self.assertEqual(state.status, native_bridge.RELOAD_STATUS_OK)

    def test_inspect_native_image_source_marks_newer_source_stale(self) -> None:
        image = FakeImage("sample", 1, 1)
        image[native_bridge.CLIP_SOURCE_KEY] = "sample.clip"
        image[native_bridge.CLIP_MTIME_KEY] = "10.0"

        state = native_bridge.inspect_native_image_source(
            image,
            exists=lambda path: True,
            getmtime=lambda path: 11.0,
        )

        self.assertTrue(state.should_reload)
        self.assertEqual(state.status, native_bridge.RELOAD_STATUS_STALE)

    def test_inspect_native_image_source_keeps_missing_source_pixels(self) -> None:
        image = FakeImage("sample", 1, 1)
        image[native_bridge.CLIP_SOURCE_KEY] = "missing.clip"
        image[native_bridge.CLIP_MTIME_KEY] = "10.0"

        state = native_bridge.inspect_native_image_source(
            image,
            exists=lambda path: False,
            getmtime=lambda path: 11.0,
        )

        self.assertFalse(state.should_reload)
        self.assertIsNone(state.current_mtime)
        self.assertEqual(state.status, native_bridge.RELOAD_STATUS_MISSING)

    def test_inspect_native_image_source_refreshes_unknown_mtime(self) -> None:
        image = FakeImage("sample", 1, 1)
        image[native_bridge.CLIP_SOURCE_KEY] = "sample.clip"

        state = native_bridge.inspect_native_image_source(
            image,
            exists=lambda path: True,
            getmtime=lambda path: 12.0,
        )

        self.assertIsNone(state.stored_mtime)
        self.assertEqual(state.current_mtime, 12.0)
        self.assertTrue(state.should_reload)
        self.assertEqual(state.status, native_bridge.RELOAD_STATUS_STALE)


if __name__ == "__main__":
    unittest.main()
