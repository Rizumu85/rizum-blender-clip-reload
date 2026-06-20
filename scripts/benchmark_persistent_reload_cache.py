"""Benchmark persistent native reload cache behavior.

This script drives one `clip_cli --blender-render-server` process, renders a
baseline image, then performs deterministic patch reload requests by mutating a
single compressed-tile hash in the previous reload manifest. It is diagnostic:
it does not modify `.clip` files and does not change renderer semantics.
"""

from __future__ import annotations

import argparse
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from typing import Any


DEFAULT_SAMPLES = ["Test_Clipping", "Test_RealArt", "Ref_Terra404_Live2D"]


class RenderServer:
    def __init__(self, clip_cli: Path, env: dict[str, str]) -> None:
        self._process = subprocess.Popen(
            [str(clip_cli), "--blender-render-server"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            env=env,
        )

    def render(
        self,
        clip_path: Path,
        rgba_path: Path,
        json_path: Path,
        previous_manifest_path: Path | None = None,
    ) -> None:
        request: dict[str, Any] = {
            "clip_path": str(clip_path),
            "rgba_path": str(rgba_path),
            "json_path": str(json_path),
        }
        if previous_manifest_path is not None:
            request["previous_manifest_path"] = str(previous_manifest_path)
        self._send(request)

    def close(self) -> None:
        if self._process.poll() is not None:
            return
        try:
            self._send({"shutdown": True})
        finally:
            try:
                self._process.communicate(timeout=10)
            except subprocess.TimeoutExpired:
                self._process.kill()
                self._process.communicate()

    def _send(self, request: dict[str, Any]) -> None:
        if self._process.stdin is None or self._process.stdout is None:
            raise RuntimeError("render server pipes are unavailable")
        self._process.stdin.write(json.dumps(request) + "\n")
        self._process.stdin.flush()
        line = self._process.stdout.readline()
        if not line:
            stderr = self._process.stderr.read() if self._process.stderr else ""
            raise RuntimeError(f"render server exited without a response: {stderr}")
        response = json.loads(line)
        if not response.get("ok"):
            stderr = self._process.stderr.read() if self._process.stderr else ""
            raise RuntimeError(
                f"render server request failed: {response.get('error', '')}\n{stderr}"
            )


def find_mutation_candidates(manifest: dict[str, Any]) -> list[dict[str, Any]]:
    source_tiles: dict[tuple[str, int, int, int, int], dict[str, Any]] = {}
    for source in manifest.get("sources", []):
        for tile_index, tile in enumerate(source.get("tiles", [])):
            source_tiles[
                (
                    source.get("kind"),
                    int(source.get("layer_id")),
                    int(source.get("resource_id")),
                    int(tile.get("tile_x")),
                    int(tile.get("tile_y")),
                )
            ] = {"source": source, "tile": tile, "tile_index": tile_index}

    candidates: list[dict[str, Any]] = []
    for segment in manifest.get("segments", []):
        if segment.get("kind") != "RasterRun":
            continue
        for tile_ref in segment.get("tile_work_list", []):
            if tile_ref.get("kind") != "raster":
                continue
            key = (
                tile_ref.get("kind"),
                int(tile_ref.get("layer_id")),
                int(tile_ref.get("resource_id")),
                int(tile_ref.get("tile_x")),
                int(tile_ref.get("tile_y")),
            )
            match = source_tiles.get(key)
            if match is None:
                continue
            candidates.append(
                {
                    "segment_ordinal": segment.get("ordinal"),
                    "segment_kind": segment.get("kind"),
                    "layer_id": tile_ref.get("layer_id"),
                    "resource_id": tile_ref.get("resource_id"),
                    "tile_x": tile_ref.get("tile_x"),
                    "tile_y": tile_ref.get("tile_y"),
                    "source": match["source"],
                    "tile": match["tile"],
                    "tile_index": match["tile_index"],
                }
            )
    if candidates:
        return candidates

    for source in manifest.get("sources", []):
        if source.get("kind") != "raster":
            continue
        for tile_index, tile in enumerate(source.get("tiles", [])):
            candidates.append(
                {
                    "segment_ordinal": None,
                    "segment_kind": "fallback_source_scan",
                    "layer_id": source.get("layer_id"),
                    "resource_id": source.get("resource_id"),
                    "tile_x": tile.get("tile_x"),
                    "tile_y": tile.get("tile_y"),
                    "source": source,
                    "tile": tile,
                    "tile_index": tile_index,
                }
            )
    return candidates


def mutated_previous_manifest(
    manifest: dict[str, Any], iteration: int
) -> tuple[dict[str, Any], dict[str, Any]]:
    mutated = copy.deepcopy(manifest)
    candidates = find_mutation_candidates(mutated)
    if not candidates:
        raise ValueError("reload manifest has no compressed raster tile to mutate")
    target = candidates[(iteration - 1) % len(candidates)]
    tile = target["tile"]
    original_hash = int(tile["compressed_hash"])
    bit = 1 << ((iteration - 1) % 31)
    tile["compressed_hash"] = original_hash ^ bit
    mutation = {
        "segment_ordinal": target["segment_ordinal"],
        "segment_kind": target["segment_kind"],
        "layer_id": target["layer_id"],
        "resource_id": target["resource_id"],
        "tile_x": target["tile_x"],
        "tile_y": target["tile_y"],
        "original_hash": original_hash,
        "mutated_hash": tile["compressed_hash"],
    }
    return mutated, mutation


def summarize_metadata(
    sample: str,
    request_index: str,
    metadata: dict[str, Any],
    previous_checkpoint: dict[str, int] | None = None,
    mutation: dict[str, Any] | None = None,
) -> dict[str, Any]:
    reload_diff = metadata.get("reload_diff") or {}
    sparse = metadata.get("sparse_atlas_cache") or {}
    diagnostics = metadata.get("tile_cache_diagnostics") or {}
    checkpoint = (diagnostics.get("checkpoint_cache") or {}) if diagnostics else {}
    render_profile = metadata.get("render_profile") or {}
    task_graph = metadata.get("render_task_graph") or {}
    tasks = task_graph.get("tasks") or []

    checkpoint_delta = {}
    for key in ["hits", "misses", "stores", "evictions"]:
        current = int(checkpoint.get(key) or 0)
        previous = int((previous_checkpoint or {}).get(key) or 0)
        checkpoint_delta[key] = current - previous

    dirty_segments = reload_diff.get("dirty_segments") or []
    manifest_segments = (
        (reload_diff.get("manifest") or {}).get("segments") or []
        if reload_diff
        else []
    )
    dirty_segment_details = describe_dirty_segments(dirty_segments, manifest_segments)
    first_later_barrier = describe_first_later_barrier(
        dirty_segments, manifest_segments
    )
    dirty_event_ranges = sum(
        len(segment.get("dirty_event_ranges") or []) for segment in dirty_segments
    )
    region_fallback = any(
        task.get("kind") == "RegionFallback" and bool(task.get("executed"))
        for task in tasks
    )
    first_skipped_sparse_reason = first_skipped_sparse_task_reason(tasks)
    dominant_task = dominant_executed_task(tasks)
    sparse_upload_work = int(sparse.get("inserted_tiles") or 0) + int(
        sparse.get("changed_tiles") or 0
    )

    return {
        "sample": sample,
        "request": request_index,
        "mode": reload_diff.get("mode"),
        "patch_renderer": reload_diff.get("patch_renderer") or "full",
        "patch_renderer_fallback_reason": reload_diff.get(
            "patch_renderer_fallback_reason"
        ),
        "worker_ms": int(render_profile.get("worker_total_ms") or 0),
        "sparse_atlas_update_ms": int(
            render_profile.get("sparse_atlas_update_ms") or 0
        ),
        "legacy_barrier_segment_count": int(
            render_profile.get("legacy_barrier_segment_count") or 0
        ),
        "legacy_barrier_segment_ms": int(
            render_profile.get("legacy_barrier_segment_ms") or 0
        ),
        "tile_local_segment_count": int(
            render_profile.get("tile_local_segment_count") or 0
        ),
        "tile_local_segment_ms": int(render_profile.get("tile_local_segment_ms") or 0),
        "sparse_reused_tiles": int(sparse.get("reused_tiles") or 0),
        "sparse_inserted_tiles": int(sparse.get("inserted_tiles") or 0),
        "sparse_changed_tiles": int(sparse.get("changed_tiles") or 0),
        "sparse_evicted_tiles": int(sparse.get("evicted_tiles") or 0),
        "sparse_resident_bytes": int(sparse.get("cached_bytes") or 0),
        "checkpoint_hits_delta": checkpoint_delta["hits"],
        "checkpoint_misses_delta": checkpoint_delta["misses"],
        "checkpoint_stores_delta": checkpoint_delta["stores"],
        "checkpoint_evictions_delta": checkpoint_delta["evictions"],
        "checkpoint_cached_entries": int(checkpoint.get("cached_entries") or 0),
        "checkpoint_cached_bytes": int(checkpoint.get("cached_bytes") or 0),
        "dirty_segments": len(dirty_segments),
        "dirty_segment_details": dirty_segment_details,
        "first_later_barrier": first_later_barrier,
        "dirty_event_ranges": dirty_event_ranges,
        "readback_patch_bytes": int(reload_diff.get("payload_bytes") or 0),
        "region_fallback_executed": region_fallback,
        "first_skipped_sparse_task_reason": first_skipped_sparse_reason,
        "dominant_task": dominant_task,
        "sparse_upload_before_region_fallback": bool(
            region_fallback
            and (
                sparse_upload_work > 0
                or int(render_profile.get("sparse_atlas_update_ms") or 0) > 0
            )
        ),
        "mutation": mutation,
    }


def first_skipped_sparse_task_reason(tasks: list[dict[str, Any]]) -> str | None:
    sparse_task_kinds = {
        "DecodeTile",
        "UploadAtlasSlot",
        "BuildCheckpoint",
        "RunSegment",
    }
    for task in tasks:
        if task.get("kind") in sparse_task_kinds and not task.get("executed"):
            return task.get("skip_fallback_reason")
    return None


def dominant_executed_task(tasks: list[dict[str, Any]]) -> str | None:
    timed = [
        task
        for task in tasks
        if task.get("executed") and task.get("actual_ms") is not None
    ]
    if not timed:
        return None
    task = max(timed, key=lambda item: int(item.get("actual_ms") or 0))
    return f"{task.get('kind')}:{task.get('actual_ms')}ms"


def describe_dirty_segments(
    dirty_segments: list[dict[str, Any]], manifest_segments: list[dict[str, Any]]
) -> list[str]:
    by_ordinal = {
        int(segment.get("ordinal")): segment
        for segment in manifest_segments
        if segment.get("ordinal") is not None
    }
    descriptions = []
    for dirty in dirty_segments:
        if dirty.get("ordinal") is None:
            descriptions.append("unknown:unknown:unknown")
            continue
        ordinal = int(dirty.get("ordinal"))
        segment = by_ordinal.get(ordinal, {})
        reason = segment.get("barrier_reason") or "tile-local"
        descriptions.append(f"{ordinal}:{segment.get('kind')}:{reason}")
    return descriptions


def describe_first_later_barrier(
    dirty_segments: list[dict[str, Any]], manifest_segments: list[dict[str, Any]]
) -> str | None:
    if not dirty_segments:
        return None
    ordinals = [
        int(segment.get("ordinal"))
        for segment in dirty_segments
        if segment.get("ordinal") is not None
    ]
    if not ordinals:
        return None
    first_dirty = min(ordinals)
    later = [
        segment
        for segment in manifest_segments
        if int(segment.get("ordinal") or 0) > first_dirty
        and segment.get("barrier_reason")
    ]
    if not later:
        return None
    segment = later[0]
    return (
        f"{segment.get('ordinal')}:{segment.get('kind')}:"
        f"{segment.get('barrier_reason')}"
    )


def benchmark_sample(
    server: RenderServer,
    sample: str,
    clip_path: Path,
    temp_root: Path,
    iterations: int,
) -> dict[str, Any]:
    sample_dir = temp_root / sample
    sample_dir.mkdir(parents=True, exist_ok=True)
    baseline_json_path = sample_dir / "baseline.json"
    server.render(clip_path, sample_dir / "baseline.rgba", baseline_json_path)
    baseline_metadata = read_json(baseline_json_path)
    baseline_manifest = baseline_metadata["reload_diff"]["manifest"]
    rows = [summarize_metadata(sample, "baseline", baseline_metadata)]
    previous_checkpoint = checkpoint_snapshot(baseline_metadata)
    mutations = []

    for iteration in range(1, iterations + 1):
        old_manifest, mutation = mutated_previous_manifest(baseline_manifest, iteration)
        mutations.append(mutation)
        manifest_path = sample_dir / f"previous_manifest_{iteration}.json"
        manifest_path.write_text(json.dumps(old_manifest), encoding="utf-8")
        json_path = sample_dir / f"reload_{iteration}.json"
        server.render(
            clip_path,
            sample_dir / f"reload_{iteration}.rgba",
            json_path,
            manifest_path,
        )
        metadata = read_json(json_path)
        rows.append(
            summarize_metadata(
                sample,
                f"reload_{iteration}",
                metadata,
                previous_checkpoint=previous_checkpoint,
                mutation=mutation,
            )
        )
        previous_checkpoint = checkpoint_snapshot(metadata)

    return {
        "sample": sample,
        "clip_path": str(clip_path),
        "rows": rows,
        "mutations": mutations,
    }


def checkpoint_snapshot(metadata: dict[str, Any]) -> dict[str, int]:
    diagnostics = metadata.get("tile_cache_diagnostics") or {}
    checkpoint = diagnostics.get("checkpoint_cache") or {}
    return {
        "hits": int(checkpoint.get("hits") or 0),
        "misses": int(checkpoint.get("misses") or 0),
        "stores": int(checkpoint.get("stores") or 0),
        "evictions": int(checkpoint.get("evictions") or 0),
    }


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def print_markdown(results: list[dict[str, Any]]) -> None:
    for result in results:
        print(f"## {result['sample']}")
        print()
        print(
            "| request | mode | patch_renderer | worker_ms | sparse_update_ms | "
            "sparse reused/inserted/changed/evicted | resident MiB | "
            "checkpoint delta h/m/s/e | dirty segs | dirty ranges | "
            "readback bytes | RegionFallback | sparse upload before fallback | dominant task |"
        )
        print(
            "| --- | --- | --- | ---: | ---: | --- | ---: | --- | ---: | ---: | "
            "---: | --- | --- | --- |"
        )
        for row in result["rows"]:
            resident_mib = row["sparse_resident_bytes"] / (1024 * 1024)
            print(
                f"| {row['request']} | {row['mode']} | {row['patch_renderer']} | "
                f"{row['worker_ms']} | {row['sparse_atlas_update_ms']} | "
                f"{row['sparse_reused_tiles']}/{row['sparse_inserted_tiles']}/"
                f"{row['sparse_changed_tiles']}/{row['sparse_evicted_tiles']} | "
                f"{resident_mib:.1f} | "
                f"{row['checkpoint_hits_delta']}/{row['checkpoint_misses_delta']}/"
                f"{row['checkpoint_stores_delta']}/{row['checkpoint_evictions_delta']} | "
                f"{row['dirty_segments']} | {row['dirty_event_ranges']} | "
                f"{row['readback_patch_bytes']} | "
                f"{yes_no(row['region_fallback_executed'])} | "
                f"{yes_no(row['sparse_upload_before_region_fallback'])} | "
                f"{row['dominant_task'] or ''} |"
            )
        print()
        first_patch = next(
            (row for row in result["rows"] if row["request"] == "reload_1"),
            None,
        )
        if first_patch:
            reason = first_patch["first_skipped_sparse_task_reason"]
            fallback = first_patch["patch_renderer_fallback_reason"]
            print(f"- First skipped sparse task reason: `{reason or 'none'}`")
            print(f"- Patch fallback reason: `{fallback or 'none'}`")
            print(
                "- Dirty segment(s): "
                f"`{'; '.join(first_patch['dirty_segment_details']) or 'none'}`"
            )
            print(
                "- First later barrier: "
                f"`{first_patch['first_later_barrier'] or 'none'}`"
            )
            mutation = first_patch.get("mutation") or {}
            if mutation:
                print(
                    "- Mutated tile: "
                    f"`layer={mutation.get('layer_id')} "
                    f"resource={mutation.get('resource_id')} "
                    f"tile=({mutation.get('tile_x')},{mutation.get('tile_y')}) "
                    f"segment={mutation.get('segment_ordinal')}`"
                )
            print()


def yes_no(value: bool) -> str:
    return "yes" if value else "no"


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--clip-cli",
        type=Path,
        default=default_clip_cli(),
        help="Path to clip_cli executable, defaults to native/rust target release binary.",
    )
    parser.add_argument(
        "--fixture-root",
        type=Path,
        default=Path("img"),
        help="Directory containing .clip fixtures.",
    )
    parser.add_argument(
        "--samples",
        nargs="*",
        default=DEFAULT_SAMPLES,
        help="Sample base names without .clip extension.",
    )
    parser.add_argument("--iterations", type=int, default=5)
    parser.add_argument("--output-json", type=Path)
    parser.add_argument("--keep-temp", action="store_true")
    return parser.parse_args(argv)


def default_clip_cli() -> Path:
    exe = "clip_cli.exe" if os.name == "nt" else "clip_cli"
    return Path("native") / "rust" / "target" / "release" / exe


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    clip_cli = args.clip_cli.resolve()
    if not clip_cli.exists():
        print(f"clip_cli does not exist: {clip_cli}", file=sys.stderr)
        return 2
    fixture_root = args.fixture_root.resolve()
    env = os.environ.copy()
    env["RIZUM_CLIP_RENDER_PROFILE"] = "1"
    if args.keep_temp:
        temp_context = None
        temp_root = Path(tempfile.mkdtemp(prefix="rizum_reload_cache_benchmark_"))
    else:
        temp_context = tempfile.TemporaryDirectory(
            prefix="rizum_reload_cache_benchmark_"
        )
        temp_root = Path(temp_context.name)
    server = RenderServer(clip_cli, env)
    results: list[dict[str, Any]] = []
    try:
        for sample in args.samples:
            clip_path = fixture_root / f"{sample}.clip"
            if not clip_path.exists():
                results.append(
                    {
                        "sample": sample,
                        "clip_path": str(clip_path),
                        "missing": True,
                        "rows": [],
                    }
                )
                continue
            results.append(
                benchmark_sample(server, sample, clip_path, temp_root, args.iterations)
            )
    finally:
        server.close()
        if args.keep_temp:
            print(f"Temporary files kept at: {temp_root}", file=sys.stderr)
        else:
            assert temp_context is not None
            temp_context.cleanup()

    if args.output_json is not None:
        args.output_json.write_text(json.dumps(results, indent=2), encoding="utf-8")
    print_markdown([result for result in results if not result.get("missing")])
    for result in results:
        if result.get("missing"):
            print(f"## {result['sample']}\n\nMissing fixture: `{result['clip_path']}`\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
