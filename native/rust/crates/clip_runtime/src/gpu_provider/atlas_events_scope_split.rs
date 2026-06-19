use clip_model::{CanvasSize, Rect};

use super::atlas_events_types::SparseAtlasRasterEventSkipReason;
use super::atlas_rerun::{SparseAtlasRerunSegment, SparseAtlasRerunSlot};

pub(super) fn container_scope_batches(
    segment: &SparseAtlasRerunSegment,
    tile_events: Vec<clip_gpu::GpuSparseAtlasTileEvent>,
    opacity: f32,
    blend_mode: clip_gpu::GpuRasterBlendMode,
    mask_key: Option<clip_gpu::GpuMaskResourceKey>,
    bounds: Rect,
) -> Result<Vec<clip_gpu::GpuSparseAtlasRasterEventBatch>, SparseAtlasRasterEventSkipReason> {
    let Some(mask_key) = mask_key else {
        return Ok(vec![
            clip_gpu::GpuSparseAtlasRasterEventBatch::simple_container_scope_tile_events(
                tile_events,
                opacity,
                blend_mode,
                bounds,
                None,
            ),
        ]);
    };
    let masks = scope_mask_tile_refs_for_bounds(segment, mask_key, bounds)?;
    let mut batches = Vec::with_capacity(masks.len());
    for mask in masks {
        let clipped_events = clip_tile_events_to_bounds(&tile_events, mask.local_bounds)?;
        batches.push(
            clip_gpu::GpuSparseAtlasRasterEventBatch::simple_container_scope_tile_events(
                clipped_events,
                opacity,
                blend_mode,
                mask.local_bounds,
                Some(mask.tile_ref),
            ),
        );
    }
    Ok(batches)
}

pub(super) fn through_scope_batches(
    segment: &SparseAtlasRerunSegment,
    tile_events: Vec<clip_gpu::GpuSparseAtlasTileEvent>,
    opacity: f32,
    mask_key: Option<clip_gpu::GpuMaskResourceKey>,
    bounds: Rect,
) -> Result<Vec<clip_gpu::GpuSparseAtlasRasterEventBatch>, SparseAtlasRasterEventSkipReason> {
    let Some(mask_key) = mask_key else {
        return Ok(vec![
            clip_gpu::GpuSparseAtlasRasterEventBatch::simple_through_scope_tile_events(
                tile_events,
                opacity,
                bounds,
                None,
            ),
        ]);
    };
    let masks = scope_mask_tile_refs_for_bounds(segment, mask_key, bounds)?;
    let mut batches = Vec::with_capacity(masks.len());
    for mask in masks {
        let clipped_events = clip_tile_events_to_bounds(&tile_events, mask.local_bounds)?;
        batches.push(
            clip_gpu::GpuSparseAtlasRasterEventBatch::simple_through_scope_tile_events(
                clipped_events,
                opacity,
                mask.local_bounds,
                Some(mask.tile_ref),
            ),
        );
    }
    Ok(batches)
}

#[derive(Clone, Copy)]
pub(super) struct ScopeMaskEventRef {
    pub(super) local_bounds: Rect,
    pub(super) tile_ref: clip_gpu::GpuSparseAtlasTileRef,
}

pub(super) fn scope_mask_tile_refs_for_bounds(
    segment: &SparseAtlasRerunSegment,
    mask_key: clip_gpu::GpuMaskResourceKey,
    bounds: Rect,
) -> Result<Vec<ScopeMaskEventRef>, SparseAtlasRasterEventSkipReason> {
    let mut refs = Vec::new();
    for slot in segment.resident_slots.iter().filter(|slot| {
        slot.kind == "mask"
            && slot.layer_id == mask_key.layer_id.0
            && slot.resource_id == mask_key.mask_mipmap_id
    }) {
        if let Some(intersection) = rect_intersection(bounds, slot_canvas_rect(slot)) {
            refs.push(ScopeMaskEventRef {
                local_bounds: intersection,
                tile_ref: mask_tile_ref_for_bounds(slot, intersection)?,
            });
        }
    }
    if refs.is_empty() || !rects_cover_bounds(&refs, bounds) {
        return Err(SparseAtlasRasterEventSkipReason::ScopeMaskNotLowered {
            layer_id: mask_key.layer_id.0,
            resource_id: mask_key.mask_mipmap_id,
        });
    }
    refs.sort_by_key(|entry| (entry.local_bounds.y, entry.local_bounds.x));
    Ok(refs)
}

pub(super) fn clip_tile_events_to_bounds(
    events: &[clip_gpu::GpuSparseAtlasTileEvent],
    bounds: Rect,
) -> Result<Vec<clip_gpu::GpuSparseAtlasTileEvent>, SparseAtlasRasterEventSkipReason> {
    let mut clipped = Vec::new();
    for event in events {
        match event {
            clip_gpu::GpuSparseAtlasTileEvent::Raster(event) => {
                if let Some(event) = clip_raster_event(*event, bounds)? {
                    clipped.push(clip_gpu::GpuSparseAtlasTileEvent::Raster(event));
                }
            }
            clip_gpu::GpuSparseAtlasTileEvent::ClipBaseRaster(event) => {
                if let Some(event) = clip_raster_event(*event, bounds)? {
                    clipped.push(clip_gpu::GpuSparseAtlasTileEvent::ClipBaseRaster(event));
                }
            }
            clip_gpu::GpuSparseAtlasTileEvent::ClippedRaster(event) => {
                if let Some(event) = clip_raster_event(*event, bounds)? {
                    clipped.push(clip_gpu::GpuSparseAtlasTileEvent::ClippedRaster(event));
                }
            }
            clip_gpu::GpuSparseAtlasTileEvent::PointFilter(filter) => {
                if let Some(filter) = clip_filter_event(filter, bounds)? {
                    clipped.push(clip_gpu::GpuSparseAtlasTileEvent::PointFilter(filter));
                }
            }
            clip_gpu::GpuSparseAtlasTileEvent::BeginScope(scope) => {
                if let Some(scope) = clip_scope_event(*scope, bounds)? {
                    clipped.push(clip_gpu::GpuSparseAtlasTileEvent::BeginScope(scope));
                }
            }
            clip_gpu::GpuSparseAtlasTileEvent::EndScope(scope) => {
                if let Some(scope) = clip_scope_event(*scope, bounds)? {
                    clipped.push(clip_gpu::GpuSparseAtlasTileEvent::EndScope(scope));
                }
            }
            clip_gpu::GpuSparseAtlasTileEvent::BeginClippedScope(scope) => {
                if let Some(scope) = clip_scope_event(*scope, bounds)? {
                    clipped.push(clip_gpu::GpuSparseAtlasTileEvent::BeginClippedScope(scope));
                }
            }
            clip_gpu::GpuSparseAtlasTileEvent::EndClippedScope(scope) => {
                if let Some(scope) = clip_scope_event(*scope, bounds)? {
                    clipped.push(clip_gpu::GpuSparseAtlasTileEvent::EndClippedScope(scope));
                }
            }
            clip_gpu::GpuSparseAtlasTileEvent::BeginClipBase(scope) => {
                if let Some(scope) = clip_scope_event(*scope, bounds)? {
                    clipped.push(clip_gpu::GpuSparseAtlasTileEvent::BeginClipBase(scope));
                }
            }
            clip_gpu::GpuSparseAtlasTileEvent::ResolveClipBase(scope) => {
                if let Some(scope) = clip_scope_event(*scope, bounds)? {
                    clipped.push(clip_gpu::GpuSparseAtlasTileEvent::ResolveClipBase(scope));
                }
            }
        }
    }
    Ok(clipped)
}

fn clip_raster_event(
    event: clip_gpu::GpuSparseAtlasRasterEvent,
    bounds: Rect,
) -> Result<Option<clip_gpu::GpuSparseAtlasRasterEvent>, SparseAtlasRasterEventSkipReason> {
    let event_bounds = raster_event_bounds(event)?;
    let Some(intersection) = rect_intersection(event_bounds, bounds) else {
        return Ok(None);
    };
    let dx = intersection.x.saturating_sub(event_bounds.x);
    let dy = intersection.y.saturating_sub(event_bounds.y);
    Ok(Some(clip_gpu::GpuSparseAtlasRasterEvent {
        raster: shifted_tile_ref(
            event.raster,
            dx,
            dy,
            intersection.width,
            intersection.height,
        )?,
        source_offset_x: i32::try_from(intersection.x)
            .map_err(|_| SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?,
        source_offset_y: i32::try_from(intersection.y)
            .map_err(|_| SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?,
        opacity: event.opacity,
        blend_mode: event.blend_mode,
        mask: event
            .mask
            .map(|mask| shifted_tile_ref(mask, dx, dy, intersection.width, intersection.height))
            .transpose()?,
    }))
}

fn clip_filter_event(
    filter: &clip_gpu::GpuSparseAtlasPointFilterEvent,
    bounds: Rect,
) -> Result<Option<clip_gpu::GpuSparseAtlasPointFilterEvent>, SparseAtlasRasterEventSkipReason> {
    let Some(intersection) = rect_intersection(filter.local_bounds, bounds) else {
        return Ok(None);
    };
    let dx = intersection.x.saturating_sub(filter.local_bounds.x);
    let dy = intersection.y.saturating_sub(filter.local_bounds.y);
    Ok(Some(clip_gpu::GpuSparseAtlasPointFilterEvent {
        lut_rgba: filter.lut_rgba.clone(),
        opacity: filter.opacity,
        filter_mode: filter.filter_mode,
        local_bounds: intersection,
        mask: filter
            .mask
            .map(|mask| shifted_tile_ref(mask, dx, dy, intersection.width, intersection.height))
            .transpose()?,
    }))
}

fn clip_scope_event(
    scope: clip_gpu::GpuSparseAtlasScopeEvent,
    bounds: Rect,
) -> Result<Option<clip_gpu::GpuSparseAtlasScopeEvent>, SparseAtlasRasterEventSkipReason> {
    let Some(intersection) = rect_intersection(scope.local_bounds, bounds) else {
        return Ok(None);
    };
    let dx = intersection.x.saturating_sub(scope.local_bounds.x);
    let dy = intersection.y.saturating_sub(scope.local_bounds.y);
    Ok(Some(clip_gpu::GpuSparseAtlasScopeEvent {
        kind: scope.kind,
        opacity: scope.opacity,
        blend_mode: scope.blend_mode,
        local_bounds: intersection,
        mask: scope
            .mask
            .map(|mask| shifted_tile_ref(mask, dx, dy, intersection.width, intersection.height))
            .transpose()?,
    }))
}

fn raster_event_bounds(
    event: clip_gpu::GpuSparseAtlasRasterEvent,
) -> Result<Rect, SparseAtlasRasterEventSkipReason> {
    let x = u32::try_from(event.source_offset_x)
        .map_err(|_| SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?;
    let y = u32::try_from(event.source_offset_y)
        .map_err(|_| SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?;
    Ok(Rect::new(
        x,
        y,
        event.raster.size.width,
        event.raster.size.height,
    ))
}

fn shifted_tile_ref(
    tile_ref: clip_gpu::GpuSparseAtlasTileRef,
    dx: u32,
    dy: u32,
    width: u32,
    height: u32,
) -> Result<clip_gpu::GpuSparseAtlasTileRef, SparseAtlasRasterEventSkipReason> {
    Ok(clip_gpu::GpuSparseAtlasTileRef {
        key: tile_ref.key,
        atlas_x: tile_ref
            .atlas_x
            .checked_add(dx)
            .ok_or(SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?,
        atlas_y: tile_ref
            .atlas_y
            .checked_add(dy)
            .ok_or(SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?,
        size: CanvasSize::new(width, height),
    })
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
        size: CanvasSize::new(bounds.width, bounds.height),
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

fn rects_cover_bounds(refs: &[ScopeMaskEventRef], bounds: Rect) -> bool {
    let area = u64::from(bounds.width) * u64::from(bounds.height);
    let covered = refs.iter().fold(0u64, |total, entry| {
        total + u64::from(entry.local_bounds.width) * u64::from(entry.local_bounds.height)
    });
    covered == area
}
