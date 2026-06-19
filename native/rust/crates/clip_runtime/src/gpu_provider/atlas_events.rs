use clip_model::{CanvasSize, Rect};

use super::atlas_events_filter::lower_point_filter_run_segment;
pub(crate) use super::atlas_events_types::{
    SparseAtlasRasterEventPlan, SparseAtlasRasterEventSegment, SparseAtlasRasterEventSkip,
    SparseAtlasRasterEventSkipReason,
};
use super::atlas_rerun::{
    SparseAtlasReloadPlan, SparseAtlasRerunSegment, SparseAtlasRerunSlot, affected_rerun_segments,
};
use crate::reload_diff::{ReloadDiffPlan, ReloadDiffSegment};

pub(crate) fn sparse_atlas_raster_event_plan(
    diff: &ReloadDiffPlan,
    reload: &SparseAtlasReloadPlan,
    sources: &[clip_gpu::GpuNormalStackSource],
) -> SparseAtlasRasterEventPlan {
    sparse_atlas_raster_event_plan_for_segments(diff, &reload.rerunnable_segments, sources)
}

pub(crate) fn sparse_atlas_raster_affected_event_plan(
    diff: &ReloadDiffPlan,
    reload: &SparseAtlasReloadPlan,
    sources: &[clip_gpu::GpuNormalStackSource],
) -> SparseAtlasRasterEventPlan {
    let affected_segments = affected_rerun_segments(diff, &reload.cache);
    sparse_atlas_raster_event_plan_for_segments(diff, &affected_segments, sources)
}

fn sparse_atlas_raster_event_plan_for_segments(
    diff: &ReloadDiffPlan,
    rerunnable_segments: &[SparseAtlasRerunSegment],
    sources: &[clip_gpu::GpuNormalStackSource],
) -> SparseAtlasRasterEventPlan {
    let mut segments = Vec::new();
    let mut skipped_segments = Vec::new();
    for rerun_segment in rerunnable_segments {
        match lower_raster_run_segment(diff, rerun_segment, sources) {
            Ok(segment) => segments.push(segment),
            Err(reason) => skipped_segments.push(SparseAtlasRasterEventSkip {
                ordinal: rerun_segment.ordinal,
                reason,
            }),
        }
    }
    SparseAtlasRasterEventPlan {
        segments,
        skipped_segments,
    }
}

fn lower_raster_run_segment(
    diff: &ReloadDiffPlan,
    rerun_segment: &SparseAtlasRerunSegment,
    sources: &[clip_gpu::GpuNormalStackSource],
) -> Result<SparseAtlasRasterEventSegment, SparseAtlasRasterEventSkipReason> {
    let segment = diff
        .manifest
        .segments
        .iter()
        .find(|segment| segment.ordinal == rerun_segment.ordinal)
        .ok_or(SparseAtlasRasterEventSkipReason::SegmentManifestMissing)?;
    if segment.kind == "PointFilterRun" {
        return lower_point_filter_run_segment(diff, segment, rerun_segment, sources);
    }
    if segment.kind == "RasterClippingRun" {
        return lower_raster_clipping_run_segment(segment, rerun_segment, sources);
    }
    if segment.kind == "SimpleContainerScope" || segment.kind == "SimpleThroughScope" {
        return lower_simple_scope_segment(segment, rerun_segment, sources);
    }
    if segment.kind != "RasterRun" {
        return Err(SparseAtlasRasterEventSkipReason::NonRasterRun);
    }

    let source_span = segment_source_span(segment, sources)?;
    let mut raster_slots = rerun_segment
        .resident_slots
        .iter()
        .filter(|slot| slot.kind == "raster")
        .collect::<Vec<_>>();
    if raster_slots.is_empty() {
        return Err(SparseAtlasRasterEventSkipReason::EmptyRasterSlots);
    }
    raster_slots.sort_by_key(|slot| {
        (
            slot.event_start,
            slot.event_end,
            slot.canvas_y,
            slot.canvas_x,
            slot.tile_y,
            slot.tile_x,
        )
    });

    let mut events = Vec::with_capacity(raster_slots.len());
    for slot in raster_slots {
        let raster = raster_source_for_slot(source_span, slot).ok_or(
            SparseAtlasRasterEventSkipReason::RasterSourceMissing {
                layer_id: slot.layer_id,
                resource_id: slot.resource_id,
            },
        )?;
        let mask = mask_tile_ref_for_raster(rerun_segment, raster, slot)?;
        events.push(clip_gpu::GpuSparseAtlasRasterEvent {
            raster: tile_ref_for_slot(slot),
            source_offset_x: i32::try_from(slot.canvas_x)
                .map_err(|_| SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?,
            source_offset_y: i32::try_from(slot.canvas_y)
                .map_err(|_| SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?,
            opacity: raster.opacity,
            blend_mode: raster.blend_mode,
            mask,
        });
    }

    Ok(SparseAtlasRasterEventSegment {
        ordinal: rerun_segment.ordinal,
        event_ranges: rerun_segment.event_ranges.clone(),
        batches: clip_gpu::split_sparse_atlas_raster_event_batches(&events),
    })
}

fn lower_simple_scope_segment(
    segment: &ReloadDiffSegment,
    rerun_segment: &SparseAtlasRerunSegment,
    sources: &[clip_gpu::GpuNormalStackSource],
) -> Result<SparseAtlasRasterEventSegment, SparseAtlasRasterEventSkipReason> {
    let source_span = segment_source_span(segment, sources)?;
    let [source] = source_span else {
        return Err(SparseAtlasRasterEventSkipReason::SourceSpanOutOfRange);
    };
    let mut events = Vec::new();
    let batch = match (segment.kind.as_str(), source) {
        (
            "SimpleContainerScope",
            clip_gpu::GpuNormalStackSource::Container {
                children,
                opacity,
                mask_key,
                blend_mode,
            },
        ) => {
            push_direct_scope_raster_events(rerun_segment, children, &mut events)?;
            let bounds = raster_event_bounds(&events)?;
            let mask = scope_mask_tile_ref_for_bounds(rerun_segment, *mask_key, bounds)?;
            clip_gpu::GpuSparseAtlasRasterEventBatch::simple_container_scope(
                events,
                *opacity,
                *blend_mode,
                bounds,
                mask,
            )
        }
        (
            "SimpleThroughScope",
            clip_gpu::GpuNormalStackSource::ThroughGroup {
                children,
                opacity,
                mask_key,
            },
        ) => {
            push_direct_scope_raster_events(rerun_segment, children, &mut events)?;
            let bounds = raster_event_bounds(&events)?;
            clip_gpu::GpuSparseAtlasRasterEventBatch::simple_through_scope(
                events,
                *opacity,
                bounds,
                scope_mask_tile_ref_for_bounds(rerun_segment, *mask_key, bounds)?,
            )
        }
        _ => return Err(SparseAtlasRasterEventSkipReason::NonRasterRun),
    };
    if batch.events.is_empty() {
        return Err(SparseAtlasRasterEventSkipReason::EmptyRasterSlots);
    }
    if !events_share_executor_atlases(&batch.events) {
        return Err(SparseAtlasRasterEventSkipReason::MixedSparseAtlasKeys);
    }
    Ok(SparseAtlasRasterEventSegment {
        ordinal: rerun_segment.ordinal,
        event_ranges: rerun_segment.event_ranges.clone(),
        batches: vec![batch],
    })
}

fn lower_raster_clipping_run_segment(
    segment: &ReloadDiffSegment,
    rerun_segment: &SparseAtlasRerunSegment,
    sources: &[clip_gpu::GpuNormalStackSource],
) -> Result<SparseAtlasRasterEventSegment, SparseAtlasRasterEventSkipReason> {
    let source_span = segment_source_span(segment, sources)?;
    let [clip_gpu::GpuNormalStackSource::ClippingRun { base, clipped }] = source_span else {
        return Err(SparseAtlasRasterEventSkipReason::SourceSpanOutOfRange);
    };

    let mut events = Vec::new();
    push_raster_events_for_source(rerun_segment, *base, &mut events)?;
    let base_event_count = u32::try_from(events.len())
        .map_err(|_| SparseAtlasRasterEventSkipReason::SourceSpanOutOfRange)?;
    for clipped_source in clipped {
        let clip_gpu::GpuClippedStackSource::Raster(raster) = clipped_source else {
            return Err(SparseAtlasRasterEventSkipReason::NonRasterRun);
        };
        push_raster_events_for_source(rerun_segment, *raster, &mut events)?;
    }
    if events.is_empty() {
        return Err(SparseAtlasRasterEventSkipReason::EmptyRasterSlots);
    }
    if !events_share_executor_atlases(&events) {
        return Err(SparseAtlasRasterEventSkipReason::MixedSparseAtlasKeys);
    }

    Ok(SparseAtlasRasterEventSegment {
        ordinal: rerun_segment.ordinal,
        event_ranges: rerun_segment.event_ranges.clone(),
        batches: vec![
            clip_gpu::GpuSparseAtlasRasterEventBatch::raster_clipping_run(
                events,
                base_event_count,
                base.blend_mode,
            ),
        ],
    })
}

fn scope_mask_tile_ref_for_bounds(
    segment: &SparseAtlasRerunSegment,
    mask_key: Option<clip_gpu::GpuMaskResourceKey>,
    bounds: Rect,
) -> Result<Option<clip_gpu::GpuSparseAtlasTileRef>, SparseAtlasRasterEventSkipReason> {
    let Some(mask_key) = mask_key else {
        return Ok(None);
    };
    let Some(slot) = segment.resident_slots.iter().find(|slot| {
        slot.kind == "mask"
            && slot.layer_id == mask_key.layer_id.0
            && slot.resource_id == mask_key.mask_mipmap_id
            && slot_covers_bounds(slot, bounds)
    }) else {
        return Err(SparseAtlasRasterEventSkipReason::ScopeMaskNotLowered {
            layer_id: mask_key.layer_id.0,
            resource_id: mask_key.mask_mipmap_id,
        });
    };
    let atlas_x = slot
        .slot
        .x
        .checked_add(bounds.x.saturating_sub(slot.canvas_x))
        .ok_or(SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?;
    let atlas_y = slot
        .slot
        .y
        .checked_add(bounds.y.saturating_sub(slot.canvas_y))
        .ok_or(SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?;
    Ok(Some(clip_gpu::GpuSparseAtlasTileRef {
        key: clip_gpu::GpuSparseAtlasTextureKey {
            format: slot.format,
            atlas_id: slot.slot.atlas_id,
        },
        atlas_x,
        atlas_y,
        size: CanvasSize::new(bounds.width, bounds.height),
    }))
}

fn push_direct_scope_raster_events(
    segment: &SparseAtlasRerunSegment,
    children: &[clip_gpu::GpuNormalStackSource],
    events: &mut Vec<clip_gpu::GpuSparseAtlasRasterEvent>,
) -> Result<(), SparseAtlasRasterEventSkipReason> {
    for child in children {
        let clip_gpu::GpuNormalStackSource::Raster(raster) = child else {
            return Err(SparseAtlasRasterEventSkipReason::NonRasterRun);
        };
        push_raster_events_for_source(segment, *raster, events)?;
    }
    Ok(())
}

fn raster_event_bounds(
    events: &[clip_gpu::GpuSparseAtlasRasterEvent],
) -> Result<Rect, SparseAtlasRasterEventSkipReason> {
    let Some(first) = events.first() else {
        return Err(SparseAtlasRasterEventSkipReason::EmptyRasterSlots);
    };
    let mut x0 = u32::try_from(first.source_offset_x)
        .map_err(|_| SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?;
    let mut y0 = u32::try_from(first.source_offset_y)
        .map_err(|_| SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?;
    let mut x1 = x0
        .checked_add(first.raster.size.width)
        .ok_or(SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?;
    let mut y1 = y0
        .checked_add(first.raster.size.height)
        .ok_or(SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?;
    for event in events.iter().skip(1) {
        let x = u32::try_from(event.source_offset_x)
            .map_err(|_| SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?;
        let y = u32::try_from(event.source_offset_y)
            .map_err(|_| SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?;
        let right = x
            .checked_add(event.raster.size.width)
            .ok_or(SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?;
        let bottom = y
            .checked_add(event.raster.size.height)
            .ok_or(SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?;
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(right);
        y1 = y1.max(bottom);
    }
    if x1 <= x0 || y1 <= y0 {
        return Err(SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange);
    }
    Ok(Rect::new(x0, y0, x1 - x0, y1 - y0))
}

fn slot_covers_bounds(slot: &SparseAtlasRerunSlot, bounds: Rect) -> bool {
    let Some(slot_right) = slot.canvas_x.checked_add(slot.slot.width) else {
        return false;
    };
    let Some(slot_bottom) = slot.canvas_y.checked_add(slot.slot.height) else {
        return false;
    };
    let Some(bounds_right) = bounds.x.checked_add(bounds.width) else {
        return false;
    };
    let Some(bounds_bottom) = bounds.y.checked_add(bounds.height) else {
        return false;
    };
    slot.canvas_x <= bounds.x
        && slot.canvas_y <= bounds.y
        && bounds_right <= slot_right
        && bounds_bottom <= slot_bottom
}

pub(crate) fn segment_source_span<'a>(
    segment: &ReloadDiffSegment,
    sources: &'a [clip_gpu::GpuNormalStackSource],
) -> Result<&'a [clip_gpu::GpuNormalStackSource], SparseAtlasRasterEventSkipReason> {
    let start = usize::try_from(segment.source_start)
        .map_err(|_| SparseAtlasRasterEventSkipReason::SourceSpanOutOfRange)?;
    let end = usize::try_from(segment.source_end)
        .map_err(|_| SparseAtlasRasterEventSkipReason::SourceSpanOutOfRange)?;
    sources
        .get(start..end)
        .ok_or(SparseAtlasRasterEventSkipReason::SourceSpanOutOfRange)
}

fn raster_source_for_slot(
    sources: &[clip_gpu::GpuNormalStackSource],
    slot: &SparseAtlasRerunSlot,
) -> Option<clip_gpu::GpuNormalRasterSource> {
    sources.iter().find_map(|source| {
        let clip_gpu::GpuNormalStackSource::Raster(raster) = source else {
            return None;
        };
        (raster.key.layer_id.0 == slot.layer_id && raster.key.render_mipmap_id == slot.resource_id)
            .then_some(*raster)
    })
}

fn push_raster_events_for_source(
    segment: &SparseAtlasRerunSegment,
    raster: clip_gpu::GpuNormalRasterSource,
    events: &mut Vec<clip_gpu::GpuSparseAtlasRasterEvent>,
) -> Result<(), SparseAtlasRasterEventSkipReason> {
    let mut raster_slots = segment
        .resident_slots
        .iter()
        .filter(|slot| {
            slot.kind == "raster"
                && slot.layer_id == raster.key.layer_id.0
                && slot.resource_id == raster.key.render_mipmap_id
        })
        .collect::<Vec<_>>();
    raster_slots.sort_by_key(|slot| {
        (
            slot.event_start,
            slot.event_end,
            slot.canvas_y,
            slot.canvas_x,
            slot.tile_y,
            slot.tile_x,
        )
    });

    for slot in raster_slots {
        let mask = mask_tile_ref_for_raster(segment, raster, slot)?;
        events.push(clip_gpu::GpuSparseAtlasRasterEvent {
            raster: tile_ref_for_slot(slot),
            source_offset_x: i32::try_from(slot.canvas_x)
                .map_err(|_| SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?,
            source_offset_y: i32::try_from(slot.canvas_y)
                .map_err(|_| SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?,
            opacity: raster.opacity,
            blend_mode: raster.blend_mode,
            mask,
        });
    }
    Ok(())
}

fn events_share_executor_atlases(events: &[clip_gpu::GpuSparseAtlasRasterEvent]) -> bool {
    let Some(first) = events.first() else {
        return true;
    };
    let raster_key = first.raster.key;
    let mut mask_key = None;
    for event in events {
        if event.raster.key != raster_key {
            return false;
        }
        let Some(next_mask_key) = event.mask.map(|mask| mask.key) else {
            continue;
        };
        if let Some(current_mask_key) = mask_key {
            if current_mask_key != next_mask_key {
                return false;
            }
        } else {
            mask_key = Some(next_mask_key);
        }
    }
    true
}

fn mask_tile_ref_for_raster(
    segment: &SparseAtlasRerunSegment,
    raster: clip_gpu::GpuNormalRasterSource,
    raster_slot: &SparseAtlasRerunSlot,
) -> Result<Option<clip_gpu::GpuSparseAtlasTileRef>, SparseAtlasRasterEventSkipReason> {
    let Some(mask_key) = raster.mask_key else {
        return Ok(None);
    };
    let mask_slot = segment.resident_slots.iter().find(|slot| {
        slot.kind == "mask"
            && slot.layer_id == mask_key.layer_id.0
            && slot.resource_id == mask_key.mask_mipmap_id
            && slot.canvas_x == raster_slot.canvas_x
            && slot.canvas_y == raster_slot.canvas_y
            && slot.slot.width == raster_slot.slot.width
            && slot.slot.height == raster_slot.slot.height
    });
    mask_slot.map(tile_ref_for_slot).map(Some).ok_or(
        SparseAtlasRasterEventSkipReason::MaskSlotMissing {
            layer_id: mask_key.layer_id.0,
            resource_id: mask_key.mask_mipmap_id,
            canvas_x: raster_slot.canvas_x,
            canvas_y: raster_slot.canvas_y,
        },
    )
}

fn tile_ref_for_slot(slot: &SparseAtlasRerunSlot) -> clip_gpu::GpuSparseAtlasTileRef {
    clip_gpu::GpuSparseAtlasTileRef {
        key: clip_gpu::GpuSparseAtlasTextureKey {
            format: slot.format,
            atlas_id: slot.slot.atlas_id,
        },
        atlas_x: slot.slot.x,
        atlas_y: slot.slot.y,
        size: CanvasSize::new(slot.slot.width, slot.slot.height),
    }
}
