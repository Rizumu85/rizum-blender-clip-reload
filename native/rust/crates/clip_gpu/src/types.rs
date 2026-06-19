use clip_model::{CanvasSize, LayerId, Rgba8};

use crate::{
    GpuMaskResourceCache, GpuMaskResourceKey, GpuRasterResourceCache, GpuRasterResourceInfo,
    GpuRasterResourceKey,
};

#[derive(Debug)]
pub struct GpuRasterDrawOutput {
    pub resource_info: GpuRasterResourceInfo,
    pub size: CanvasSize,
    pub pixels: Vec<u8>,
}

#[derive(Debug)]
pub struct GpuRasterStackOutput {
    pub drawn_resources: Vec<GpuRasterResourceInfo>,
    pub size: CanvasSize,
    pub pixels: Vec<u8>,
}

#[derive(Debug)]
pub struct GpuRasterPatchOutput {
    pub size: CanvasSize,
    pub payload: Vec<u8>,
}

#[derive(Debug)]
pub struct GpuNormalStackChunk {
    pub raster_cache: GpuRasterResourceCache,
    pub mask_cache: Option<GpuMaskResourceCache>,
    pub sources: Vec<GpuNormalStackSource>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuNormalRasterSource {
    pub key: GpuRasterResourceKey,
    pub opacity: f32,
    pub mask_key: Option<GpuMaskResourceKey>,
    pub offset_x: i32,
    pub offset_y: i32,
    pub blend_mode: GpuRasterBlendMode,
}

#[derive(Clone, Debug)]
pub enum GpuClippedStackSource {
    Raster(GpuNormalRasterSource),
    Container {
        layer_id: LayerId,
        children: Vec<GpuNormalStackSource>,
        opacity: f32,
        mask_key: Option<GpuMaskResourceKey>,
        blend_mode: GpuRasterBlendMode,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GpuRasterBlendMode {
    Normal,
    Add,
    AddGlow,
    ColorDodge,
    ColorBurn,
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
    SoftLight,
    Screen,
    Subtract,
    VividLight,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GpuLutFilterMode {
    ToneCurveRgb,
    GradientMapLum,
    ThresholdLum,
    Hsl(GpuHslFilterParams),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuHslFilterParams {
    pub hue_turns: f32,
    pub saturation_delta: f32,
    pub luminosity_delta: f32,
}

#[derive(Clone, Debug)]
pub enum GpuNormalStackSource {
    Raster(GpuNormalRasterSource),
    ClippingRun {
        base: GpuNormalRasterSource,
        clipped: Vec<GpuClippedStackSource>,
    },
    ContainerClippingRun {
        children: Vec<GpuNormalStackSource>,
        opacity: f32,
        mask_key: Option<GpuMaskResourceKey>,
        blend_mode: GpuRasterBlendMode,
        clipped: Vec<GpuClippedStackSource>,
    },
    Container {
        children: Vec<GpuNormalStackSource>,
        opacity: f32,
        mask_key: Option<GpuMaskResourceKey>,
        blend_mode: GpuRasterBlendMode,
    },
    ThroughGroup {
        children: Vec<GpuNormalStackSource>,
        opacity: f32,
        mask_key: Option<GpuMaskResourceKey>,
    },
    SolidColor {
        color: Rgba8,
        opacity: f32,
    },
    LutFilter {
        lut_rgba: Vec<u8>,
        opacity: f32,
        mask_key: Option<GpuMaskResourceKey>,
        filter_mode: GpuLutFilterMode,
    },
}
