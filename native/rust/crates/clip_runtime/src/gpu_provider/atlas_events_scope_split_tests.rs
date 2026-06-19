use clip_model::LayerId;

use super::atlas_events::{SparseAtlasRasterEventSkipReason, sparse_atlas_raster_event_plan};
use super::atlas_events_test_support::*;

#[test]
fn simple_scope_with_multi_slot_scope_mask_lowers_to_clipped_scope_batches() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("raster", 10, 1, 1, 76, 34),
            slot("mask", 99, 5, 0, 12, 34),
            slot("mask", 99, 5, 1, 76, 34),
        ]),
        &[clip_gpu::GpuNormalStackSource::Container {
            children: vec![clip_gpu::GpuNormalStackSource::Raster(raster_source(
                10,
                1,
                1.0,
                clip_gpu::GpuRasterBlendMode::Normal,
                None,
            ))],
            opacity: 1.0,
            mask_key: Some(clip_gpu::GpuMaskResourceKey {
                layer_id: LayerId(99),
                mask_mipmap_id: 5,
            }),
            blend_mode: clip_gpu::GpuRasterBlendMode::Normal,
        }],
    );

    assert!(plan.skipped_segments.is_empty());
    let batches = &plan.segments[0].batches;
    assert_eq!(batches.len(), 2);
    assert_eq!(
        batches[0].scope.expect("first scope").local_bounds,
        clip_model::Rect::new(12, 34, 64, 32)
    );
    assert_eq!(
        batches[1].scope.expect("second scope").local_bounds,
        clip_model::Rect::new(76, 34, 64, 32)
    );
    assert_eq!(batches[0].events[0].source_offset_x, 12);
    assert_eq!(batches[0].events[0].raster.size.width, 64);
    assert_eq!(batches[1].events[0].source_offset_x, 76);
    assert_eq!(batches[1].events[0].raster.size.width, 64);
    assert_eq!(
        batches[0]
            .scope
            .expect("first scope")
            .mask
            .expect("first mask")
            .atlas_x,
        16
    );
    assert_eq!(
        batches[1]
            .scope
            .expect("second scope")
            .mask
            .expect("second mask")
            .atlas_x,
        80
    );
}

#[test]
fn simple_scope_with_partial_multi_slot_scope_mask_is_not_lowered() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("raster", 10, 1, 1, 76, 34),
            slot("mask", 99, 5, 0, 12, 34),
        ]),
        &[clip_gpu::GpuNormalStackSource::Container {
            children: vec![clip_gpu::GpuNormalStackSource::Raster(raster_source(
                10,
                1,
                1.0,
                clip_gpu::GpuRasterBlendMode::Normal,
                None,
            ))],
            opacity: 1.0,
            mask_key: Some(clip_gpu::GpuMaskResourceKey {
                layer_id: LayerId(99),
                mask_mipmap_id: 5,
            }),
            blend_mode: clip_gpu::GpuRasterBlendMode::Normal,
        }],
    );

    assert!(plan.segments.is_empty());
    assert_eq!(
        plan.skipped_segments[0].reason,
        SparseAtlasRasterEventSkipReason::ScopeMaskNotLowered {
            layer_id: 99,
            resource_id: 5,
        }
    );
}
