use std::collections::HashMap;

use clip_graph::RenderNodeId;
use clip_model::{CanvasSize, LayerId};

use crate::{
    GpuDeviceConfig, GpuMaskResourceCache, GpuMaskResourceKey, GpuNormalRasterSource,
    GpuNormalStackResourceProvider, GpuNormalStackSource, GpuRasterBlendMode,
    GpuRasterResourceCache, GpuRasterResourceKey, GpuRasterUpload, GpuRenderError, GpuRenderer,
};

struct InlineProvider {
    rasters: HashMap<GpuRasterResourceKey, InlineRaster>,
}

struct InlineRaster {
    render_node_id: RenderNodeId,
    size: CanvasSize,
    offset: (i32, i32),
    pixels: Vec<u8>,
}

impl InlineProvider {
    fn new(rasters: Vec<(GpuRasterResourceKey, InlineRaster)>) -> Self {
        Self {
            rasters: rasters.into_iter().collect(),
        }
    }
}

impl GpuNormalStackResourceProvider for InlineProvider {
    type Error = GpuRenderError;

    fn raster_resource(
        &mut self,
        renderer: &GpuRenderer,
        source: GpuNormalRasterSource,
    ) -> Result<GpuRasterResourceCache, Self::Error> {
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
        _renderer: &GpuRenderer,
        _key: GpuMaskResourceKey,
    ) -> Result<GpuMaskResourceCache, Self::Error> {
        unreachable!("stream origin fixtures have no masks")
    }
}

#[test]
fn streamed_clipping_cache_resolves_from_cropped_origin() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let base_key = raster_key(1);
    let clipped_key = raster_key(2);
    let mut provider = InlineProvider::new(vec![
        (
            base_key,
            InlineRaster {
                render_node_id: RenderNodeId(1),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [255, 0, 0, 255].repeat(4),
            },
        ),
        (
            clipped_key,
            InlineRaster {
                render_node_id: RenderNodeId(2),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [0, 0, 255, 255].repeat(4),
            },
        ),
    ]);
    let sources = [GpuNormalStackSource::ClippingRun {
        base: raster_source(base_key),
        clipped: vec![raster_source(clipped_key)],
    }];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(4, 4), &sources, &mut provider)
        .expect("draw streamed cropped clipping run");

    let mut expected = [255, 255, 255, 0].repeat(16);
    for y in 1..=2 {
        for x in 1..=2 {
            let offset = ((y * 4 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[0, 0, 255, 255]);
        }
    }
    assert_eq!(output.pixels, expected);
}

#[test]
fn streamed_container_cache_resolves_from_cropped_origin() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = raster_key(3);
    let mut provider = InlineProvider::new(vec![(
        key,
        InlineRaster {
            render_node_id: RenderNodeId(3),
            size: CanvasSize::new(2, 2),
            offset: (1, 1),
            pixels: [0, 255, 0, 255].repeat(4),
        },
    )]);
    let sources = [GpuNormalStackSource::Container {
        children: vec![GpuNormalStackSource::Raster(raster_source(key))],
        opacity: 1.0,
        mask_key: None,
        blend_mode: GpuRasterBlendMode::Normal,
    }];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(4, 4), &sources, &mut provider)
        .expect("draw streamed cropped container");

    let mut expected = [255, 255, 255, 0].repeat(16);
    for y in 1..=2 {
        for x in 1..=2 {
            let offset = ((y * 4 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[0, 255, 0, 255]);
        }
    }
    assert_eq!(output.pixels, expected);
}

#[test]
fn streamed_nested_container_resolves_into_cropped_parent_origin() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = raster_key(4);
    let mut provider = InlineProvider::new(vec![(
        key,
        InlineRaster {
            render_node_id: RenderNodeId(4),
            size: CanvasSize::new(2, 2),
            offset: (2, 1),
            pixels: [255, 255, 0, 255].repeat(4),
        },
    )]);
    let nested = GpuNormalStackSource::Container {
        children: vec![GpuNormalStackSource::Raster(raster_source_at(key, 2, 1))],
        opacity: 1.0,
        mask_key: None,
        blend_mode: GpuRasterBlendMode::Normal,
    };
    let sources = [GpuNormalStackSource::Container {
        children: vec![nested],
        opacity: 1.0,
        mask_key: None,
        blend_mode: GpuRasterBlendMode::Normal,
    }];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(5, 4), &sources, &mut provider)
        .expect("draw streamed nested cropped container");

    let mut expected = [255, 255, 255, 0].repeat(20);
    for y in 1..=2 {
        for x in 2..=3 {
            let offset = ((y * 5 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[255, 255, 0, 255]);
        }
    }
    assert_eq!(output.pixels, expected);
}

fn raster_source(key: GpuRasterResourceKey) -> GpuNormalRasterSource {
    raster_source_at(key, 1, 1)
}

fn raster_source_at(
    key: GpuRasterResourceKey,
    offset_x: i32,
    offset_y: i32,
) -> GpuNormalRasterSource {
    GpuNormalRasterSource {
        key,
        opacity: 1.0,
        mask_key: None,
        offset_x,
        offset_y,
        blend_mode: GpuRasterBlendMode::Normal,
    }
}

fn raster_key(id: u32) -> GpuRasterResourceKey {
    GpuRasterResourceKey {
        layer_id: LayerId(id),
        render_mipmap_id: id,
    }
}
