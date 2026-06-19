use clip_model::CanvasSize;

use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::{CanvasRect, union_optional};
use crate::stream_clipping_tile_silo::clipping_run_as_normal_sources;
use crate::stream_context::StreamingExecutionContext;
use crate::stream_tile_event::{
    PointFilterTileEventPayload, ScopeTileEventPayload, TileEventPayload,
};
use crate::stream_tile_filter_silo::{filter_mask_can_lower, raster_payload};
use crate::stream_tile_mask_atlas_plan::MaskAtlasPlan;
use crate::stream_tile_scope_silo_plan::{
    SIMPLE_CONTAINER_SCOPE_DEPTH_LIMIT, SIMPLE_THROUGH_SCOPE_DEPTH_LIMIT,
};
use crate::stream_tile_scope_silo_rules::{
    scope_mask_can_lower, simple_scope_clipping_run_can_lower,
};
use crate::stream_tile_silo_plan::PreparedSiloSource;
use crate::stream_utils::local_pass_bounds;
use crate::{
    GpuMaskAtlasSource, GpuMaskResourceKey, GpuNormalStackSource, GpuRasterBlendMode,
    GpuRenderError,
};

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

#[derive(Clone)]
pub(crate) struct ScopeProgramInputs<'a> {
    pub(crate) payloads: Vec<TileEventPayload>,
    pub(crate) event_bounds: Vec<CanvasRect>,
    pub(crate) lut_rows: Vec<&'a [u8]>,
    pub(crate) scope_mask_sources: Vec<GpuMaskAtlasSource>,
    pub(crate) mask_atlas_size: CanvasSize,
    pub(crate) final_dirty_bounds: Option<CanvasRect>,
}

pub(crate) fn build_scope_event_program_inputs<'a, P>(
    context: &StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    kind: ScopeProgramKind,
    children: &'a [GpuNormalStackSource],
    prepared: Vec<PreparedSiloSource>,
    base_mask_atlas_size: CanvasSize,
    max_mask_atlas_size: u32,
    initial_dirty_bounds: Option<CanvasRect>,
) -> Result<Option<ScopeProgramInputs<'a>>, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let mut child_payloads = Vec::new();
    let mut child_bounds = Vec::new();
    let mut lut_rows = Vec::new();
    let mut scope_dirty = None;
    let mut mask_plan = MaskAtlasPlan::new(base_mask_atlas_size, max_mask_atlas_size);
    if !append_scope_child_events(
        context,
        target_origin,
        kind.container_depth_remaining(),
        kind.through_depth_remaining(),
        kind.clipping_run_policy(),
        children,
        &prepared,
        &mut child_payloads,
        &mut child_bounds,
        &mut lut_rows,
        &mut mask_plan,
        &mut scope_dirty,
    )? {
        return Ok(None);
    }

    let Some(scope_bounds) = context.state.clip_pass_bounds(scope_dirty) else {
        return Ok(None);
    };
    let local_scope_bounds = local_pass_bounds(scope_bounds, target_origin);
    let Some(mask_atlas_origin) = mask_plan.append_mask(
        context.provider,
        kind.mask_key(),
        clip_model::Rect::new(
            scope_bounds.x,
            scope_bounds.y,
            scope_bounds.width,
            scope_bounds.height,
        ),
    ) else {
        return Ok(None);
    };
    let (begin_scope, end_scope) = kind.payloads(local_scope_bounds, mask_atlas_origin);
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
    let mask_atlas_size = mask_plan.size();
    Ok(Some(ScopeProgramInputs {
        payloads,
        event_bounds,
        lut_rows,
        scope_mask_sources: mask_plan.into_sources(),
        mask_atlas_size,
        final_dirty_bounds: union_optional(initial_dirty_bounds, Some(scope_bounds)),
    }))
}

#[allow(clippy::too_many_arguments)]
fn append_scope_child_events<'a, P>(
    context: &StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    container_depth_remaining: usize,
    through_depth_remaining: usize,
    clipping_run_policy: ClippingRunPolicy,
    children: &'a [GpuNormalStackSource],
    prepared: &[PreparedSiloSource],
    payloads: &mut Vec<TileEventPayload>,
    event_bounds: &mut Vec<CanvasRect>,
    lut_rows: &mut Vec<&'a [u8]>,
    mask_plan: &mut MaskAtlasPlan,
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
                if *opacity <= 0.0
                    || !scope_mask_can_lower(context.provider, *mask_key)
                    || children.is_empty()
                {
                    return Ok(false);
                }
                let mut nested_payloads = Vec::new();
                let mut nested_bounds = Vec::new();
                let mut nested_dirty = None;
                if !append_scope_child_events(
                    context,
                    target_origin,
                    container_depth_remaining - 1,
                    through_depth_remaining,
                    clipping_run_policy.for_nested_container(),
                    children,
                    prepared,
                    &mut nested_payloads,
                    &mut nested_bounds,
                    lut_rows,
                    mask_plan,
                    &mut nested_dirty,
                )? {
                    return Ok(false);
                }
                let Some(nested_scope_bounds) = context.state.clip_pass_bounds(nested_dirty) else {
                    continue;
                };
                let local_scope_bounds = local_pass_bounds(nested_scope_bounds, target_origin);
                let Some(mask_atlas_origin) = mask_plan.append_mask(
                    context.provider,
                    *mask_key,
                    clip_model::Rect::new(
                        nested_scope_bounds.x,
                        nested_scope_bounds.y,
                        nested_scope_bounds.width,
                        nested_scope_bounds.height,
                    ),
                ) else {
                    return Ok(false);
                };
                let (begin_scope, end_scope) = ScopeProgramKind::Container {
                    opacity: *opacity,
                    blend_mode: *blend_mode,
                    mask_key: *mask_key,
                }
                .payloads(local_scope_bounds, mask_atlas_origin);
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
                if *opacity <= 0.0
                    || !scope_mask_can_lower(context.provider, *mask_key)
                    || children.is_empty()
                {
                    return Ok(false);
                }
                let mut nested_payloads = Vec::new();
                let mut nested_bounds = Vec::new();
                let mut nested_dirty = None;
                if !append_scope_child_events(
                    context,
                    target_origin,
                    container_depth_remaining,
                    through_depth_remaining - 1,
                    ClippingRunPolicy::DirectOnly,
                    children,
                    prepared,
                    &mut nested_payloads,
                    &mut nested_bounds,
                    lut_rows,
                    mask_plan,
                    &mut nested_dirty,
                )? {
                    return Ok(false);
                }
                let Some(nested_scope_bounds) = context.state.clip_pass_bounds(nested_dirty) else {
                    continue;
                };
                let local_scope_bounds = local_pass_bounds(nested_scope_bounds, target_origin);
                let Some(mask_atlas_origin) = mask_plan.append_mask(
                    context.provider,
                    *mask_key,
                    clip_model::Rect::new(
                        nested_scope_bounds.x,
                        nested_scope_bounds.y,
                        nested_scope_bounds.width,
                        nested_scope_bounds.height,
                    ),
                ) else {
                    return Ok(false);
                };
                let (begin_scope, end_scope) = ScopeProgramKind::Through {
                    opacity: *opacity,
                    mask_key: *mask_key,
                }
                .payloads(local_scope_bounds, mask_atlas_origin);
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
                let Some(mask_atlas_origin) = mask_plan.append_mask(
                    context.provider,
                    *mask_key,
                    clip_model::Rect::new(
                        filter_bounds.x,
                        filter_bounds.y,
                        filter_bounds.width,
                        filter_bounds.height,
                    ),
                ) else {
                    return Ok(false);
                };
                let lut_row = u32::try_from(lut_rows.len())
                    .map_err(|_| GpuRenderError::TextureSizeOverflow)
                    .map_err(P::Error::from)?;
                payloads.push(TileEventPayload::PointFilter(PointFilterTileEventPayload {
                    lut_row,
                    opacity: *opacity,
                    filter_mode: *filter_mode,
                    local_bounds,
                    mask_atlas_origin,
                }));
                event_bounds.push(local_bounds);
                lut_rows.push(lut_rgba.as_slice());
                *scope_dirty = Some(filter_bounds);
            }
            GpuNormalStackSource::ClippingRun { base, clipped } => {
                if !clipping_run_policy.allows_current()
                    || !simple_scope_clipping_run_can_lower(clipped)
                {
                    return Ok(false);
                }
                let Some(normal_sources) = clipping_run_as_normal_sources(*base, clipped) else {
                    return Ok(false);
                };
                let base_prepared: Vec<_> = prepared
                    .iter()
                    .filter(|item| item.source.key == base.key)
                    .collect();
                let Some(base_bounds) = base_prepared
                    .iter()
                    .map(|source| source.bounds)
                    .reduce(CanvasRect::union)
                else {
                    return Ok(false);
                };
                let local_clip_bounds = local_pass_bounds(base_bounds, target_origin);
                let clip_scope = ScopeTileEventPayload {
                    opacity: 1.0,
                    blend_mode: base.blend_mode,
                    local_bounds: local_clip_bounds,
                    mask_atlas_origin: None,
                };
                payloads.push(TileEventPayload::BeginClipBase(clip_scope));
                event_bounds.push(local_clip_bounds);
                for prepared_source in base_prepared {
                    payloads.push(TileEventPayload::ClipBaseRaster(raster_payload(
                        prepared_source,
                    )));
                    event_bounds.push(prepared_source.local_bounds);
                }
                for source in normal_sources.iter().skip(1) {
                    let GpuNormalStackSource::Raster(raster) = source else {
                        return Ok(false);
                    };
                    for prepared_source in prepared.iter().filter(|item| item.source == *raster) {
                        payloads.push(TileEventPayload::ClippedRaster(raster_payload(
                            prepared_source,
                        )));
                        event_bounds.push(prepared_source.local_bounds);
                    }
                }
                payloads.push(TileEventPayload::ResolveClipBase(clip_scope));
                event_bounds.push(local_clip_bounds);
                *scope_dirty = union_optional(*scope_dirty, Some(base_bounds));
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
        mask_key: Option<GpuMaskResourceKey>,
    },
    Through {
        opacity: f32,
        mask_key: Option<GpuMaskResourceKey>,
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
            Self::Container { .. } => 1,
            Self::Through { .. } => SIMPLE_THROUGH_SCOPE_DEPTH_LIMIT - 1,
        }
    }

    fn clipping_run_policy(self) -> ClippingRunPolicy {
        match self {
            Self::Container { .. } => ClippingRunPolicy::NestedContainers,
            Self::Through { .. } => ClippingRunPolicy::DirectOnly,
        }
    }

    fn mask_key(self) -> Option<GpuMaskResourceKey> {
        match self {
            Self::Container { mask_key, .. } | Self::Through { mask_key, .. } => mask_key,
        }
    }

    fn payloads(
        self,
        local_bounds: CanvasRect,
        mask_atlas_origin: Option<(u32, u32)>,
    ) -> (TileEventPayload, TileEventPayload) {
        match self {
            Self::Container {
                opacity,
                blend_mode,
                ..
            } => {
                let scope = ScopeTileEventPayload {
                    opacity,
                    blend_mode,
                    local_bounds,
                    mask_atlas_origin,
                };
                (
                    TileEventPayload::BeginContainer(scope),
                    TileEventPayload::EndContainer(scope),
                )
            }
            Self::Through { opacity, .. } => {
                let scope = ScopeTileEventPayload {
                    opacity,
                    blend_mode: GpuRasterBlendMode::Normal,
                    local_bounds,
                    mask_atlas_origin,
                };
                (
                    TileEventPayload::BeginThrough(scope),
                    TileEventPayload::EndThrough(scope),
                )
            }
        }
    }
}

fn event_bounds_fit_target(bounds: &[CanvasRect], target_size: CanvasSize) -> bool {
    bounds.iter().all(|bounds| {
        bounds.width > 0
            && bounds.height > 0
            && bounds.x.saturating_add(bounds.width) <= target_size.width
            && bounds.y.saturating_add(bounds.height) <= target_size.height
    })
}
