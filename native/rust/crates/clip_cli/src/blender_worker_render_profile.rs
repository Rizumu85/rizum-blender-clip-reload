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
            "worker_total_ms": worker_total_ms,
        })
    })
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
