use clip_model::Rgba8;

use crate::blend::blend_kind;
use crate::{GpuLutFilterMode, GpuNormalRasterSource, GpuRasterBlendMode};

pub(crate) fn raster_source_uniform_bytes(source: GpuNormalRasterSource) -> [u8; 48] {
    normal_source_uniform_bytes(
        [0.0, 0.0, 0.0, 0.0],
        source.opacity,
        0,
        u32::from(source.mask_key.is_some()),
        blend_kind(source.blend_mode),
        source.offset_x,
        source.offset_y,
    )
}

pub(crate) fn generated_raster_source_uniform_bytes(opacity: f32, has_mask: bool) -> [u8; 48] {
    normal_source_uniform_bytes(
        [0.0, 0.0, 0.0, 0.0],
        opacity,
        0,
        u32::from(has_mask),
        0,
        0,
        0,
    )
}

pub(crate) fn generated_raster_source_uniform_bytes_with_blend(
    opacity: f32,
    has_mask: bool,
    blend_mode: GpuRasterBlendMode,
) -> [u8; 48] {
    normal_source_uniform_bytes(
        [0.0, 0.0, 0.0, 0.0],
        opacity,
        0,
        u32::from(has_mask),
        blend_kind(blend_mode),
        0,
        0,
    )
}

pub(crate) fn lut_filter_uniform_bytes(
    opacity: f32,
    has_mask: bool,
    filter_mode: GpuLutFilterMode,
) -> [u8; 16] {
    let mut bytes = [0u8; 16];
    bytes[0..4].copy_from_slice(&opacity.to_ne_bytes());
    bytes[4..8].copy_from_slice(&(u32::from(has_mask)).to_ne_bytes());
    bytes[8..12].copy_from_slice(&lut_filter_mode_kind(filter_mode).to_ne_bytes());
    bytes
}

pub(crate) fn solid_source_uniform_bytes(color: Rgba8, opacity: f32) -> [u8; 48] {
    normal_source_uniform_bytes(
        [
            f32::from(color.r) / 255.0,
            f32::from(color.g) / 255.0,
            f32::from(color.b) / 255.0,
            f32::from(color.a) / 255.0,
        ],
        opacity,
        1,
        0,
        0,
        0,
        0,
    )
}

fn lut_filter_mode_kind(filter_mode: GpuLutFilterMode) -> u32 {
    match filter_mode {
        GpuLutFilterMode::ToneCurveRgb => 0,
        GpuLutFilterMode::GradientMapLum => 1,
    }
}

fn normal_source_uniform_bytes(
    color: [f32; 4],
    opacity: f32,
    source_kind: u32,
    has_mask: u32,
    blend_kind: u32,
    offset_x: i32,
    offset_y: i32,
) -> [u8; 48] {
    let mut bytes = [0u8; 48];
    for (index, value) in color.iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_ne_bytes());
    }
    bytes[16..20].copy_from_slice(&opacity.to_ne_bytes());
    bytes[20..24].copy_from_slice(&source_kind.to_ne_bytes());
    bytes[24..28].copy_from_slice(&has_mask.to_ne_bytes());
    bytes[28..32].copy_from_slice(&blend_kind.to_ne_bytes());
    bytes[32..36].copy_from_slice(&offset_x.to_ne_bytes());
    bytes[36..40].copy_from_slice(&offset_y.to_ne_bytes());
    bytes
}
