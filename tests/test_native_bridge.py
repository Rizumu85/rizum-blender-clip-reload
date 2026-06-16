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

    def test_update_existing_image_rejects_size_mismatch(self) -> None:
        bpy = FakeBpy()
        result = FakeRenderer().render_rgba8("sample.clip")
        image = FakeImage("sample", 2, 2)

        with self.assertRaises(native_bridge.NativeBridgeError):
            native_bridge.create_or_update_image(bpy, result, image=image)


if __name__ == "__main__":
    unittest.main()
