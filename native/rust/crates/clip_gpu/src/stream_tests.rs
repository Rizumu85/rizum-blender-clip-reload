use std::collections::HashMap;

use clip_graph::RenderNodeId;
use clip_model::{CanvasSize, LayerId};

use crate::{
    GpuDeviceConfig, GpuLutFilterMode, GpuMaskResourceCache, GpuMaskResourceKey, GpuMaskUpload,
    GpuNormalRasterSource, GpuNormalStackResourceProvider, GpuNormalStackSource,
    GpuRasterBlendMode, GpuRasterResourceCache, GpuRasterResourceKey, GpuRasterUpload,
    GpuRenderError, GpuRenderer,
};

struct InlineProvider {
    rasters: HashMap<GpuRasterResourceKey, InlineRaster>,
    masks: HashMap<GpuMaskResourceKey, InlineMask>,
    raster_requests: HashMap<GpuRasterResourceKey, usize>,
    mask_requests: HashMap<GpuMaskResourceKey, usize>,
}

struct InlineRaster {
    render_node_id: RenderNodeId,
    size: CanvasSize,
    offset: (i32, i32),
    pixels: Vec<u8>,
}

struct InlineMask {
    render_node_id: RenderNodeId,
    size: CanvasSize,
    pixels: Vec<u8>,
}

impl InlineProvider {
    fn new(rasters: Vec<(GpuRasterResourceKey, InlineRaster)>) -> Self {
        Self {
            rasters: rasters.into_iter().collect(),
            masks: HashMap::new(),
            raster_requests: HashMap::new(),
            mask_requests: HashMap::new(),
        }
    }

    fn with_masks(mut self, masks: Vec<(GpuMaskResourceKey, InlineMask)>) -> Self {
        self.masks = masks.into_iter().collect();
        self
    }

    fn raster_request_count(&self, key: GpuRasterResourceKey) -> usize {
        self.raster_requests.get(&key).copied().unwrap_or(0)
    }

    fn mask_request_count(&self, key: GpuMaskResourceKey) -> usize {
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
            upload_origin_x: 0,
            upload_origin_y: 0,
            upload_size: mask.size,
            pixels: &mask.pixels,
        }])
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
        clipped: vec![raster_source(clipped_key), raster_source(clipped_key)],
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
    assert_eq!(provider.raster_request_count(clipped_key), 1);
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

#[test]
fn streamed_through_cache_preserves_parent_dirty_outside_cropped_bounds() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let red_key = raster_key(5);
    let green_key = raster_key(6);
    let mut provider = InlineProvider::new(vec![
        (
            red_key,
            InlineRaster {
                render_node_id: RenderNodeId(5),
                size: CanvasSize::new(5, 4),
                offset: (0, 0),
                pixels: [255, 0, 0, 255].repeat(20),
            },
        ),
        (
            green_key,
            InlineRaster {
                render_node_id: RenderNodeId(6),
                size: CanvasSize::new(2, 2),
                offset: (2, 1),
                pixels: [0, 255, 0, 255].repeat(4),
            },
        ),
    ]);
    let sources = [
        GpuNormalStackSource::Raster(raster_source_at(red_key, 0, 0)),
        GpuNormalStackSource::ThroughGroup {
            children: vec![GpuNormalStackSource::Raster(raster_source_at(
                green_key, 2, 1,
            ))],
            opacity: 1.0,
            mask_key: None,
        },
    ];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(5, 4), &sources, &mut provider)
        .expect("draw streamed cropped through group");

    let mut expected = [255, 0, 0, 255].repeat(20);
    for y in 1..=2 {
        for x in 2..=3 {
            let offset = ((y * 5 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[0, 255, 0, 255]);
        }
    }
    assert_eq!(output.pixels, expected);
}

#[test]
fn streamed_nested_through_group_resolves_inside_cropped_container() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let red_key = raster_key(7);
    let green_key = raster_key(8);
    let mut provider = InlineProvider::new(vec![
        (
            red_key,
            InlineRaster {
                render_node_id: RenderNodeId(7),
                size: CanvasSize::new(3, 3),
                offset: (2, 1),
                pixels: [255, 0, 0, 255].repeat(9),
            },
        ),
        (
            green_key,
            InlineRaster {
                render_node_id: RenderNodeId(8),
                size: CanvasSize::new(1, 1),
                offset: (3, 2),
                pixels: [0, 255, 0, 255].to_vec(),
            },
        ),
    ]);
    let sources = [GpuNormalStackSource::Container {
        children: vec![
            GpuNormalStackSource::Raster(raster_source_at(red_key, 2, 1)),
            GpuNormalStackSource::ThroughGroup {
                children: vec![GpuNormalStackSource::Raster(raster_source_at(
                    green_key, 3, 2,
                ))],
                opacity: 1.0,
                mask_key: None,
            },
        ],
        opacity: 1.0,
        mask_key: None,
        blend_mode: GpuRasterBlendMode::Normal,
    }];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(6, 5), &sources, &mut provider)
        .expect("draw nested through group inside cropped container");

    let mut expected = [255, 255, 255, 0].repeat(30);
    for y in 1..=3 {
        for x in 2..=4 {
            let offset = ((y * 6 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[255, 0, 0, 255]);
        }
    }
    expected[((2 * 6 + 3) * 4) as usize..((2 * 6 + 3) * 4 + 4) as usize]
        .copy_from_slice(&[0, 255, 0, 255]);
    assert_eq!(output.pixels, expected);
}

#[test]
fn streamed_lut_filter_scissors_to_existing_dirty_bounds() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = raster_key(7);
    let mut provider = InlineProvider::new(vec![(
        key,
        InlineRaster {
            render_node_id: RenderNodeId(7),
            size: CanvasSize::new(2, 2),
            offset: (1, 1),
            pixels: [255, 0, 0, 255].repeat(4),
        },
    )]);
    let sources = [
        GpuNormalStackSource::Raster(raster_source(key)),
        GpuNormalStackSource::LutFilter {
            lut_rgba: inverted_tone_curve_lut(),
            opacity: 1.0,
            mask_key: None,
            filter_mode: GpuLutFilterMode::ToneCurveRgb,
        },
    ];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(4, 4), &sources, &mut provider)
        .expect("draw streamed scissored LUT filter");

    let mut expected = [255, 255, 255, 0].repeat(16);
    for y in 1..=2 {
        for x in 1..=2 {
            let offset = ((y * 4 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[0, 255, 255, 255]);
        }
    }
    assert_eq!(output.pixels, expected);
}

#[test]
fn streamed_masked_lut_filter_samples_mask_at_cropped_target_origin() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = raster_key(9);
    let mask_key = mask_key(9);
    let mut mask_pixels = vec![0u8; 4 * 4];
    mask_pixels[2 * 4 + 2] = 255;
    let mut provider = InlineProvider::new(vec![(
        key,
        InlineRaster {
            render_node_id: RenderNodeId(9),
            size: CanvasSize::new(2, 2),
            offset: (1, 1),
            pixels: [255, 0, 0, 255].repeat(4),
        },
    )])
    .with_masks(vec![(
        mask_key,
        InlineMask {
            render_node_id: RenderNodeId(90),
            size: CanvasSize::new(4, 4),
            pixels: mask_pixels,
        },
    )]);
    let sources = [GpuNormalStackSource::Container {
        children: vec![
            GpuNormalStackSource::Raster(raster_source_with_mask(key, mask_key)),
            GpuNormalStackSource::LutFilter {
                lut_rgba: inverted_tone_curve_lut(),
                opacity: 1.0,
                mask_key: Some(mask_key),
                filter_mode: GpuLutFilterMode::ToneCurveRgb,
            },
        ],
        opacity: 1.0,
        mask_key: None,
        blend_mode: GpuRasterBlendMode::Normal,
    }];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(4, 4), &sources, &mut provider)
        .expect("draw streamed masked LUT filter inside cropped container");

    let mut expected = [255, 255, 255, 0].repeat(16);
    expected[((2 * 4 + 2) * 4) as usize..((2 * 4 + 2) * 4 + 4) as usize]
        .copy_from_slice(&[0, 255, 255, 255]);
    assert_eq!(output.pixels, expected);
    assert_eq!(provider.mask_request_count(mask_key), 1);
}

fn raster_source(key: GpuRasterResourceKey) -> GpuNormalRasterSource {
    raster_source_at(key, 1, 1)
}

fn raster_source_with_mask(
    key: GpuRasterResourceKey,
    mask_key: GpuMaskResourceKey,
) -> GpuNormalRasterSource {
    GpuNormalRasterSource {
        mask_key: Some(mask_key),
        ..raster_source(key)
    }
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

fn mask_key(id: u32) -> GpuMaskResourceKey {
    GpuMaskResourceKey {
        layer_id: LayerId(id + 100),
        mask_mipmap_id: id + 200,
    }
}

fn inverted_tone_curve_lut() -> Vec<u8> {
    let mut lut = Vec::with_capacity(256 * 4);
    for value in 0..=255u8 {
        let inverted = 255 - value;
        lut.extend_from_slice(&[inverted, inverted, inverted, 255]);
    }
    lut
}
