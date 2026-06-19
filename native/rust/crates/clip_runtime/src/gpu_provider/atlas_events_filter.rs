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
        filters.extend(point_filter_events_for_source(
            rerun_segment,
            *mask_key,
            &local_bounds,
            lut_rgba,
            *opacity,
            *filter_mode,
        )?);
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

fn point_filter_events_for_source(
    segment: &SparseAtlasRerunSegment,
    mask_key: Option<clip_gpu::GpuMaskResourceKey>,
    local_bounds: &[Rect],
    lut_rgba: &[u8],
    opacity: f32,
    filter_mode: clip_gpu::GpuLutFilterMode,
) -> Result<Vec<clip_gpu::GpuSparseAtlasPointFilterEvent>, SparseAtlasRasterEventSkipReason> {
    let Some(mask_key) = mask_key else {
        return Ok(local_bounds
            .iter()
            .copied()
            .map(|local_bounds| clip_gpu::GpuSparseAtlasPointFilterEvent {
                lut_rgba: lut_rgba.to_vec(),
                opacity,
                filter_mode,
                local_bounds,
                mask: None,
            })
            .collect());
    };

    let mut output = Vec::new();
    for bounds in local_bounds.iter().copied() {
        let refs = filter_mask_tile_refs_for_bounds(segment, mask_key, bounds)?;
        output.extend(
            refs.into_iter()
                .map(|mask| clip_gpu::GpuSparseAtlasPointFilterEvent {
                    lut_rgba: lut_rgba.to_vec(),
                    opacity,
                    filter_mode,
                    local_bounds: mask.local_bounds,
                    mask: Some(mask.tile_ref),
                }),
        );
    }
    Ok(output)
}

struct FilterMaskEventRef {
    local_bounds: Rect,
    tile_ref: clip_gpu::GpuSparseAtlasTileRef,
}

fn filter_mask_tile_refs_for_bounds(
    segment: &SparseAtlasRerunSegment,
    mask_key: clip_gpu::GpuMaskResourceKey,
    bounds: Rect,
) -> Result<Vec<FilterMaskEventRef>, SparseAtlasRasterEventSkipReason> {
    let mut refs = Vec::new();
    for slot in segment.resident_slots.iter().filter(|slot| {
        slot.kind == "mask"
            && slot.layer_id == mask_key.layer_id.0
            && slot.resource_id == mask_key.mask_mipmap_id
    }) {
        if let Some(intersection) = rect_intersection(bounds, slot_canvas_rect(slot)) {
            refs.push(FilterMaskEventRef {
                local_bounds: intersection,
                tile_ref: mask_tile_ref_for_bounds(slot, intersection)?,
            });
        }
    }
    if refs.is_empty() || !rects_cover_bounds(&refs, bounds) {
        return Err(SparseAtlasRasterEventSkipReason::FilterMaskNotLowered {
            layer_id: mask_key.layer_id.0,
            resource_id: mask_key.mask_mipmap_id,
        });
    }
    refs.sort_by_key(|entry| (entry.local_bounds.y, entry.local_bounds.x));
    Ok(refs)
}

fn mask_tile_ref_for_bounds(
    slot: &SparseAtlasRerunSlot,
    bounds: Rect,
) -> Result<clip_gpu::GpuSparseAtlasTileRef, SparseAtlasRasterEventSkipReason> {
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
    Ok(clip_gpu::GpuSparseAtlasTileRef {
        key: clip_gpu::GpuSparseAtlasTextureKey {
            format: slot.format,
            atlas_id: slot.slot.atlas_id,
        },
        atlas_x,
        atlas_y,
        size: clip_model::CanvasSize::new(bounds.width, bounds.height),
    })
}

fn rect_intersection(left: Rect, right: Rect) -> Option<Rect> {
    let x0 = left.x.max(right.x);
    let y0 = left.y.max(right.y);
    let x1 = left
        .x
        .checked_add(left.width)?
        .min(right.x.checked_add(right.width)?);
    let y1 = left
        .y
        .checked_add(left.height)?
        .min(right.y.checked_add(right.height)?);
    (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
}

fn slot_canvas_rect(slot: &SparseAtlasRerunSlot) -> Rect {
    Rect::new(
        slot.canvas_x,
        slot.canvas_y,
        slot.slot.width,
        slot.slot.height,
    )
}

fn rects_cover_bounds(refs: &[FilterMaskEventRef], bounds: Rect) -> bool {
    let area = u64::from(bounds.width) * u64::from(bounds.height);
    let covered = refs.iter().fold(0u64, |total, entry| {
        total + u64::from(entry.local_bounds.width) * u64::from(entry.local_bounds.height)
    });
    covered == area
}

fn point_filter_local_bounds(
    diff: &ReloadDiffPlan,
) -> Result<Vec<Rect>, SparseAtlasRasterEventSkipReason> {
    if diff.dirty_rects.is_empty() {
        if diff.manifest.width == 0 || diff.manifest.height == 0 {
            return Err(SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange);
        }
        return Ok(vec![Rect::new(
            0,
            0,
            diff.manifest.width,
            diff.manifest.height,
        )]);
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
        if rect.width == 0
            || rect.height == 0
            || right > diff.manifest.width
            || bottom > diff.manifest.height
        {
            return Err(SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange);
        }
        x0 = x0.min(rect.x);
        y0 = y0.min(rect.y);
        x1 = x1.max(right);
        y1 = y1.max(bottom);
    }
    if x1 <= x0 || y1 <= y0 {
        return Err(SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange);
    }
    Ok(vec![Rect::new(x0, y0, x1 - x0, y1 - y0)])
}
