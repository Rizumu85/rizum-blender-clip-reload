use clip_model::Rect;

use super::atlas_events::segment_source_span;
use super::atlas_events_types::{SparseAtlasRasterEventSegment, SparseAtlasRasterEventSkipReason};
use super::atlas_rerun::{SparseAtlasRerunSegment, SparseAtlasRerunSlot};
use crate::reload_diff::{ReloadDiffPlan, ReloadDiffSegment};

pub(crate) fn lower_point_filter_run_segment(
    diff: &ReloadDiffPlan,
    segment: &ReloadDiffSegment,
    rerun_segment: &SparseAtlasRerunSegment,
    sources: &[clip_gpu::GpuNormalStackSource],
) -> Result<SparseAtlasRasterEventSegment, SparseAtlasRasterEventSkipReason> {
    let source_span = segment_source_span(segment, sources)?;
    let local_bounds = point_filter_local_bounds(diff)?;
    let mut filters = Vec::new();
    for source in source_span {
        let clip_gpu::GpuNormalStackSource::LutFilter {
            lut_rgba,
            opacity,
            mask_key,
            filter_mode,
        } = source
        else {
            return Err(SparseAtlasRasterEventSkipReason::SourceSpanOutOfRange);
        };
        if *opacity <= 0.0 || lut_rgba.len() != 256 * 4 {
            return Err(SparseAtlasRasterEventSkipReason::InvalidPointFilter);
        }
        let mask = filter_mask_tile_ref_for_bounds(rerun_segment, *mask_key, local_bounds)?;
        filters.push(clip_gpu::GpuSparseAtlasPointFilterEvent {
            lut_rgba: lut_rgba.clone(),
            opacity: *opacity,
            filter_mode: *filter_mode,
            local_bounds,
            mask,
        });
    }
    if filters.is_empty() {
        return Err(SparseAtlasRasterEventSkipReason::InvalidPointFilter);
    }

    Ok(SparseAtlasRasterEventSegment {
        ordinal: rerun_segment.ordinal,
        event_ranges: rerun_segment.event_ranges.clone(),
        batches: vec![clip_gpu::GpuSparseAtlasRasterEventBatch::point_filter_run(
            filters,
        )],
    })
}

fn filter_mask_tile_ref_for_bounds(
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
        return Err(SparseAtlasRasterEventSkipReason::FilterMaskNotLowered {
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
        size: clip_model::CanvasSize::new(bounds.width, bounds.height),
    }))
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

fn point_filter_local_bounds(
    diff: &ReloadDiffPlan,
) -> Result<Rect, SparseAtlasRasterEventSkipReason> {
    if diff.dirty_rects.is_empty() {
        if diff.manifest.width == 0 || diff.manifest.height == 0 {
            return Err(SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange);
        }
        return Ok(Rect::new(0, 0, diff.manifest.width, diff.manifest.height));
    }

    let mut x0 = u32::MAX;
    let mut y0 = u32::MAX;
    let mut x1 = 0u32;
    let mut y1 = 0u32;
    for rect in &diff.dirty_rects {
        let right = rect
            .x
            .checked_add(rect.width)
            .ok_or(SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?;
        let bottom = rect
            .y
            .checked_add(rect.height)
            .ok_or(SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?;
        x0 = x0.min(rect.x);
        y0 = y0.min(rect.y);
        x1 = x1.max(right);
        y1 = y1.max(bottom);
    }
    if x1 <= x0 || y1 <= y0 || x1 > diff.manifest.width || y1 > diff.manifest.height {
        return Err(SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange);
    }
    Ok(Rect::new(x0, y0, x1 - x0, y1 - y0))
}
