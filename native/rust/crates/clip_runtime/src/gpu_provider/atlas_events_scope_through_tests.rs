use super::atlas_events::sparse_atlas_raster_event_plan;
use super::atlas_events_test_support::*;

#[test]
fn simple_through_scope_with_nested_through_clipping_run_lowers_ordered_tile_events() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleThroughScope")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("raster", 11, 2, 1, 12, 34),
        ]),
        &[clip_gpu::GpuNormalStackSource::ThroughGroup {
            children: vec![clip_gpu::GpuNormalStackSource::ThroughGroup {
                children: vec![clipping_run_source()],
                opacity: 1.0,
                mask_key: None,
            }],
            opacity: 1.0,
            mask_key: None,
        }],
    );

    assert!(plan.skipped_segments.is_empty());
    let batch = &plan.segments[0].batches[0];
    assert_eq!(batch.events.len(), 2);
    assert_eq!(batch.tile_events.len(), 6);
    assert!(matches!(
        batch.tile_events[0],
        clip_gpu::GpuSparseAtlasTileEvent::BeginScope(scope)
            if scope.kind == clip_gpu::GpuSparseAtlasScopeEventKind::Through
    ));
    assert!(matches!(
        batch.tile_events[1],
        clip_gpu::GpuSparseAtlasTileEvent::BeginClipBase(_)
    ));
    assert!(matches!(
        batch.tile_events[2],
        clip_gpu::GpuSparseAtlasTileEvent::ClipBaseRaster(_)
    ));
    assert!(matches!(
        batch.tile_events[3],
        clip_gpu::GpuSparseAtlasTileEvent::ClippedRaster(_)
    ));
    assert!(matches!(
        batch.tile_events[4],
        clip_gpu::GpuSparseAtlasTileEvent::ResolveClipBase(_)
    ));
    assert!(matches!(
        batch.tile_events[5],
        clip_gpu::GpuSparseAtlasTileEvent::EndScope(scope)
            if scope.kind == clip_gpu::GpuSparseAtlasScopeEventKind::Through
    ));
}

#[test]
fn simple_through_scope_with_nested_container_clipping_run_lowers_ordered_tile_events() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleThroughScope")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("raster", 11, 2, 1, 12, 34),
        ]),
        &[clip_gpu::GpuNormalStackSource::ThroughGroup {
            children: vec![clip_gpu::GpuNormalStackSource::Container {
                children: vec![clipping_run_source()],
                opacity: 1.0,
                mask_key: None,
                blend_mode: clip_gpu::GpuRasterBlendMode::Normal,
            }],
            opacity: 1.0,
            mask_key: None,
        }],
    );

    assert!(plan.skipped_segments.is_empty());
    let batch = &plan.segments[0].batches[0];
    assert_eq!(batch.events.len(), 2);
    assert_eq!(batch.tile_events.len(), 6);
    assert!(matches!(
        batch.tile_events[0],
        clip_gpu::GpuSparseAtlasTileEvent::BeginScope(scope)
            if scope.kind == clip_gpu::GpuSparseAtlasScopeEventKind::Container
    ));
    assert!(matches!(
        batch.tile_events[1],
        clip_gpu::GpuSparseAtlasTileEvent::BeginClipBase(_)
    ));
    assert!(matches!(
        batch.tile_events[2],
        clip_gpu::GpuSparseAtlasTileEvent::ClipBaseRaster(_)
    ));
    assert!(matches!(
        batch.tile_events[3],
        clip_gpu::GpuSparseAtlasTileEvent::ClippedRaster(_)
    ));
    assert!(matches!(
        batch.tile_events[4],
        clip_gpu::GpuSparseAtlasTileEvent::ResolveClipBase(_)
    ));
    assert!(matches!(
        batch.tile_events[5],
        clip_gpu::GpuSparseAtlasTileEvent::EndScope(scope)
            if scope.kind == clip_gpu::GpuSparseAtlasScopeEventKind::Container
    ));
}

fn clipping_run_source() -> clip_gpu::GpuNormalStackSource {
    clip_gpu::GpuNormalStackSource::ClippingRun {
        base: raster_source(10, 1, 1.0, clip_gpu::GpuRasterBlendMode::Normal, None),
        clipped: vec![clip_gpu::GpuClippedStackSource::Raster(raster_source(
            11,
            2,
            1.0,
            clip_gpu::GpuRasterBlendMode::Multiply,
            None,
        ))],
    }
}
