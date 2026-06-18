use clip_model::CanvasSize;

use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::CanvasRect;
use crate::stream_extents::{KnownStackBounds, known_stack_bounds};
use crate::stream_tile_filter_silo::filter_mask_can_lower;
use crate::stream_tile_silo_plan::{MAX_SILO_EVENTS, source_is_silo_eligible};
use crate::{GpuNormalStackSource, GpuRasterBlendMode};

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
    let GpuNormalStackSource::Container {
        children,
        opacity,
        mask_key,
        blend_mode,
    } = source
    else {
        return None;
    };
    if *opacity <= 0.0
        || mask_key.is_some()
        || children.is_empty()
        || !container_resolve_is_scope_eligible(*blend_mode)
    {
        return None;
    }
    let KnownStackBounds::Bounded(bounds) = known_stack_bounds(provider, children, output_size)
    else {
        return None;
    };
    let _ = bounds.intersection(target_canvas_bounds(target_origin, target_size)?)?;

    let mut count = 2usize;
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
                    return None;
                }
                saw_raster = true;
                count = count.saturating_add(1);
            }
            GpuNormalStackSource::LutFilter {
                lut_rgba,
                opacity,
                mask_key,
                ..
            } => {
                if !saw_raster
                    || *opacity <= 0.0
                    || !filter_mask_can_lower(provider, *mask_key)
                    || lut_rgba.len() != 256 * 4
                {
                    return None;
                }
                count = count.saturating_add(1);
            }
            _ => return None,
        }
        if count > MAX_SILO_EVENTS {
            return None;
        }
    }
    saw_raster.then_some(count)
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

fn target_canvas_bounds(target_origin: (i32, i32), target_size: CanvasSize) -> Option<CanvasRect> {
    let x = u32::try_from(target_origin.0).ok()?;
    let y = u32::try_from(target_origin.1).ok()?;
    x.checked_add(target_size.width)?;
    y.checked_add(target_size.height)?;
    Some(CanvasRect {
        x,
        y,
        width: target_size.width,
        height: target_size.height,
    })
}
