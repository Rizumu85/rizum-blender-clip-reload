import importlib.util
from pathlib import Path
import unittest


SCRIPT_PATH = (
    Path(__file__).resolve().parents[1]
    / "scripts"
    / "benchmark_persistent_reload_cache.py"
)
SPEC = importlib.util.spec_from_file_location("benchmark_persistent_reload_cache", SCRIPT_PATH)
benchmark = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(benchmark)


class ReloadCacheBenchmarkTests(unittest.TestCase):
    def test_mutation_prefers_raster_run_tile_work_list(self):
        manifest = {
            "sources": [
                {
                    "kind": "raster",
                    "layer_id": 10,
                    "resource_id": 1,
                    "tiles": [
                        {
                            "tile_x": 2,
                            "tile_y": 3,
                            "compressed_hash": 100,
                        }
                    ],
                },
                {
                    "kind": "raster",
                    "layer_id": 20,
                    "resource_id": 2,
                    "tiles": [
                        {
                            "tile_x": 0,
                            "tile_y": 0,
                            "compressed_hash": 200,
                        }
                    ],
                },
            ],
            "segments": [
                {
                    "ordinal": 7,
                    "kind": "RasterRun",
                    "tile_work_list": [
                        {
                            "kind": "raster",
                            "layer_id": 20,
                            "resource_id": 2,
                            "tile_x": 0,
                            "tile_y": 0,
                        }
                    ],
                }
            ],
        }

        mutated, details = benchmark.mutated_previous_manifest(manifest, 1)

        self.assertEqual(details["segment_ordinal"], 7)
        self.assertEqual(details["layer_id"], 20)
        self.assertEqual(mutated["sources"][0]["tiles"][0]["compressed_hash"], 100)
        self.assertNotEqual(mutated["sources"][1]["tiles"][0]["compressed_hash"], 200)
        self.assertEqual(manifest["sources"][1]["tiles"][0]["compressed_hash"], 200)

    def test_summary_records_region_fallback_and_sparse_upload_work(self):
        metadata = {
            "reload_diff": {
                "mode": "patch",
                "patch_renderer": "region",
                "patch_renderer_fallback_reason": "no executable sparse patch",
                "payload_bytes": 1024,
                "dirty_rects": [{"x": 64, "y": 128, "width": 256, "height": 256}],
                "dirty_segments": [
                    {
                        "dirty_event_ranges": [
                            {"start": 1, "end": 2},
                            {"start": 3, "end": 4},
                        ]
                    }
                ],
            },
            "sparse_atlas_cache": {
                "reused_tiles": 5,
                "inserted_tiles": 1,
                "changed_tiles": 0,
                "evicted_tiles": 0,
                "cached_bytes": 2048,
            },
            "tile_cache_diagnostics": {
                "checkpoint_cache": {
                    "hits": 2,
                    "misses": 3,
                    "stores": 1,
                    "evictions": 0,
                    "cached_entries": 1,
                    "cached_bytes": 4096,
                }
            },
            "render_profile": {
                "worker_total_ms": 11,
                "sparse_atlas_update_ms": 4,
                "legacy_barrier_segment_count": 1,
                "legacy_barrier_segment_ms": 8,
                "tile_local_segment_count": 2,
                "tile_local_segment_ms": 3,
                "top_segments": [
                    {
                        "rank": 1,
                        "kind": "RasterRun",
                        "elapsed_ms": 5,
                        "target_origin": [64, 128],
                        "target_size": [256, 256],
                    },
                    {
                        "rank": 2,
                        "kind": "LegacySource",
                        "barrier_reason": "ThroughGroupNotLowered",
                        "elapsed_ms": 4,
                        "target_origin": [0, 0],
                        "target_size": [512, 512],
                    },
                ],
            },
            "render_task_graph": {
                "tasks": [
                    {
                        "kind": "DecodeTile",
                        "executed": False,
                        "skip_fallback_reason": "sparse not selected",
                    },
                    {
                        "kind": "RegionFallback",
                        "executed": True,
                        "actual_ms": 9,
                    },
                ]
            },
        }

        row = benchmark.summarize_metadata(
            "sample",
            "reload_1",
            metadata,
            previous_checkpoint={"hits": 1, "misses": 1, "stores": 1, "evictions": 0},
        )

        self.assertEqual(row["dirty_segments"], 1)
        self.assertEqual(row["dirty_event_ranges"], 2)
        self.assertEqual(row["checkpoint_hits_delta"], 1)
        self.assertEqual(row["checkpoint_misses_delta"], 2)
        self.assertTrue(row["region_fallback_executed"])
        self.assertTrue(row["sparse_upload_before_region_fallback"])
        self.assertEqual(row["dominant_task"], "RegionFallback:9ms")
        self.assertEqual(len(row["top_reload_segments"]), 2)
        self.assertTrue(row["top_reload_segments"][0]["target_rect_exactly_dirty_rect"])
        self.assertFalse(row["top_reload_segments"][1]["target_rect_exactly_dirty_rect"])
        self.assertIn("unsafe barrier", row["top_reload_segments"][1]["reason_cannot_skip"])


if __name__ == "__main__":
    unittest.main()
