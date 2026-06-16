use clip_model::Rgba8;

use crate::blend::blend_kind;
use crate::{GpuLutFilterMode, GpuMaskSamplingInfo, GpuNormalRasterSource, GpuRasterBlendMode};

pub(crate) fn raster_source_uniform_bytes(source: GpuNormalRasterSource) -> [u8; 64] {
    raster_source_uniform_bytes_with_target_origin(source, (0, 0))
}

pub(crate) fn raster_source_uniform_bytes_with_target_origin(
    source: GpuNormalRasterSource,
    target_origin: (i32, i32),
) -> [u8; 64] {
    raster_source_uniform_bytes_with_target_origin_and_mask(
        source,
        target_origin,
        GpuMaskSamplingInfo::default(),
    )
}

pub(crate) fn raster_source_uniform_bytes_with_target_origin_and_mask(
    source: GpuNormalRasterSource,
    target_origin: (i32, i32),
    mask_sampling: GpuMaskSamplingInfo,
) -> [u8; 64] {
    normal_source_uniform_bytes(
        [0.0, 0.0, 0.0, 0.0],
        source.opacity,
        0,
        u32::from(source.mask_key.is_some()),
        blend_kind(source.blend_mode),
        source.offset_x,
        source.offset_y,
        target_origin.0,
        target_origin.1,
        mask_sampling,
    )
}

pub(crate) fn generated_raster_source_uniform_bytes(opacity: f32, has_mask: bool) -> [u8; 64] {
    normal_source_uniform_bytes(
        [0.0, 0.0, 0.0, 0.0],
        opacity,
        0,
        u32::from(has_mask),
        0,
        0,
        0,
        0,
        0,
        GpuMaskSamplingInfo::default(),
    )
}

pub(crate) fn generated_raster_source_uniform_bytes_with_blend(
    opacity: f32,
    has_mask: bool,
    blend_mode: GpuRasterBlendMode,
) -> [u8; 64] {
    generated_raster_source_uniform_bytes_with_blend_and_origin(
        opacity,
        has_mask,
        blend_mode,
        (0, 0),
    )
}

pub(crate) fn generated_raster_source_uniform_bytes_with_blend_and_origin(
    opacity: f32,
    has_mask: bool,
    blend_mode: GpuRasterBlendMode,
    source_origin: (i32, i32),
) -> [u8; 64] {
    generated_raster_source_uniform_bytes_with_blend_and_origins(
        opacity,
        has_mask,
        blend_mode,
        source_origin,
        (0, 0),
    )
}

pub(crate) fn generated_raster_source_uniform_bytes_with_blend_and_origins(
    opacity: f32,
    has_mask: bool,
    blend_mode: GpuRasterBlendMode,
    source_origin: (i32, i32),
    target_origin: (i32, i32),
) -> [u8; 64] {
    generated_raster_source_uniform_bytes_with_blend_origins_and_mask(
        opacity,
        has_mask,
        blend_mode,
        source_origin,
        target_origin,
        GpuMaskSamplingInfo::default(),
    )
}

pub(crate) fn generated_raster_source_uniform_bytes_with_blend_origins_and_mask(
    opacity: f32,
    has_mask: bool,
    blend_mode: GpuRasterBlendMode,
    source_origin: (i32, i32),
    target_origin: (i32, i32),
    mask_sampling: GpuMaskSamplingInfo,
) -> [u8; 64] {
    normal_source_uniform_bytes(
        [0.0, 0.0, 0.0, 0.0],
        opacity,
        0,
        u32::from(has_mask),
        blend_kind(blend_mode),
        source_origin.0,
        source_origin.1,
        target_origin.0,
        target_origin.1,
        mask_sampling,
    )
}

pub(crate) fn lut_filter_uniform_bytes(
    opacity: f32,
    has_mask: bool,
    filter_mode: GpuLutFilterMode,
) -> [u8; 48] {
    lut_filter_uniform_bytes_with_target_origin(opacity, has_mask, filter_mode, (0, 0))
}

pub(crate) fn lut_filter_uniform_bytes_with_target_origin(
    opacity: f32,
    has_mask: bool,
    filter_mode: GpuLutFilterMode,
    target_origin: (i32, i32),
) -> [u8; 48] {
    lut_filter_uniform_bytes_with_target_origin_and_mask(
        opacity,
        has_mask,
        filter_mode,
        target_origin,
        GpuMaskSamplingInfo::default(),
    )
}

pub(crate) fn lut_filter_uniform_bytes_with_target_origin_and_mask(
    opacity: f32,
    has_mask: bool,
    filter_mode: GpuLutFilterMode,
    target_origin: (i32, i32),
    mask_sampling: GpuMaskSamplingInfo,
) -> [u8; 48] {
    let mut bytes = [0u8; 48];
    bytes[0..4].copy_from_slice(&opacity.to_ne_bytes());
    bytes[4..8].copy_from_slice(&(u32::from(has_mask)).to_ne_bytes());
    bytes[8..12].copy_from_slice(&lut_filter_mode_kind(filter_mode).to_ne_bytes());
    bytes[16..20].copy_from_slice(&target_origin.0.to_ne_bytes());
    bytes[20..24].copy_from_slice(&target_origin.1.to_ne_bytes());
    bytes[32..36].copy_from_slice(&mask_sampling.origin_x.to_ne_bytes());
    bytes[36..40].copy_from_slice(&mask_sampling.origin_y.to_ne_bytes());
    bytes[40..44].copy_from_slice(&(mask_sampling_fill(mask_sampling)).to_ne_bytes());
    bytes
}

pub(crate) fn solid_source_uniform_bytes(color: Rgba8, opacity: f32) -> [u8; 64] {
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
        0,
        0,
        GpuMaskSamplingInfo::default(),
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
    target_offset_x: i32,
    target_offset_y: i32,
    mask_sampling: GpuMaskSamplingInfo,
) -> [u8; 64] {
    let mut bytes = [0u8; 64];
    for (index, value) in color.iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_ne_bytes());
    }
    bytes[16..20].copy_from_slice(&opacity.to_ne_bytes());
    bytes[20..24].copy_from_slice(&source_kind.to_ne_bytes());
    bytes[24..28].copy_from_slice(&has_mask.to_ne_bytes());
    bytes[28..32].copy_from_slice(&blend_kind.to_ne_bytes());
    bytes[32..36].copy_from_slice(&offset_x.to_ne_bytes());
    bytes[36..40].copy_from_slice(&offset_y.to_ne_bytes());
    bytes[40..44].copy_from_slice(&target_offset_x.to_ne_bytes());
    bytes[44..48].copy_from_slice(&target_offset_y.to_ne_bytes());
    bytes[48..52].copy_from_slice(&mask_sampling.origin_x.to_ne_bytes());
    bytes[52..56].copy_from_slice(&mask_sampling.origin_y.to_ne_bytes());
    bytes[56..60].copy_from_slice(&(mask_sampling_fill(mask_sampling)).to_ne_bytes());
    bytes
}

fn mask_sampling_fill(mask_sampling: GpuMaskSamplingInfo) -> f32 {
    f32::from(mask_sampling.fill_value) / 255.0
}

#[cfg(test)]
mod tests {
    use clip_model::LayerId;

    use crate::{GpuNormalRasterSource, GpuRasterBlendMode, GpuRasterResourceKey};

    use super::{
        generated_raster_source_uniform_bytes_with_blend_and_origins,
        lut_filter_uniform_bytes_with_target_origin,
        raster_source_uniform_bytes_with_target_origin,
    };

    #[test]
    fn target_origin_uses_tail_padding_bytes() {
        let bytes = raster_source_uniform_bytes_with_target_origin(
            GpuNormalRasterSource {
                key: GpuRasterResourceKey {
                    layer_id: LayerId(1),
                    render_mipmap_id: 2,
                },
                opacity: 1.0,
                mask_key: None,
                offset_x: 3,
                offset_y: 4,
                blend_mode: GpuRasterBlendMode::Normal,
            },
            (5, 6),
        );

        assert_eq!(&bytes[32..36], &3i32.to_ne_bytes());
        assert_eq!(&bytes[36..40], &4i32.to_ne_bytes());
        assert_eq!(&bytes[40..44], &5i32.to_ne_bytes());
        assert_eq!(&bytes[44..48], &6i32.to_ne_bytes());
    }

    #[test]
    fn generated_source_records_source_and_target_origins() {
        let bytes = generated_raster_source_uniform_bytes_with_blend_and_origins(
            0.5,
            true,
            GpuRasterBlendMode::Normal,
            (7, 8),
            (9, 10),
        );

        assert_eq!(&bytes[32..36], &7i32.to_ne_bytes());
        assert_eq!(&bytes[36..40], &8i32.to_ne_bytes());
        assert_eq!(&bytes[40..44], &9i32.to_ne_bytes());
        assert_eq!(&bytes[44..48], &10i32.to_ne_bytes());
    }

    #[test]
    fn lut_filter_records_target_origin() {
        let bytes = lut_filter_uniform_bytes_with_target_origin(
            0.75,
            true,
            crate::GpuLutFilterMode::GradientMapLum,
            (11, 12),
        );

        assert_eq!(&bytes[0..4], &0.75f32.to_ne_bytes());
        assert_eq!(&bytes[4..8], &1u32.to_ne_bytes());
        assert_eq!(&bytes[8..12], &1u32.to_ne_bytes());
        assert_eq!(&bytes[16..20], &11i32.to_ne_bytes());
        assert_eq!(&bytes[20..24], &12i32.to_ne_bytes());
    }
}
