use super::atlas_events::{SparseAtlasRasterEventSkipReason, sparse_atlas_raster_event_plan};
use super::atlas_events_test_support::*;

#[test]
fn raster_clipping_run_resident_slots_lower_to_clipping_batch() {
    let base = raster_source(10, 1, 0.75, clip_gpu::GpuRasterBlendMode::Multiply, None);
    let clipped = raster_source(11, 2, 1.0, clip_gpu::GpuRasterBlendMode::Normal, None);
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("RasterClippingRun")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("raster", 11, 2, 1, 12, 34),
        ]),
        &[clip_gpu::GpuNormalStackSource::ClippingRun {
            base,
            clipped: vec![clip_gpu::GpuClippedStackSource::Raster(clipped)],
        }],
    );

    assert!(plan.skipped_segments.is_empty());
    assert_eq!(plan.segments.len(), 1);
    assert_eq!(plan.segments[0].batches.len(), 1);
    let batch = &plan.segments[0].batches[0];
    assert_eq!(
        batch.kind,
        clip_gpu::GpuSparseAtlasRasterEventBatchKind::RasterClippingRun {
            base_event_count: 1,
            resolve_blend_mode: clip_gpu::GpuRasterBlendMode::Multiply,
        }
    );
    assert_eq!(batch.events.len(), 2);
    assert_eq!(batch.events[0].raster.atlas_x, 16);
    assert_eq!(batch.events[1].raster.atlas_x, 80);
    assert_eq!(batch.events[0].opacity, 0.75);
    assert_eq!(
        batch.events[1].blend_mode,
        clip_gpu::GpuRasterBlendMode::Normal
    );
}

#[test]
fn raster_clipping_run_with_mixed_atlas_keys_is_skipped() {
    let mut clipped_slot = slot("raster", 11, 2, 1, 12, 34);
    clipped_slot.slot.atlas_id = 4;
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("RasterClippingRun")),
        &reload_with_slots(vec![slot("raster", 10, 1, 0, 12, 34), clipped_slot]),
        &[clip_gpu::GpuNormalStackSource::ClippingRun {
            base: raster_source(10, 1, 1.0, clip_gpu::GpuRasterBlendMode::Normal, None),
            clipped: vec![clip_gpu::GpuClippedStackSource::Raster(raster_source(
                11,
                2,
                1.0,
                clip_gpu::GpuRasterBlendMode::Normal,
                None,
            ))],
        }],
    );

    assert!(plan.segments.is_empty());
    assert_eq!(
        plan.skipped_segments[0].reason,
        SparseAtlasRasterEventSkipReason::MixedSparseAtlasKeys
    );
}
