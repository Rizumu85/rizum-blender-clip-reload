use clip_graph::RenderNodeId;
use clip_model::CanvasSize;

use super::common::*;
use crate::stream_tile_silo::raster_silo_run_len;
use crate::{GpuDeviceConfig, GpuNormalStackSource, GpuRasterBlendMode, GpuRenderer};

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
