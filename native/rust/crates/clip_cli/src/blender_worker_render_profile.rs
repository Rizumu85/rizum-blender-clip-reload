use clip_runtime::{
    GpuCheckpointCacheStats, GpuSparseAtlasReloadPlan, ReloadDiffPlan, ReloadDirtySegment,
};
use serde_json::json;

pub(crate) fn render_profile_metadata(worker_total_ms: u64) -> Option<serde_json::Value> {
    clip_runtime::render_profile::snapshot_if_enabled().map(|profile| {
        json!({
            "source_selection_ms": profile.source_selection_ms,
            "gpu_device_init_ms": profile.gpu_device_init_ms,
            "render_program_planning_ms": profile.render_program_planning_ms,
            "event_payload_build_ms": profile.event_payload_build_ms,
            "gpu_pass_encode_ms": profile.gpu_pass_encode_ms,
            "queue_submit_ms": profile.queue_submit_ms,
            "queue_poll_ms": profile.queue_poll_ms,
            "queue_submit_poll_ms": profile.queue_submit_ms.saturating_add(profile.queue_poll_ms),
            "gpu_execution_time_ms": null,
            "gpu_execution_time_source": "cpu_wait_proxy",
            "gpu_execution_wait_proxy_ms": profile.gpu_execution_wait_proxy_ms,
            "readback_copy_ms": profile.readback_copy_ms,
            "readback_cpu_copy_ms": profile.readback_cpu_copy_ms,
            "patch_payload_extraction_ms": profile.patch_payload_extraction_ms,
            "checkpoint_reconstruction_ms": profile.checkpoint_reconstruction_ms,
            "sparse_atlas_update_ms": profile.sparse_atlas_update_ms,
            "legacy_barrier_segment_count": profile.legacy_barrier_segment_count,
            "legacy_barrier_segment_ms": profile.legacy_barrier_segment_ms,
            "legacy_fallback_segment_count": profile.legacy_fallback_segment_count,
            "legacy_fallback_segment_ms": profile.legacy_fallback_segment_ms,
            "tile_local_segment_count": profile.tile_local_segment_count,
            "tile_local_segment_ms": profile.tile_local_segment_ms,
            "streaming_pass_count": profile.streaming_pass_count,
            "queue_submit_count": profile.queue_submit_count,
            "readback_count": profile.readback_count,
            "checkpoint_cache_hits": profile.checkpoint_cache_hits,
            "checkpoint_cache_misses": profile.checkpoint_cache_misses,
            "checkpoint_cache_stores": profile.checkpoint_cache_stores,
            "checkpoint_cache_evictions": profile.checkpoint_cache_evictions,
            "checkpoint_cache_skipped_over_budget": profile.checkpoint_cache_skipped_over_budget,
            "region_raster_resident_atlas_segment_count":
                profile.region_raster_resident_atlas_segment_count,
            "region_raster_per_run_atlas_segment_count":
                profile.region_raster_per_run_atlas_segment_count,
            "top_segments": top_segments_json(&profile.top_segments),
            "worker_total_ms": worker_total_ms,
        })
    })
}

fn top_segments_json(
    segments: &[clip_runtime::render_profile::RenderProfileSegment],
) -> Vec<serde_json::Value> {
    segments
        .iter()
        .enumerate()
        .map(|(rank, segment)| {
            let mut object = serde_json::Map::new();
            macro_rules! insert {
                ($key:literal, $value:expr) => {
                    object.insert($key.to_string(), json!($value));
                };
            }
            let atlas_status = if segment.uses_sparse_resident_atlas {
                "resident_sparse_atlas"
            } else if segment.uses_per_run_atlas {
                "per_run_atlas"
            } else {
                "legacy_or_not_applicable"
            };
            insert!("rank", rank + 1);
            insert!("ordinal", segment.ordinal);
            insert!("kind", segment.kind);
            insert!("source_shape", segment.source_shape);
            insert!("barrier_reason", segment.barrier_reason);
            insert!("elapsed_us", segment.elapsed_us);
            insert!("elapsed_ms", segment.elapsed_ms);
            insert!("cpu_encode_ms", segment.elapsed_ms);
            insert!("gpu_pass_encode_ms", segment.gpu_pass_encode_ms);
            insert!("queue_submit_ms", segment.queue_submit_ms);
            insert!("queue_poll_ms", segment.queue_poll_ms);
            insert!(
                "queue_submit_poll_ms",
                segment
                    .queue_submit_ms
                    .saturating_add(segment.queue_poll_ms)
            );
            insert!("readback_copy_ms", segment.readback_copy_ms);
            insert!("readback_cpu_copy_ms", segment.readback_cpu_copy_ms);
            insert!(
                "readback_contribution_ms",
                segment
                    .readback_copy_ms
                    .saturating_add(segment.readback_cpu_copy_ms)
            );
            insert!("streaming_pass_count", segment.streaming_pass_count);
            insert!("queue_submit_count", segment.queue_submit_count);
            insert!("readback_count", segment.readback_count);
            insert!("source_start", segment.source_start);
            insert!("source_end", segment.source_end);
            insert!("first_layer_id", segment.first_layer_id);
            insert!(
                "target_origin",
                [segment.target_origin_x, segment.target_origin_y]
            );
            insert!("target_size", [segment.target_width, segment.target_height]);
            insert!("expected_passes", segment.expected_passes);
            insert!("tile_events", segment.tile_events);
            insert!("legacy_sources", segment.legacy_sources);
            insert!("raster_source_count", segment.raster_source_count);
            insert!(
                "compressed_tile_event_count",
                segment.compressed_tile_event_count
            );
            insert!(
                "compressed_tile_event_count_basis",
                "visible source-tile intersections in the requested target"
            );
            insert!("active_canvas_tile_count", segment.active_canvas_tile_count);
            insert!(
                "max_events_per_dirty_tile",
                segment.max_events_per_dirty_tile
            );
            insert!(
                "event_sources_outside_target_rect",
                segment.event_sources_outside_target_rect
            );
            insert!("atlas_upload_reuse_status", atlas_status);
            insert!("gpu_passes", segment.expected_passes);
            insert!("gpu_batches", segment.gpu_batches);
            insert!(
                "uses_sparse_resident_atlas",
                segment.uses_sparse_resident_atlas
            );
            insert!("uses_per_run_atlas", segment.uses_per_run_atlas);
            insert!("target_rect_mode", "requested_region");
            object.insert(
                "cache_resource_reuse_count".to_string(),
                serde_json::Value::Null,
            );
            insert!("cache_resource_reuse_status", "not_measured_per_segment");
            insert!("child_source_count", segment.child_source_count);
            insert!(
                "nested_container_through_count",
                segment.nested_container_through_count
            );
            insert!("barrier_raster_count", segment.barrier_raster_count);
            insert!("barrier_mask_count", segment.barrier_mask_count);
            insert!("legacy_intermediate_cache_rect", "not_measured");
            insert!(
                "legacy_source_bounds_exceed_target_rect",
                segment.event_sources_outside_target_rect
            );
            insert!("legacy_internal_region_mode", "not_measured");
            insert!(
                "legacy_subpass_count_estimate",
                segment.legacy_subpass_count_estimate
            );
            insert!(
                "legacy_direct_subpass_count",
                segment.legacy_direct_subpass_count
            );
            insert!(
                "largest_legacy_direct_subpass_ms",
                segment.largest_legacy_direct_subpass_us.saturating_add(999) / 1000
            );
            object.insert(
                "largest_child_subpass_ms".to_string(),
                serde_json::Value::Null,
            );
            serde_json::Value::Object(object)
        })
        .collect()
}

pub(crate) fn tile_cache_diagnostics(
    plan: &ReloadDiffPlan,
    sparse_atlas: &GpuSparseAtlasReloadPlan,
    checkpoint_cache: GpuCheckpointCacheStats,
) -> Option<serde_json::Value> {
    clip_runtime::render_profile::enabled().then(|| {
        let dirty_source_tile_count = dirty_source_tile_count(&plan.dirty_segments);
        json!({
            "sparse_atlas": {
                "acquired_tiles": sparse_atlas.cache_stats.reused_tiles
                    .saturating_add(sparse_atlas.cache_stats.inserted_tiles)
                    .saturating_add(sparse_atlas.cache_stats.changed_tiles),
                "reused_tiles": sparse_atlas.cache_stats.reused_tiles,
                "changed_tiles": sparse_atlas.cache_stats.changed_tiles,
                "evicted_tiles": sparse_atlas.cache_stats.evicted_tiles,
                "uploaded_tiles": sparse_atlas.cache_stats.inserted_tiles
                    .saturating_add(sparse_atlas.cache_stats.changed_tiles),
                "uploaded_bytes": sparse_atlas.cache_stats.upload_bytes,
                "resident_tiles": sparse_atlas.cache_stats.cached_tiles,
                "resident_tile_memory_bytes": sparse_atlas.cache_stats.cached_bytes,
                "resident_atlas_count": sparse_atlas.cache_stats.atlas_count,
            },
            "dirty_source_tile_count": dirty_source_tile_count,
            "dirty_output_tile_count": dirty_output_tile_count(plan),
            "rendered_but_unchanged_output_tile_signatures": null,
            "rendered_but_unchanged_output_tile_signature_reason":
                "not_measured_without_previous_output_tile_signatures",
            "checkpoint_cache": {
                "hits": checkpoint_cache.hits,
                "misses": checkpoint_cache.misses,
                "stores": checkpoint_cache.stores,
                "evictions": checkpoint_cache.evictions,
                "skipped_stores": checkpoint_cache.skipped_stores,
                "cached_entries": checkpoint_cache.cached_entries,
                "cached_bytes": checkpoint_cache.cached_bytes,
            },
        })
    })
}

fn dirty_source_tile_count(segments: &[ReloadDirtySegment]) -> u64 {
    segments
        .iter()
        .map(|segment| u64::from(segment.dirty_tile_count))
        .sum()
}

fn dirty_output_tile_count(plan: &ReloadDiffPlan) -> u64 {
    const TILE_SIZE: u32 = clip_file::tiles::TILE_SIZE as u32;
    plan.dirty_rects
        .iter()
        .map(|rect| {
            let x0 = rect.x / TILE_SIZE;
            let y0 = rect.y / TILE_SIZE;
            let x1 = rect.x.saturating_add(rect.width).saturating_sub(1) / TILE_SIZE;
            let y1 = rect.y.saturating_add(rect.height).saturating_sub(1) / TILE_SIZE;
            u64::from(x1.saturating_sub(x0).saturating_add(1))
                .saturating_mul(u64::from(y1.saturating_sub(y0).saturating_add(1)))
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use clip_runtime::{
        ReloadDiffManifest, ReloadDiffMode, ReloadDiffPlan, ReloadDiffSegment, ReloadPatchRect,
    };

    use super::dirty_output_tile_count;

    #[test]
    fn dirty_output_tile_count_counts_intersected_tiles() {
        let plan = ReloadDiffPlan {
            manifest: ReloadDiffManifest {
                abi: 4,
                tile_size: clip_file::tiles::TILE_SIZE as u32,
                tile_event_abi_version: 0,
                width: 512,
                height: 512,
                root_layer_id: 1,
                nodes: Vec::new(),
                sources: Vec::new(),
                segments: Vec::<ReloadDiffSegment>::new(),
            },
            mode: ReloadDiffMode::Patch,
            reason: "test".to_string(),
            dirty_rects: vec![ReloadPatchRect {
                x: 250,
                y: 250,
                width: 20,
                height: 20,
            }],
            dirty_segments: Vec::new(),
        };

        assert_eq!(dirty_output_tile_count(&plan), 4);
    }
}
