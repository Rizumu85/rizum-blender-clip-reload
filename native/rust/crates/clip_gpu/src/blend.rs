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
