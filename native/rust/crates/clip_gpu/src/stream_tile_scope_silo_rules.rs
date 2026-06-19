use crate::stream::GpuNormalStackResourceProvider;
use crate::{GpuClippedStackSource, GpuMaskResourceKey, GpuNormalStackSource, GpuRasterBlendMode};

pub(crate) fn scope_mask_can_lower<P>(provider: &P, mask_key: Option<GpuMaskResourceKey>) -> bool
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

pub(crate) fn raster_sources_from_scope_clipping_run(
    base: crate::GpuNormalRasterSource,
    clipped: &[GpuClippedStackSource],
) -> Option<Vec<GpuNormalStackSource>> {
    let mut sources = vec![GpuNormalStackSource::Raster(base)];
    for source in clipped {
        match source {
            GpuClippedStackSource::Raster(raster) => {
                sources.push(GpuNormalStackSource::Raster(*raster));
            }
            GpuClippedStackSource::Container { children, .. } => {
                if children.is_empty() {
                    return None;
                }
                append_raster_sources(children, &mut sources);
            }
        }
    }
    Some(sources)
}

pub(crate) fn clipped_container_headers_can_lower<P>(
    provider: &P,
    clipped: &[GpuClippedStackSource],
) -> bool
where
    P: GpuNormalStackResourceProvider,
{
    clipped.iter().all(|source| match source {
        GpuClippedStackSource::Raster(_) => true,
        GpuClippedStackSource::Container {
            children,
            opacity,
            mask_key,
            blend_mode,
            ..
        } => {
            *opacity > 0.0
                && !children.is_empty()
                && scope_mask_can_lower(provider, *mask_key)
                && container_resolve_is_scope_eligible(*blend_mode)
        }
    })
}

pub(crate) fn container_resolve_is_scope_eligible(blend_mode: GpuRasterBlendMode) -> bool {
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
            GpuNormalStackSource::ClippingRun { base, clipped } => {
                if let Some(normal_sources) = raster_sources_from_scope_clipping_run(*base, clipped)
                {
                    sources.extend(normal_sources);
                }
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
