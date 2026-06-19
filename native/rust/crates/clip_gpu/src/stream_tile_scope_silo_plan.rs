use clip_model::CanvasSize;

use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::target_canvas_bounds;
use crate::stream_clipping_tile_silo::clipping_run_as_normal_sources;
use crate::stream_extents::{KnownStackBounds, known_stack_bounds};
use crate::stream_tile_filter_silo::filter_mask_can_lower;
use crate::stream_tile_silo_plan::{MAX_SILO_EVENTS, source_is_silo_eligible};
use crate::{GpuNormalStackSource, GpuRasterBlendMode};

pub(crate) const SIMPLE_CONTAINER_SCOPE_DEPTH_LIMIT: usize = 3;
pub(crate) const SIMPLE_THROUGH_SCOPE_DEPTH_LIMIT: usize = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SimpleScopeBarrierHint {
    ScopeDepthLimitExceeded,
    TileEventLimitExceeded,
    FilterNotLowered,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SimpleScopeReject {
    NotSimple,
    ScopeDepthLimitExceeded,
    TileEventLimitExceeded,
    FilterNotLowered,
}

#[derive(Clone, Copy)]
enum ThroughBudget {
    Remaining(usize),
}

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

pub(crate) fn simple_container_scope_event_count<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    source: &GpuNormalStackSource,
) -> Option<usize>
where
    P: GpuNormalStackResourceProvider,
{
    simple_container_scope_event_count_result(
        provider,
        output_size,
        target_origin,
        target_size,
        source,
    )
    .ok()
}

pub(crate) fn simple_through_scope_event_count<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    source: &GpuNormalStackSource,
) -> Option<usize>
where
    P: GpuNormalStackResourceProvider,
{
    simple_through_scope_event_count_result(
        provider,
        output_size,
        target_origin,
        target_size,
        source,
    )
    .ok()
}

pub(crate) fn simple_scope_barrier_hint<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    source: &GpuNormalStackSource,
) -> Option<SimpleScopeBarrierHint>
where
    P: GpuNormalStackResourceProvider,
{
    let result = match source {
        GpuNormalStackSource::Container { .. } => simple_container_scope_event_count_result(
            provider,
            output_size,
            target_origin,
            target_size,
            source,
        ),
        GpuNormalStackSource::ThroughGroup { .. } => simple_through_scope_event_count_result(
            provider,
            output_size,
            target_origin,
            target_size,
            source,
        ),
        _ => return None,
    };
    match result {
        Err(SimpleScopeReject::ScopeDepthLimitExceeded) => {
            Some(SimpleScopeBarrierHint::ScopeDepthLimitExceeded)
        }
        Err(SimpleScopeReject::TileEventLimitExceeded) => {
            Some(SimpleScopeBarrierHint::TileEventLimitExceeded)
        }
        Err(SimpleScopeReject::FilterNotLowered) => Some(SimpleScopeBarrierHint::FilterNotLowered),
        _ => None,
    }
}

fn simple_container_scope_event_count_result<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    source: &GpuNormalStackSource,
) -> Result<usize, SimpleScopeReject>
where
    P: GpuNormalStackResourceProvider,
{
    let GpuNormalStackSource::Container {
        children,
        opacity,
        mask_key,
        blend_mode,
    } = source
    else {
        return Err(SimpleScopeReject::NotSimple);
    };
    ensure_container_scope_header(provider, *opacity, *mask_key, children, *blend_mode)?;
    let KnownStackBounds::Bounded(bounds) = known_stack_bounds(provider, children, output_size)
    else {
        return Err(SimpleScopeReject::NotSimple);
    };
    let _ = bounds
        .intersection(
            target_canvas_bounds(target_origin, target_size).ok_or(SimpleScopeReject::NotSimple)?,
        )
        .ok_or(SimpleScopeReject::NotSimple)?;

    let child_count = simple_scope_children_event_count(
        provider,
        output_size,
        target_origin,
        target_size,
        children,
        SIMPLE_CONTAINER_SCOPE_DEPTH_LIMIT - 1,
        ThroughBudget::Remaining(1),
        ClippingRunPolicy::NestedContainers,
    )?;
    checked_scope_event_count(2, child_count)
}

fn simple_through_scope_event_count_result<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    source: &GpuNormalStackSource,
) -> Result<usize, SimpleScopeReject>
where
    P: GpuNormalStackResourceProvider,
{
    let GpuNormalStackSource::ThroughGroup {
        children,
        opacity,
        mask_key,
    } = source
    else {
        return Err(SimpleScopeReject::NotSimple);
    };
    ensure_through_scope_header(provider, *opacity, *mask_key, children)?;
    let KnownStackBounds::Bounded(bounds) = known_stack_bounds(provider, children, output_size)
    else {
        return Err(SimpleScopeReject::NotSimple);
    };
    let _ = bounds
        .intersection(
            target_canvas_bounds(target_origin, target_size).ok_or(SimpleScopeReject::NotSimple)?,
        )
        .ok_or(SimpleScopeReject::NotSimple)?;

    let child_count = simple_scope_children_event_count(
        provider,
        output_size,
        target_origin,
        target_size,
        children,
        SIMPLE_CONTAINER_SCOPE_DEPTH_LIMIT,
        ThroughBudget::Remaining(SIMPLE_THROUGH_SCOPE_DEPTH_LIMIT - 1),
        ClippingRunPolicy::DirectOnly,
    )?;
    checked_scope_event_count(2, child_count)
}

fn simple_scope_children_event_count<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    children: &[GpuNormalStackSource],
    container_depth_remaining: usize,
    through_budget: ThroughBudget,
    clipping_run_policy: ClippingRunPolicy,
) -> Result<usize, SimpleScopeReject>
where
    P: GpuNormalStackResourceProvider,
{
    let mut count = 0usize;
    let mut saw_raster = false;
    for child in children {
        match child {
            GpuNormalStackSource::Raster(_) => {
                if !source_is_silo_eligible(
                    provider,
                    output_size,
                    target_origin,
                    target_size,
                    child,
                ) {
                    return Err(SimpleScopeReject::NotSimple);
                }
                saw_raster = true;
                count = add_scope_events(count, 1)?;
            }
            GpuNormalStackSource::Container {
                children,
                opacity,
                mask_key,
                blend_mode,
            } => {
                if container_depth_remaining == 0 {
                    ensure_container_scope_header(
                        provider,
                        *opacity,
                        *mask_key,
                        children,
                        *blend_mode,
                    )?;
                    return Err(SimpleScopeReject::ScopeDepthLimitExceeded);
                }
                ensure_container_scope_header(
                    provider,
                    *opacity,
                    *mask_key,
                    children,
                    *blend_mode,
                )?;
                let KnownStackBounds::Bounded(bounds) =
                    known_stack_bounds(provider, children, output_size)
                else {
                    return Err(SimpleScopeReject::NotSimple);
                };
                let _ = bounds
                    .intersection(
                        target_canvas_bounds(target_origin, target_size)
                            .ok_or(SimpleScopeReject::NotSimple)?,
                    )
                    .ok_or(SimpleScopeReject::NotSimple)?;
                let child_count = simple_scope_children_event_count(
                    provider,
                    output_size,
                    target_origin,
                    target_size,
                    children,
                    container_depth_remaining - 1,
                    through_budget,
                    clipping_run_policy.for_nested_container(),
                )?;
                count = add_scope_events(count, 2)?;
                count = add_scope_events(count, child_count)?;
                saw_raster = true;
            }
            GpuNormalStackSource::ThroughGroup {
                children,
                opacity,
                mask_key,
            } => {
                let through_depth_remaining = match through_budget {
                    ThroughBudget::Remaining(0) => {
                        ensure_nested_through_scope_header(
                            provider, *opacity, *mask_key, children,
                        )?;
                        return Err(SimpleScopeReject::ScopeDepthLimitExceeded);
                    }
                    ThroughBudget::Remaining(remaining) => remaining,
                };
                ensure_nested_through_scope_header(provider, *opacity, *mask_key, children)?;
                let KnownStackBounds::Bounded(bounds) =
                    known_stack_bounds(provider, children, output_size)
                else {
                    return Err(SimpleScopeReject::NotSimple);
                };
                let _ = bounds
                    .intersection(
                        target_canvas_bounds(target_origin, target_size)
                            .ok_or(SimpleScopeReject::NotSimple)?,
                    )
                    .ok_or(SimpleScopeReject::NotSimple)?;
                let child_count = simple_scope_children_event_count(
                    provider,
                    output_size,
                    target_origin,
                    target_size,
                    children,
                    container_depth_remaining,
                    ThroughBudget::Remaining(through_depth_remaining - 1),
                    ClippingRunPolicy::None,
                )?;
                count = add_scope_events(count, 2)?;
                count = add_scope_events(count, child_count)?;
                saw_raster = true;
            }
            GpuNormalStackSource::ClippingRun { base, clipped } => {
                if !clipping_run_policy.allows_current()
                    || !simple_scope_clipping_run_can_lower(*base, clipped)
                {
                    return Err(SimpleScopeReject::NotSimple);
                }
                let Some(normal_sources) = clipping_run_as_normal_sources(*base, clipped) else {
                    return Err(SimpleScopeReject::NotSimple);
                };
                if !normal_sources.iter().all(|source| {
                    source_is_silo_eligible(
                        provider,
                        output_size,
                        target_origin,
                        target_size,
                        source,
                    )
                }) {
                    return Err(SimpleScopeReject::NotSimple);
                }
                count = add_scope_events(count, 2)?;
                count = add_scope_events(count, normal_sources.len())?;
                saw_raster = true;
            }
            GpuNormalStackSource::LutFilter {
                lut_rgba,
                opacity,
                mask_key,
                ..
            } => {
                if !saw_raster || *opacity <= 0.0 {
                    return Err(SimpleScopeReject::NotSimple);
                }
                if !filter_mask_can_lower(provider, *mask_key) || lut_rgba.len() != 256 * 4 {
                    return Err(SimpleScopeReject::FilterNotLowered);
                }
                count = add_scope_events(count, 1)?;
            }
            _ => return Err(SimpleScopeReject::NotSimple),
        }
    }
    if saw_raster {
        Ok(count)
    } else {
        Err(SimpleScopeReject::NotSimple)
    }
}

fn container_resolve_is_scope_eligible(blend_mode: GpuRasterBlendMode) -> bool {
    match blend_mode {
        GpuRasterBlendMode::Normal
        | GpuRasterBlendMode::Add
        | GpuRasterBlendMode::AddGlow
        | GpuRasterBlendMode::ColorDodge
        | GpuRasterBlendMode::ColorBurn
        | GpuRasterBlendMode::Darken
        | GpuRasterBlendMode::DarkerColor
        | GpuRasterBlendMode::Difference
        | GpuRasterBlendMode::Divide
        | GpuRasterBlendMode::Exclusion
        | GpuRasterBlendMode::GlowDodge
        | GpuRasterBlendMode::HardMix
        | GpuRasterBlendMode::HardLight
        | GpuRasterBlendMode::Hue
        | GpuRasterBlendMode::Lighten
        | GpuRasterBlendMode::LighterColor
        | GpuRasterBlendMode::LinearBurn
        | GpuRasterBlendMode::LinearLight
        | GpuRasterBlendMode::Multiply
        | GpuRasterBlendMode::Overlay
        | GpuRasterBlendMode::PinLight
        | GpuRasterBlendMode::Saturation
        | GpuRasterBlendMode::Brightness
        | GpuRasterBlendMode::Color
        | GpuRasterBlendMode::SoftLight
        | GpuRasterBlendMode::Screen
        | GpuRasterBlendMode::Subtract
        | GpuRasterBlendMode::VividLight => true,
    }
}

fn ensure_container_scope_header<P>(
    provider: &P,
    opacity: f32,
    mask_key: Option<crate::GpuMaskResourceKey>,
    children: &[GpuNormalStackSource],
    blend_mode: GpuRasterBlendMode,
) -> Result<(), SimpleScopeReject>
where
    P: GpuNormalStackResourceProvider,
{
    if opacity <= 0.0
        || !scope_mask_can_lower(provider, mask_key)
        || children.is_empty()
        || !container_resolve_is_scope_eligible(blend_mode)
    {
        return Err(SimpleScopeReject::NotSimple);
    }
    Ok(())
}

fn ensure_through_scope_header<P>(
    provider: &P,
    opacity: f32,
    mask_key: Option<crate::GpuMaskResourceKey>,
    children: &[GpuNormalStackSource],
) -> Result<(), SimpleScopeReject>
where
    P: GpuNormalStackResourceProvider,
{
    if opacity <= 0.0 || !scope_mask_can_lower(provider, mask_key) || children.is_empty() {
        return Err(SimpleScopeReject::NotSimple);
    }
    Ok(())
}

fn ensure_nested_through_scope_header<P>(
    provider: &P,
    opacity: f32,
    mask_key: Option<crate::GpuMaskResourceKey>,
    children: &[GpuNormalStackSource],
) -> Result<(), SimpleScopeReject>
where
    P: GpuNormalStackResourceProvider,
{
    if opacity <= 0.0 || !scope_mask_can_lower(provider, mask_key) || children.is_empty() {
        return Err(SimpleScopeReject::NotSimple);
    }
    Ok(())
}

pub(crate) fn simple_scope_clipping_run_can_lower(
    _base: crate::GpuNormalRasterSource,
    clipped: &[crate::GpuClippedStackSource],
) -> bool {
    clipped
        .iter()
        .all(|source| matches!(source, crate::GpuClippedStackSource::Raster(_)))
}

fn checked_scope_event_count(base: usize, child_count: usize) -> Result<usize, SimpleScopeReject> {
    add_scope_events(base, child_count)
}

fn add_scope_events(count: usize, additional: usize) -> Result<usize, SimpleScopeReject> {
    let count = count
        .checked_add(additional)
        .ok_or(SimpleScopeReject::TileEventLimitExceeded)?;
    if count > MAX_SILO_EVENTS {
        return Err(SimpleScopeReject::TileEventLimitExceeded);
    }
    Ok(count)
}

pub(crate) fn scope_mask_can_lower<P>(
    provider: &P,
    mask_key: Option<crate::GpuMaskResourceKey>,
) -> bool
where
    P: GpuNormalStackResourceProvider,
{
    match mask_key {
        Some(key) => {
            provider.mask_is_fully_opaque(key) == Some(true)
                || provider.mask_atlas_tiles_supported()
        }
        None => true,
    }
}
