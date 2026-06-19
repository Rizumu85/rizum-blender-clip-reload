use clip_model::{CanvasSize, Rect};

use super::atlas_events::{
    events_share_executor_atlases, push_raster_events_for_source, segment_source_span,
};
use super::atlas_events_filter::point_filter_events_for_source;
use super::atlas_events_types::{SparseAtlasRasterEventSegment, SparseAtlasRasterEventSkipReason};
use super::atlas_rerun::{SparseAtlasRerunSegment, SparseAtlasRerunSlot};
use crate::reload_diff::ReloadDiffSegment;

const SPARSE_SCOPE_STACK_LIMIT: usize = 4;

#[derive(Clone, Copy)]
enum ClippingRunPolicy {
    None,
    DirectOnly,
    NestedContainers,
}

impl ClippingRunPolicy {
    fn allows_current(self) -> bool {
        matches!(self, Self::DirectOnly | Self::NestedContainers)
    }

    fn for_nested_container(self) -> Self {
        match self {
            Self::NestedContainers => Self::NestedContainers,
            Self::DirectOnly | Self::None => Self::None,
        }
    }
}

pub(super) fn lower_simple_scope_segment(
    segment: &ReloadDiffSegment,
    rerun_segment: &SparseAtlasRerunSegment,
    sources: &[clip_gpu::GpuNormalStackSource],
) -> Result<SparseAtlasRasterEventSegment, SparseAtlasRasterEventSkipReason> {
    let source_span = segment_source_span(segment, sources)?;
    let [source] = source_span else {
        return Err(SparseAtlasRasterEventSkipReason::SourceSpanOutOfRange);
    };
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
            let (tile_events, bounds) = scope_child_tile_events(
                rerun_segment,
                children,
                ClippingRunPolicy::NestedContainers,
            )?;
            let mask = scope_mask_tile_ref_for_bounds(rerun_segment, *mask_key, bounds)?;
            clip_gpu::GpuSparseAtlasRasterEventBatch::simple_container_scope_tile_events(
                tile_events,
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
            let (tile_events, bounds) =
                scope_child_tile_events(rerun_segment, children, ClippingRunPolicy::DirectOnly)?;
            clip_gpu::GpuSparseAtlasRasterEventBatch::simple_through_scope_tile_events(
                tile_events,
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
    if !events_share_executor_atlases(&batch.events) || !batch_masks_share_executor_atlas(&batch) {
        return Err(SparseAtlasRasterEventSkipReason::MixedSparseAtlasKeys);
    }
    Ok(SparseAtlasRasterEventSegment {
        ordinal: rerun_segment.ordinal,
        event_ranges: rerun_segment.event_ranges.clone(),
        batches: vec![batch],
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

fn scope_child_tile_events(
    segment: &SparseAtlasRerunSegment,
    children: &[clip_gpu::GpuNormalStackSource],
    clipping_run_policy: ClippingRunPolicy,
) -> Result<(Vec<clip_gpu::GpuSparseAtlasTileEvent>, Rect), SparseAtlasRasterEventSkipReason> {
    scope_child_tile_events_at_depth(segment, children, 1, clipping_run_policy)
}

fn scope_child_tile_events_at_depth(
    segment: &SparseAtlasRerunSegment,
    children: &[clip_gpu::GpuNormalStackSource],
    scope_depth: usize,
    clipping_run_policy: ClippingRunPolicy,
) -> Result<(Vec<clip_gpu::GpuSparseAtlasTileEvent>, Rect), SparseAtlasRasterEventSkipReason> {
    let mut tile_events = Vec::new();
    let mut current_bounds = None;
    for child in children {
        match child {
            clip_gpu::GpuNormalStackSource::Raster(raster) => {
                let mut events = Vec::new();
                push_raster_events_for_source(segment, *raster, &mut events)?;
                for event in events {
                    current_bounds = Some(match current_bounds {
                        Some(bounds) => union_rect(bounds, raster_event_bounds(&[event])?),
                        None => raster_event_bounds(&[event])?,
                    });
                    tile_events.push(clip_gpu::GpuSparseAtlasTileEvent::Raster(event));
                }
            }
            clip_gpu::GpuNormalStackSource::ClippingRun { base, clipped } => {
                if !clipping_run_policy.allows_current() {
                    return Err(SparseAtlasRasterEventSkipReason::NonRasterRun);
                }
                let bounds =
                    push_clipping_run_tile_events(segment, *base, clipped, &mut tile_events)?;
                current_bounds = Some(match current_bounds {
                    Some(current) => union_rect(current, bounds),
                    None => bounds,
                });
            }
            clip_gpu::GpuNormalStackSource::LutFilter {
                lut_rgba,
                opacity,
                mask_key,
                filter_mode,
            } => {
                let Some(bounds) = current_bounds else {
                    return Err(SparseAtlasRasterEventSkipReason::InvalidPointFilter);
                };
                if *opacity <= 0.0 || lut_rgba.len() != 256 * 4 {
                    return Err(SparseAtlasRasterEventSkipReason::InvalidPointFilter);
                }
                for filter in point_filter_events_for_source(
                    segment,
                    *mask_key,
                    &[bounds],
                    lut_rgba,
                    *opacity,
                    *filter_mode,
                )? {
                    tile_events.push(clip_gpu::GpuSparseAtlasTileEvent::PointFilter(filter));
                }
            }
            clip_gpu::GpuNormalStackSource::Container {
                children,
                opacity,
                mask_key,
                blend_mode,
            } => {
                let (scope_events, bounds) = nested_scope_tile_events(
                    segment,
                    children,
                    scope_depth,
                    *opacity,
                    *blend_mode,
                    *mask_key,
                    clip_gpu::GpuSparseAtlasScopeEventKind::Container,
                    clipping_run_policy.for_nested_container(),
                )?;
                current_bounds = Some(match current_bounds {
                    Some(current) => union_rect(current, bounds),
                    None => bounds,
                });
                tile_events.extend(scope_events);
            }
            clip_gpu::GpuNormalStackSource::ThroughGroup {
                children,
                opacity,
                mask_key,
            } => {
                let (scope_events, bounds) = nested_scope_tile_events(
                    segment,
                    children,
                    scope_depth,
                    *opacity,
                    clip_gpu::GpuRasterBlendMode::Normal,
                    *mask_key,
                    clip_gpu::GpuSparseAtlasScopeEventKind::Through,
                    ClippingRunPolicy::None,
                )?;
                current_bounds = Some(match current_bounds {
                    Some(current) => union_rect(current, bounds),
                    None => bounds,
                });
                tile_events.extend(scope_events);
            }
            _ => return Err(SparseAtlasRasterEventSkipReason::NonRasterRun),
        }
    }
    let Some(bounds) = current_bounds else {
        return Err(SparseAtlasRasterEventSkipReason::EmptyRasterSlots);
    };
    Ok((tile_events, bounds))
}

fn nested_scope_tile_events(
    segment: &SparseAtlasRerunSegment,
    children: &[clip_gpu::GpuNormalStackSource],
    parent_scope_depth: usize,
    opacity: f32,
    blend_mode: clip_gpu::GpuRasterBlendMode,
    mask_key: Option<clip_gpu::GpuMaskResourceKey>,
    kind: clip_gpu::GpuSparseAtlasScopeEventKind,
    clipping_run_policy: ClippingRunPolicy,
) -> Result<(Vec<clip_gpu::GpuSparseAtlasTileEvent>, Rect), SparseAtlasRasterEventSkipReason> {
    if parent_scope_depth >= SPARSE_SCOPE_STACK_LIMIT {
        return Err(SparseAtlasRasterEventSkipReason::ScopeDepthLimitExceeded);
    }
    let (child_events, bounds) = scope_child_tile_events_at_depth(
        segment,
        children,
        parent_scope_depth + 1,
        clipping_run_policy,
    )?;
    let scope = clip_gpu::GpuSparseAtlasScopeEvent {
        kind,
        opacity,
        blend_mode,
        local_bounds: bounds,
        mask: scope_mask_tile_ref_for_bounds(segment, mask_key, bounds)?,
    };
    let mut tile_events = Vec::with_capacity(child_events.len() + 2);
    tile_events.push(clip_gpu::GpuSparseAtlasTileEvent::BeginScope(scope));
    tile_events.extend(child_events);
    tile_events.push(clip_gpu::GpuSparseAtlasTileEvent::EndScope(scope));
    Ok((tile_events, bounds))
}

fn push_clipping_run_tile_events(
    segment: &SparseAtlasRerunSegment,
    base: clip_gpu::GpuNormalRasterSource,
    clipped: &[clip_gpu::GpuClippedStackSource],
    tile_events: &mut Vec<clip_gpu::GpuSparseAtlasTileEvent>,
) -> Result<Rect, SparseAtlasRasterEventSkipReason> {
    let mut base_events = Vec::new();
    push_raster_events_for_source(segment, base, &mut base_events)?;
    let base_bounds = raster_event_bounds(&base_events)?;
    let clip_scope = clip_gpu::GpuSparseAtlasScopeEvent {
        kind: clip_gpu::GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: base.blend_mode,
        local_bounds: base_bounds,
        mask: None,
    };
    tile_events.push(clip_gpu::GpuSparseAtlasTileEvent::BeginClipBase(clip_scope));
    tile_events.extend(
        base_events
            .into_iter()
            .map(clip_gpu::GpuSparseAtlasTileEvent::ClipBaseRaster),
    );
    for clipped_source in clipped {
        let clip_gpu::GpuClippedStackSource::Raster(raster) = clipped_source else {
            return Err(SparseAtlasRasterEventSkipReason::NonRasterRun);
        };
        let mut clipped_events = Vec::new();
        push_raster_events_for_source(segment, *raster, &mut clipped_events)?;
        tile_events.extend(
            clipped_events
                .into_iter()
                .map(clip_gpu::GpuSparseAtlasTileEvent::ClippedRaster),
        );
    }
    tile_events.push(clip_gpu::GpuSparseAtlasTileEvent::ResolveClipBase(
        clip_scope,
    ));
    Ok(base_bounds)
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

fn union_rect(left: Rect, right: Rect) -> Rect {
    let x0 = left.x.min(right.x);
    let y0 = left.y.min(right.y);
    let x1 = left
        .x
        .saturating_add(left.width)
        .max(right.x.saturating_add(right.width));
    let y1 = left
        .y
        .saturating_add(left.height)
        .max(right.y.saturating_add(right.height));
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

fn batch_masks_share_executor_atlas(batch: &clip_gpu::GpuSparseAtlasRasterEventBatch) -> bool {
    let mut key = None;
    for mask in batch
        .events
        .iter()
        .filter_map(|event| event.mask)
        .chain(batch.filters.iter().filter_map(|filter| filter.mask))
        .chain(batch.tile_events.iter().filter_map(tile_event_mask))
        .chain(batch.scope.into_iter().filter_map(|scope| scope.mask))
    {
        if let Some(current) = key {
            if current != mask.key {
                return false;
            }
        } else {
            key = Some(mask.key);
        }
    }
    true
}

fn tile_event_mask(
    event: &clip_gpu::GpuSparseAtlasTileEvent,
) -> Option<clip_gpu::GpuSparseAtlasTileRef> {
    match event {
        clip_gpu::GpuSparseAtlasTileEvent::Raster(event)
        | clip_gpu::GpuSparseAtlasTileEvent::ClipBaseRaster(event)
        | clip_gpu::GpuSparseAtlasTileEvent::ClippedRaster(event) => event.mask,
        clip_gpu::GpuSparseAtlasTileEvent::PointFilter(filter) => filter.mask,
        clip_gpu::GpuSparseAtlasTileEvent::BeginScope(scope)
        | clip_gpu::GpuSparseAtlasTileEvent::EndScope(scope)
        | clip_gpu::GpuSparseAtlasTileEvent::BeginClipBase(scope)
        | clip_gpu::GpuSparseAtlasTileEvent::ResolveClipBase(scope) => scope.mask,
    }
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
