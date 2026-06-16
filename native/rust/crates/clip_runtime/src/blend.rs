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
    match node.composite {
        0 => Some(StrictRasterBlendMode::Normal),
        LAYER_COMPOSITE_DARKEN
            if options.allow_simple_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Darken,
                ) =>
        {
            Some(StrictRasterBlendMode::Darken)
        }
        LAYER_COMPOSITE_MULTIPLY
            if options.allow_w3c_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Multiply,
                ) =>
        {
            Some(StrictRasterBlendMode::Multiply)
        }
        LAYER_COMPOSITE_LINEAR_BURN
            if options.allow_extended_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::LinearBurn,
                ) =>
        {
            Some(StrictRasterBlendMode::LinearBurn)
        }
        LAYER_COMPOSITE_ADD_GLOW
            if options.allow_add_glow_blend
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::AddGlow,
                ) =>
        {
            Some(StrictRasterBlendMode::AddGlow)
        }
        LAYER_COMPOSITE_DARKER_COLOR
            if options.allow_extended_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::DarkerColor,
                ) =>
        {
            Some(StrictRasterBlendMode::DarkerColor)
        }
        LAYER_COMPOSITE_SUBTRACT
            if options.allow_simple_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Subtract,
                ) =>
        {
            Some(StrictRasterBlendMode::Subtract)
        }
        LAYER_COMPOSITE_LIGHTEN
            if options.allow_simple_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Lighten,
                ) =>
        {
            Some(StrictRasterBlendMode::Lighten)
        }
        LAYER_COMPOSITE_SCREEN
            if options.allow_simple_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Screen,
                ) =>
        {
            Some(StrictRasterBlendMode::Screen)
        }
        LAYER_COMPOSITE_COLOR_BURN
            if options.allow_color_burn_blend
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::ColorBurn,
                ) =>
        {
            Some(StrictRasterBlendMode::ColorBurn)
        }
        LAYER_COMPOSITE_COLOR_DODGE
            if options.allow_color_dodge_blend
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::ColorDodge,
                ) =>
        {
            Some(StrictRasterBlendMode::ColorDodge)
        }
        LAYER_COMPOSITE_GLOW_DODGE
            if options.allow_glow_dodge_blend
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::GlowDodge,
                ) =>
        {
            Some(StrictRasterBlendMode::GlowDodge)
        }
        LAYER_COMPOSITE_ADD
            if options.allow_add_blend
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Add,
                ) =>
        {
            Some(StrictRasterBlendMode::Add)
        }
        LAYER_COMPOSITE_LIGHTER_COLOR
            if options.allow_extended_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::LighterColor,
                ) =>
        {
            Some(StrictRasterBlendMode::LighterColor)
        }
        LAYER_COMPOSITE_OVERLAY
            if options.allow_w3c_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Overlay,
                ) =>
        {
            Some(StrictRasterBlendMode::Overlay)
        }
        LAYER_COMPOSITE_HARD_MIX
            if options.allow_hard_mix_blend
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::HardMix,
                ) =>
        {
            Some(StrictRasterBlendMode::HardMix)
        }
        LAYER_COMPOSITE_HARD_LIGHT
            if options.allow_w3c_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::HardLight,
                ) =>
        {
            Some(StrictRasterBlendMode::HardLight)
        }
        LAYER_COMPOSITE_LINEAR_LIGHT
            if options.allow_extended_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::LinearLight,
                ) =>
        {
            Some(StrictRasterBlendMode::LinearLight)
        }
        LAYER_COMPOSITE_PIN_LIGHT
            if options.allow_extended_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::PinLight,
                ) =>
        {
            Some(StrictRasterBlendMode::PinLight)
        }
        LAYER_COMPOSITE_HUE
            if options.allow_hsl_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Hue,
                ) =>
        {
            Some(StrictRasterBlendMode::Hue)
        }
        LAYER_COMPOSITE_SATURATION
            if options.allow_hsl_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Saturation,
                ) =>
        {
            Some(StrictRasterBlendMode::Saturation)
        }
        LAYER_COMPOSITE_COLOR
            if options.allow_hsl_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Color,
                ) =>
        {
            Some(StrictRasterBlendMode::Color)
        }
        LAYER_COMPOSITE_SOFT_LIGHT
            if options.allow_soft_light_blend
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::SoftLight,
                ) =>
        {
            Some(StrictRasterBlendMode::SoftLight)
        }
        LAYER_COMPOSITE_DIFFERENCE
            if options.allow_simple_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Difference,
                ) =>
        {
            Some(StrictRasterBlendMode::Difference)
        }
        LAYER_COMPOSITE_EXCLUSION
            if options.allow_extended_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Exclusion,
                ) =>
        {
            Some(StrictRasterBlendMode::Exclusion)
        }
        LAYER_COMPOSITE_BRIGHTNESS
            if options.allow_extended_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Brightness,
                ) =>
        {
            Some(StrictRasterBlendMode::Brightness)
        }
        LAYER_COMPOSITE_VIVID_LIGHT
            if options.allow_vivid_light_blend
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::VividLight,
                ) =>
        {
            Some(StrictRasterBlendMode::VividLight)
        }
        LAYER_COMPOSITE_DIVIDE
            if options.allow_extended_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Divide,
                ) =>
        {
            Some(StrictRasterBlendMode::Divide)
        }
        _ => None,
    }
}

fn raster_blend_allowed_at_position(
    node: &clip_graph::RenderNode,
    allow_clip_flag: bool,
    blend_mode: StrictRasterBlendMode,
) -> bool {
    if !node.clip && !allow_clip_flag {
        return true;
    }
    node.clip && allow_clip_flag && clipped_blend_supported(blend_mode)
}

fn clipped_blend_supported(blend_mode: StrictRasterBlendMode) -> bool {
    matches!(
        blend_mode,
        StrictRasterBlendMode::Normal
            | StrictRasterBlendMode::Add
            | StrictRasterBlendMode::AddGlow
            | StrictRasterBlendMode::ColorBurn
            | StrictRasterBlendMode::ColorDodge
            | StrictRasterBlendMode::Darken
            | StrictRasterBlendMode::DarkerColor
            | StrictRasterBlendMode::Difference
            | StrictRasterBlendMode::Divide
            | StrictRasterBlendMode::Exclusion
            | StrictRasterBlendMode::GlowDodge
            | StrictRasterBlendMode::HardMix
            | StrictRasterBlendMode::HardLight
            | StrictRasterBlendMode::Hue
            | StrictRasterBlendMode::Lighten
            | StrictRasterBlendMode::LighterColor
            | StrictRasterBlendMode::LinearBurn
            | StrictRasterBlendMode::LinearLight
            | StrictRasterBlendMode::Multiply
            | StrictRasterBlendMode::Overlay
            | StrictRasterBlendMode::PinLight
            | StrictRasterBlendMode::Saturation
            | StrictRasterBlendMode::Brightness
            | StrictRasterBlendMode::Color
            | StrictRasterBlendMode::SoftLight
            | StrictRasterBlendMode::Screen
            | StrictRasterBlendMode::Subtract
            | StrictRasterBlendMode::VividLight
    )
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
