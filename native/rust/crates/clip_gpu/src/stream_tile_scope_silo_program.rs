use clip_model::CanvasSize;

use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::{CanvasRect, union_optional};
use crate::stream_context::StreamingExecutionContext;
use crate::stream_tile_event::{
    PointFilterTileEventPayload, ScopeTileEventPayload, TileEventPayload,
};
use crate::stream_tile_filter_silo::{filter_mask_can_lower, raster_payload};
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

    for child in children {
        match child {
            GpuNormalStackSource::Raster(raster) => {
                for prepared_source in prepared.iter().filter(|item| item.source == *raster) {
                    child_payloads.push(TileEventPayload::Raster(raster_payload(prepared_source)));
                    child_bounds.push(prepared_source.local_bounds);
                    scope_dirty = union_optional(scope_dirty, Some(prepared_source.bounds));
                }
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
                    return Ok(None);
                }
                let Some(filter_bounds) = context.state.clip_pass_bounds(scope_dirty) else {
                    return Ok(None);
                };
                let local_bounds = local_pass_bounds(filter_bounds, target_origin);
                let lut_row = u32::try_from(lut_rows.len())
                    .map_err(|_| GpuRenderError::TextureSizeOverflow)
                    .map_err(P::Error::from)?;
                child_payloads.push(TileEventPayload::PointFilter(PointFilterTileEventPayload {
                    lut_row,
                    opacity: *opacity,
                    filter_mode: *filter_mode,
                    local_bounds,
                }));
                child_bounds.push(local_bounds);
                lut_rows.push(lut_rgba.as_slice());
                scope_dirty = Some(filter_bounds);
            }
            _ => return Ok(None),
        }
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
    children
        .iter()
        .filter_map(|child| match child {
            GpuNormalStackSource::Raster(raster) => Some(GpuNormalStackSource::Raster(*raster)),
            _ => None,
        })
        .collect()
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
