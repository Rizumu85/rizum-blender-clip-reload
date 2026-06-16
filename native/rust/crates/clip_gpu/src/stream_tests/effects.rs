use clip_graph::RenderNodeId;
use clip_model::{CanvasSize, Rgba8};

use super::common::*;
use crate::{
    GpuClippedStackSource, GpuDeviceConfig, GpuNormalRasterSource, GpuNormalStackSource,
    GpuRasterBlendMode, GpuRasterResourceKey, GpuRenderer,
};

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
        clipped: vec![GpuClippedStackSource::Raster(
            raster_source_at_with_opacity(clipped_key, 1, 1, 0.0),
        )],
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

#[test]
fn streamed_ineffective_clipping_run_matches_direct_base() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let base_key = raster_key(15);
    let clipped_key = raster_key(16);
    let base = GpuNormalRasterSource {
        blend_mode: GpuRasterBlendMode::Multiply,
        ..raster_source_at_with_opacity(base_key, 1, 1, 0.5)
    };
    let clipped = raster_source_at_with_opacity(clipped_key, 1, 1, 0.0);
    let background = GpuNormalStackSource::SolidColor {
        color: Rgba8 {
            r: 120,
            g: 180,
            b: 80,
            a: 255,
        },
        opacity: 1.0,
    };

    let direct_sources = [background.clone(), GpuNormalStackSource::Raster(base)];
    let clipping_sources = [
        background,
        GpuNormalStackSource::ClippingRun {
            base,
            clipped: vec![GpuClippedStackSource::Raster(clipped)],
        },
    ];
    let mut direct_provider = clipping_run_provider(base_key, clipped_key);
    let mut clipping_provider = clipping_run_provider(base_key, clipped_key);

    let direct = renderer
        .draw_normal_stack_with_provider_to_rgba8(
            CanvasSize::new(4, 4),
            &direct_sources,
            &mut direct_provider,
        )
        .expect("draw direct base");
    let clipping = renderer
        .draw_normal_stack_with_provider_to_rgba8(
            CanvasSize::new(4, 4),
            &clipping_sources,
            &mut clipping_provider,
        )
        .expect("draw clipping run with ineffective sibling");

    assert_eq!(clipping.pixels, direct.pixels);
    assert_eq!(clipping_provider.raster_request_count(base_key), 1);
    assert_eq!(clipping_provider.raster_request_count(clipped_key), 0);
}

fn clipping_run_provider(
    base_key: GpuRasterResourceKey,
    clipped_key: GpuRasterResourceKey,
) -> InlineProvider {
    InlineProvider::new(vec![
        (
            base_key,
            InlineRaster {
                render_node_id: RenderNodeId(base_key.render_mipmap_id),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [240, 80, 120, 255].repeat(4),
            },
        ),
        (
            clipped_key,
            InlineRaster {
                render_node_id: RenderNodeId(clipped_key.render_mipmap_id),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [0, 0, 255, 255].repeat(4),
            },
        ),
    ])
}
