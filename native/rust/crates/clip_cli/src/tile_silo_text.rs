use clip_runtime::{ClipSession, NativeTileSiloEstimateResult};

pub(crate) fn tile_silo_estimate_text(
    session: &ClipSession,
    estimate: &NativeTileSiloEstimateResult,
) -> String {
    let summary = session.summary();
    let mut lines = Vec::new();
    lines.push(format!(
        "tile-silo estimate canvas={}x{} tile_size={} canvas_tiles={}x{} tile_count={}",
        summary.canvas.width,
        summary.canvas.height,
        estimate.tile_size,
        estimate.canvas_tiles_x,
        estimate.canvas_tiles_y,
        estimate.canvas_tile_count,
    ));
    lines.push(format!(
        "  sources top_level={} total_events={} unsupported={}",
        estimate.top_level_source_count,
        estimate.total_source_event_count,
        estimate.unsupported_count,
    ));
    lines.push(format!(
        "  source kinds rasters={} clipped_rasters={} solids={} clipping_runs={} container_clipping_runs={} containers={} clipped_containers={} through_groups={} lut_filters={}",
        estimate.raster_source_count,
        estimate.clipped_raster_source_count,
        estimate.solid_source_count,
        estimate.clipping_run_count,
        estimate.container_clipping_run_count,
        estimate.container_count,
        estimate.clipped_container_count,
        estimate.through_group_count,
        estimate.lut_filter_count,
    ));
    lines.push(format!(
        "  resources unique_rasters={} raster_tile_slots={} unique_masks={} mask_refs={} mask_tile_slots={}",
        estimate.unique_raster_resource_count,
        estimate.raster_tile_slot_count,
        estimate.unique_mask_resource_count,
        estimate.mask_reference_count,
        estimate.mask_tile_slot_count,
    ));
    lines.push(format!(
        "  external blocks raster_compressed_tiles={} raster_empty_tiles={} mask_compressed_tiles={} mask_empty_tiles={} compressed_bytes={}",
        estimate.raster_compressed_tile_slot_count,
        estimate.raster_empty_tile_slot_count,
        estimate.mask_compressed_tile_slot_count,
        estimate.mask_empty_tile_slot_count,
        estimate.external_compressed_bytes,
    ));
    lines.push(format!(
        "  tile events raster_tile_events={} solid_tile_events={} active_tiles={} max_raster_events_per_tile={} mean_raster_events_per_active_tile={:.2}",
        estimate.raster_tile_event_count,
        estimate.solid_tile_event_count,
        estimate.active_canvas_tile_count,
        estimate.max_raster_events_per_tile,
        estimate.mean_raster_events_per_active_tile,
    ));
    lines.push(format!(
        "  compressed tile events raster_tile_events={} active_tiles={} max_raster_events_per_tile={} mean_raster_events_per_active_tile={:.2}",
        estimate.compressed_raster_tile_event_count,
        estimate.active_compressed_canvas_tile_count,
        estimate.max_compressed_raster_events_per_tile,
        estimate.mean_compressed_raster_events_per_active_tile,
    ));
    lines.push(format!(
        "  collapse estimate collapsible_segments={} collapsible_source_events={} semantic_barriers={}",
        estimate.collapsible_segment_count,
        estimate.collapsible_source_event_count,
        estimate.semantic_barrier_count,
    ));
    lines.push(String::new());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use clip_runtime::ClipSession;

    use super::tile_silo_estimate_text;

    #[test]
    fn writes_tile_silo_estimate_text() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../img/Test_Clipping.clip");
        let session = ClipSession::open(path).expect("open Test_Clipping.clip");
        let estimate = session
            .estimate_tile_silo_plan(256)
            .expect("estimate tile-silo plan");

        let report = tile_silo_estimate_text(&session, &estimate);

        assert!(report.contains("tile-silo estimate canvas=512x512 tile_size=256"));
        assert!(report.contains("source kinds rasters=2"));
        assert!(report.contains("compressed tile events"));
        assert!(report.contains("collapse estimate"));
    }
}
