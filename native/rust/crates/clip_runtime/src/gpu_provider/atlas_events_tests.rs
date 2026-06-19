use clip_model::CanvasSize;

use super::atlas_events::{
    SparseAtlasRasterEventSkipReason, sparse_atlas_raster_affected_event_plan,
    sparse_atlas_raster_event_plan,
};
use super::atlas_events_test_support::*;
use crate::reload_diff::{ReloadDiffSegment, ReloadDirtySegmentEventRange};

#[test]
fn raster_run_resident_slots_lower_to_sparse_atlas_events() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("RasterRun")),
        &reload_with_slots(vec![slot("raster", 10, 1, 0, 12, 34)]),
        &[clip_gpu::GpuNormalStackSource::Raster(raster_source(
            10,
            1,
            0.5,
            clip_gpu::GpuRasterBlendMode::Multiply,
            None,
        ))],
    );

    assert!(plan.skipped_segments.is_empty());
    assert_eq!(plan.segments.len(), 1);
    let segment = &plan.segments[0];
    assert_eq!(segment.ordinal, 7);
    assert_eq!(
        segment.event_ranges,
        vec![ReloadDirtySegmentEventRange { start: 2, end: 3 }]
    );
    assert_eq!(segment.batches.len(), 1);
    assert_eq!(segment.batches[0].events.len(), 1);
    let event = segment.batches[0].events[0];
    assert_eq!(
        event.raster.key.format,
        clip_gpu::GpuSparseAtlasFormat::Rgba8
    );
    assert_eq!(event.raster.key.atlas_id, 3);
    assert_eq!(event.raster.atlas_x, 16);
    assert_eq!(event.raster.atlas_y, 32);
    assert_eq!(event.raster.size, CanvasSize::new(64, 32));
    assert_eq!(event.source_offset_x, 12);
    assert_eq!(event.source_offset_y, 34);
    assert_eq!(event.opacity, 0.5);
    assert_eq!(event.blend_mode, clip_gpu::GpuRasterBlendMode::Multiply);
    assert_eq!(event.mask, None);
}

#[test]
fn masked_raster_run_pairs_canvas_matching_mask_slot() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("RasterRun")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("mask", 10, 9, 1, 12, 34),
        ]),
        &[clip_gpu::GpuNormalStackSource::Raster(raster_source(
            10,
            1,
            1.0,
            clip_gpu::GpuRasterBlendMode::Normal,
            Some(9),
        ))],
    );

    assert!(plan.skipped_segments.is_empty());
    let mask = plan.segments[0].batches[0].events[0]
        .mask
        .expect("mask tile ref");
    assert_eq!(mask.key.format, clip_gpu::GpuSparseAtlasFormat::R8);
    assert_eq!(mask.key.atlas_id, 3);
    assert_eq!(mask.atlas_x, 80);
    assert_eq!(mask.atlas_y, 32);
}

#[test]
fn masked_raster_run_without_matching_mask_slot_is_not_lowered() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("RasterRun")),
        &reload_with_slots(vec![slot("raster", 10, 1, 0, 12, 34)]),
        &[clip_gpu::GpuNormalStackSource::Raster(raster_source(
            10,
            1,
            1.0,
            clip_gpu::GpuRasterBlendMode::Normal,
            Some(9),
        ))],
    );

    assert!(plan.segments.is_empty());
    assert_eq!(
        plan.skipped_segments[0].reason,
        SparseAtlasRasterEventSkipReason::MaskSlotMissing {
            layer_id: 10,
            resource_id: 9,
            canvas_x: 12,
            canvas_y: 34,
        }
    );
}

#[test]
fn non_raster_run_is_explained_as_skipped() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![slot("raster", 10, 1, 0, 12, 34)]),
        &[clip_gpu::GpuNormalStackSource::Raster(raster_source(
            10,
            1,
            1.0,
            clip_gpu::GpuRasterBlendMode::Normal,
            None,
        ))],
    );

    assert!(plan.segments.is_empty());
    assert_eq!(
        plan.skipped_segments[0].reason,
        SparseAtlasRasterEventSkipReason::NonRasterRun
    );
}

#[test]
fn raster_run_splits_resident_events_by_executor_compatible_atlas_keys() {
    let first = slot("raster", 10, 1, 0, 12, 34);
    let mut second = slot("raster", 11, 2, 1, 20, 34);
    second.slot.atlas_id = 4;
    let mut third = slot("raster", 12, 3, 2, 28, 34);
    third.slot.atlas_id = 3;
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(ReloadDiffSegment {
            source_end: 3,
            ..segment("RasterRun")
        }),
        &reload_with_slots(vec![first, second, third]),
        &[
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
                1.0,
                clip_gpu::GpuRasterBlendMode::Normal,
                None,
            )),
            clip_gpu::GpuNormalStackSource::Raster(raster_source(
                12,
                3,
                1.0,
                clip_gpu::GpuRasterBlendMode::Normal,
                None,
            )),
        ],
    );

    assert!(plan.skipped_segments.is_empty());
    let segment = &plan.segments[0];
    assert_eq!(segment.batches.len(), 3);
    assert_eq!(segment.batches[0].events[0].raster.key.atlas_id, 3);
    assert_eq!(segment.batches[1].events[0].raster.key.atlas_id, 4);
    assert_eq!(segment.batches[2].events[0].raster.key.atlas_id, 3);
}

#[test]
fn affected_plan_skips_later_non_overlapping_barrier_segment() {
    let diff = diff_with_segments_and_rects(
        vec![
            manifest_segment_at_rect(7, 0, 1, "RasterRun", 10, 1, 12, 34),
            manifest_segment_at_rect(8, 1, 2, "SimpleContainerScope", 11, 2, 96, 96),
        ],
        vec![
            source_manifest_at_rect(10, 1, 12, 34),
            source_manifest_at_rect(11, 2, 96, 96),
        ],
        vec![dirty_segment(7)],
        vec![crate::ReloadPatchRect {
            x: 12,
            y: 34,
            width: 64,
            height: 32,
        }],
    );
    let reload = reload_with_cache_updates(vec![cache_update(10, 1, 0), cache_update(11, 2, 1)]);
    let plan = sparse_atlas_raster_affected_event_plan(
        &diff,
        &reload,
        &[
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
                1.0,
                clip_gpu::GpuRasterBlendMode::Normal,
                None,
            )),
        ],
    );

    assert!(plan.skipped_segments.is_empty());
    assert_eq!(plan.segments.len(), 1);
    assert_eq!(plan.segments[0].ordinal, 7);
}

#[test]
fn affected_plan_lowers_later_overlapping_raster_segment() {
    let diff = diff_with_segments_and_rects(
        vec![
            manifest_segment_at_rect(7, 0, 1, "RasterRun", 10, 1, 12, 34),
            manifest_segment_at_rect(8, 1, 2, "RasterRun", 11, 2, 20, 40),
        ],
        vec![
            source_manifest_at_rect(10, 1, 12, 34),
            source_manifest_at_rect(11, 2, 20, 40),
        ],
        vec![dirty_segment(7)],
        vec![crate::ReloadPatchRect {
            x: 12,
            y: 34,
            width: 64,
            height: 32,
        }],
    );
    let reload = reload_with_cache_updates(vec![cache_update(10, 1, 0), cache_update(11, 2, 1)]);
    let plan = sparse_atlas_raster_affected_event_plan(
        &diff,
        &reload,
        &[
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
    );

    assert!(plan.skipped_segments.is_empty());
    assert_eq!(plan.segments.len(), 2);
    assert_eq!(plan.segments[0].ordinal, 7);
    assert_eq!(plan.segments[1].ordinal, 8);
    assert_eq!(
        plan.segments[0].event_ranges,
        vec![ReloadDirtySegmentEventRange { start: 0, end: 1 }]
    );
    assert_eq!(
        plan.segments[1].event_ranges,
        vec![ReloadDirtySegmentEventRange { start: 0, end: 1 }]
    );
    assert_eq!(plan.segments[1].batches[0].events[0].opacity, 0.5);
    assert_eq!(
        plan.segments[1].batches[0].events[0].blend_mode,
        clip_gpu::GpuRasterBlendMode::Multiply
    );
}

#[test]
fn affected_plan_narrows_event_ranges_to_overlapping_tiles() {
    let diff = diff_with_segments_and_rects(
        vec![manifest_segment_with_tiles(
            7,
            0,
            1,
            "RasterRun",
            10,
            1,
            vec![
                tile_ref_at_event(10, 1, 0, 0, 1),
                tile_ref_at_event(10, 1, 1, 3, 4),
            ],
        )],
        vec![source_manifest_with_tiles(
            10,
            1,
            vec![source_tile_at(0, 0, 0), source_tile_at(1, 96, 96)],
        )],
        vec![dirty_segment(7)],
        vec![crate::ReloadPatchRect {
            x: 96,
            y: 96,
            width: 32,
            height: 32,
        }],
    );
    let reload = reload_with_cache_updates(vec![
        cache_update_at_tile(10, 1, 0, 0),
        cache_update_at_tile(10, 1, 1, 1),
    ]);
    let plan = sparse_atlas_raster_affected_event_plan(
        &diff,
        &reload,
        &[clip_gpu::GpuNormalStackSource::Raster(raster_source(
            10,
            1,
            1.0,
            clip_gpu::GpuRasterBlendMode::Normal,
            None,
        ))],
    );

    assert!(plan.skipped_segments.is_empty());
    assert_eq!(plan.segments.len(), 1);
    assert_eq!(
        plan.segments[0].event_ranges,
        vec![ReloadDirtySegmentEventRange { start: 3, end: 4 }]
    );
    assert_eq!(plan.segments[0].batches[0].events.len(), 1);
    assert_eq!(plan.segments[0].batches[0].events[0].source_offset_x, 96);
    assert_eq!(plan.segments[0].batches[0].events[0].source_offset_y, 96);
}

#[test]
fn affected_plan_explains_later_overlapping_non_raster_segment() {
    let diff = diff_with_segments_and_rects(
        vec![
            manifest_segment_at_rect(7, 0, 1, "RasterRun", 10, 1, 12, 34),
            manifest_segment_at_rect(8, 1, 2, "SimpleContainerScope", 11, 2, 20, 40),
        ],
        vec![
            source_manifest_at_rect(10, 1, 12, 34),
            source_manifest_at_rect(11, 2, 20, 40),
        ],
        vec![dirty_segment(7)],
        vec![crate::ReloadPatchRect {
            x: 12,
            y: 34,
            width: 64,
            height: 32,
        }],
    );
    let reload = reload_with_cache_updates(vec![cache_update(10, 1, 0), cache_update(11, 2, 1)]);
    let plan = sparse_atlas_raster_affected_event_plan(
        &diff,
        &reload,
        &[
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
                1.0,
                clip_gpu::GpuRasterBlendMode::Normal,
                None,
            )),
        ],
    );

    assert_eq!(plan.segments.len(), 1);
    assert_eq!(plan.skipped_segments.len(), 1);
    assert_eq!(plan.skipped_segments[0].ordinal, 8);
    assert_eq!(
        plan.skipped_segments[0].reason,
        SparseAtlasRasterEventSkipReason::NonRasterRun
    );
}

#[test]
fn affected_plan_fails_closed_for_unknown_work_list_tile() {
    let diff = diff_with_segments_and_rects(
        vec![manifest_segment_at_rect(
            7,
            0,
            1,
            "RasterRun",
            10,
            1,
            12,
            34,
        )],
        Vec::new(),
        vec![dirty_segment(7)],
        vec![crate::ReloadPatchRect {
            x: 12,
            y: 34,
            width: 64,
            height: 32,
        }],
    );
    let reload = reload_with_cache_updates(Vec::new());
    let plan = sparse_atlas_raster_affected_event_plan(
        &diff,
        &reload,
        &[clip_gpu::GpuNormalStackSource::Raster(raster_source(
            10,
            1,
            1.0,
            clip_gpu::GpuRasterBlendMode::Normal,
            None,
        ))],
    );

    assert!(plan.segments.is_empty());
    assert_eq!(plan.skipped_segments.len(), 1);
    assert_eq!(
        plan.skipped_segments[0].reason,
        SparseAtlasRasterEventSkipReason::EmptyRasterSlots
    );
}
