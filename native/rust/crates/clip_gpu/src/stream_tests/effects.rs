use clip_graph::RenderNodeId;
use clip_model::CanvasSize;

use super::common::*;
use crate::{GpuDeviceConfig, GpuNormalStackSource, GpuRasterBlendMode, GpuRenderer};

#[test]
fn streamed_zero_opacity_raster_skips_provider_request() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = raster_key(11);
    let mut provider = InlineProvider::new(vec![(
        key,
        InlineRaster {
            render_node_id: RenderNodeId(11),
            size: CanvasSize::new(2, 2),
            offset: (1, 1),
            pixels: [255, 0, 0, 255].repeat(4),
        },
    )]);
    let sources = [GpuNormalStackSource::Raster(raster_source_at_with_opacity(
        key, 1, 1, 0.0,
    ))];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(4, 4), &sources, &mut provider)
        .expect("draw zero-opacity raster");

    assert_eq!(output.pixels, [255, 255, 255, 0].repeat(16));
    assert_eq!(provider.raster_request_count(key), 0);
}

#[test]
fn streamed_zero_opacity_container_skips_child_provider_request() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = raster_key(12);
    let mut provider = InlineProvider::new(vec![(
        key,
        InlineRaster {
            render_node_id: RenderNodeId(12),
            size: CanvasSize::new(2, 2),
            offset: (1, 1),
            pixels: [0, 255, 0, 255].repeat(4),
        },
    )]);
    let sources = [GpuNormalStackSource::Container {
        children: vec![GpuNormalStackSource::Raster(raster_source(key))],
        opacity: 0.0,
        mask_key: None,
        blend_mode: GpuRasterBlendMode::Normal,
    }];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(4, 4), &sources, &mut provider)
        .expect("draw zero-opacity container");

    assert_eq!(output.pixels, [255, 255, 255, 0].repeat(16));
    assert_eq!(provider.raster_request_count(key), 0);
}

#[test]
fn streamed_zero_opacity_clipped_sibling_skips_provider_request() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let base_key = raster_key(13);
    let clipped_key = raster_key(14);
    let mut provider = InlineProvider::new(vec![
        (
            base_key,
            InlineRaster {
                render_node_id: RenderNodeId(13),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [255, 0, 0, 255].repeat(4),
            },
        ),
        (
            clipped_key,
            InlineRaster {
                render_node_id: RenderNodeId(14),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [0, 255, 0, 255].repeat(4),
            },
        ),
    ]);
    let sources = [GpuNormalStackSource::ClippingRun {
        base: raster_source(base_key),
        clipped: vec![raster_source_at_with_opacity(clipped_key, 1, 1, 0.0)],
    }];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(4, 4), &sources, &mut provider)
        .expect("draw clipping run with zero-opacity clipped sibling");

    let mut expected = [255, 255, 255, 0].repeat(16);
    for y in 1..=2 {
        for x in 1..=2 {
            let offset = ((y * 4 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[255, 0, 0, 255]);
        }
    }
    assert_eq!(output.pixels, expected);
    assert_eq!(provider.raster_request_count(base_key), 1);
    assert_eq!(provider.raster_request_count(clipped_key), 0);
}
