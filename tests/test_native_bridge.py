from __future__ import annotations

import os
import unittest
import importlib.util
import sys
import types
from pathlib import Path


def _load_native_bridge():
    package_dir = Path(__file__).resolve().parents[1] / "clip_studio_importer"
    package = types.ModuleType("clip_studio_importer")
    package.__path__ = [str(package_dir)]
    sys.modules["clip_studio_importer"] = package

    image_state_spec = importlib.util.spec_from_file_location(
        "clip_studio_importer.image_state",
        package_dir / "image_state.py",
    )
    image_state = importlib.util.module_from_spec(image_state_spec)
    assert image_state_spec.loader is not None
    sys.modules[image_state_spec.name] = image_state
    image_state_spec.loader.exec_module(image_state)

    worker_protocol_spec = importlib.util.spec_from_file_location(
        "clip_studio_importer.worker_protocol",
        package_dir / "worker_protocol.py",
    )
    worker_protocol = importlib.util.module_from_spec(worker_protocol_spec)
    assert worker_protocol_spec.loader is not None
    sys.modules[worker_protocol_spec.name] = worker_protocol
    worker_protocol_spec.loader.exec_module(worker_protocol)

    spec = importlib.util.spec_from_file_location(
        "clip_studio_importer.native_bridge",
        package_dir / "native_bridge.py",
    )
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


native_bridge = _load_native_bridge()
worker_protocol = sys.modules["clip_studio_importer.worker_protocol"]


class FakePixels:
    def __init__(self) -> None:
        self.values = None
        self.slices = []

    def foreach_set(self, values) -> None:
        self.values = list(values)

    def __setitem__(self, key, values) -> None:
        self.slices.append((key, list(values)))


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


class BlenderIntBoundedFakeImage(FakeImage):
    def __setitem__(self, key, value) -> None:
        if isinstance(value, int) and not isinstance(value, bool):
            if value < -(2**31) or value > 2**31 - 1:
                raise OverflowError("Python int too large to convert to C int")
        super().__setitem__(key, value)


class ScalableFakeImage(FakeImage):
    def scale(self, width: int, height: int) -> None:
        self.size = (width, height)


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
    def render_rgba8(self, clip_path, **_kwargs):
        return native_bridge.NativeRenderResult(
            clip_path=str(clip_path),
            width=1,
            height=1,
            root_layer_id=2,
            layer_count=3,
            external_data_count=4,
            renderer_abi=native_bridge.EXPECTED_ABI_VERSION,
            renderer_version="0.1.0-test",
            source_mtime=123.5,
            source_size=4,
            source_sha256="abcd1234",
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
    def test_packaged_renderer_candidates_prefer_runtime_platform_directory(self) -> None:
        cases = [
            ("linux-x64", "libclip_capi.so"),
            ("macos-arm64", "libclip_capi.dylib"),
        ]
        original_platform_id = native_bridge._runtime_platform_id
        try:
            for platform_id, library_name in cases:
                with self.subTest(platform=platform_id):
                    native_bridge._runtime_platform_id = lambda platform_id=platform_id: platform_id
                    module_dir = Path("/addon")

                    self.assertEqual(
                        native_bridge._packaged_renderer_library_candidates(module_dir)[0],
                        module_dir / "native" / platform_id / library_name,
                    )
                    self.assertEqual(
                        native_bridge._packaged_renderer_worker_candidates(module_dir)[0],
                        module_dir / "native" / platform_id / "clip_cli",
                    )
        finally:
            native_bridge._runtime_platform_id = original_platform_id

    def test_render_clip_requires_packaged_worker(self) -> None:
        original = native_bridge.packaged_renderer_worker_path
        native_bridge.packaged_renderer_worker_path = lambda: None
        try:
            with self.assertRaisesRegex(
                native_bridge.NativeBridgeError,
                "packaged native renderer worker not found",
            ):
                native_bridge.render_clip_rgba8("sample.clip")
        finally:
            native_bridge.packaged_renderer_worker_path = original

    def test_render_clip_uses_persistent_worker_by_default(self) -> None:
        original_path = native_bridge.packaged_renderer_worker_path
        original_shared = native_bridge._shared_renderer_worker
        old_env = os.environ.pop("RIZUM_CLIP_DISABLE_PERSISTENT_WORKER", None)
        calls = []

        class FakePersistentWorker:
            def render_rgba8(self, clip_path, *, previous_manifest_json=None):
                calls.append((clip_path, previous_manifest_json))
                return FakeRenderer().render_rgba8(clip_path)

        native_bridge.packaged_renderer_worker_path = lambda: "C:/addon/clip_cli.exe"
        native_bridge._shared_renderer_worker = lambda _path: FakePersistentWorker()
        try:
            result = native_bridge.render_clip_rgba8(
                "sample.clip",
                previous_manifest_json='{"abi":1}',
            )
        finally:
            native_bridge.packaged_renderer_worker_path = original_path
            native_bridge._shared_renderer_worker = original_shared
            if old_env is not None:
                os.environ["RIZUM_CLIP_DISABLE_PERSISTENT_WORKER"] = old_env

        self.assertEqual(result.width, 1)
        self.assertEqual(calls, [("sample.clip", '{"abi":1}')])

    def test_render_clip_falls_back_to_one_shot_on_persistent_transport_error(self) -> None:
        original_path = native_bridge.packaged_renderer_worker_path
        original_shared = native_bridge._shared_renderer_worker
        original_worker_class = native_bridge.NativeRendererWorker
        original_shutdown = native_bridge.shutdown_renderer_worker
        old_env = os.environ.pop("RIZUM_CLIP_DISABLE_PERSISTENT_WORKER", None)
        calls = []
        shutdown_calls = []

        class FakeOneShotWorker:
            def __init__(self, executable_path):
                self.executable_path = executable_path

            def render_rgba8(self, clip_path, *, previous_manifest_json=None):
                calls.append((self.executable_path, clip_path, previous_manifest_json))
                return FakeRenderer().render_rgba8(clip_path)

        def broken_shared_worker(_path):
            raise native_bridge.NativeWorkerTransportError("pipe failed")

        native_bridge.packaged_renderer_worker_path = lambda: "C:/addon/clip_cli.exe"
        native_bridge._shared_renderer_worker = broken_shared_worker
        native_bridge.NativeRendererWorker = FakeOneShotWorker
        native_bridge.shutdown_renderer_worker = lambda: shutdown_calls.append(True)
        try:
            result = native_bridge.render_clip_rgba8(
                "sample.clip",
                previous_manifest_json='{"abi":1}',
            )
        finally:
            native_bridge.packaged_renderer_worker_path = original_path
            native_bridge._shared_renderer_worker = original_shared
            native_bridge.NativeRendererWorker = original_worker_class
            native_bridge.shutdown_renderer_worker = original_shutdown
            if old_env is not None:
                os.environ["RIZUM_CLIP_DISABLE_PERSISTENT_WORKER"] = old_env

        self.assertEqual(result.width, 1)
        self.assertEqual(shutdown_calls, [True])
        self.assertEqual(calls, [("C:/addon/clip_cli.exe", "sample.clip", '{"abi":1}')])

    def test_create_or_update_image_flips_rows_for_blender_pixel_storage(self) -> None:
        bpy = FakeBpy()
        result = native_bridge.NativeRenderResult(
            clip_path="sample.clip",
            width=2,
            height=2,
            root_layer_id=2,
            layer_count=3,
            external_data_count=4,
            renderer_abi=native_bridge.EXPECTED_ABI_VERSION,
            renderer_version="0.1.0-test",
            source_mtime=None,
            source_size=None,
            source_sha256="",
            pixels_rgba8=bytes(
                [
                    255,
                    0,
                    0,
                    255,
                    0,
                    255,
                    0,
                    255,
                    0,
                    0,
                    255,
                    255,
                    255,
                    255,
                    255,
                    255,
                ]
            ),
            support_summary=None,
        )

        image = native_bridge.create_or_update_image(
            bpy,
            result,
            image_name="sample",
            pack=False,
        )

        self.assertEqual(
            image.pixels.values,
            [
                0.0,
                0.0,
                1.0,
                1.0,
                1.0,
                1.0,
                1.0,
                1.0,
                1.0,
                0.0,
                0.0,
                1.0,
                0.0,
                1.0,
                0.0,
                1.0,
            ],
        )

    def test_create_or_update_image_uploads_pixels_and_tracks_source(self) -> None:
        bpy = FakeBpy()

        image = native_bridge.create_or_update_image(
            bpy,
            FakeRenderer().render_rgba8("sample.clip"),
            image_name="sample",
        )

        self.assertEqual(image.name, "sample")
        self.assertEqual(image.size, (1, 1))
        self.assertEqual(image.source, "GENERATED")
        self.assertEqual(image.colorspace_settings.name, "sRGB")
        self.assertEqual(image.pixels.values, [0.0, 128 / 255.0, 1.0, 1.0])
        self.assertTrue(image.updated)
        self.assertTrue(image.packed)
        self.assertIn(native_bridge.CLIP_PHASE_CONVERT_SECONDS_KEY, image)
        self.assertIn(native_bridge.CLIP_PHASE_FOREACH_SECONDS_KEY, image)
        self.assertIn(native_bridge.CLIP_PHASE_UPDATE_SECONDS_KEY, image)
        self.assertIn(native_bridge.CLIP_PHASE_PACK_SECONDS_KEY, image)
        self.assertIn(native_bridge.CLIP_PHASE_UPLOAD_SECONDS_KEY, image)
        self.assertNotIn(native_bridge.CLIP_PHASE_WORKER_SECONDS_KEY, image)
        self.assertEqual(image[native_bridge.CLIP_SOURCE_KEY], "sample.clip")
        self.assertEqual(image[native_bridge.CLIP_MTIME_KEY], "123.5")
        self.assertEqual(image[native_bridge.CLIP_SIZE_KEY], "4")
        self.assertEqual(image[native_bridge.CLIP_SHA256_KEY], "abcd1234")
        self.assertTrue(image[native_bridge.CLIP_NATIVE_KEY])
        self.assertEqual(image[native_bridge.CLIP_RENDERER_ABI_KEY], 1)
        self.assertEqual(image[native_bridge.CLIP_RENDERER_VERSION_KEY], "0.1.0-test")
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
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_MAX_RASTER_LAYER_KEY], 10)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_MAX_RASTER_WIDTH_KEY], 1)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_MAX_RASTER_HEIGHT_KEY], 1)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_MASK_COUNT_KEY], 0)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_MAX_MASK_LAYER_KEY], 0)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_MAX_MASK_WIDTH_KEY], 0)
        self.assertEqual(image[native_bridge.CLIP_SUPPORT_MAX_MASK_HEIGHT_KEY], 0)
        self.assertNotIn(native_bridge.CLIP_SUPPORT_RASTER_BYTES_KEY, image)
        self.assertNotIn(native_bridge.CLIP_SUPPORT_MAX_RASTER_BYTES_KEY, image)
        self.assertNotIn(native_bridge.CLIP_SUPPORT_MASK_BYTES_KEY, image)
        self.assertNotIn(native_bridge.CLIP_SUPPORT_MAX_MASK_BYTES_KEY, image)
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
            renderer_version=result.renderer_version,
            source_mtime=result.source_mtime,
            source_size=result.source_size,
            source_sha256=result.source_sha256,
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

    def test_large_support_byte_counts_are_not_written_to_blender_idprops(self) -> None:
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
            renderer_version=result.renderer_version,
            source_mtime=result.source_mtime,
            source_size=result.source_size,
            source_sha256=result.source_sha256,
            pixels_rgba8=result.pixels_rgba8,
            support_summary=native_bridge.NativeSupportSummary(
                source_count=343,
                unsupported_count=0,
                raster_count=343,
                raster_bytes=33_476_800_000,
                max_raster_layer_id=10,
                max_raster_width=8_192,
                max_raster_height=8_192,
                max_raster_bytes=3_221_225_472,
                mask_count=6,
                mask_bytes=2_415_919_104,
                max_mask_layer_id=11,
                max_mask_width=8_192,
                max_mask_height=8_192,
                max_mask_bytes=2_415_919_104,
                report="Full native support for 343 source(s).",
                details=(),
            ),
        )
        image = BlenderIntBoundedFakeImage("sample", 1, 1)

        native_bridge.create_or_update_image(bpy, result, image=image)

        self.assertNotIn(native_bridge.CLIP_SUPPORT_RASTER_BYTES_KEY, image)
        self.assertNotIn(native_bridge.CLIP_SUPPORT_MAX_RASTER_BYTES_KEY, image)
        self.assertNotIn(native_bridge.CLIP_SUPPORT_MASK_BYTES_KEY, image)
        self.assertNotIn(native_bridge.CLIP_SUPPORT_MAX_MASK_BYTES_KEY, image)

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
            renderer_version=result.renderer_version,
            source_mtime=result.source_mtime,
            source_size=result.source_size,
            source_sha256=result.source_sha256,
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

    def test_support_detail_records_extract_names_and_reasons(self) -> None:
        details = (
            "- layer 9 [Tone curve] node 4 Filter: filter layer is not supported",
            "- layer 10 node 5 Raster: raster colour type None is not supported",
            "plain summary line",
        )

        records = native_bridge.support_detail_records(details)

        self.assertEqual(len(records), 2)
        self.assertEqual(records[0].layer_id, 9)
        self.assertEqual(records[0].layer_name, "Tone curve")
        self.assertEqual(records[0].node_id, 4)
        self.assertEqual(records[0].kind, "Filter")
        self.assertEqual(records[0].reason, "filter layer is not supported")
        self.assertEqual(records[0].location, "layer 9 [Tone curve] node 4 Filter")
        self.assertEqual(records[1].layer_name, "")
        self.assertEqual(records[1].reason, "raster colour type None is not supported")

        self.assertEqual(
            native_bridge.support_detail_locations(details),
            ("layer 9 [Tone curve] node 4 Filter", "layer 10 node 5 Raster"),
        )
        self.assertEqual(
            native_bridge.support_detail_locations(
                (
                    "- layer 9 node 4 Filter: filter layer is not supported",
                    "- layer 10 [Layer 2] node 5 Raster: raster colour type None is not supported",
                    "plain summary line",
                )
            ),
            ("layer 9 node 4 Filter", "layer 10 [Layer 2] node 5 Raster"),
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

    def test_write_reload_status_and_error_manage_error_and_started_keys(self) -> None:
        with self.subTest(case="error_records_message_and_clears_started"):
            image = FakeImage("sample", 1, 1)
            image[native_bridge.CLIP_RELOAD_STARTED_AT_KEY] = "10.0"

            native_bridge.write_reload_error(image, "render failed")

            self.assertEqual(
                image[native_bridge.CLIP_RELOAD_STATUS_KEY],
                native_bridge.RELOAD_STATUS_ERROR,
            )
            self.assertEqual(image[native_bridge.CLIP_RELOAD_ERROR_KEY], "render failed")
            self.assertNotIn(native_bridge.CLIP_RELOAD_STARTED_AT_KEY, image)

        with self.subTest(case="status_clears_previous_error_and_started"):
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

        with self.subTest(case="refreshing_keeps_started_time"):
            image = FakeImage("sample", 1, 1)
            image[native_bridge.CLIP_RELOAD_STARTED_AT_KEY] = "10.0"

            native_bridge.write_reload_status(image, native_bridge.RELOAD_STATUS_REFRESHING)

            self.assertEqual(
                image[native_bridge.CLIP_RELOAD_STATUS_KEY],
                native_bridge.RELOAD_STATUS_REFRESHING,
            )
            self.assertEqual(image[native_bridge.CLIP_RELOAD_STARTED_AT_KEY], "10.0")

    def test_create_or_update_applies_patch_rows_to_existing_image(self) -> None:
        bpy = FakeBpy()
        image = FakeImage("sample", 4, 4)
        result = native_bridge.NativeRenderResult(
            clip_path="sample.clip",
            width=4,
            height=4,
            root_layer_id=2,
            layer_count=3,
            external_data_count=4,
            renderer_abi=native_bridge.EXPECTED_ABI_VERSION,
            renderer_version="0.1.0-test",
            source_mtime=123.5,
            source_size=8,
            source_sha256="abcd1234",
            pixels_rgba8=bytes(
                [
                    255,
                    0,
                    0,
                    255,
                    0,
                    255,
                    0,
                    255,
                    0,
                    0,
                    255,
                    255,
                    255,
                    255,
                    0,
                    255,
                ]
            ),
            support_summary=None,
            reload_manifest_json='{"abi":1}',
            reload_diff_mode="patch",
            reload_diff_reason="source tile payload changed",
            patches=(
                native_bridge.NativeRenderPatch(
                    x=1,
                    y=2,
                    width=2,
                    height=2,
                    byte_offset=0,
                ),
            ),
        )

        native_bridge.create_or_update_image(bpy, result, image=image)

        self.assertIsNone(image.pixels.values)
        self.assertEqual(len(image.pixels.slices), 2)
        first_key, first_values = image.pixels.slices[0]
        second_key, second_values = image.pixels.slices[1]
        self.assertEqual(first_key, slice(20, 28, None))
        self.assertEqual(second_key, slice(4, 12, None))
        self.assertEqual(first_values, [1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0])
        self.assertEqual(second_values, [0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0])
        self.assertEqual(image[native_bridge.CLIP_RELOAD_MANIFEST_KEY], '{"abi":1}')
        self.assertEqual(image[native_bridge.CLIP_RELOAD_DIFF_MODE_KEY], "patch")
        self.assertEqual(image[native_bridge.CLIP_RELOAD_PATCH_COUNT_KEY], 1)

    def test_create_or_update_no_change_updates_properties_without_pixels(self) -> None:
        bpy = FakeBpy()
        image = FakeImage("sample", 1, 1)
        result = native_bridge.NativeRenderResult(
            clip_path="sample.clip",
            width=1,
            height=1,
            root_layer_id=2,
            layer_count=3,
            external_data_count=4,
            renderer_abi=native_bridge.EXPECTED_ABI_VERSION,
            renderer_version="0.1.0-test",
            source_mtime=123.5,
            source_size=8,
            source_sha256="abcd1234",
            pixels_rgba8=b"",
            support_summary=None,
            reload_manifest_json='{"abi":1}',
            reload_diff_mode="no_change",
            reload_diff_reason="reload manifest is unchanged",
        )

        native_bridge.create_or_update_image(bpy, result, image=image)

        self.assertIsNone(image.pixels.values)
        self.assertEqual(image.pixels.slices, [])
        self.assertFalse(image.updated)
        self.assertEqual(image[native_bridge.CLIP_RELOAD_DIFF_MODE_KEY], "no_change")

    def test_update_existing_image_rejects_size_mismatch(self) -> None:
        bpy = FakeBpy()
        result = FakeRenderer().render_rgba8("sample.clip")
        image = FakeImage("sample", 2, 2)

        with self.assertRaises(native_bridge.NativeBridgeError):
            native_bridge.create_or_update_image(bpy, result, image=image)

    def test_update_existing_image_can_resize_when_explicitly_allowed(self) -> None:
        bpy = FakeBpy()
        result = FakeRenderer().render_rgba8("sample.clip")
        image = ScalableFakeImage("sample", 2, 2)

        native_bridge.create_or_update_image(
            bpy,
            result,
            image=image,
            allow_resize=True,
            pack=False,
        )

        self.assertEqual(image.size, (1, 1))
        self.assertTrue(image.updated)

    def test_inspect_native_image_source(self) -> None:
        source = native_bridge.CLIP_SOURCE_KEY
        mtime = native_bridge.CLIP_MTIME_KEY
        size = native_bridge.CLIP_SIZE_KEY
        sha256 = native_bridge.CLIP_SHA256_KEY
        ok = native_bridge.RELOAD_STATUS_OK
        stale = native_bridge.RELOAD_STATUS_STALE
        missing = native_bridge.RELOAD_STATUS_MISSING

        # (name, stored image properties, inspect kwargs, expected state attributes,
        #  expected paths passed to exists(), or None to skip that check).
        cases = [
            (
                "fresh_source_ok",
                {source: "sample.clip", mtime: "10.0", size: "42"},
                dict(exists=lambda path: True, getmtime=lambda path: 10.0, getsize=lambda path: 42),
                dict(
                    clip_path="sample.clip",
                    stored_mtime=10.0,
                    current_mtime=10.0,
                    stored_size=42,
                    current_size=42,
                    should_reload=False,
                    status=ok,
                ),
                ["sample.clip"],
            ),
            (
                "resolves_blender_relative_path",
                {source: "//img/sample.clip", mtime: "10.0", size: "42"},
                dict(
                    resolve_path=lambda path: "C:/project/img/sample.clip",
                    exists=lambda path: True,
                    getmtime=lambda path: 10.0,
                    getsize=lambda path: 42,
                ),
                dict(clip_path="C:/project/img/sample.clip", should_reload=False, status=ok),
                ["C:/project/img/sample.clip"],
            ),
            (
                "newer_source_stale",
                {source: "sample.clip", mtime: "10.0"},
                dict(exists=lambda path: True, getmtime=lambda path: 11.0, getsize=lambda path: 42),
                dict(should_reload=True, status=stale),
                None,
            ),
            (
                "size_change_stale",
                {source: "sample.clip", mtime: "10.0", size: "42"},
                dict(exists=lambda path: True, getmtime=lambda path: 10.0, getsize=lambda path: 43),
                dict(should_reload=True, status=stale),
                None,
            ),
            (
                "hash_check_catches_same_stat_change",
                {source: "sample.clip", mtime: "10.0", size: "42", sha256: "old"},
                dict(
                    exists=lambda path: True,
                    getmtime=lambda path: 10.0,
                    getsize=lambda path: 42,
                    getsha256=lambda path: "new",
                    check_hash=True,
                ),
                dict(stored_sha256="old", current_sha256="new", should_reload=True, status=stale),
                None,
            ),
            (
                "hash_check_refreshes_missing_hash",
                {source: "sample.clip", mtime: "10.0", size: "42"},
                dict(
                    exists=lambda path: True,
                    getmtime=lambda path: 10.0,
                    getsize=lambda path: 42,
                    getsha256=lambda path: "current",
                    check_hash=True,
                ),
                dict(current_sha256="current", should_reload=True, status=stale),
                None,
            ),
            (
                "missing_source_keeps_pixels",
                {source: "missing.clip", mtime: "10.0"},
                dict(exists=lambda path: False, getmtime=lambda path: 11.0, getsize=lambda path: 42),
                dict(should_reload=False, current_mtime=None, status=missing),
                None,
            ),
            (
                "unknown_mtime_refreshes",
                {source: "sample.clip"},
                dict(exists=lambda path: True, getmtime=lambda path: 12.0, getsize=lambda path: 42),
                dict(stored_mtime=None, current_mtime=12.0, should_reload=True, status=stale),
                None,
            ),
        ]

        for name, props, kwargs, expected, expected_seen_paths in cases:
            with self.subTest(name=name):
                image = FakeImage("sample", 1, 1)
                for key, value in props.items():
                    image[key] = value
                seen_paths = []
                exists = kwargs["exists"]

                def record_exists(path: str, exists=exists) -> bool:
                    seen_paths.append(path)
                    return exists(path)

                state = native_bridge.inspect_native_image_source(
                    image,
                    **{**kwargs, "exists": record_exists},
                )

                for attribute, value in expected.items():
                    self.assertEqual(getattr(state, attribute), value, attribute)
                if expected_seen_paths is not None:
                    self.assertEqual(seen_paths, expected_seen_paths)


if __name__ == "__main__":
    unittest.main()
