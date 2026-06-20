from __future__ import annotations

import tempfile
import unittest
import zipfile
from pathlib import Path

from tools import build_blender_addon


class BuildBlenderAddonTests(unittest.TestCase):
    def test_build_zip_uses_extension_layout_and_excludes_python_compositor(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            output = Path(tmp_dir) / "clip_studio_importer.zip"

            written = build_blender_addon.build_zip(output, include_native=False)

            self.assertIn("blender_manifest.toml", written)
            self.assertIn("LICENSE", written)
            self.assertIn("NOTICE.md", written)
            self.assertIn("__init__.py", written)
            self.assertIn("image_state.py", written)
            self.assertIn("native_bridge.py", written)
            self.assertIn("worker_protocol.py", written)
            self.assertNotIn("clip_loader.py", written)

            with zipfile.ZipFile(output) as archive:
                names = set(archive.namelist())

        self.assertIn("blender_manifest.toml", names)
        self.assertIn("LICENSE", names)
        self.assertIn("NOTICE.md", names)
        self.assertIn("__init__.py", names)
        self.assertIn("image_state.py", names)
        self.assertIn("native_bridge.py", names)
        self.assertIn("worker_protocol.py", names)
        self.assertNotIn("clip_loader.py", names)

    def test_build_zip_can_package_linux_native_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            artifacts = root / "linux"
            artifacts.mkdir()
            (artifacts / "libclip_capi.so").write_bytes(b"fake library")
            (artifacts / "clip_cli").write_bytes(b"fake worker")
            output = root / "clip_studio_importer_linux.zip"

            written = build_blender_addon.build_zip(
                output,
                include_native=True,
                platforms=("linux-x64",),
                native_artifact_dirs={"linux-x64": artifacts},
            )

            self.assertIn("native/linux-x64/libclip_capi.so", written)
            self.assertIn("native/linux-x64/clip_cli", written)

            with zipfile.ZipFile(output) as archive:
                names = set(archive.namelist())
                manifest = archive.read("blender_manifest.toml").decode("utf-8")

        self.assertIn("native/linux-x64/libclip_capi.so", names)
        self.assertIn("native/linux-x64/clip_cli", names)
        self.assertIn('platforms = ["linux-x64"]', manifest)

    def test_build_zip_can_package_macos_native_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            artifacts = root / "macos-arm64"
            artifacts.mkdir()
            (artifacts / "libclip_capi.dylib").write_bytes(b"fake library")
            (artifacts / "clip_cli").write_bytes(b"fake worker")
            output = root / "clip_studio_importer_macos.zip"

            written = build_blender_addon.build_zip(
                output,
                include_native=True,
                platforms=("macos-arm64",),
                native_artifact_dirs={"macos-arm64": artifacts},
            )

            self.assertIn("native/macos-arm64/libclip_capi.dylib", written)
            self.assertIn("native/macos-arm64/clip_cli", written)

            with zipfile.ZipFile(output) as archive:
                names = set(archive.namelist())
                manifest = archive.read("blender_manifest.toml").decode("utf-8")

        self.assertIn("native/macos-arm64/libclip_capi.dylib", names)
        self.assertIn("native/macos-arm64/clip_cli", names)
        self.assertIn('platforms = ["macos-arm64"]', manifest)


if __name__ == "__main__":
    unittest.main()
