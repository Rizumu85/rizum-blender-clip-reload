use clip_model::CanvasSize;

use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_effects::raster_can_affect_output;
use crate::stream_tile_scope_silo_plan::{
    SimpleScopeBarrierHint, scope_mask_can_lower, simple_scope_barrier_hint,
};
use crate::stream_tile_silo_plan::{source_bounds, source_local_bounds};
use crate::{
    GpuClippedStackSource, GpuMaskResourceKey, GpuNormalRasterSource, GpuNormalStackSource,
    GpuRasterBlendMode,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderProgramBarrierCounts {
    pub raster_run_too_short: u32,
    pub raster_cannot_affect_output: u32,
    pub raster_bounds_or_resource_not_lowered: u32,
    pub mask_atlas_not_available: u32,
    pub byte_domain_blend_not_lowered: u32,
    pub solid_color_not_lowered: u32,
    pub filter_not_lowered: u32,
    pub clipping_run_not_lowered: u32,
    pub clipped_container_sibling_not_lowered: u32,
    pub scope_mask_not_lowered: u32,
    pub scope_depth_limit_exceeded: u32,
    pub tile_event_limit_exceeded: u32,
    pub isolated_container_requires_intermediate: u32,
    pub through_group_not_lowered: u32,
}

impl RenderProgramBarrierCounts {
    pub(crate) fn add(&mut self, reason: RenderProgramBarrierReason) {
        match reason {
            RenderProgramBarrierReason::RasterRunTooShort => self.raster_run_too_short += 1,
            RenderProgramBarrierReason::RasterCannotAffectOutput => {
                self.raster_cannot_affect_output += 1;
            }
            RenderProgramBarrierReason::RasterBoundsOrResourceNotLowered => {
                self.raster_bounds_or_resource_not_lowered += 1;
            }
            RenderProgramBarrierReason::MaskAtlasNotAvailable => {
                self.mask_atlas_not_available += 1;
            }
            RenderProgramBarrierReason::ByteDomainBlendNotLowered => {
                self.byte_domain_blend_not_lowered += 1;
            }
            RenderProgramBarrierReason::SolidColorNotLowered => self.solid_color_not_lowered += 1,
            RenderProgramBarrierReason::FilterNotLowered => self.filter_not_lowered += 1,
            RenderProgramBarrierReason::ClippingRunNotLowered => {
                self.clipping_run_not_lowered += 1;
            }
            RenderProgramBarrierReason::ClippedContainerSiblingNotLowered => {
                self.clipped_container_sibling_not_lowered += 1;
            }
            RenderProgramBarrierReason::ScopeMaskNotLowered => {
                self.scope_mask_not_lowered += 1;
            }
            RenderProgramBarrierReason::ScopeDepthLimitExceeded => {
                self.scope_depth_limit_exceeded += 1;
            }
            RenderProgramBarrierReason::TileEventLimitExceeded => {
                self.tile_event_limit_exceeded += 1;
            }
            RenderProgramBarrierReason::IsolatedContainerRequiresIntermediate => {
                self.isolated_container_requires_intermediate += 1;
            }
            RenderProgramBarrierReason::ThroughGroupNotLowered => {
                self.through_group_not_lowered += 1
            }
        }
    }

    pub(crate) fn add_counts(&mut self, other: Self) {
        self.raster_run_too_short = self
            .raster_run_too_short
            .saturating_add(other.raster_run_too_short);
        self.raster_cannot_affect_output = self
            .raster_cannot_affect_output
            .saturating_add(other.raster_cannot_affect_output);
        self.raster_bounds_or_resource_not_lowered = self
            .raster_bounds_or_resource_not_lowered
            .saturating_add(other.raster_bounds_or_resource_not_lowered);
        self.mask_atlas_not_available = self
            .mask_atlas_not_available
            .saturating_add(other.mask_atlas_not_available);
        self.byte_domain_blend_not_lowered = self
            .byte_domain_blend_not_lowered
            .saturating_add(other.byte_domain_blend_not_lowered);
        self.solid_color_not_lowered = self
            .solid_color_not_lowered
            .saturating_add(other.solid_color_not_lowered);
        self.filter_not_lowered = self
            .filter_not_lowered
            .saturating_add(other.filter_not_lowered);
        self.clipping_run_not_lowered = self
            .clipping_run_not_lowered
            .saturating_add(other.clipping_run_not_lowered);
        self.clipped_container_sibling_not_lowered = self
            .clipped_container_sibling_not_lowered
            .saturating_add(other.clipped_container_sibling_not_lowered);
        self.scope_mask_not_lowered = self
            .scope_mask_not_lowered
            .saturating_add(other.scope_mask_not_lowered);
        self.scope_depth_limit_exceeded = self
            .scope_depth_limit_exceeded
            .saturating_add(other.scope_depth_limit_exceeded);
        self.tile_event_limit_exceeded = self
            .tile_event_limit_exceeded
            .saturating_add(other.tile_event_limit_exceeded);
        self.isolated_container_requires_intermediate = self
            .isolated_container_requires_intermediate
            .saturating_add(other.isolated_container_requires_intermediate);
        self.through_group_not_lowered = self
            .through_group_not_lowered
            .saturating_add(other.through_group_not_lowered);
    }

    pub fn nonzero_counts(&self) -> Vec<(RenderProgramBarrierReason, u32)> {
        [
            (
                RenderProgramBarrierReason::RasterRunTooShort,
                self.raster_run_too_short,
            ),
            (
                RenderProgramBarrierReason::RasterCannotAffectOutput,
                self.raster_cannot_affect_output,
            ),
            (
                RenderProgramBarrierReason::RasterBoundsOrResourceNotLowered,
                self.raster_bounds_or_resource_not_lowered,
            ),
            (
                RenderProgramBarrierReason::MaskAtlasNotAvailable,
                self.mask_atlas_not_available,
            ),
            (
                RenderProgramBarrierReason::ByteDomainBlendNotLowered,
                self.byte_domain_blend_not_lowered,
            ),
            (
                RenderProgramBarrierReason::SolidColorNotLowered,
                self.solid_color_not_lowered,
            ),
            (
                RenderProgramBarrierReason::FilterNotLowered,
                self.filter_not_lowered,
            ),
            (
                RenderProgramBarrierReason::ClippingRunNotLowered,
                self.clipping_run_not_lowered,
            ),
            (
                RenderProgramBarrierReason::ClippedContainerSiblingNotLowered,
                self.clipped_container_sibling_not_lowered,
            ),
            (
                RenderProgramBarrierReason::ScopeMaskNotLowered,
                self.scope_mask_not_lowered,
            ),
            (
                RenderProgramBarrierReason::ScopeDepthLimitExceeded,
                self.scope_depth_limit_exceeded,
            ),
            (
                RenderProgramBarrierReason::TileEventLimitExceeded,
                self.tile_event_limit_exceeded,
            ),
            (
                RenderProgramBarrierReason::IsolatedContainerRequiresIntermediate,
                self.isolated_container_requires_intermediate,
            ),
            (
                RenderProgramBarrierReason::ThroughGroupNotLowered,
                self.through_group_not_lowered,
            ),
        ]
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderProgramBarrierReason {
    RasterRunTooShort,
    RasterCannotAffectOutput,
    RasterBoundsOrResourceNotLowered,
    MaskAtlasNotAvailable,
    ByteDomainBlendNotLowered,
    SolidColorNotLowered,
    FilterNotLowered,
    ClippingRunNotLowered,
    ClippedContainerSiblingNotLowered,
    ScopeMaskNotLowered,
    ScopeDepthLimitExceeded,
    TileEventLimitExceeded,
    IsolatedContainerRequiresIntermediate,
    ThroughGroupNotLowered,
}

impl RenderProgramBarrierReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RasterRunTooShort => "RasterRunTooShort",
            Self::RasterCannotAffectOutput => "RasterCannotAffectOutput",
            Self::RasterBoundsOrResourceNotLowered => "RasterBoundsOrResourceNotLowered",
            Self::MaskAtlasNotAvailable => "MaskAtlasNotAvailable",
            Self::ByteDomainBlendNotLowered => "ByteDomainBlendNotLowered",
            Self::SolidColorNotLowered => "SolidColorNotLowered",
            Self::FilterNotLowered => "FilterNotLowered",
            Self::ClippingRunNotLowered => "ClippingRunNotLowered",
            Self::ClippedContainerSiblingNotLowered => "ClippedContainerSiblingNotLowered",
            Self::ScopeMaskNotLowered => "ScopeMaskNotLowered",
            Self::ScopeDepthLimitExceeded => "ScopeDepthLimitExceeded",
            Self::TileEventLimitExceeded => "TileEventLimitExceeded",
            Self::IsolatedContainerRequiresIntermediate => "IsolatedContainerRequiresIntermediate",
            Self::ThroughGroupNotLowered => "ThroughGroupNotLowered",
        }
    }
}

pub(crate) fn barrier_reason_for_source<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    source: &GpuNormalStackSource,
) -> RenderProgramBarrierReason
where
    P: GpuNormalStackResourceProvider,
{
    match source {
        GpuNormalStackSource::Raster(raster) => {
            raster_barrier_reason(provider, output_size, target_origin, target_size, *raster)
        }
        GpuNormalStackSource::ClippingRun { base, clipped } => {
            clipping_run_barrier_reason(*base, clipped)
        }
        GpuNormalStackSource::ContainerClippingRun { .. } => {
            RenderProgramBarrierReason::IsolatedContainerRequiresIntermediate
        }
        GpuNormalStackSource::Container {
            children, mask_key, ..
        } => {
            if scope_mask_not_lowered(provider, *mask_key, children) {
                RenderProgramBarrierReason::ScopeMaskNotLowered
            } else if let Some(reason) =
                scope_barrier_hint_reason(provider, output_size, target_origin, target_size, source)
            {
                reason
            } else {
                RenderProgramBarrierReason::IsolatedContainerRequiresIntermediate
            }
        }
        GpuNormalStackSource::ThroughGroup {
            children, mask_key, ..
        } => {
            if scope_mask_not_lowered(provider, *mask_key, children) {
                RenderProgramBarrierReason::ScopeMaskNotLowered
            } else if let Some(reason) =
                scope_barrier_hint_reason(provider, output_size, target_origin, target_size, source)
            {
                reason
            } else {
                RenderProgramBarrierReason::ThroughGroupNotLowered
            }
        }
        GpuNormalStackSource::SolidColor { .. } => RenderProgramBarrierReason::SolidColorNotLowered,
        GpuNormalStackSource::LutFilter { .. } => RenderProgramBarrierReason::FilterNotLowered,
    }
}

fn scope_barrier_hint_reason<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    source: &GpuNormalStackSource,
) -> Option<RenderProgramBarrierReason>
where
    P: GpuNormalStackResourceProvider,
{
    match simple_scope_barrier_hint(provider, output_size, target_origin, target_size, source)? {
        SimpleScopeBarrierHint::ScopeDepthLimitExceeded => {
            Some(RenderProgramBarrierReason::ScopeDepthLimitExceeded)
        }
        SimpleScopeBarrierHint::TileEventLimitExceeded => {
            Some(RenderProgramBarrierReason::TileEventLimitExceeded)
        }
        SimpleScopeBarrierHint::FilterNotLowered => {
            Some(RenderProgramBarrierReason::FilterNotLowered)
        }
    }
}

fn raster_barrier_reason<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    raster: GpuNormalRasterSource,
) -> RenderProgramBarrierReason
where
    P: GpuNormalStackResourceProvider,
{
    if !raster_can_affect_output(raster) {
        return RenderProgramBarrierReason::RasterCannotAffectOutput;
    }
    if byte_domain_blend_not_lowered(raster.blend_mode) {
        return RenderProgramBarrierReason::ByteDomainBlendNotLowered;
    }
    if raster.mask_key.is_some() && !provider.raster_run_atlas_supports_masks() {
        return RenderProgramBarrierReason::MaskAtlasNotAvailable;
    }
    let Some(size) = provider.raster_resource_size(raster) else {
        return RenderProgramBarrierReason::RasterBoundsOrResourceNotLowered;
    };
    if size.width == 0 || size.height == 0 {
        return RenderProgramBarrierReason::RasterBoundsOrResourceNotLowered;
    }
    let offset = provider
        .raster_resource_offset(raster)
        .unwrap_or((raster.offset_x, raster.offset_y));
    if source_bounds(offset, size, output_size).is_none()
        || source_local_bounds(offset, size, target_origin, target_size).is_none()
    {
        return RenderProgramBarrierReason::RasterBoundsOrResourceNotLowered;
    }
    RenderProgramBarrierReason::RasterRunTooShort
}

fn scope_mask_not_lowered<P>(
    provider: &P,
    mask_key: Option<GpuMaskResourceKey>,
    children: &[GpuNormalStackSource],
) -> bool
where
    P: GpuNormalStackResourceProvider,
{
    if !scope_mask_can_lower(provider, mask_key) {
        return true;
    }
    children.iter().any(|child| match child {
        GpuNormalStackSource::Container {
            children, mask_key, ..
        }
        | GpuNormalStackSource::ThroughGroup {
            children, mask_key, ..
        } => scope_mask_not_lowered(provider, *mask_key, children),
        _ => false,
    })
}

fn clipping_run_barrier_reason(
    base: GpuNormalRasterSource,
    clipped: &[GpuClippedStackSource],
) -> RenderProgramBarrierReason {
    if clipped
        .iter()
        .any(|source| matches!(source, GpuClippedStackSource::Container { .. }))
    {
        return RenderProgramBarrierReason::ClippedContainerSiblingNotLowered;
    }
    if byte_domain_blend_not_lowered(base.blend_mode)
        || clipped.iter().any(|source| match source {
            GpuClippedStackSource::Raster(raster) => {
                byte_domain_blend_not_lowered(raster.blend_mode)
            }
            GpuClippedStackSource::Container { blend_mode, .. } => {
                byte_domain_blend_not_lowered(*blend_mode)
            }
        })
    {
        return RenderProgramBarrierReason::ByteDomainBlendNotLowered;
    }
    RenderProgramBarrierReason::ClippingRunNotLowered
}

fn byte_domain_blend_not_lowered(blend_mode: GpuRasterBlendMode) -> bool {
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
        | GpuRasterBlendMode::VividLight => false,
    }
}
