"""Benchmark persistent native reload cache behavior.

This script drives one `clip_cli --blender-render-server` process, renders a
baseline image, then performs deterministic patch reload requests by mutating a
single compressed-tile hash in the previous reload manifest. It is diagnostic:
it does not modify `.clip` files and does not change renderer semantics.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
from typing import Any


DEFAULT_SAMPLES = ["Test_Clipping", "Test_RealArt", "Ref_Terra404_Live2D"]


class RenderServer:
    def __init__(
        self, clip_cli: Path, env: dict[str, str], request_timeout_seconds: float
    ) -> None:
        self._request_timeout_seconds = request_timeout_seconds
        self._responses: queue.Queue[str | None] = queue.Queue()
        self._stderr_file = tempfile.TemporaryFile(mode="w+", encoding="utf-8")
        self._process = subprocess.Popen(
            [str(clip_cli), "--blender-render-server"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=self._stderr_file,
            text=True,
            encoding="utf-8",
            env=env,
        )
        self._reader = threading.Thread(target=self._read_stdout, daemon=True)
        self._reader.start()

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
            self._close_stderr()
            return
        try:
            self._send({"shutdown": True})
        finally:
            try:
                self._process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self._process.kill()
                self._process.wait()
            self._reader.join(timeout=1)
            self._close_stderr()

    def _send(self, request: dict[str, Any]) -> None:
        if self._process.stdin is None or self._process.stdout is None:
            raise RuntimeError("render server pipes are unavailable")
        self._process.stdin.write(json.dumps(request) + "\n")
        self._process.stdin.flush()
        try:
            line = self._responses.get(timeout=self._request_timeout_seconds)
        except queue.Empty as exc:
            self._process.kill()
            raise TimeoutError(
                f"render server request timed out after "
                f"{self._request_timeout_seconds:.0f}s: {request}\n"
                f"{self._stderr_tail()}"
            ) from exc
        if not line:
            raise RuntimeError(
                f"render server exited without a response: {self._stderr_tail()}"
            )
        response = json.loads(line)
        if not response.get("ok"):
            raise RuntimeError(
                f"render server request failed: {response.get('error', '')}\n"
                f"{self._stderr_tail()}"
            )

    def _read_stdout(self) -> None:
        if self._process.stdout is None:
            self._responses.put(None)
            return
        for line in self._process.stdout:
            self._responses.put(line)
        self._responses.put(None)

    def _stderr_tail(self) -> str:
        self._stderr_file.flush()
        self._stderr_file.seek(0)
        text = self._stderr_file.read()
        self._stderr_file.seek(0, os.SEEK_END)
        return text[-4000:]

    def _close_stderr(self) -> None:
        if not self._stderr_file.closed:
            self._stderr_file.close()


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
    rgba_path: Path | None = None,
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
    dirty_rects = reload_diff.get("dirty_rects") or reload_diff.get("rects") or []
    top_reload_segments = enrich_top_reload_segments(
        render_profile.get("top_segments") or [],
        dirty_rects,
        reload_diff.get("patch_renderer") or "full",
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
        "region_raster_resident_atlas_segment_count": int(
            render_profile.get("region_raster_resident_atlas_segment_count") or 0
        ),
        "region_raster_per_run_atlas_segment_count": int(
            render_profile.get("region_raster_per_run_atlas_segment_count") or 0
        ),
        "payload_sha256": payload_sha256(rgba_path),
        "first_skipped_sparse_task_reason": first_skipped_sparse_reason,
        "dominant_task": dominant_task,
        "top_reload_segments": top_reload_segments,
        "sparse_upload_before_region_fallback": bool(
            region_fallback
            and (
                sparse_upload_work > 0
                or int(render_profile.get("sparse_atlas_update_ms") or 0) > 0
            )
        ),
        "mutation": mutation,
    }


def enrich_top_reload_segments(
    segments: list[dict[str, Any]],
    dirty_rects: list[dict[str, Any]],
    patch_renderer: str,
) -> list[dict[str, Any]]:
    return [
        enrich_top_reload_segment(segment, dirty_rects, patch_renderer)
        for segment in segments[:10]
    ]


def enrich_top_reload_segment(
    segment: dict[str, Any],
    dirty_rects: list[dict[str, Any]],
    patch_renderer: str,
) -> dict[str, Any]:
    enriched = dict(segment)
    enriched["target_rect_exactly_dirty_rect"] = segment_target_matches_dirty_rect(
        segment, dirty_rects
    )
    enriched["reason_cannot_skip"] = cannot_skip_reason(segment, patch_renderer)
    return enriched


def segment_target_matches_dirty_rect(
    segment: dict[str, Any], dirty_rects: list[dict[str, Any]]
) -> bool:
    origin = segment.get("target_origin") or []
    size = segment.get("target_size") or []
    if len(origin) != 2 or len(size) != 2:
        return False
    target = {
        "x": int(origin[0]),
        "y": int(origin[1]),
        "width": int(size[0]),
        "height": int(size[1]),
    }
    return any(
        target["x"] == int(rect.get("x") or 0)
        and target["y"] == int(rect.get("y") or 0)
        and target["width"] == int(rect.get("width") or 0)
        and target["height"] == int(rect.get("height") or 0)
        for rect in dirty_rects
    )


def cannot_skip_reason(segment: dict[str, Any], patch_renderer: str) -> str:
    if patch_renderer == "region":
        if segment.get("kind") == "LegacySource":
            return "region fallback replays unsafe barrier because it overlaps the dirty rect"
        return "region fallback replays tile-local segment because it contributes to the dirty rect stack"
    if segment.get("event_sources_outside_target_rect"):
        return "segment has source events whose global bounds exceed the requested target"
    return "requested by selected patch renderer"


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
    rows = [
        summarize_metadata(
            sample,
            "baseline",
            baseline_metadata,
            rgba_path=sample_dir / "baseline.rgba",
        )
    ]
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
        rgba_path = sample_dir / f"reload_{iteration}.rgba"
        rows.append(
            summarize_metadata(
                sample,
                f"reload_{iteration}",
                metadata,
                rgba_path=rgba_path,
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


def benchmark_sample_ab(
    clip_cli: Path,
    env: dict[str, str],
    sample: str,
    clip_path: Path,
    temp_root: Path,
    iterations: int,
    request_timeout_seconds: float,
) -> dict[str, Any]:
    off_env = dict(env)
    off_env.pop("RIZUM_CLIP_REGION_RASTER_RESIDENT_ATLAS", None)
    on_env = dict(env)
    on_env["RIZUM_CLIP_REGION_RASTER_RESIDENT_ATLAS"] = "1"
    off_server = RenderServer(clip_cli, off_env, request_timeout_seconds)
    on_server = RenderServer(clip_cli, on_env, request_timeout_seconds)
    try:
        off = benchmark_sample(
            off_server, sample, clip_path, temp_root / f"{sample}_resident_off", iterations
        )
        on = benchmark_sample(
            on_server, sample, clip_path, temp_root / f"{sample}_resident_on", iterations
        )
    finally:
        off_server.close()
        on_server.close()
    return {
        "sample": sample,
        "clip_path": str(clip_path),
        "resident_atlas_ab": True,
        "off": off,
        "on": on,
        "comparisons": compare_ab_rows(off["rows"], on["rows"]),
    }


def compare_ab_rows(
    off_rows: list[dict[str, Any]], on_rows: list[dict[str, Any]]
) -> list[dict[str, Any]]:
    on_by_request = {row["request"]: row for row in on_rows}
    comparisons = []
    for off in off_rows:
        on = on_by_request.get(off["request"])
        if on is None:
            comparisons.append(
                {
                    "request": off["request"],
                    "patch_payload_equal": False,
                    "metadata_equal": False,
                    "reason": "missing on-row",
                }
            )
            continue
        payload_equal = (
            off.get("payload_sha256") is not None
            and off.get("payload_sha256") == on.get("payload_sha256")
        )
        comparisons.append(
            {
                "request": off["request"],
                "patch_payload_equal": payload_equal,
                "dirty_rect_metadata_equal": comparable_dirty_rect_metadata(off)
                == comparable_dirty_rect_metadata(on),
                "support_metadata_equal": comparable_support_metadata(off)
                == comparable_support_metadata(on),
                "reload_diff_fields_equal": comparable_reload_fields(off)
                == comparable_reload_fields(on),
                "off_payload_sha256": off.get("payload_sha256"),
                "on_payload_sha256": on.get("payload_sha256"),
                "off_resident_segments": off.get(
                    "region_raster_resident_atlas_segment_count"
                ),
                "on_resident_segments": on.get(
                    "region_raster_resident_atlas_segment_count"
                ),
                "off_per_run_segments": off.get(
                    "region_raster_per_run_atlas_segment_count"
                ),
                "on_per_run_segments": on.get(
                    "region_raster_per_run_atlas_segment_count"
                ),
            }
        )
    return comparisons


def comparable_dirty_rect_metadata(row: dict[str, Any]) -> tuple[Any, ...]:
    return (
        row.get("mode"),
        row.get("readback_patch_bytes"),
        row.get("dirty_segments"),
        row.get("dirty_event_ranges"),
        tuple(row.get("dirty_segment_details") or []),
    )


def comparable_support_metadata(row: dict[str, Any]) -> tuple[Any, ...]:
    return (
        row.get("legacy_barrier_segment_count"),
        row.get("tile_local_segment_count"),
    )


def comparable_reload_fields(row: dict[str, Any]) -> tuple[Any, ...]:
    return (
        row.get("patch_renderer"),
        row.get("patch_renderer_fallback_reason"),
        row.get("readback_patch_bytes"),
        row.get("region_fallback_executed"),
    )


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


def payload_sha256(path: Path | None) -> str | None:
    if path is None or not path.exists():
        return None
    return hashlib.sha256(path.read_bytes()).hexdigest()


def print_markdown(results: list[dict[str, Any]]) -> None:
    for result in results:
        if result.get("resident_atlas_ab"):
            print_ab_markdown(result)
            continue
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
        if any(
            row.get("region_raster_resident_atlas_segment_count")
            or row.get("region_raster_per_run_atlas_segment_count")
            for row in result["rows"]
        ):
            print()
            print(
                "| request | resident sparse RasterRun segs | per-run RasterRun segs |"
            )
            print("| --- | ---: | ---: |")
            for row in result["rows"]:
                print(
                    f"| {row['request']} | "
                    f"{row['region_raster_resident_atlas_segment_count']} | "
                    f"{row['region_raster_per_run_atlas_segment_count']} |"
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
            if first_patch.get("top_reload_segments"):
                print()
                print("| rank | kind | reason | first layer | ms | target dirty | rasters | child sources | masks | active tiles | max events/tile | atlas | queue/poll | outside target |")
                print("| ---: | --- | --- | ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: | --- | ---: | --- |")
                for segment in first_patch["top_reload_segments"]:
                    queue_poll = int(segment.get("queue_submit_ms") or 0) + int(
                        segment.get("queue_poll_ms") or 0
                    )
                    print(
                        f"| {segment.get('rank')} | {segment.get('kind')} | "
                        f"{segment.get('barrier_reason') or ''} | "
                        f"{segment.get('first_layer_id') or ''} | "
                        f"{segment.get('elapsed_ms')} | "
                        f"{yes_no(bool(segment.get('target_rect_exactly_dirty_rect')))} | "
                        f"{segment.get('raster_source_count') or 0} | "
                        f"{segment.get('child_source_count') or 0} | "
                        f"{segment.get('barrier_mask_count') or segment.get('mask_count') or 0} | "
                        f"{segment.get('active_canvas_tile_count') or 0} | "
                        f"{segment.get('max_events_per_dirty_tile') or 0} | "
                        f"{segment.get('atlas_upload_reuse_status') or ''} | "
                        f"{queue_poll} | "
                        f"{yes_no(bool(segment.get('event_sources_outside_target_rect')))} |"
                    )
            print()


def print_ab_markdown(result: dict[str, Any]) -> None:
    print(f"## {result['sample']} A/B resident sparse atlas")
    print()
    print("### Current region path")
    print_markdown([result["off"]])
    print("### Resident sparse atlas prototype")
    print_markdown([result["on"]])
    print("### Equality")
    print()
    print(
        "| request | payload equal | dirty rect metadata | support metadata | reload fields | resident segs off/on | per-run segs off/on |"
    )
    print("| --- | --- | --- | --- | --- | --- | --- |")
    for row in result["comparisons"]:
        print(
            f"| {row['request']} | "
            f"{yes_no(row.get('patch_payload_equal'))} | "
            f"{yes_no(row.get('dirty_rect_metadata_equal'))} | "
            f"{yes_no(row.get('support_metadata_equal'))} | "
            f"{yes_no(row.get('reload_diff_fields_equal'))} | "
            f"{row.get('off_resident_segments')}/{row.get('on_resident_segments')} | "
            f"{row.get('off_per_run_segments')}/{row.get('on_per_run_segments')} |"
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
    parser.add_argument("--request-timeout-seconds", type=float, default=600.0)
    parser.add_argument("--output-json", type=Path)
    parser.add_argument("--keep-temp", action="store_true")
    parser.add_argument(
        "--ab-region-resident-atlas",
        action="store_true",
        help=(
            "Run the current region path and RIZUM_CLIP_REGION_RASTER_RESIDENT_ATLAS=1 "
            "prototype in separate persistent servers and compare patch payload hashes."
        ),
    )
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
    results: list[dict[str, Any]] = []
    if args.ab_region_resident_atlas:
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
                benchmark_sample_ab(
                    clip_cli,
                    env,
                    sample,
                    clip_path,
                    temp_root,
                    args.iterations,
                    args.request_timeout_seconds,
                )
            )
    else:
        server = RenderServer(clip_cli, env, args.request_timeout_seconds)
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
