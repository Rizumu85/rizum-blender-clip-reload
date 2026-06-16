use super::ClipSession;

#[test]
fn estimates_clipping_sample_without_rendering() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
    let session = ClipSession::open(path).expect("open Test_Clipping.clip");

    let estimate = session
        .estimate_tile_silo_plan(256)
        .expect("estimate tile-silo plan");

    assert_eq!(estimate.canvas_tiles_x, 2);
    assert_eq!(estimate.canvas_tiles_y, 2);
    assert_eq!(estimate.canvas_tile_count, 4);
    assert_eq!(estimate.unsupported_count, 0);
    assert_eq!(estimate.raster_source_count, 2);
    assert_eq!(estimate.clipping_run_count, 1);
    assert!(estimate.raster_tile_event_count >= 4);
    assert!(estimate.compressed_raster_tile_event_count > 0);
    assert!(estimate.compressed_raster_tile_event_count <= estimate.raster_tile_event_count);
    assert!(estimate.active_compressed_canvas_tile_count > 0);
    assert!(estimate.active_compressed_canvas_tile_count <= estimate.active_canvas_tile_count);
    assert!(estimate.collapsible_segment_count >= 1);
}

#[test]
fn rejects_zero_tile_size() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
    let session = ClipSession::open(path).expect("open Test_Clipping.clip");

    let err = session
        .estimate_tile_silo_plan(0)
        .expect_err("zero tile size should fail");

    assert_eq!(err.to_string(), "tile size must be greater than zero");
}
