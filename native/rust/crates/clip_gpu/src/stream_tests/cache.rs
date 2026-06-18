use std::collections::HashMap;

use clip_graph::RenderNodeId;
use clip_model::CanvasSize;

use super::common::*;
use crate::{
    GpuClippedStackSource, GpuDeviceConfig, GpuLutFilterMode, GpuMaskResourceCache,
    GpuMaskResourceKey, GpuNormalRasterSource, GpuNormalStackResourceProvider,
    GpuNormalStackSource, GpuRasterAtlasSource, GpuRasterAtlasTileChunk, GpuRasterAtlasTilePixels,
    GpuRasterBlendMode, GpuRasterResourceCache, GpuRasterResourceInfo, GpuRasterResourceKey,
    GpuRasterUpload, GpuRenderError, GpuRenderer,
};

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
        clipped: vec![
            GpuClippedStackSource::Raster(raster_source(clipped_key)),
            GpuClippedStackSource::Raster(raster_source(clipped_key)),
        ],
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
fn streamed_clipping_run_collapses_clipped_raster_siblings_with_atlas_tiles() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let base_key = raster_key(101);
    let clipped_key = raster_key(102);
    let multiply_key = raster_key(103);
    let mut multiply = raster_source(multiply_key);
    multiply.blend_mode = GpuRasterBlendMode::Multiply;
    let mut provider = ClippedAtlasProvider::new(vec![
        (
            base_key,
            InlineRaster {
                render_node_id: RenderNodeId(101),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [255, 0, 0, 255].repeat(4),
            },
        ),
        (
            clipped_key,
            InlineRaster {
                render_node_id: RenderNodeId(102),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [0, 0, 255, 255].repeat(4),
            },
        ),
        (
            multiply_key,
            InlineRaster {
                render_node_id: RenderNodeId(103),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [128, 128, 128, 255].repeat(4),
            },
        ),
    ]);
    let sources = [GpuNormalStackSource::ClippingRun {
        base: raster_source(base_key),
        clipped: vec![
            GpuClippedStackSource::Raster(raster_source(clipped_key)),
            GpuClippedStackSource::Raster(multiply),
        ],
    }];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(4, 4), &sources, &mut provider)
        .expect("draw streamed clipping run with clipped tile-silo");

    let mut expected = [255, 255, 255, 0].repeat(16);
    for y in 1..=2 {
        for x in 1..=2 {
            let offset = ((y * 4 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[0, 0, 128, 255]);
        }
    }
    assert_eq!(output.pixels, expected);
    assert_eq!(provider.atlas_requests, 1);
    assert_eq!(provider.raster_request_count(base_key), 0);
    assert_eq!(provider.raster_request_count(clipped_key), 0);
    assert_eq!(provider.raster_request_count(multiply_key), 0);
}

#[test]
fn streamed_clipped_tile_silo_skips_chunks_outside_cropped_base() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let base_key = raster_key(111);
    let clipped_key = raster_key(112);
    let multiply_key = raster_key(113);
    let mut multiply = raster_source_at(multiply_key, 0, 0);
    multiply.blend_mode = GpuRasterBlendMode::Multiply;
    let mut provider = ClippedAtlasProvider::new(vec![
        (
            base_key,
            InlineRaster {
                render_node_id: RenderNodeId(111),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [255, 0, 0, 255].repeat(4),
            },
        ),
        (
            clipped_key,
            InlineRaster {
                render_node_id: RenderNodeId(112),
                size: CanvasSize::new(3, 3),
                offset: (0, 0),
                pixels: [0, 0, 255, 255].repeat(9),
            },
        ),
        (
            multiply_key,
            InlineRaster {
                render_node_id: RenderNodeId(113),
                size: CanvasSize::new(3, 3),
                offset: (0, 0),
                pixels: [128, 128, 128, 255].repeat(9),
            },
        ),
    ])
    .with_outside_chunks();
    let sources = [GpuNormalStackSource::ClippingRun {
        base: raster_source(base_key),
        clipped: vec![
            GpuClippedStackSource::Raster(raster_source_at(clipped_key, 0, 0)),
            GpuClippedStackSource::Raster(multiply),
        ],
    }];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(4, 4), &sources, &mut provider)
        .expect("draw clipped tile-silo with chunks outside base crop");

    let mut expected = [255, 255, 255, 0].repeat(16);
    for y in 1..=2 {
        for x in 1..=2 {
            let offset = ((y * 4 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[0, 0, 128, 255]);
        }
    }
    assert_eq!(output.pixels, expected);
    assert_eq!(provider.atlas_requests, 1);
    assert_eq!(provider.raster_request_count(clipped_key), 0);
    assert_eq!(provider.raster_request_count(multiply_key), 0);
}

#[test]
fn streamed_clipping_run_collapses_base_and_clipped_rasters_with_atlas_tiles() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let base_key = raster_key(121);
    let clipped_key = raster_key(122);
    let multiply_key = raster_key(123);
    let mut multiply = raster_source(multiply_key);
    multiply.blend_mode = GpuRasterBlendMode::Multiply;
    let mut provider = ClippedAtlasProvider::new(vec![
        (
            base_key,
            InlineRaster {
                render_node_id: RenderNodeId(121),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [255, 0, 0, 255].repeat(4),
            },
        ),
        (
            clipped_key,
            InlineRaster {
                render_node_id: RenderNodeId(122),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [0, 0, 255, 255].repeat(4),
            },
        ),
        (
            multiply_key,
            InlineRaster {
                render_node_id: RenderNodeId(123),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [128, 128, 128, 255].repeat(4),
            },
        ),
    ]);
    let sources = [GpuNormalStackSource::ClippingRun {
        base: raster_source(base_key),
        clipped: vec![
            GpuClippedStackSource::Raster(raster_source(clipped_key)),
            GpuClippedStackSource::Raster(multiply),
        ],
    }];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(4, 4), &sources, &mut provider)
        .expect("draw tile-local raster clipping run");

    let mut expected = [255, 255, 255, 0].repeat(16);
    for y in 1..=2 {
        for x in 1..=2 {
            let offset = ((y * 4 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[0, 0, 128, 255]);
        }
    }
    assert_eq!(output.pixels, expected);
    assert_eq!(provider.atlas_requests, 1);
    assert_eq!(provider.raster_request_count(base_key), 0);
    assert_eq!(provider.raster_request_count(clipped_key), 0);
    assert_eq!(provider.raster_request_count(multiply_key), 0);
}

#[test]
fn streamed_clipping_run_tile_silo_resolves_base_blend_to_parent() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let background_key = raster_key(131);
    let base_key = raster_key(132);
    let clipped_key = raster_key(133);
    let mut base = raster_source(base_key);
    base.blend_mode = GpuRasterBlendMode::Multiply;
    let mut provider = ClippedAtlasProvider::new(vec![
        (
            background_key,
            InlineRaster {
                render_node_id: RenderNodeId(131),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [128, 128, 128, 255].repeat(4),
            },
        ),
        (
            base_key,
            InlineRaster {
                render_node_id: RenderNodeId(132),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [255, 0, 0, 255].repeat(4),
            },
        ),
        (
            clipped_key,
            InlineRaster {
                render_node_id: RenderNodeId(133),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [0, 0, 255, 255].repeat(4),
            },
        ),
    ]);
    let sources = [
        GpuNormalStackSource::Raster(raster_source(background_key)),
        GpuNormalStackSource::ClippingRun {
            base,
            clipped: vec![GpuClippedStackSource::Raster(raster_source(clipped_key))],
        },
    ];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(4, 4), &sources, &mut provider)
        .expect("draw tile-local clipping run with base blend resolve");

    let mut expected = [255, 255, 255, 0].repeat(16);
    for y in 1..=2 {
        for x in 1..=2 {
            let offset = ((y * 4 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[0, 0, 128, 255]);
        }
    }
    assert_eq!(output.pixels, expected);
    assert_eq!(provider.atlas_requests, 1);
    assert_eq!(provider.raster_request_count(background_key), 1);
    assert_eq!(provider.raster_request_count(base_key), 0);
    assert_eq!(provider.raster_request_count(clipped_key), 0);
}

#[test]
fn streamed_clipping_run_accepts_clipped_container_source() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let base_key = raster_key(30);
    let clipped_key = raster_key(31);
    let mut provider = InlineProvider::new(vec![
        (
            base_key,
            InlineRaster {
                render_node_id: RenderNodeId(30),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [255, 0, 0, 255].repeat(4),
            },
        ),
        (
            clipped_key,
            InlineRaster {
                render_node_id: RenderNodeId(31),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: [0, 0, 255, 255].repeat(4),
            },
        ),
    ]);
    let sources = [GpuNormalStackSource::ClippingRun {
        base: raster_source(base_key),
        clipped: vec![GpuClippedStackSource::Container {
            layer_id: base_key.layer_id,
            children: vec![GpuNormalStackSource::Raster(raster_source(clipped_key))],
            opacity: 1.0,
            mask_key: None,
            blend_mode: GpuRasterBlendMode::Normal,
        }],
    }];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(4, 4), &sources, &mut provider)
        .expect("draw streamed clipping run with clipped container");

    let mut expected = [255, 255, 255, 0].repeat(16);
    for y in 1..=2 {
        for x in 1..=2 {
            let offset = ((y * 4 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[0, 0, 255, 255]);
        }
    }
    assert_eq!(output.pixels, expected);
    assert_eq!(provider.raster_request_count(base_key), 1);
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
            filter_mode: lut_mode(),
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
    let mut mask_pixels = vec![0u8; 2 * 2];
    mask_pixels[1 * 2 + 1] = 255;
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
            size: CanvasSize::new(2, 2),
            origin: (1, 1),
            fill_value: 0,
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
                filter_mode: lut_mode(),
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

#[test]
fn streamed_threshold_lut_filter_uses_threshold_luminance_weights() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = raster_key(19);
    let mut provider = InlineProvider::new(vec![(
        key,
        InlineRaster {
            render_node_id: RenderNodeId(19),
            size: CanvasSize::new(1, 1),
            offset: (1, 1),
            pixels: vec![0, 255, 0, 255],
        },
    )]);
    let sources = [
        GpuNormalStackSource::Raster(raster_source(key)),
        GpuNormalStackSource::LutFilter {
            lut_rgba: threshold_lut(150),
            opacity: 1.0,
            mask_key: None,
            filter_mode: GpuLutFilterMode::ThresholdLum,
        },
    ];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(3, 3), &sources, &mut provider)
        .expect("draw streamed threshold LUT filter");

    let mut expected = [255, 255, 255, 0].repeat(9);
    expected[((4 * 4) as usize)..((4 * 4 + 4) as usize)].copy_from_slice(&[0, 0, 0, 255]);
    assert_eq!(output.pixels, expected);
}

#[test]
fn streamed_hsl_filter_matches_csp_fixed_point_hsv_adjust_formula() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = raster_key(29);
    let mut provider = InlineProvider::new(vec![(
        key,
        InlineRaster {
            render_node_id: RenderNodeId(29),
            size: CanvasSize::new(1, 1),
            offset: (1, 1),
            pixels: vec![120, 80, 200, 255],
        },
    )]);
    let sources = [
        GpuNormalStackSource::Raster(raster_source(key)),
        GpuNormalStackSource::LutFilter {
            lut_rgba: identity_lut(),
            opacity: 1.0,
            mask_key: None,
            filter_mode: hsl_mode(1.0 / 12.0, -0.25, 0.25),
        },
    ];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(3, 3), &sources, &mut provider)
        .expect("draw streamed HSL filter");

    let mut expected = [255, 255, 255, 0].repeat(9);
    expected[((4 * 4) as usize)..((4 * 4 + 4) as usize)].copy_from_slice(&[190, 133, 201, 255]);
    assert_eq!(output.pixels, expected);
}

#[test]
fn streamed_cropped_mask_uses_fill_value_outside_texture() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = raster_key(10);
    let mask_key = mask_key(10);
    let mut provider = InlineProvider::new(vec![(
        key,
        InlineRaster {
            render_node_id: RenderNodeId(10),
            size: CanvasSize::new(3, 3),
            offset: (0, 0),
            pixels: [255, 0, 0, 255].repeat(9),
        },
    )])
    .with_masks(vec![(
        mask_key,
        InlineMask {
            render_node_id: RenderNodeId(100),
            size: CanvasSize::new(1, 1),
            origin: (1, 1),
            fill_value: 255,
            pixels: vec![0],
        },
    )]);
    let sources = [GpuNormalStackSource::Raster(raster_source_at_with_mask(
        key, mask_key, 0, 0,
    ))];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(3, 3), &sources, &mut provider)
        .expect("draw raster with cropped fill mask");

    let mut expected = [255, 0, 0, 255].repeat(9);
    expected[((1 * 3 + 1) * 4) as usize..((1 * 3 + 1) * 4 + 4) as usize]
        .copy_from_slice(&[255, 255, 255, 0]);
    assert_eq!(output.pixels, expected);
}

struct ClippedAtlasProvider {
    rasters: HashMap<GpuRasterResourceKey, InlineRaster>,
    raster_requests: HashMap<GpuRasterResourceKey, usize>,
    atlas_requests: usize,
    outside_chunks: bool,
}

impl ClippedAtlasProvider {
    fn new(rasters: Vec<(GpuRasterResourceKey, InlineRaster)>) -> Self {
        Self {
            rasters: rasters.into_iter().collect(),
            raster_requests: HashMap::new(),
            atlas_requests: 0,
            outside_chunks: false,
        }
    }

    fn with_outside_chunks(mut self) -> Self {
        self.outside_chunks = true;
        self
    }

    fn raster_request_count(&self, key: GpuRasterResourceKey) -> usize {
        self.raster_requests.get(&key).copied().unwrap_or(0)
    }
}

impl GpuNormalStackResourceProvider for ClippedAtlasProvider {
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

    fn raster_run_atlas_tile_pixels(
        &mut self,
        sources: &[GpuRasterAtlasSource],
        atlas_size: CanvasSize,
    ) -> Result<Option<GpuRasterAtlasTilePixels>, Self::Error> {
        self.atlas_requests += 1;
        let mut chunks = Vec::with_capacity(sources.len());
        let mut resources = Vec::with_capacity(sources.len());
        for request in sources {
            let raster = self.rasters.get(&request.source.key).ok_or(
                GpuRenderError::MissingRasterResource {
                    layer_id: request.source.key.layer_id,
                    render_mipmap_id: request.source.key.render_mipmap_id,
                },
            )?;
            if self.outside_chunks
                && request.offset_x == 0
                && request.offset_y == 0
                && raster.size.width > 1
                && raster.size.height > 1
            {
                chunks.push(GpuRasterAtlasTileChunk {
                    source: request.source,
                    atlas_x: request.atlas_x,
                    atlas_y: request.atlas_y,
                    mask_atlas_x: None,
                    mask_atlas_y: None,
                    size: CanvasSize::new(1, 1),
                    offset_x: request.offset_x,
                    offset_y: request.offset_y,
                    pixels: raster_chunk_pixels(raster, 0, 0, 1, 1),
                });
                let inner_width = raster.size.width - 1;
                let inner_height = raster.size.height - 1;
                chunks.push(GpuRasterAtlasTileChunk {
                    source: request.source,
                    atlas_x: request.atlas_x + 1,
                    atlas_y: request.atlas_y + 1,
                    mask_atlas_x: None,
                    mask_atlas_y: None,
                    size: CanvasSize::new(inner_width, inner_height),
                    offset_x: request.offset_x + 1,
                    offset_y: request.offset_y + 1,
                    pixels: raster_chunk_pixels(raster, 1, 1, inner_width, inner_height),
                });
            } else {
                chunks.push(GpuRasterAtlasTileChunk {
                    source: request.source,
                    atlas_x: request.atlas_x,
                    atlas_y: request.atlas_y,
                    mask_atlas_x: None,
                    mask_atlas_y: None,
                    size: raster.size,
                    offset_x: request.offset_x,
                    offset_y: request.offset_y,
                    pixels: raster.pixels.clone(),
                });
            }
            resources.push(GpuRasterResourceInfo {
                key: request.source.key,
                render_node_id: raster.render_node_id,
                size: raster.size,
                byte_len: raster.pixels.len(),
            });
        }
        Ok(Some(GpuRasterAtlasTilePixels {
            size: atlas_size,
            chunks,
            mask_chunks: Vec::new(),
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

fn raster_chunk_pixels(raster: &InlineRaster, x: u32, y: u32, width: u32, height: u32) -> Vec<u8> {
    let source_width = usize::try_from(raster.size.width).expect("fixture width fits usize");
    let x = usize::try_from(x).expect("fixture x fits usize");
    let y = usize::try_from(y).expect("fixture y fits usize");
    let width = usize::try_from(width).expect("fixture width fits usize");
    let height = usize::try_from(height).expect("fixture height fits usize");
    let mut pixels = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        let start = ((y + row) * source_width + x) * 4;
        let end = start + width * 4;
        pixels.extend_from_slice(&raster.pixels[start..end]);
    }
    pixels
}
