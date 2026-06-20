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


if __name__ == "__main__":
    unittest.main()
