import re
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
STREAM_SEQUENCE = (
    REPO_ROOT / "native" / "rust" / "crates" / "clip_gpu" / "src" / "stream_sequence.rs"
)


class NativeTileEventModuleBoundaryTests(unittest.TestCase):
    def test_stream_sequence_stays_segment_dispatch_only(self):
        source = STREAM_SEQUENCE.read_text(encoding="utf-8")

        forbidden_tokens = [
            "BarrierReason",
            "LoweringDecision",
            "barrier_reason",
            "classify_barrier",
            "classify_",
            "lower_source",
            "lower_segment",
            "source_is_silo_eligible",
            "blend_is_silo_eligible",
            "clipping_run_silo_is_eligible",
        ]
        for token in forbidden_tokens:
            self.assertNotIn(token, source)

        forbidden_function_names = re.findall(
            r"fn\s+([A-Za-z0-9_]*(?:lower|classif|eligible)[A-Za-z0-9_]*)\s*\(",
            source,
        )
        self.assertEqual(forbidden_function_names, [])

        self.assertEqual(source.count("RenderSegmentKind::Barrier"), 1)
        self.assertEqual(source.count("BarrierProgramKind::LegacySource"), 1)


if __name__ == "__main__":
    unittest.main()
