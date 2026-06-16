from __future__ import annotations

import tempfile
import unittest
import zipfile
from pathlib import Path

from tools import build_blender_addon


class BuildBlenderAddonTests(unittest.TestCase):
    def test_build_zip_excludes_python_compositor(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            output = Path(tmp_dir) / "clip_studio_importer.zip"

            written = build_blender_addon.build_zip(output, include_native=False)

            self.assertIn("clip_studio_importer/__init__.py", written)
            self.assertIn("clip_studio_importer/native_bridge.py", written)
            self.assertNotIn("clip_studio_importer/clip_loader.py", written)

            with zipfile.ZipFile(output) as archive:
                names = set(archive.namelist())

        self.assertIn("clip_studio_importer/__init__.py", names)
        self.assertIn("clip_studio_importer/native_bridge.py", names)
        self.assertNotIn("clip_studio_importer/clip_loader.py", names)


if __name__ == "__main__":
    unittest.main()
