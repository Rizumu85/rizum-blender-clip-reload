use crate::GpuRasterBlendMode;

pub(crate) fn blend_kind(blend_mode: GpuRasterBlendMode) -> u32 {
    match blend_mode {
        GpuRasterBlendMode::ColorBurn => 3,
        GpuRasterBlendMode::Darken => 1,
        GpuRasterBlendMode::Multiply => 2,
        GpuRasterBlendMode::LinearBurn => 4,
        GpuRasterBlendMode::Subtract => 5,
        GpuRasterBlendMode::DarkerColor => 6,
        GpuRasterBlendMode::Lighten => 7,
        GpuRasterBlendMode::Screen => 8,
        GpuRasterBlendMode::ColorDodge => 9,
        GpuRasterBlendMode::GlowDodge => 10,
        GpuRasterBlendMode::Add => 11,
        GpuRasterBlendMode::AddGlow => 12,
        GpuRasterBlendMode::LighterColor => 13,
        GpuRasterBlendMode::Overlay => 14,
        GpuRasterBlendMode::SoftLight => 15,
        GpuRasterBlendMode::HardLight => 16,
        GpuRasterBlendMode::LinearLight => 18,
        GpuRasterBlendMode::PinLight => 19,
        GpuRasterBlendMode::Difference => 21,
        GpuRasterBlendMode::Exclusion => 22,
        GpuRasterBlendMode::Hue => 23,
        GpuRasterBlendMode::Saturation => 24,
        GpuRasterBlendMode::Color => 25,
        GpuRasterBlendMode::Brightness => 26,
        GpuRasterBlendMode::Divide => 36,
        GpuRasterBlendMode::VividLight => 17,
        GpuRasterBlendMode::HardMix => 20,
        _ => 0,
    }
}

pub(crate) fn clipped_source_pipeline<'a>(
    blend_mode: GpuRasterBlendMode,
    standard_preserve_pipeline: &'a wgpu::RenderPipeline,
    byte_preserve_pipeline: &'a wgpu::RenderPipeline,
) -> &'a wgpu::RenderPipeline {
    match blend_mode {
        GpuRasterBlendMode::AddGlow
        | GpuRasterBlendMode::ColorBurn
        | GpuRasterBlendMode::ColorDodge
        | GpuRasterBlendMode::GlowDodge => byte_preserve_pipeline,
        _ => standard_preserve_pipeline,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn raster_source_pipeline<'a>(
    blend_mode: GpuRasterBlendMode,
    normal_pipeline: &'a wgpu::RenderPipeline,
    add_glow_pipeline: &'a wgpu::RenderPipeline,
    color_dodge_pipeline: &'a wgpu::RenderPipeline,
    color_burn_pipeline: &'a wgpu::RenderPipeline,
    glow_dodge_pipeline: &'a wgpu::RenderPipeline,
    standard_blend_pipeline: &'a wgpu::RenderPipeline,
) -> &'a wgpu::RenderPipeline {
    match blend_mode {
        GpuRasterBlendMode::Normal => normal_pipeline,
        GpuRasterBlendMode::AddGlow => add_glow_pipeline,
        GpuRasterBlendMode::ColorDodge => color_dodge_pipeline,
        GpuRasterBlendMode::ColorBurn => color_burn_pipeline,
        GpuRasterBlendMode::GlowDodge => glow_dodge_pipeline,
        GpuRasterBlendMode::Add
        | GpuRasterBlendMode::Darken
        | GpuRasterBlendMode::DarkerColor
        | GpuRasterBlendMode::Difference
        | GpuRasterBlendMode::Divide
        | GpuRasterBlendMode::Exclusion
        | GpuRasterBlendMode::Lighten
        | GpuRasterBlendMode::LighterColor
        | GpuRasterBlendMode::LinearBurn
        | GpuRasterBlendMode::LinearLight
        | GpuRasterBlendMode::HardMix
        | GpuRasterBlendMode::HardLight
        | GpuRasterBlendMode::Hue
        | GpuRasterBlendMode::Multiply
        | GpuRasterBlendMode::Overlay
        | GpuRasterBlendMode::PinLight
        | GpuRasterBlendMode::Saturation
        | GpuRasterBlendMode::Brightness
        | GpuRasterBlendMode::Color
        | GpuRasterBlendMode::SoftLight
        | GpuRasterBlendMode::Screen
        | GpuRasterBlendMode::Subtract
        | GpuRasterBlendMode::VividLight => standard_blend_pipeline,
    }
}
