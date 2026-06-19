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

#[test]
fn simple_scope_with_nested_multi_slot_scope_mask_lowers_ordered_split_events() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("raster", 10, 1, 1, 76, 34),
            slot("mask", 99, 5, 0, 12, 34),
            slot("mask", 99, 5, 1, 76, 34),
        ]),
        &[clip_gpu::GpuNormalStackSource::Container {
            children: vec![clip_gpu::GpuNormalStackSource::Container {
                children: vec![clip_gpu::GpuNormalStackSource::Raster(raster_source(
                    10,
                    1,
                    1.0,
                    clip_gpu::GpuRasterBlendMode::Normal,
                    None,
                ))],
                opacity: 0.5,
                mask_key: Some(clip_gpu::GpuMaskResourceKey {
                    layer_id: LayerId(99),
                    mask_mipmap_id: 5,
                }),
                blend_mode: clip_gpu::GpuRasterBlendMode::Screen,
            }],
            opacity: 1.0,
            mask_key: None,
            blend_mode: clip_gpu::GpuRasterBlendMode::Normal,
        }],
    );

    assert!(plan.skipped_segments.is_empty());
    let batches = &plan.segments[0].batches;
    assert_eq!(batches.len(), 1);
    let batch = &batches[0];
    assert_eq!(
        batch.scope.expect("outer scope").local_bounds,
        clip_model::Rect::new(12, 34, 128, 32)
    );
    assert_eq!(batch.events.len(), 2);
    assert_eq!(batch.tile_events.len(), 6);

    let clip_gpu::GpuSparseAtlasTileEvent::BeginScope(first_scope) = batch.tile_events[0] else {
        panic!("expected first nested scope begin");
    };
    assert_eq!(
        first_scope.local_bounds,
        clip_model::Rect::new(12, 34, 64, 32)
    );
    assert_eq!(first_scope.opacity, 0.5);
    assert_eq!(first_scope.blend_mode, clip_gpu::GpuRasterBlendMode::Screen);
    assert_eq!(first_scope.mask.expect("first nested mask").atlas_x, 16);
    let clip_gpu::GpuSparseAtlasTileEvent::Raster(first_raster) = batch.tile_events[1] else {
        panic!("expected first clipped nested raster");
    };
    assert_eq!(first_raster.source_offset_x, 12);
    assert_eq!(first_raster.raster.size.width, 64);
    assert!(matches!(
        batch.tile_events[2],
        clip_gpu::GpuSparseAtlasTileEvent::EndScope(_)
    ));

    let clip_gpu::GpuSparseAtlasTileEvent::BeginScope(second_scope) = batch.tile_events[3] else {
        panic!("expected second nested scope begin");
    };
    assert_eq!(
        second_scope.local_bounds,
        clip_model::Rect::new(76, 34, 64, 32)
    );
    assert_eq!(second_scope.mask.expect("second nested mask").atlas_x, 80);
    let clip_gpu::GpuSparseAtlasTileEvent::Raster(second_raster) = batch.tile_events[4] else {
        panic!("expected second clipped nested raster");
    };
    assert_eq!(second_raster.source_offset_x, 76);
    assert_eq!(second_raster.raster.size.width, 64);
    assert!(matches!(
        batch.tile_events[5],
        clip_gpu::GpuSparseAtlasTileEvent::EndScope(_)
    ));
}

#[test]
fn simple_scope_with_nested_partial_multi_slot_scope_mask_is_not_lowered() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("raster", 10, 1, 1, 76, 34),
            slot("mask", 99, 5, 0, 12, 34),
        ]),
        &[clip_gpu::GpuNormalStackSource::Container {
            children: vec![clip_gpu::GpuNormalStackSource::Container {
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
            opacity: 1.0,
            mask_key: None,
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
