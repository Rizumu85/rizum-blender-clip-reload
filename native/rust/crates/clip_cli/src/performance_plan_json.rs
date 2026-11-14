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
    let top_barrier_reasons = top_barrier_reasons(result);
    let product_render_path = if stats.barrier_segments == 0 {
        "tile_event_only"
    } else {
        "tile_event_with_legacy_barriers"
    };

    serde_json::to_string_pretty(&json!({
        "canvas": [summary.canvas.width, summary.canvas.height],
        "tile_size": estimate.tile_size,
        "sources": estimate.total_source_event_count,
        "planned_passes": stats.planned_passes,
        "tile_local_segments": stats.tile_local_segments,
        "barrier_segments": stats.barrier_segments,
        "barrier_reasons": Value::Object(barrier_reasons),
        "coverage": {
            "product_render_path": product_render_path,
            "legacy_segment_count": stats.legacy_source_segments,
            "legacy_segment_ratio": segment_ratio(stats.legacy_source_segments, stats.segments),
            "tile_local_segment_ratio": segment_ratio(stats.tile_local_segments, stats.segments),
            "top_barrier_reasons": top_barrier_reasons,
        },
        "fallback": {
            "legacy_segment_count": stats.legacy_source_segments,
            "legacy_barrier_count": stats.barrier_segments,
            "sparse_patch_attempted": false,
            "sparse_patch_succeeded": false,
            "sparse_patch_fallback_reason": "not_applicable_performance_plan_only",
        },
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
            "point_filter_run_segments": stats.point_filter_run_segments,
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

fn segment_ratio(count: u32, total: u32) -> f64 {
    if total == 0 {
        0.0
    } else {
        f64::from(count) / f64::from(total)
    }
}

fn top_barrier_reasons(result: &NativePerformancePlanResult) -> Vec<Value> {
    let mut counts = result.render_program_stats.barrier_reasons.nonzero_counts();
    counts.sort_by(|(left_reason, left_count), (right_reason, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_reason.as_str().cmp(right_reason.as_str()))
    });
    counts
        .into_iter()
        .take(5)
        .map(|(reason, count)| {
            json!({
                "reason": reason.as_str(),
                "count": count,
            })
        })
        .collect()
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
        assert_eq!(parsed["canvas"][1], 512);
        assert_eq!(parsed["tile_size"], 256);
        // The ABI version comes from the runtime constant carried by the plan result;
        // the literal number is a convergence-gate detail, not a JSON contract.
        assert_eq!(
            parsed["tile_event_abi_version"],
            Value::from(result.tile_event_abi_version)
        );

        // Coverage: the render path name is one of the two known routes and must agree
        // with the barrier segment count; the legacy counts and barrier reason list are
        // checked for shape only because the convergence gate is expected to move them.
        let barrier_segments = parsed["barrier_segments"].as_u64().unwrap();
        let product_render_path = parsed["coverage"]["product_render_path"].as_str().unwrap();
        if barrier_segments == 0 {
            assert_eq!(product_render_path, "tile_event_only");
        } else {
            assert_eq!(product_render_path, "tile_event_with_legacy_barriers");
        }
        let legacy_segment_count = parsed["coverage"]["legacy_segment_count"].as_u64().unwrap();
        assert_eq!(
            parsed["fallback"]["legacy_segment_count"].as_u64().unwrap(),
            legacy_segment_count
        );
        assert_eq!(
            parsed["planner"]["legacy_source_segments"]
                .as_u64()
                .unwrap(),
            legacy_segment_count
        );
        assert_eq!(
            parsed["fallback"]["legacy_barrier_count"].as_u64().unwrap(),
            barrier_segments
        );
        assert!(
            parsed["coverage"]["legacy_segment_ratio"]
                .as_f64()
                .is_some()
        );
        assert!(
            parsed["coverage"]["tile_local_segment_ratio"]
                .as_f64()
                .is_some()
        );
        assert_eq!(parsed["fallback"]["sparse_patch_attempted"], false);
        assert_eq!(parsed["fallback"]["sparse_patch_succeeded"], false);
        assert_eq!(
            parsed["fallback"]["sparse_patch_fallback_reason"],
            "not_applicable_performance_plan_only"
        );

        let barrier_reasons = parsed["barrier_reasons"]
            .as_object()
            .expect("barrier_reasons should be an object");
        let top_barrier_reasons = parsed["coverage"]["top_barrier_reasons"]
            .as_array()
            .expect("top_barrier_reasons should be an array");
        assert!(top_barrier_reasons.len() <= 5);
        assert_eq!(
            top_barrier_reasons.len(),
            barrier_reasons.len().min(5),
            "top_barrier_reasons should list the nonzero reasons"
        );
        for entry in top_barrier_reasons {
            let reason = entry["reason"]
                .as_str()
                .expect("top barrier reason should be a string");
            assert!(!reason.is_empty());
            let count = entry["count"]
                .as_u64()
                .expect("top barrier count should be an integer");
            assert!(count > 0);
            assert_eq!(barrier_reasons[reason], Value::from(count));
        }

        assert!(parsed["planned_passes"].as_u64().unwrap() > 0);
        assert!(parsed["tile_local_segments"].as_u64().is_some());
        assert!(
            parsed["planner"]["point_filter_run_segments"]
                .as_u64()
                .is_some()
        );
        assert!(parsed["planner"]["segments"].as_u64().unwrap() > 0);
        assert!(parsed["compressed_raster_tiles"].as_u64().unwrap() > 0);
        assert!(parsed["mask_tiles"].as_u64().is_some());
        assert!(parsed["atlas_upload_bytes"].as_u64().is_some());
        assert!(parsed["estimated_tile_events"].as_u64().is_some());
        assert!(parsed["source_counts"]["rasters"].as_u64().unwrap() > 0);
        assert!(parsed["resources"]["unique_rasters"].as_u64().unwrap() > 0);
        assert!(
            parsed["tile_occupancy"]["canvas_tile_count"]
                .as_u64()
                .unwrap()
                > 0
        );
    }
}
