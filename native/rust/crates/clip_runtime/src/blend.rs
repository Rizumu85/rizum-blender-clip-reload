use crate::stack_plan::StrictRasterStackOptions;

pub(crate) const LAYER_COMPOSITE_DARKEN: u32 = 1;
pub(crate) const LAYER_COMPOSITE_MULTIPLY: u32 = 2;
pub(crate) const LAYER_COMPOSITE_COLOR_BURN: u32 = 3;
pub(crate) const LAYER_COMPOSITE_LINEAR_BURN: u32 = 4;
pub(crate) const LAYER_COMPOSITE_SUBTRACT: u32 = 5;
pub(crate) const LAYER_COMPOSITE_DARKER_COLOR: u32 = 6;
pub(crate) const LAYER_COMPOSITE_LIGHTEN: u32 = 7;
pub(crate) const LAYER_COMPOSITE_SCREEN: u32 = 8;
pub(crate) const LAYER_COMPOSITE_COLOR_DODGE: u32 = 9;
pub(crate) const LAYER_COMPOSITE_GLOW_DODGE: u32 = 10;
pub(crate) const LAYER_COMPOSITE_ADD: u32 = 11;
pub(crate) const LAYER_COMPOSITE_ADD_GLOW: u32 = 12;
pub(crate) const LAYER_COMPOSITE_LIGHTER_COLOR: u32 = 13;
pub(crate) const LAYER_COMPOSITE_OVERLAY: u32 = 14;
pub(crate) const LAYER_COMPOSITE_SOFT_LIGHT: u32 = 15;
pub(crate) const LAYER_COMPOSITE_HARD_LIGHT: u32 = 16;
pub(crate) const LAYER_COMPOSITE_VIVID_LIGHT: u32 = 17;
pub(crate) const LAYER_COMPOSITE_LINEAR_LIGHT: u32 = 18;
pub(crate) const LAYER_COMPOSITE_PIN_LIGHT: u32 = 19;
pub(crate) const LAYER_COMPOSITE_HARD_MIX: u32 = 20;
pub(crate) const LAYER_COMPOSITE_DIFFERENCE: u32 = 21;
pub(crate) const LAYER_COMPOSITE_EXCLUSION: u32 = 22;
pub(crate) const LAYER_COMPOSITE_HUE: u32 = 23;
pub(crate) const LAYER_COMPOSITE_SATURATION: u32 = 24;
pub(crate) const LAYER_COMPOSITE_COLOR: u32 = 25;
pub(crate) const LAYER_COMPOSITE_BRIGHTNESS: u32 = 26;
pub(crate) const LAYER_COMPOSITE_DIVIDE: u32 = 36;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StrictRasterBlendMode {
    Normal,
    Add,
    AddGlow,
    ColorBurn,
    ColorDodge,
    Darken,
    DarkerColor,
    Difference,
    Divide,
    Exclusion,
    GlowDodge,
    HardMix,
    HardLight,
    Hue,
    Lighten,
    LighterColor,
    LinearBurn,
    LinearLight,
    Multiply,
    Overlay,
    PinLight,
    Saturation,
    Brightness,
    Color,
    Screen,
    SoftLight,
    Subtract,
    VividLight,
}

pub(crate) fn strict_raster_blend_mode(
    node: &clip_graph::RenderNode,
    options: StrictRasterStackOptions,
    allow_clip_flag: bool,
) -> Option<StrictRasterBlendMode> {
    let (blend_mode, enabled) = match node.composite {
        0 => return Some(StrictRasterBlendMode::Normal),
        LAYER_COMPOSITE_DARKEN => (StrictRasterBlendMode::Darken, options.allow_simple_blends),
        LAYER_COMPOSITE_MULTIPLY => (StrictRasterBlendMode::Multiply, options.allow_w3c_blends),
        LAYER_COMPOSITE_LINEAR_BURN => (
            StrictRasterBlendMode::LinearBurn,
            options.allow_extended_blends,
        ),
        LAYER_COMPOSITE_ADD_GLOW => (StrictRasterBlendMode::AddGlow, options.allow_add_glow_blend),
        LAYER_COMPOSITE_DARKER_COLOR => (
            StrictRasterBlendMode::DarkerColor,
            options.allow_extended_blends,
        ),
        LAYER_COMPOSITE_SUBTRACT => (StrictRasterBlendMode::Subtract, options.allow_simple_blends),
        LAYER_COMPOSITE_LIGHTEN => (StrictRasterBlendMode::Lighten, options.allow_simple_blends),
        LAYER_COMPOSITE_SCREEN => (StrictRasterBlendMode::Screen, options.allow_simple_blends),
        LAYER_COMPOSITE_COLOR_BURN => (
            StrictRasterBlendMode::ColorBurn,
            options.allow_color_burn_blend,
        ),
        LAYER_COMPOSITE_COLOR_DODGE => (
            StrictRasterBlendMode::ColorDodge,
            options.allow_color_dodge_blend,
        ),
        LAYER_COMPOSITE_GLOW_DODGE => (
            StrictRasterBlendMode::GlowDodge,
            options.allow_glow_dodge_blend,
        ),
        LAYER_COMPOSITE_ADD => (StrictRasterBlendMode::Add, options.allow_add_blend),
        LAYER_COMPOSITE_LIGHTER_COLOR => (
            StrictRasterBlendMode::LighterColor,
            options.allow_extended_blends,
        ),
        LAYER_COMPOSITE_OVERLAY => (StrictRasterBlendMode::Overlay, options.allow_w3c_blends),
        LAYER_COMPOSITE_HARD_MIX => (StrictRasterBlendMode::HardMix, options.allow_hard_mix_blend),
        LAYER_COMPOSITE_HARD_LIGHT => (StrictRasterBlendMode::HardLight, options.allow_w3c_blends),
        LAYER_COMPOSITE_LINEAR_LIGHT => (
            StrictRasterBlendMode::LinearLight,
            options.allow_extended_blends,
        ),
        LAYER_COMPOSITE_PIN_LIGHT => (
            StrictRasterBlendMode::PinLight,
            options.allow_extended_blends,
        ),
        LAYER_COMPOSITE_HUE => (StrictRasterBlendMode::Hue, options.allow_hsl_blends),
        LAYER_COMPOSITE_SATURATION => (StrictRasterBlendMode::Saturation, options.allow_hsl_blends),
        LAYER_COMPOSITE_COLOR => (StrictRasterBlendMode::Color, options.allow_hsl_blends),
        LAYER_COMPOSITE_SOFT_LIGHT => (
            StrictRasterBlendMode::SoftLight,
            options.allow_soft_light_blend,
        ),
        LAYER_COMPOSITE_DIFFERENCE => (
            StrictRasterBlendMode::Difference,
            options.allow_simple_blends,
        ),
        LAYER_COMPOSITE_EXCLUSION => (
            StrictRasterBlendMode::Exclusion,
            options.allow_extended_blends,
        ),
        LAYER_COMPOSITE_BRIGHTNESS => (
            StrictRasterBlendMode::Brightness,
            options.allow_extended_blends,
        ),
        LAYER_COMPOSITE_VIVID_LIGHT => (
            StrictRasterBlendMode::VividLight,
            options.allow_vivid_light_blend,
        ),
        LAYER_COMPOSITE_DIVIDE => (StrictRasterBlendMode::Divide, options.allow_extended_blends),
        _ => return None,
    };
    // A non-normal blend resolves only at the position the caller selects for:
    // clipped layers inside a clipping run, unclipped layers outside one.
    (enabled && node.clip == allow_clip_flag).then_some(blend_mode)
}

pub(crate) fn gpu_raster_blend_mode(
    blend_mode: StrictRasterBlendMode,
) -> clip_gpu::GpuRasterBlendMode {
    match blend_mode {
        StrictRasterBlendMode::Normal => clip_gpu::GpuRasterBlendMode::Normal,
        StrictRasterBlendMode::Add => clip_gpu::GpuRasterBlendMode::Add,
        StrictRasterBlendMode::AddGlow => clip_gpu::GpuRasterBlendMode::AddGlow,
        StrictRasterBlendMode::ColorBurn => clip_gpu::GpuRasterBlendMode::ColorBurn,
        StrictRasterBlendMode::ColorDodge => clip_gpu::GpuRasterBlendMode::ColorDodge,
        StrictRasterBlendMode::Darken => clip_gpu::GpuRasterBlendMode::Darken,
        StrictRasterBlendMode::DarkerColor => clip_gpu::GpuRasterBlendMode::DarkerColor,
        StrictRasterBlendMode::Difference => clip_gpu::GpuRasterBlendMode::Difference,
        StrictRasterBlendMode::Divide => clip_gpu::GpuRasterBlendMode::Divide,
        StrictRasterBlendMode::Exclusion => clip_gpu::GpuRasterBlendMode::Exclusion,
        StrictRasterBlendMode::GlowDodge => clip_gpu::GpuRasterBlendMode::GlowDodge,
        StrictRasterBlendMode::HardMix => clip_gpu::GpuRasterBlendMode::HardMix,
        StrictRasterBlendMode::HardLight => clip_gpu::GpuRasterBlendMode::HardLight,
        StrictRasterBlendMode::Hue => clip_gpu::GpuRasterBlendMode::Hue,
        StrictRasterBlendMode::Lighten => clip_gpu::GpuRasterBlendMode::Lighten,
        StrictRasterBlendMode::LighterColor => clip_gpu::GpuRasterBlendMode::LighterColor,
        StrictRasterBlendMode::LinearBurn => clip_gpu::GpuRasterBlendMode::LinearBurn,
        StrictRasterBlendMode::LinearLight => clip_gpu::GpuRasterBlendMode::LinearLight,
        StrictRasterBlendMode::Multiply => clip_gpu::GpuRasterBlendMode::Multiply,
        StrictRasterBlendMode::Overlay => clip_gpu::GpuRasterBlendMode::Overlay,
        StrictRasterBlendMode::PinLight => clip_gpu::GpuRasterBlendMode::PinLight,
        StrictRasterBlendMode::Saturation => clip_gpu::GpuRasterBlendMode::Saturation,
        StrictRasterBlendMode::Brightness => clip_gpu::GpuRasterBlendMode::Brightness,
        StrictRasterBlendMode::Color => clip_gpu::GpuRasterBlendMode::Color,
        StrictRasterBlendMode::Screen => clip_gpu::GpuRasterBlendMode::Screen,
        StrictRasterBlendMode::SoftLight => clip_gpu::GpuRasterBlendMode::SoftLight,
        StrictRasterBlendMode::Subtract => clip_gpu::GpuRasterBlendMode::Subtract,
        StrictRasterBlendMode::VividLight => clip_gpu::GpuRasterBlendMode::VividLight,
    }
}
