use clip_model::LayerId;

use super::atlas_events::{SparseAtlasRasterEventSkipReason, sparse_atlas_raster_event_plan};
use super::atlas_events_test_support::*;

#[test]
fn simple_container_scope_with_direct_rasters_lowers_to_scope_batch() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("raster", 11, 2, 1, 64, 34),
        ]),
        &[clip_gpu::GpuNormalStackSource::Container {
            children: vec![
                clip_gpu::GpuNormalStackSource::Raster(raster_source(
                    10,
                    1,
                    1.0,
                    clip_gpu::GpuRasterBlendMode::Normal,
                    None,
                )),
                clip_gpu::GpuNormalStackSource::Raster(raster_source(
                    11,
                    2,
                    0.5,
                    clip_gpu::GpuRasterBlendMode::Multiply,
                    None,
                )),
            ],
            opacity: 0.75,
            mask_key: None,
            blend_mode: clip_gpu::GpuRasterBlendMode::Screen,
        }],
    );

    assert!(plan.skipped_segments.is_empty());
    let batch = &plan.segments[0].batches[0];
    assert_eq!(
        batch.kind,
        clip_gpu::GpuSparseAtlasRasterEventBatchKind::SimpleContainerScope
    );
    assert_eq!(batch.events.len(), 2);
    let scope = batch.scope.expect("container scope payload");
    assert_eq!(
        scope.kind,
        clip_gpu::GpuSparseAtlasScopeEventKind::Container
    );
    assert_eq!(scope.opacity, 0.75);
    assert_eq!(scope.blend_mode, clip_gpu::GpuRasterBlendMode::Screen);
    assert_eq!(scope.local_bounds, clip_model::Rect::new(12, 34, 116, 32));
}

#[test]
fn simple_scope_with_scope_mask_is_not_lowered_yet() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![slot("raster", 10, 1, 0, 12, 34)]),
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
