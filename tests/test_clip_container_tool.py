from __future__ import annotations

import unittest
from pathlib import Path

from tools.clip_container import split_clip


class ClipContainerToolTests(unittest.TestCase):
    def test_split_clip_indexes_sqlite_and_external_chunks(self) -> None:
        clip_path = Path("img") / "Test_Clipping.clip"

        external_chunks, sqlite_bytes = split_clip(str(clip_path))

        self.assertGreater(len(sqlite_bytes), 0)
        self.assertGreaterEqual(len(external_chunks), 1)
        self.assertTrue(all(key.startswith("extrnlid") for key in external_chunks))


if __name__ == "__main__":
    unittest.main()
