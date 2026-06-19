use clip_model::{CanvasSize, LayerId};

use super::atlas_cache::{SparseAtlasSlot, SparseAtlasUpdateAction, SparseAtlasUpdatePlan};
use super::atlas_events::{SparseAtlasRasterEventSkipReason, sparse_atlas_raster_event_plan};
use super::atlas_rerun::{SparseAtlasReloadPlan, SparseAtlasRerunSegment, SparseAtlasRerunSlot};
use crate::reload_diff::{
    ReloadDiffManifest, ReloadDiffMode, ReloadDiffPlan, ReloadDiffSegment,
    ReloadDirtySegmentEventRange,
};

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

fn diff_with_segment(segment: ReloadDiffSegment) -> ReloadDiffPlan {
    ReloadDiffPlan {
        manifest: ReloadDiffManifest {
            abi: 4,
            tile_size: 256,
            tile_event_abi_version: clip_gpu::TILE_EVENT_ABI_VERSION,
            width: 128,
            height: 128,
            root_layer_id: 1,
            nodes: Vec::new(),
            sources: Vec::new(),
            segments: vec![segment],
        },
        mode: ReloadDiffMode::Patch,
        reason: "test".to_string(),
        dirty_rects: Vec::new(),
        dirty_segments: Vec::new(),
    }
}

fn segment(kind: &str) -> ReloadDiffSegment {
    ReloadDiffSegment {
        ordinal: 7,
        depth: 0,
        source_start: 0,
        source_end: 1,
        kind: kind.to_string(),
        barrier_reason: None,
        expected_passes: 1,
        tile_events: 4,
        legacy_sources: 0,
        resources: Vec::new(),
        tile_work_list_source_count: 0,
        tile_work_list_tile_count: 0,
        tile_work_list_signature: 0,
        tile_work_list: Vec::new(),
        signature: 0,
    }
}

fn reload_with_slots(slots: Vec<SparseAtlasRerunSlot>) -> SparseAtlasReloadPlan {
    SparseAtlasReloadPlan {
        cache: SparseAtlasUpdatePlan::default(),
        rerunnable_segments: vec![SparseAtlasRerunSegment {
            ordinal: 7,
            event_ranges: vec![ReloadDirtySegmentEventRange { start: 2, end: 3 }],
            resident_slots: slots.clone(),
            updated_slots: slots,
        }],
    }
}

fn slot(
    kind: &str,
    layer_id: u32,
    resource_id: u32,
    slot_index: u32,
    canvas_x: u32,
    canvas_y: u32,
) -> SparseAtlasRerunSlot {
    let format = if kind == "mask" {
        clip_gpu::GpuSparseAtlasFormat::R8
    } else {
        clip_gpu::GpuSparseAtlasFormat::Rgba8
    };
    SparseAtlasRerunSlot {
        kind: kind.to_string(),
        layer_id,
        resource_id,
        tile_x: slot_index,
        tile_y: 0,
        event_start: 2,
        event_end: 3,
        canvas_x,
        canvas_y,
        source_x: canvas_x,
        source_y: canvas_y,
        action: SparseAtlasUpdateAction::Reuse,
        format,
        atlas_size: CanvasSize::new(256, 256),
        slot: SparseAtlasSlot {
            index: slot_index,
            atlas_id: 3,
            x: 16 + slot_index * 64,
            y: 32,
            width: 64,
            height: 32,
        },
    }
}

fn raster_source(
    layer_id: u32,
    render_mipmap_id: u32,
    opacity: f32,
    blend_mode: clip_gpu::GpuRasterBlendMode,
    mask_mipmap_id: Option<u32>,
) -> clip_gpu::GpuNormalRasterSource {
    clip_gpu::GpuNormalRasterSource {
        key: clip_gpu::GpuRasterResourceKey {
            layer_id: LayerId(layer_id),
            render_mipmap_id,
        },
        opacity,
        mask_key: mask_mipmap_id.map(|mask_mipmap_id| clip_gpu::GpuMaskResourceKey {
            layer_id: LayerId(layer_id),
            mask_mipmap_id,
        }),
        offset_x: 0,
        offset_y: 0,
        blend_mode,
    }
}
