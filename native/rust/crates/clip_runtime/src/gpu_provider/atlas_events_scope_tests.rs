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
fn simple_scope_with_resident_scope_mask_lowers_to_scope_batch() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("mask", 99, 5, 1, 12, 34),
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
    let scope = plan.segments[0].batches[0]
        .scope
        .expect("container scope payload");
    let mask = scope.mask.expect("scope mask tile ref");
    assert_eq!(mask.key.format, clip_gpu::GpuSparseAtlasFormat::R8);
    assert_eq!(mask.key.atlas_id, 3);
    assert_eq!(mask.atlas_x, 80);
    assert_eq!(mask.atlas_y, 32);
    assert_eq!(mask.size, clip_model::CanvasSize::new(64, 32));
}

#[test]
fn simple_container_scope_with_point_filter_child_lowers_ordered_tile_events() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![slot("raster", 10, 1, 0, 12, 34)]),
        &[clip_gpu::GpuNormalStackSource::Container {
            children: vec![
                clip_gpu::GpuNormalStackSource::Raster(raster_source(
                    10,
                    1,
                    1.0,
                    clip_gpu::GpuRasterBlendMode::Normal,
                    None,
                )),
                clip_gpu::GpuNormalStackSource::LutFilter {
                    lut_rgba: invert_lut(),
                    opacity: 1.0,
                    mask_key: None,
                    filter_mode: clip_gpu::GpuLutFilterMode::ToneCurveRgb,
                },
            ],
            opacity: 1.0,
            mask_key: None,
            blend_mode: clip_gpu::GpuRasterBlendMode::Normal,
        }],
    );

    assert!(plan.skipped_segments.is_empty());
    let batch = &plan.segments[0].batches[0];
    assert_eq!(batch.events.len(), 1);
    assert_eq!(batch.filters.len(), 1);
    assert_eq!(batch.tile_events.len(), 2);
    assert!(matches!(
        batch.tile_events[0],
        clip_gpu::GpuSparseAtlasTileEvent::Raster(_)
    ));
    let clip_gpu::GpuSparseAtlasTileEvent::PointFilter(filter) = &batch.tile_events[1] else {
        panic!("expected point filter tile event");
    };
    assert_eq!(filter.local_bounds, clip_model::Rect::new(12, 34, 64, 32));
}

#[test]
fn simple_container_scope_with_masked_point_filter_child_splits_filter_events() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("raster", 10, 1, 1, 76, 34),
            slot("mask", 10, 9, 0, 12, 34),
            slot("mask", 10, 9, 1, 76, 34),
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
                clip_gpu::GpuNormalStackSource::LutFilter {
                    lut_rgba: invert_lut(),
                    opacity: 1.0,
                    mask_key: Some(clip_gpu::GpuMaskResourceKey {
                        layer_id: LayerId(10),
                        mask_mipmap_id: 9,
                    }),
                    filter_mode: clip_gpu::GpuLutFilterMode::ToneCurveRgb,
                },
            ],
            opacity: 1.0,
            mask_key: None,
            blend_mode: clip_gpu::GpuRasterBlendMode::Normal,
        }],
    );

    assert!(plan.skipped_segments.is_empty());
    let batch = &plan.segments[0].batches[0];
    assert_eq!(batch.events.len(), 2);
    assert_eq!(batch.filters.len(), 2);
    assert_eq!(batch.tile_events.len(), 4);
    assert_eq!(
        batch.filters[0].local_bounds,
        clip_model::Rect::new(12, 34, 64, 32)
    );
    assert_eq!(
        batch.filters[1].local_bounds,
        clip_model::Rect::new(76, 34, 64, 32)
    );
    assert_eq!(
        batch.filters[0].mask.expect("first filter mask").atlas_x,
        16
    );
    assert_eq!(
        batch.filters[1].mask.expect("second filter mask").atlas_x,
        80
    );
}

#[test]
fn simple_container_scope_with_nested_container_child_lowers_ordered_tile_events() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![slot("raster", 10, 1, 0, 12, 34)]),
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
                mask_key: None,
                blend_mode: clip_gpu::GpuRasterBlendMode::Screen,
            }],
            opacity: 1.0,
            mask_key: None,
            blend_mode: clip_gpu::GpuRasterBlendMode::Normal,
        }],
    );

    assert!(plan.skipped_segments.is_empty());
    let batch = &plan.segments[0].batches[0];
    assert_eq!(batch.events.len(), 1);
    assert_eq!(batch.tile_events.len(), 3);
    let clip_gpu::GpuSparseAtlasTileEvent::BeginScope(scope) = batch.tile_events[0] else {
        panic!("expected nested scope begin");
    };
    assert_eq!(
        scope.kind,
        clip_gpu::GpuSparseAtlasScopeEventKind::Container
    );
    assert_eq!(scope.opacity, 0.5);
    assert_eq!(scope.blend_mode, clip_gpu::GpuRasterBlendMode::Screen);
    assert_eq!(scope.local_bounds, clip_model::Rect::new(12, 34, 64, 32));
    assert!(matches!(
        batch.tile_events[1],
        clip_gpu::GpuSparseAtlasTileEvent::Raster(_)
    ));
    assert!(matches!(
        batch.tile_events[2],
        clip_gpu::GpuSparseAtlasTileEvent::EndScope(_)
    ));
}

#[test]
fn simple_container_scope_with_nested_clipping_run_is_not_lowered() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("raster", 11, 2, 1, 12, 34),
        ]),
        &[clip_gpu::GpuNormalStackSource::Container {
            children: vec![clip_gpu::GpuNormalStackSource::Container {
                children: vec![clip_gpu::GpuNormalStackSource::ClippingRun {
                    base: raster_source(10, 1, 1.0, clip_gpu::GpuRasterBlendMode::Normal, None),
                    clipped: vec![clip_gpu::GpuClippedStackSource::Raster(raster_source(
                        11,
                        2,
                        1.0,
                        clip_gpu::GpuRasterBlendMode::Multiply,
                        None,
                    ))],
                }],
                opacity: 1.0,
                mask_key: None,
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
        SparseAtlasRasterEventSkipReason::NonRasterRun
    );
}

#[test]
fn simple_container_scope_with_too_deep_nested_scope_is_not_lowered() {
    let leaf = clip_gpu::GpuNormalStackSource::Raster(raster_source(
        10,
        1,
        1.0,
        clip_gpu::GpuRasterBlendMode::Normal,
        None,
    ));
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![slot("raster", 10, 1, 0, 12, 34)]),
        &[clip_gpu::GpuNormalStackSource::Container {
            children: vec![nested_container(vec![nested_container(vec![
                nested_container(vec![nested_container(vec![leaf])]),
            ])])],
            opacity: 1.0,
            mask_key: None,
            blend_mode: clip_gpu::GpuRasterBlendMode::Normal,
        }],
    );

    assert!(plan.segments.is_empty());
    assert_eq!(
        plan.skipped_segments[0].reason,
        SparseAtlasRasterEventSkipReason::ScopeDepthLimitExceeded
    );
}

#[test]
fn simple_container_scope_without_resident_filter_mask_is_not_lowered() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![slot("raster", 10, 1, 0, 12, 34)]),
        &[clip_gpu::GpuNormalStackSource::Container {
            children: vec![
                clip_gpu::GpuNormalStackSource::Raster(raster_source(
                    10,
                    1,
                    1.0,
                    clip_gpu::GpuRasterBlendMode::Normal,
                    None,
                )),
                clip_gpu::GpuNormalStackSource::LutFilter {
                    lut_rgba: invert_lut(),
                    opacity: 1.0,
                    mask_key: Some(clip_gpu::GpuMaskResourceKey {
                        layer_id: LayerId(10),
                        mask_mipmap_id: 9,
                    }),
                    filter_mode: clip_gpu::GpuLutFilterMode::ToneCurveRgb,
                },
            ],
            opacity: 1.0,
            mask_key: None,
            blend_mode: clip_gpu::GpuRasterBlendMode::Normal,
        }],
    );

    assert!(plan.segments.is_empty());
    assert_eq!(
        plan.skipped_segments[0].reason,
        SparseAtlasRasterEventSkipReason::FilterMaskNotLowered {
            layer_id: 10,
            resource_id: 9,
        }
    );
}

#[test]
fn simple_container_scope_with_raster_clipping_run_lowers_ordered_tile_events() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("raster", 11, 2, 1, 12, 34),
        ]),
        &[clip_gpu::GpuNormalStackSource::Container {
            children: vec![clip_gpu::GpuNormalStackSource::ClippingRun {
                base: raster_source(10, 1, 1.0, clip_gpu::GpuRasterBlendMode::Normal, None),
                clipped: vec![clip_gpu::GpuClippedStackSource::Raster(raster_source(
                    11,
                    2,
                    1.0,
                    clip_gpu::GpuRasterBlendMode::Multiply,
                    None,
                ))],
            }],
            opacity: 1.0,
            mask_key: None,
            blend_mode: clip_gpu::GpuRasterBlendMode::Normal,
        }],
    );

    assert!(plan.skipped_segments.is_empty());
    let batch = &plan.segments[0].batches[0];
    assert_eq!(batch.events.len(), 2);
    assert_eq!(batch.tile_events.len(), 4);
    assert!(matches!(
        batch.tile_events[0],
        clip_gpu::GpuSparseAtlasTileEvent::BeginClipBase(_)
    ));
    assert!(matches!(
        batch.tile_events[1],
        clip_gpu::GpuSparseAtlasTileEvent::ClipBaseRaster(_)
    ));
    assert!(matches!(
        batch.tile_events[2],
        clip_gpu::GpuSparseAtlasTileEvent::ClippedRaster(_)
    ));
    assert!(matches!(
        batch.tile_events[3],
        clip_gpu::GpuSparseAtlasTileEvent::ResolveClipBase(_)
    ));
}

#[test]
fn simple_scope_without_resident_scope_mask_is_not_lowered() {
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

fn invert_lut() -> Vec<u8> {
    let mut lut = Vec::with_capacity(256 * 4);
    for value in 0u8..=255 {
        lut.extend_from_slice(&[255 - value, 255 - value, 255 - value, 255]);
    }
    lut
}

fn nested_container(
    children: Vec<clip_gpu::GpuNormalStackSource>,
) -> clip_gpu::GpuNormalStackSource {
    clip_gpu::GpuNormalStackSource::Container {
        children,
        opacity: 1.0,
        mask_key: None,
        blend_mode: clip_gpu::GpuRasterBlendMode::Normal,
    }
}
