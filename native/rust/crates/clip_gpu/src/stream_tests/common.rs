use std::collections::HashMap;

use clip_graph::RenderNodeId;
use clip_model::{CanvasSize, LayerId};

use crate::{
    GpuLutFilterMode, GpuMaskResourceCache, GpuMaskResourceKey, GpuMaskUpload,
    GpuNormalRasterSource, GpuNormalStackResourceProvider, GpuRasterBlendMode,
    GpuRasterResourceCache, GpuRasterResourceKey, GpuRasterUpload, GpuRenderError, GpuRenderer,
};

pub(super) struct InlineProvider {
    rasters: HashMap<GpuRasterResourceKey, InlineRaster>,
    masks: HashMap<GpuMaskResourceKey, InlineMask>,
    raster_requests: HashMap<GpuRasterResourceKey, usize>,
    mask_requests: HashMap<GpuMaskResourceKey, usize>,
}

pub(super) struct InlineRaster {
    pub(super) render_node_id: RenderNodeId,
    pub(super) size: CanvasSize,
    pub(super) offset: (i32, i32),
    pub(super) pixels: Vec<u8>,
}

pub(super) struct InlineMask {
    pub(super) render_node_id: RenderNodeId,
    pub(super) size: CanvasSize,
    pub(super) origin: (i32, i32),
    pub(super) fill_value: u8,
    pub(super) pixels: Vec<u8>,
}

impl InlineProvider {
    pub(super) fn new(rasters: Vec<(GpuRasterResourceKey, InlineRaster)>) -> Self {
        Self {
            rasters: rasters.into_iter().collect(),
            masks: HashMap::new(),
            raster_requests: HashMap::new(),
            mask_requests: HashMap::new(),
        }
    }

    pub(super) fn with_masks(mut self, masks: Vec<(GpuMaskResourceKey, InlineMask)>) -> Self {
        self.masks = masks.into_iter().collect();
        self
    }

    pub(super) fn raster_request_count(&self, key: GpuRasterResourceKey) -> usize {
        self.raster_requests.get(&key).copied().unwrap_or(0)
    }

    pub(super) fn mask_request_count(&self, key: GpuMaskResourceKey) -> usize {
        self.mask_requests.get(&key).copied().unwrap_or(0)
    }
}

impl GpuNormalStackResourceProvider for InlineProvider {
    type Error = GpuRenderError;

    fn raster_resource(
        &mut self,
        renderer: &GpuRenderer,
        source: GpuNormalRasterSource,
    ) -> Result<GpuRasterResourceCache, Self::Error> {
        *self.raster_requests.entry(source.key).or_default() += 1;
        let raster =
            self.rasters
                .get(&source.key)
                .ok_or(GpuRenderError::MissingRasterResource {
                    layer_id: source.key.layer_id,
                    render_mipmap_id: source.key.render_mipmap_id,
                })?;
        renderer.upload_raster_resources(&[GpuRasterUpload {
            layer_id: source.key.layer_id,
            render_node_id: raster.render_node_id,
            render_mipmap_id: source.key.render_mipmap_id,
            size: raster.size,
            pixels: &raster.pixels,
        }])
    }

    fn raster_resource_size(&self, source: GpuNormalRasterSource) -> Option<CanvasSize> {
        self.rasters.get(&source.key).map(|raster| raster.size)
    }

    fn raster_resource_offset(&self, source: GpuNormalRasterSource) -> Option<(i32, i32)> {
        self.rasters.get(&source.key).map(|raster| raster.offset)
    }

    fn mask_resource(
        &mut self,
        renderer: &GpuRenderer,
        key: GpuMaskResourceKey,
    ) -> Result<GpuMaskResourceCache, Self::Error> {
        *self.mask_requests.entry(key).or_default() += 1;
        let mask = self
            .masks
            .get(&key)
            .ok_or(GpuRenderError::MissingMaskResource {
                layer_id: key.layer_id,
                mask_mipmap_id: key.mask_mipmap_id,
            })?;
        renderer.upload_mask_resources(&[GpuMaskUpload {
            layer_id: key.layer_id,
            render_node_id: mask.render_node_id,
            mask_mipmap_id: key.mask_mipmap_id,
            size: mask.size,
            origin_x: mask.origin.0,
            origin_y: mask.origin.1,
            fill_value: mask.fill_value,
            upload_origin_x: 0,
            upload_origin_y: 0,
            upload_size: mask.size,
            pixels: &mask.pixels,
        }])
    }
}

pub(super) fn raster_source(key: GpuRasterResourceKey) -> GpuNormalRasterSource {
    raster_source_at(key, 1, 1)
}

pub(super) fn raster_source_with_mask(
    key: GpuRasterResourceKey,
    mask_key: GpuMaskResourceKey,
) -> GpuNormalRasterSource {
    raster_source_at_with_mask(key, mask_key, 1, 1)
}

pub(super) fn raster_source_at_with_mask(
    key: GpuRasterResourceKey,
    mask_key: GpuMaskResourceKey,
    offset_x: i32,
    offset_y: i32,
) -> GpuNormalRasterSource {
    GpuNormalRasterSource {
        mask_key: Some(mask_key),
        ..raster_source_at(key, offset_x, offset_y)
    }
}

pub(super) fn raster_source_at(
    key: GpuRasterResourceKey,
    offset_x: i32,
    offset_y: i32,
) -> GpuNormalRasterSource {
    raster_source_at_with_opacity(key, offset_x, offset_y, 1.0)
}

pub(super) fn raster_source_at_with_opacity(
    key: GpuRasterResourceKey,
    offset_x: i32,
    offset_y: i32,
    opacity: f32,
) -> GpuNormalRasterSource {
    GpuNormalRasterSource {
        key,
        opacity,
        mask_key: None,
        offset_x,
        offset_y,
        blend_mode: GpuRasterBlendMode::Normal,
    }
}

pub(super) fn raster_key(id: u32) -> GpuRasterResourceKey {
    GpuRasterResourceKey {
        layer_id: LayerId(id),
        render_mipmap_id: id,
    }
}

pub(super) fn mask_key(id: u32) -> GpuMaskResourceKey {
    GpuMaskResourceKey {
        layer_id: LayerId(id + 100),
        mask_mipmap_id: id + 200,
    }
}

pub(super) fn inverted_tone_curve_lut() -> Vec<u8> {
    let mut lut = Vec::with_capacity(256 * 4);
    for value in 0..=255u8 {
        let inverted = 255 - value;
        lut.extend_from_slice(&[inverted, inverted, inverted, 255]);
    }
    lut
}

pub(super) fn lut_mode() -> GpuLutFilterMode {
    GpuLutFilterMode::ToneCurveRgb
}
