use clip_graph::RenderNodeId;
use clip_model::CanvasSize;

use super::common::*;
use crate::{
    GpuDeviceConfig, GpuLutFilterMode, GpuNormalStackSource, GpuRasterBlendMode, GpuRenderer,
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
