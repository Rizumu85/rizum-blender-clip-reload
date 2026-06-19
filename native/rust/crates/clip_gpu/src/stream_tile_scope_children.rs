use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::{CanvasRect, union_optional};
use crate::stream_context::StreamingExecutionContext;
use crate::stream_tile_event::{
    PointFilterTileEventPayload, ScopeTileEventPayload, TileEventPayload,
};
use crate::stream_tile_filter_silo::{filter_mask_can_lower, raster_payload};
use crate::stream_tile_mask_atlas_plan::MaskAtlasPlan;
use crate::stream_tile_scope_silo_plan::{
    SIMPLE_CONTAINER_SCOPE_DEPTH_LIMIT, SIMPLE_THROUGH_SCOPE_DEPTH_LIMIT,
};
use crate::stream_tile_scope_silo_rules::scope_mask_can_lower;
use crate::stream_tile_silo_plan::PreparedSiloSource;
use crate::stream_utils::local_pass_bounds;
use crate::{
    GpuClippedStackSource, GpuMaskResourceKey, GpuNormalStackSource, GpuRasterBlendMode,
    GpuRenderError,
};

#[derive(Clone, Copy)]
pub(crate) enum ClippingRunPolicy {
    None,
    DirectOnly,
    NestedContainers,
}

impl ClippingRunPolicy {
    pub(crate) fn allows_current(self) -> bool {
        matches!(self, Self::DirectOnly | Self::NestedContainers)
    }

    pub(crate) fn for_nested_container(self) -> Self {
        match self {
            Self::NestedContainers => Self::NestedContainers,
            Self::DirectOnly => Self::DirectOnly,
            Self::None => Self::None,
        }
    }
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
    pub(crate) fn container_depth_remaining(self) -> usize {
        match self {
            Self::Container { .. } => SIMPLE_CONTAINER_SCOPE_DEPTH_LIMIT - 1,
            Self::Through { .. } => SIMPLE_CONTAINER_SCOPE_DEPTH_LIMIT,
        }
    }

    pub(crate) fn through_depth_remaining(self) -> usize {
        match self {
            Self::Container { .. } => 1,
            Self::Through { .. } => SIMPLE_THROUGH_SCOPE_DEPTH_LIMIT - 1,
        }
    }

    pub(crate) fn clipping_run_policy(self) -> ClippingRunPolicy {
        match self {
            Self::Container { .. } => ClippingRunPolicy::NestedContainers,
            Self::Through { .. } => ClippingRunPolicy::DirectOnly,
        }
    }

    pub(crate) fn mask_key(self) -> Option<GpuMaskResourceKey> {
        match self {
            Self::Container { mask_key, .. } | Self::Through { mask_key, .. } => mask_key,
        }
    }

    pub(crate) fn payloads(
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn append_scope_child_events<'a, P>(
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
                append_raster_payloads(raster, prepared, payloads, event_bounds, scope_dirty);
            }
            GpuNormalStackSource::Container {
                children,
                opacity,
                mask_key,
                blend_mode,
            } if container_depth_remaining > 0 => {
                if !append_nested_scope_events(
                    context,
                    target_origin,
                    ScopeProgramKind::Container {
                        opacity: *opacity,
                        blend_mode: *blend_mode,
                        mask_key: *mask_key,
                    },
                    container_depth_remaining - 1,
                    through_depth_remaining,
                    clipping_run_policy.for_nested_container(),
                    children,
                    prepared,
                    payloads,
                    event_bounds,
                    lut_rows,
                    mask_plan,
                    scope_dirty,
                )? {
                    return Ok(false);
                }
            }
            GpuNormalStackSource::ThroughGroup {
                children,
                opacity,
                mask_key,
            } if through_depth_remaining > 0 => {
                if !append_nested_scope_events(
                    context,
                    target_origin,
                    ScopeProgramKind::Through {
                        opacity: *opacity,
                        mask_key: *mask_key,
                    },
                    container_depth_remaining,
                    through_depth_remaining - 1,
                    ClippingRunPolicy::DirectOnly,
                    children,
                    prepared,
                    payloads,
                    event_bounds,
                    lut_rows,
                    mask_plan,
                    scope_dirty,
                )? {
                    return Ok(false);
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
                if !clipping_run_policy.allows_current() {
                    return Ok(false);
                }
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
                if !append_clipped_sibling_events(
                    context,
                    target_origin,
                    container_depth_remaining,
                    clipping_run_policy,
                    clipped,
                    prepared,
                    payloads,
                    event_bounds,
                    lut_rows,
                    mask_plan,
                )? {
                    return Ok(false);
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

#[allow(clippy::too_many_arguments)]
fn append_nested_scope_events<'a, P>(
    context: &StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    kind: ScopeProgramKind,
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
    if !scope_header_can_lower(context.provider, kind, children) {
        return Ok(false);
    }
    let mut nested_payloads = Vec::new();
    let mut nested_bounds = Vec::new();
    let mut nested_dirty = None;
    if !append_scope_child_events(
        context,
        target_origin,
        container_depth_remaining,
        through_depth_remaining,
        clipping_run_policy,
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
        return Ok(true);
    };
    let local_scope_bounds = local_pass_bounds(nested_scope_bounds, target_origin);
    let Some(mask_atlas_origin) = mask_plan.append_mask(
        context.provider,
        kind.mask_key(),
        clip_model::Rect::new(
            nested_scope_bounds.x,
            nested_scope_bounds.y,
            nested_scope_bounds.width,
            nested_scope_bounds.height,
        ),
    ) else {
        return Ok(false);
    };
    let (begin_scope, end_scope) = kind.payloads(local_scope_bounds, mask_atlas_origin);
    payloads.push(begin_scope);
    event_bounds.push(local_scope_bounds);
    payloads.extend(nested_payloads);
    event_bounds.extend(nested_bounds);
    payloads.push(end_scope);
    event_bounds.push(local_scope_bounds);
    *scope_dirty = union_optional(*scope_dirty, Some(nested_scope_bounds));
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn append_clipped_sibling_events<'a, P>(
    context: &StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    container_depth_remaining: usize,
    _clipping_run_policy: ClippingRunPolicy,
    clipped: &'a [GpuClippedStackSource],
    prepared: &[PreparedSiloSource],
    payloads: &mut Vec<TileEventPayload>,
    event_bounds: &mut Vec<CanvasRect>,
    lut_rows: &mut Vec<&'a [u8]>,
    mask_plan: &mut MaskAtlasPlan,
) -> Result<bool, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    for clipped_source in clipped {
        match clipped_source {
            GpuClippedStackSource::Raster(raster) => {
                for prepared_source in prepared.iter().filter(|item| item.source == *raster) {
                    payloads.push(TileEventPayload::ClippedRaster(raster_payload(
                        prepared_source,
                    )));
                    event_bounds.push(prepared_source.local_bounds);
                }
            }
            GpuClippedStackSource::Container {
                children,
                opacity,
                mask_key,
                blend_mode,
                ..
            } => {
                if container_depth_remaining == 0 {
                    return Ok(false);
                }
                let kind = ScopeProgramKind::Container {
                    opacity: *opacity,
                    blend_mode: *blend_mode,
                    mask_key: *mask_key,
                };
                if !scope_header_can_lower(context.provider, kind, children) {
                    return Ok(false);
                }
                let mut child_payloads = Vec::new();
                let mut child_bounds = Vec::new();
                let mut child_dirty = None;
                if !append_scope_child_events(
                    context,
                    target_origin,
                    container_depth_remaining - 1,
                    1,
                    ClippingRunPolicy::DirectOnly,
                    children,
                    prepared,
                    &mut child_payloads,
                    &mut child_bounds,
                    lut_rows,
                    mask_plan,
                    &mut child_dirty,
                )? {
                    return Ok(false);
                }
                let Some(scope_bounds) = context.state.clip_pass_bounds(child_dirty) else {
                    continue;
                };
                let local_scope_bounds = local_pass_bounds(scope_bounds, target_origin);
                let Some(mask_atlas_origin) = mask_plan.append_mask(
                    context.provider,
                    *mask_key,
                    clip_model::Rect::new(
                        scope_bounds.x,
                        scope_bounds.y,
                        scope_bounds.width,
                        scope_bounds.height,
                    ),
                ) else {
                    return Ok(false);
                };
                let scope = ScopeTileEventPayload {
                    opacity: *opacity,
                    blend_mode: *blend_mode,
                    local_bounds: local_scope_bounds,
                    mask_atlas_origin,
                };
                payloads.push(TileEventPayload::BeginClippedContainer(scope));
                event_bounds.push(local_scope_bounds);
                payloads.extend(child_payloads);
                event_bounds.extend(child_bounds);
                payloads.push(TileEventPayload::EndClippedContainer(scope));
                event_bounds.push(local_scope_bounds);
            }
        }
    }
    Ok(true)
}

fn append_raster_payloads(
    raster: &crate::GpuNormalRasterSource,
    prepared: &[PreparedSiloSource],
    payloads: &mut Vec<TileEventPayload>,
    event_bounds: &mut Vec<CanvasRect>,
    scope_dirty: &mut Option<CanvasRect>,
) {
    for prepared_source in prepared.iter().filter(|item| item.source == *raster) {
        payloads.push(TileEventPayload::Raster(raster_payload(prepared_source)));
        event_bounds.push(prepared_source.local_bounds);
        *scope_dirty = union_optional(*scope_dirty, Some(prepared_source.bounds));
    }
}

fn scope_header_can_lower<P>(
    provider: &P,
    kind: ScopeProgramKind,
    children: &[GpuNormalStackSource],
) -> bool
where
    P: GpuNormalStackResourceProvider,
{
    match kind {
        ScopeProgramKind::Container {
            opacity,
            mask_key,
            blend_mode,
        } => {
            opacity > 0.0
                && !children.is_empty()
                && scope_mask_can_lower(provider, mask_key)
                && crate::stream_tile_scope_silo_rules::container_resolve_is_scope_eligible(
                    blend_mode,
                )
        }
        ScopeProgramKind::Through {
            opacity, mask_key, ..
        } => opacity > 0.0 && !children.is_empty() && scope_mask_can_lower(provider, mask_key),
    }
}
