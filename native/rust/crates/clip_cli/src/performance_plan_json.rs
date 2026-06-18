use clip_runtime::{ClipSession, NativePerformancePlanResult};
use serde_json::{Map, Value, json};

pub(crate) fn performance_plan_json(
    session: &ClipSession,
    result: &NativePerformancePlanResult,
) -> String {
    let summary = session.summary();
    let estimate = &result.tile_estimate;
    let stats = result.render_program_stats;
    let mut barrier_reasons = Map::new();
    for (reason, count) in stats.barrier_reasons.nonzero_counts() {
        barrier_reasons.insert(reason.as_str().to_string(), json!(count));
    }

    serde_json::to_string_pretty(&json!({
        "canvas": [summary.canvas.width, summary.canvas.height],
        "tile_size": estimate.tile_size,
        "sources": estimate.total_source_event_count,
        "planned_passes": stats.planned_passes,
        "tile_local_segments": stats.tile_local_segments,
        "barrier_segments": stats.barrier_segments,
        "barrier_reasons": Value::Object(barrier_reasons),
        "tile_event_abi_version": result.tile_event_abi_version,
        "compressed_raster_tiles": estimate.raster_compressed_tile_slot_count,
        "mask_tiles": estimate.mask_compressed_tile_slot_count,
        "atlas_upload_bytes": result.estimated_atlas_upload_bytes,
        "estimated_tile_events": result.estimated_tile_events,
        "planner": {
            "segments": stats.segments,
            "raster_run_segments": stats.raster_run_segments,
            "raster_clipping_run_segments": stats.raster_clipping_run_segments,
            "raster_filter_run_segments": stats.raster_filter_run_segments,
            "simple_container_scope_segments": stats.simple_container_scope_segments,
            "simple_through_scope_segments": stats.simple_through_scope_segments,
            "legacy_source_segments": stats.legacy_source_segments,
            "planned_tile_source_events": stats.planned_tile_events,
        },
        "source_counts": {
            "top_level": estimate.top_level_source_count,
            "total_events": estimate.total_source_event_count,
            "rasters": estimate.raster_source_count,
            "clipped_rasters": estimate.clipped_raster_source_count,
            "solids": estimate.solid_source_count,
            "clipping_runs": estimate.clipping_run_count,
            "container_clipping_runs": estimate.container_clipping_run_count,
            "containers": estimate.container_count,
            "clipped_containers": estimate.clipped_container_count,
            "through_groups": estimate.through_group_count,
            "lut_filters": estimate.lut_filter_count,
            "unsupported": estimate.unsupported_count,
        },
        "resources": {
            "unique_rasters": estimate.unique_raster_resource_count,
            "unique_masks": estimate.unique_mask_resource_count,
            "mask_references": estimate.mask_reference_count,
            "raster_tile_slots": estimate.raster_tile_slot_count,
            "mask_tile_slots": estimate.mask_tile_slot_count,
            "raster_empty_tiles": estimate.raster_empty_tile_slot_count,
            "mask_empty_tiles": estimate.mask_empty_tile_slot_count,
            "external_compressed_bytes": estimate.external_compressed_bytes,
        },
        "tile_occupancy": {
            "canvas_tiles_x": estimate.canvas_tiles_x,
            "canvas_tiles_y": estimate.canvas_tiles_y,
            "canvas_tile_count": estimate.canvas_tile_count,
            "active_raster_tiles": estimate.active_canvas_tile_count,
            "max_raster_events_per_tile": estimate.max_raster_events_per_tile,
            "mean_raster_events_per_active_tile": estimate.mean_raster_events_per_active_tile,
            "active_compressed_raster_tiles": estimate.active_compressed_canvas_tile_count,
            "max_compressed_raster_events_per_tile": estimate.max_compressed_raster_events_per_tile,
            "mean_compressed_raster_events_per_active_tile": estimate.mean_compressed_raster_events_per_active_tile,
        },
    }))
    .expect("performance plan JSON is serializable")
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::performance_plan_json;

    #[test]
    fn writes_performance_plan_as_json() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../img/Test_Clipping.clip");
        let session = clip_runtime::ClipSession::open(path).expect("open Test_Clipping.clip");
        let result = session
            .performance_plan(256)
            .expect("build performance plan");

        let report = performance_plan_json(&session, &result);
        let parsed: Value =
            serde_json::from_str(&report).expect("performance report should be valid JSON");

        assert_eq!(parsed["canvas"][0], 512);
        assert_eq!(parsed["tile_event_abi_version"], 5);
        assert!(parsed["planned_passes"].as_u64().unwrap() > 0);
        assert!(parsed["compressed_raster_tiles"].as_u64().unwrap() > 0);
        assert!(parsed["source_counts"]["rasters"].as_u64().unwrap() > 0);
    }
}
