use std::collections::HashMap;

use clip_graph::RenderNodeId;
use clip_model::CanvasSize;

use super::common::*;
use crate::stream_tile_silo::raster_silo_run_len;
use crate::{
    GpuDeviceConfig, GpuMaskResourceCache, GpuMaskResourceKey, GpuNormalRasterSource,
    GpuNormalStackResourceProvider, GpuNormalStackSource, GpuRasterAtlasPixels,
    GpuRasterAtlasSource, GpuRasterBlendMode, GpuRasterResourceCache, GpuRasterResourceInfo,
    GpuRasterResourceKey, GpuRenderError, GpuRenderer,
};

#[test]
fn streamed_tile_silo_collapses_opaque_normal_raster_run() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let red_key = raster_key(40);
    let blue_key = raster_key(41);
    let mut provider = InlineProvider::new(vec![
        (
            red_key,
            InlineRaster {
                render_node_id: RenderNodeId(40),
                size: CanvasSize::new(3, 2),
                offset: (1, 1),
                pixels: [255, 0, 0, 255].repeat(6),
            },
        ),
        (
            blue_key,
            InlineRaster {
                render_node_id: RenderNodeId(41),
                size: CanvasSize::new(2, 2),
                offset: (2, 1),
                pixels: [0, 0, 255, 255].repeat(4),
            },
        ),
    ]);
    let sources = [
        GpuNormalStackSource::Raster(raster_source_at(red_key, 1, 1)),
        GpuNormalStackSource::Raster(raster_source_at(blue_key, 2, 1)),
    ];
    assert_eq!(
        raster_silo_run_len(
            &provider,
            CanvasSize::new(5, 4),
            (0, 0),
            CanvasSize::new(5, 4),
            &sources,
        ),
        2
    );

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(5, 4), &sources, &mut provider)
        .expect("draw tile-silo normal run");

    let mut expected = [255, 255, 255, 0].repeat(20);
    for y in 1..=2 {
        for x in 1..=3 {
            let offset = ((y * 5 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[255, 0, 0, 255]);
        }
        for x in 2..=3 {
            let offset = ((y * 5 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[0, 0, 255, 255]);
        }
    }
    assert_eq!(output.pixels, expected);
    assert_eq!(provider.raster_request_count(red_key), 1);
    assert_eq!(provider.raster_request_count(blue_key), 1);
}

#[test]
fn streamed_tile_silo_accepts_provider_backed_atlas_pixels() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let red_key = raster_key(140);
    let blue_key = raster_key(141);
    let mut provider = AtlasInlineProvider::new(vec![
        (
            red_key,
            AtlasInlineRaster {
                render_node_id: RenderNodeId(140),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [255, 0, 0, 255].repeat(4),
            },
        ),
        (
            blue_key,
            AtlasInlineRaster {
                render_node_id: RenderNodeId(141),
                size: CanvasSize::new(2, 2),
                offset: (2, 1),
                pixels: [0, 0, 255, 255].repeat(4),
            },
        ),
    ]);
    let sources = [
        GpuNormalStackSource::Raster(raster_source_at(red_key, 1, 1)),
        GpuNormalStackSource::Raster(raster_source_at(blue_key, 2, 1)),
    ];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(5, 4), &sources, &mut provider)
        .expect("draw provider-backed atlas tile-silo run");

    let mut expected = [255, 255, 255, 0].repeat(20);
    for y in 1..=2 {
        for x in 1..=2 {
            let offset = ((y * 5 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[255, 0, 0, 255]);
        }
        for x in 2..=3 {
            let offset = ((y * 5 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[0, 0, 255, 255]);
        }
    }
    assert_eq!(output.pixels, expected);
    assert_eq!(provider.atlas_requests, 1);
    assert_eq!(provider.raster_requests, 0);
}

#[test]
fn streamed_tile_silo_accepts_provider_backed_masked_normal_atlas_pixels() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let red_key = raster_key(240);
    let blue_key = raster_key(241);
    let blue_mask_key = mask_key(241);
    let mut provider = AtlasInlineProvider::new(vec![
        (
            red_key,
            AtlasInlineRaster {
                render_node_id: RenderNodeId(240),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [255, 0, 0, 255].repeat(4),
            },
        ),
        (
            blue_key,
            AtlasInlineRaster {
                render_node_id: RenderNodeId(241),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [0, 0, 255, 255].repeat(4),
            },
        ),
    ])
    .with_masks(vec![(
        blue_mask_key,
        AtlasInlineMask {
            size: CanvasSize::new(2, 2),
            origin: (1, 1),
            fill_value: 0,
            pixels: vec![255, 0, 128, 255],
        },
    )]);
    let sources = [
        GpuNormalStackSource::Raster(raster_source_at(red_key, 1, 1)),
        GpuNormalStackSource::Raster(raster_source_at_with_mask(blue_key, blue_mask_key, 1, 1)),
    ];
    assert_eq!(
        raster_silo_run_len(
            &provider,
            CanvasSize::new(4, 4),
            (0, 0),
            CanvasSize::new(4, 4),
            &sources,
        ),
        2
    );

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(4, 4), &sources, &mut provider)
        .expect("draw provider-backed masked atlas tile-silo run");

    let mut expected = [255, 255, 255, 0].repeat(16);
    expected[((1 * 4 + 1) * 4) as usize..((1 * 4 + 1) * 4 + 4) as usize]
        .copy_from_slice(&[0, 0, 255, 255]);
    expected[((1 * 4 + 2) * 4) as usize..((1 * 4 + 2) * 4 + 4) as usize]
        .copy_from_slice(&[255, 0, 0, 255]);
    expected[((2 * 4 + 1) * 4) as usize..((2 * 4 + 1) * 4 + 4) as usize]
        .copy_from_slice(&[127, 0, 128, 255]);
    expected[((2 * 4 + 2) * 4) as usize..((2 * 4 + 2) * 4 + 4) as usize]
        .copy_from_slice(&[0, 0, 255, 255]);
    assert_eq!(output.pixels, expected);
    assert_eq!(provider.atlas_requests, 1);
    assert_eq!(provider.raster_requests, 0);
}

#[test]
fn streamed_tile_silo_accepts_nonzero_target_origin() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let red_key = raster_key(47);
    let green_key = raster_key(48);
    let mut provider = InlineProvider::new(vec![
        (
            red_key,
            InlineRaster {
                render_node_id: RenderNodeId(47),
                size: CanvasSize::new(2, 2),
                offset: (2, 1),
                pixels: [255, 0, 0, 255].repeat(4),
            },
        ),
        (
            green_key,
            InlineRaster {
                render_node_id: RenderNodeId(48),
                size: CanvasSize::new(2, 2),
                offset: (3, 1),
                pixels: [0, 255, 0, 255].repeat(4),
            },
        ),
    ]);
    let children = vec![
        GpuNormalStackSource::Raster(raster_source_at(red_key, 2, 1)),
        GpuNormalStackSource::Raster(raster_source_at(green_key, 3, 1)),
    ];
    assert_eq!(
        raster_silo_run_len(
            &provider,
            CanvasSize::new(6, 4),
            (2, 1),
            CanvasSize::new(3, 2),
            &children,
        ),
        2
    );
    let sources = [GpuNormalStackSource::Container {
        children,
        opacity: 1.0,
        mask_key: None,
        blend_mode: GpuRasterBlendMode::Normal,
    }];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(6, 4), &sources, &mut provider)
        .expect("draw tile-silo run inside cropped container");

    let mut expected = [255, 255, 255, 0].repeat(24);
    for y in 1..=2 {
        for x in 2..=3 {
            let offset = ((y * 6 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[255, 0, 0, 255]);
        }
        for x in 3..=4 {
            let offset = ((y * 6 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[0, 255, 0, 255]);
        }
    }
    assert_eq!(output.pixels, expected);
}

#[test]
fn streamed_tile_silo_applies_standard_blend_order() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let base_key = raster_key(42);
    let multiply_key = raster_key(43);
    let mut multiply = raster_source_at(multiply_key, 1, 1);
    multiply.blend_mode = GpuRasterBlendMode::Multiply;
    let mut provider = InlineProvider::new(vec![
        (
            base_key,
            InlineRaster {
                render_node_id: RenderNodeId(42),
                size: CanvasSize::new(1, 1),
                offset: (1, 1),
                pixels: vec![200, 100, 50, 255],
            },
        ),
        (
            multiply_key,
            InlineRaster {
                render_node_id: RenderNodeId(43),
                size: CanvasSize::new(1, 1),
                offset: (1, 1),
                pixels: vec![128, 128, 128, 255],
            },
        ),
    ]);
    let sources = [
        GpuNormalStackSource::Raster(raster_source_at(base_key, 1, 1)),
        GpuNormalStackSource::Raster(multiply),
    ];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(3, 3), &sources, &mut provider)
        .expect("draw tile-silo multiply run");

    let mut expected = [255, 255, 255, 0].repeat(9);
    expected[((1 * 3 + 1) * 4) as usize..((1 * 3 + 1) * 4 + 4) as usize]
        .copy_from_slice(&[100, 50, 25, 255]);
    assert_eq!(output.pixels, expected);
}

#[test]
fn tile_silo_planner_stops_at_masks_and_byte_domain_blends() {
    let first_key = raster_key(44);
    let add_glow_key = raster_key(45);
    let masked_key = raster_key(46);
    let mask_key = mask_key(46);
    let mut add_glow = raster_source_at(add_glow_key, 0, 0);
    add_glow.blend_mode = GpuRasterBlendMode::AddGlow;
    let masked = raster_source_at_with_mask(masked_key, mask_key, 0, 0);
    let provider = InlineProvider::new(vec![
        (
            first_key,
            InlineRaster {
                render_node_id: RenderNodeId(44),
                size: CanvasSize::new(1, 1),
                offset: (0, 0),
                pixels: vec![255, 0, 0, 255],
            },
        ),
        (
            add_glow_key,
            InlineRaster {
                render_node_id: RenderNodeId(45),
                size: CanvasSize::new(1, 1),
                offset: (0, 0),
                pixels: vec![255, 255, 255, 255],
            },
        ),
        (
            masked_key,
            InlineRaster {
                render_node_id: RenderNodeId(46),
                size: CanvasSize::new(1, 1),
                offset: (0, 0),
                pixels: vec![0, 255, 0, 255],
            },
        ),
    ]);

    assert_eq!(
        raster_silo_run_len(
            &provider,
            CanvasSize::new(2, 2),
            (0, 0),
            CanvasSize::new(2, 2),
            &[
                GpuNormalStackSource::Raster(raster_source_at(first_key, 0, 0)),
                GpuNormalStackSource::Raster(add_glow),
            ],
        ),
        1
    );
    assert_eq!(
        raster_silo_run_len(
            &provider,
            CanvasSize::new(2, 2),
            (0, 0),
            CanvasSize::new(2, 2),
            &[
                GpuNormalStackSource::Raster(raster_source_at(first_key, 0, 0)),
                GpuNormalStackSource::Raster(masked),
            ],
        ),
        1
    );
}

struct AtlasInlineProvider {
    rasters: HashMap<GpuRasterResourceKey, AtlasInlineRaster>,
    masks: HashMap<GpuMaskResourceKey, AtlasInlineMask>,
    atlas_requests: usize,
    raster_requests: usize,
}

struct AtlasInlineRaster {
    render_node_id: RenderNodeId,
    size: CanvasSize,
    offset: (i32, i32),
    pixels: Vec<u8>,
}

struct AtlasInlineMask {
    size: CanvasSize,
    origin: (i32, i32),
    fill_value: u8,
    pixels: Vec<u8>,
}

impl AtlasInlineProvider {
    fn new(rasters: Vec<(GpuRasterResourceKey, AtlasInlineRaster)>) -> Self {
        Self {
            rasters: rasters.into_iter().collect(),
            masks: HashMap::new(),
            atlas_requests: 0,
            raster_requests: 0,
        }
    }

    fn with_masks(mut self, masks: Vec<(GpuMaskResourceKey, AtlasInlineMask)>) -> Self {
        self.masks = masks.into_iter().collect();
        self
    }
}

impl GpuNormalStackResourceProvider for AtlasInlineProvider {
    type Error = GpuRenderError;

    fn raster_resource(
        &mut self,
        _renderer: &GpuRenderer,
        _source: GpuNormalRasterSource,
    ) -> Result<GpuRasterResourceCache, Self::Error> {
        self.raster_requests += 1;
        Err(GpuRenderError::NotImplemented)
    }

    fn raster_resource_size(&self, source: GpuNormalRasterSource) -> Option<CanvasSize> {
        self.rasters.get(&source.key).map(|raster| raster.size)
    }

    fn raster_resource_offset(&self, source: GpuNormalRasterSource) -> Option<(i32, i32)> {
        self.rasters.get(&source.key).map(|raster| raster.offset)
    }

    fn raster_run_atlas_applies_masks(&self) -> bool {
        !self.masks.is_empty()
    }

    fn raster_run_atlas_pixels(
        &mut self,
        sources: &[GpuRasterAtlasSource],
        atlas_size: CanvasSize,
    ) -> Result<Option<GpuRasterAtlasPixels>, Self::Error> {
        self.atlas_requests += 1;
        let mut pixels = vec![0u8; (atlas_size.width * atlas_size.height * 4) as usize];
        let mut resources = Vec::with_capacity(sources.len());
        for request in sources {
            let raster = self.rasters.get(&request.source.key).ok_or(
                GpuRenderError::MissingRasterResource {
                    layer_id: request.source.key.layer_id,
                    render_mipmap_id: request.source.key.render_mipmap_id,
                },
            )?;
            for y in 0..raster.size.height {
                for x in 0..raster.size.width {
                    let src = ((y * raster.size.width + x) * 4) as usize;
                    let dst = (((request.atlas_y + y) * atlas_size.width + request.atlas_x + x) * 4)
                        as usize;
                    pixels[dst..dst + 4].copy_from_slice(&raster.pixels[src..src + 4]);
                    if let Some(mask_key) = request.source.mask_key {
                        let mask = self.masks.get(&mask_key).ok_or(
                            GpuRenderError::MissingMaskResource {
                                layer_id: mask_key.layer_id,
                                mask_mipmap_id: mask_key.mask_mipmap_id,
                            },
                        )?;
                        let global_x = request.offset_x + x as i32;
                        let global_y = request.offset_y + y as i32;
                        let mask_value = atlas_mask_value(mask, global_x, global_y);
                        pixels[dst + 3] =
                            ((u16::from(pixels[dst + 3]) * u16::from(mask_value)) / 255) as u8;
                    }
                }
            }
            resources.push(GpuRasterResourceInfo {
                key: request.source.key,
                render_node_id: raster.render_node_id,
                size: raster.size,
                byte_len: raster.pixels.len(),
            });
        }
        Ok(Some(GpuRasterAtlasPixels {
            size: atlas_size,
            pixels,
            resources,
        }))
    }

    fn mask_resource(
        &mut self,
        _renderer: &GpuRenderer,
        key: GpuMaskResourceKey,
    ) -> Result<GpuMaskResourceCache, Self::Error> {
        Err(GpuRenderError::MissingMaskResource {
            layer_id: key.layer_id,
            mask_mipmap_id: key.mask_mipmap_id,
        })
    }
}

fn atlas_mask_value(mask: &AtlasInlineMask, global_x: i32, global_y: i32) -> u8 {
    let local_x = global_x - mask.origin.0;
    let local_y = global_y - mask.origin.1;
    if local_x < 0 || local_y < 0 {
        return mask.fill_value;
    }
    let (Ok(local_x), Ok(local_y)) = (u32::try_from(local_x), u32::try_from(local_y)) else {
        return mask.fill_value;
    };
    if local_x >= mask.size.width || local_y >= mask.size.height {
        return mask.fill_value;
    }
    mask.pixels[(local_y * mask.size.width + local_x) as usize]
}
