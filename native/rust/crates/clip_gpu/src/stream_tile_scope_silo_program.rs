use clip_model::CanvasSize;

use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::{CanvasRect, union_optional};
use crate::stream_context::StreamingExecutionContext;
use crate::stream_tile_event::{
    PointFilterTileEventPayload, ScopeTileEventPayload, TileEventPayload,
};
use crate::stream_tile_filter_silo::{filter_mask_can_lower, raster_payload};
use crate::stream_tile_scope_silo_plan::{
    SIMPLE_CONTAINER_SCOPE_DEPTH_LIMIT, SIMPLE_THROUGH_SCOPE_DEPTH_LIMIT,
};
use crate::stream_tile_silo_plan::PreparedSiloSource;
use crate::stream_utils::local_pass_bounds;
use crate::{GpuNormalStackSource, GpuRasterBlendMode, GpuRenderError};

#[derive(Clone)]
pub(crate) struct ScopeProgramInputs<'a> {
    pub(crate) payloads: Vec<TileEventPayload>,
    pub(crate) event_bounds: Vec<CanvasRect>,
    pub(crate) lut_rows: Vec<&'a [u8]>,
    pub(crate) final_dirty_bounds: Option<CanvasRect>,
}

pub(crate) fn build_scope_event_program_inputs<'a, P>(
    context: &StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    kind: ScopeProgramKind,
    children: &'a [GpuNormalStackSource],
    prepared: Vec<PreparedSiloSource>,
    initial_dirty_bounds: Option<CanvasRect>,
) -> Result<Option<ScopeProgramInputs<'a>>, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let mut child_payloads = Vec::new();
    let mut child_bounds = Vec::new();
    let mut lut_rows = Vec::new();
    let mut scope_dirty = None;
    if !append_scope_child_events(
        context,
        target_origin,
        kind.container_depth_remaining(),
        kind.through_depth_remaining(),
        children,
        &prepared,
        &mut child_payloads,
        &mut child_bounds,
        &mut lut_rows,
        &mut scope_dirty,
    )? {
        return Ok(None);
    }

    let Some(scope_bounds) = context.state.clip_pass_bounds(scope_dirty) else {
        return Ok(None);
    };
    let local_scope_bounds = local_pass_bounds(scope_bounds, target_origin);
    let (begin_scope, end_scope) = kind.payloads(local_scope_bounds);
    let mut payloads = Vec::with_capacity(child_payloads.len() + 2);
    let mut event_bounds = Vec::with_capacity(child_bounds.len() + 2);
    payloads.push(begin_scope);
    event_bounds.push(local_scope_bounds);
    payloads.extend(child_payloads);
    event_bounds.extend(child_bounds);
    payloads.push(end_scope);
    event_bounds.push(local_scope_bounds);

    if !event_bounds_fit_target(&event_bounds, target_size) {
        return Ok(None);
    }
    Ok(Some(ScopeProgramInputs {
        payloads,
        event_bounds,
        lut_rows,
        final_dirty_bounds: union_optional(initial_dirty_bounds, Some(scope_bounds)),
    }))
}

#[allow(clippy::too_many_arguments)]
fn append_scope_child_events<'a, P>(
    context: &StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    container_depth_remaining: usize,
    through_depth_remaining: usize,
    children: &'a [GpuNormalStackSource],
    prepared: &[PreparedSiloSource],
    payloads: &mut Vec<TileEventPayload>,
    event_bounds: &mut Vec<CanvasRect>,
    lut_rows: &mut Vec<&'a [u8]>,
    scope_dirty: &mut Option<CanvasRect>,
) -> Result<bool, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    for child in children {
        match child {
            GpuNormalStackSource::Raster(raster) => {
                for prepared_source in prepared.iter().filter(|item| item.source == *raster) {
                    payloads.push(TileEventPayload::Raster(raster_payload(prepared_source)));
                    event_bounds.push(prepared_source.local_bounds);
                    *scope_dirty = union_optional(*scope_dirty, Some(prepared_source.bounds));
                }
            }
            GpuNormalStackSource::Container {
                children,
                opacity,
                mask_key,
                blend_mode,
            } if container_depth_remaining > 0 => {
                if *opacity <= 0.0 || mask_key.is_some() || children.is_empty() {
                    return Ok(false);
                }
                let mut nested_payloads = Vec::new();
                let mut nested_bounds = Vec::new();
                let mut nested_dirty = None;
                if !append_scope_child_events(
                    context,
                    target_origin,
                    container_depth_remaining - 1,
                    0,
                    children,
                    prepared,
                    &mut nested_payloads,
                    &mut nested_bounds,
                    lut_rows,
                    &mut nested_dirty,
                )? {
                    return Ok(false);
                }
                let Some(nested_scope_bounds) = context.state.clip_pass_bounds(nested_dirty) else {
                    continue;
                };
                let local_scope_bounds = local_pass_bounds(nested_scope_bounds, target_origin);
                let (begin_scope, end_scope) = ScopeProgramKind::Container {
                    opacity: *opacity,
                    blend_mode: *blend_mode,
                }
                .payloads(local_scope_bounds);
                payloads.push(begin_scope);
                event_bounds.push(local_scope_bounds);
                payloads.extend(nested_payloads);
                event_bounds.extend(nested_bounds);
                payloads.push(end_scope);
                event_bounds.push(local_scope_bounds);
                *scope_dirty = union_optional(*scope_dirty, Some(nested_scope_bounds));
            }
            GpuNormalStackSource::ThroughGroup {
                children,
                opacity,
                mask_key,
            } if through_depth_remaining > 0 => {
                if *opacity != 1.0 || mask_key.is_some() || children.is_empty() {
                    return Ok(false);
                }
                let mut nested_payloads = Vec::new();
                let mut nested_bounds = Vec::new();
                let mut nested_dirty = None;
                if !append_scope_child_events(
                    context,
                    target_origin,
                    SIMPLE_CONTAINER_SCOPE_DEPTH_LIMIT,
                    through_depth_remaining - 1,
                    children,
                    prepared,
                    &mut nested_payloads,
                    &mut nested_bounds,
                    lut_rows,
                    &mut nested_dirty,
                )? {
                    return Ok(false);
                }
                let Some(nested_scope_bounds) = context.state.clip_pass_bounds(nested_dirty) else {
                    continue;
                };
                let local_scope_bounds = local_pass_bounds(nested_scope_bounds, target_origin);
                let (begin_scope, end_scope) =
                    ScopeProgramKind::Through { opacity: *opacity }.payloads(local_scope_bounds);
                payloads.push(begin_scope);
                event_bounds.push(local_scope_bounds);
                payloads.extend(nested_payloads);
                event_bounds.extend(nested_bounds);
                payloads.push(end_scope);
                event_bounds.push(local_scope_bounds);
                *scope_dirty = union_optional(*scope_dirty, Some(nested_scope_bounds));
            }
            GpuNormalStackSource::LutFilter {
                lut_rgba,
                opacity,
                mask_key,
                filter_mode,
            } => {
                if !filter_mask_can_lower(context.provider, *mask_key)
                    || *opacity <= 0.0
                    || lut_rgba.len() != 256 * 4
                {
                    return Ok(false);
                }
                let Some(filter_bounds) = context.state.clip_pass_bounds(*scope_dirty) else {
                    return Ok(false);
                };
                let local_bounds = local_pass_bounds(filter_bounds, target_origin);
                let lut_row = u32::try_from(lut_rows.len())
                    .map_err(|_| GpuRenderError::TextureSizeOverflow)
                    .map_err(P::Error::from)?;
                payloads.push(TileEventPayload::PointFilter(PointFilterTileEventPayload {
                    lut_row,
                    opacity: *opacity,
                    filter_mode: *filter_mode,
                    local_bounds,
                }));
                event_bounds.push(local_bounds);
                lut_rows.push(lut_rgba.as_slice());
                *scope_dirty = Some(filter_bounds);
            }
            _ => return Ok(false),
        }
    }
    Ok(true)
}

#[derive(Clone, Copy)]
pub(crate) enum ScopeProgramKind {
    Container {
        opacity: f32,
        blend_mode: GpuRasterBlendMode,
    },
    Through {
        opacity: f32,
    },
}

impl ScopeProgramKind {
    fn container_depth_remaining(self) -> usize {
        match self {
            Self::Container { .. } => SIMPLE_CONTAINER_SCOPE_DEPTH_LIMIT - 1,
            Self::Through { .. } => SIMPLE_CONTAINER_SCOPE_DEPTH_LIMIT,
        }
    }

    fn through_depth_remaining(self) -> usize {
        match self {
            Self::Container { .. } => 0,
            Self::Through { .. } => SIMPLE_THROUGH_SCOPE_DEPTH_LIMIT - 1,
        }
    }

    fn payloads(self, local_bounds: CanvasRect) -> (TileEventPayload, TileEventPayload) {
        match self {
            Self::Container {
                opacity,
                blend_mode,
            } => {
                let scope = ScopeTileEventPayload {
                    opacity,
                    blend_mode,
                    local_bounds,
                };
                (
                    TileEventPayload::BeginContainer(scope),
                    TileEventPayload::EndContainer(scope),
                )
            }
            Self::Through { opacity } => {
                let scope = ScopeTileEventPayload {
                    opacity,
                    blend_mode: GpuRasterBlendMode::Normal,
                    local_bounds,
                };
                (
                    TileEventPayload::BeginThrough(scope),
                    TileEventPayload::EndThrough(scope),
                )
            }
        }
    }
}

pub(crate) fn raster_sources_from_scope_children(
    children: &[GpuNormalStackSource],
) -> Vec<GpuNormalStackSource> {
    let mut sources = Vec::new();
    append_raster_sources(children, &mut sources);
    sources
}

fn append_raster_sources(
    children: &[GpuNormalStackSource],
    sources: &mut Vec<GpuNormalStackSource>,
) {
    for child in children {
        match child {
            GpuNormalStackSource::Raster(raster) => {
                sources.push(GpuNormalStackSource::Raster(*raster));
            }
            GpuNormalStackSource::Container { children, .. } => {
                append_raster_sources(children, sources);
            }
            GpuNormalStackSource::ThroughGroup { children, .. } => {
                append_raster_sources(children, sources);
            }
            _ => {}
        }
    }
}

pub(crate) fn raster_sources_have_masks(sources: &[GpuNormalStackSource]) -> bool {
    sources.iter().any(|source| match source {
        GpuNormalStackSource::Raster(raster) => raster.mask_key.is_some(),
        _ => false,
    })
}

fn event_bounds_fit_target(bounds: &[CanvasRect], target_size: CanvasSize) -> bool {
    bounds.iter().all(|bounds| {
        bounds.width > 0
            && bounds.height > 0
            && bounds.x.saturating_add(bounds.width) <= target_size.width
            && bounds.y.saturating_add(bounds.height) <= target_size.height
    })
}
