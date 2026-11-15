use clip_graph::RenderNodeId;
use clip_model::{CanvasSize, Rgba8};

use super::common::*;
use crate::{
    GpuClippedStackSource, GpuNormalRasterSource, GpuNormalStackSource, GpuRasterBlendMode,
    GpuRasterResourceKey,
};

#[test]
fn streamed_zero_opacity_raster_skips_provider_request() {
    let renderer = crate::test_support::shared_renderer();
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
    let renderer = crate::test_support::shared_renderer();
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
fn streamed_ineffective_clipping_run_matches_direct_base() {
    struct Case {
        name: &'static str,
        base_key: GpuRasterResourceKey,
        clipped_key: GpuRasterResourceKey,
        base_rgba: [u8; 4],
        clipped_rgba: [u8; 4],
        base: GpuNormalRasterSource,
        background: Option<GpuNormalStackSource>,
        /// Hand-derived pixels for the clipping-run render, when the case has them.
        expected: Option<Vec<u8>>,
    }

    let renderer = crate::test_support::shared_renderer();

    let mut zero_opacity_expected = [255, 255, 255, 0].repeat(16);
    for y in 1..=2 {
        for x in 1..=2 {
            let offset = ((y * 4 + x) * 4) as usize;
            zero_opacity_expected[offset..offset + 4].copy_from_slice(&[255, 0, 0, 255]);
        }
    }

    let cases = [
        Case {
            name: "zero-opacity clipped sibling over opaque normal base",
            base_key: raster_key(13),
            clipped_key: raster_key(14),
            base_rgba: [255, 0, 0, 255],
            clipped_rgba: [0, 255, 0, 255],
            base: raster_source(raster_key(13)),
            background: None,
            expected: Some(zero_opacity_expected),
        },
        Case {
            name: "zero-opacity clipped sibling over half-opacity multiply base",
            base_key: raster_key(15),
            clipped_key: raster_key(16),
            base_rgba: [240, 80, 120, 255],
            clipped_rgba: [0, 0, 255, 255],
            base: GpuNormalRasterSource {
                blend_mode: GpuRasterBlendMode::Multiply,
                ..raster_source_at_with_opacity(raster_key(15), 1, 1, 0.5)
            },
            background: Some(GpuNormalStackSource::SolidColor {
                color: Rgba8 {
                    r: 120,
                    g: 180,
                    b: 80,
                    a: 255,
                },
                opacity: 1.0,
            }),
            expected: None,
        },
    ];

    for case in cases {
        let clipped = raster_source_at_with_opacity(case.clipped_key, 1, 1, 0.0);
        let mut direct_sources: Vec<GpuNormalStackSource> =
            case.background.iter().cloned().collect();
        direct_sources.push(GpuNormalStackSource::Raster(case.base));
        let mut clipping_sources: Vec<GpuNormalStackSource> =
            case.background.iter().cloned().collect();
        clipping_sources.push(GpuNormalStackSource::ClippingRun {
            base: case.base,
            clipped: vec![GpuClippedStackSource::Raster(clipped)],
        });
        let mut direct_provider = clipping_run_provider(
            case.base_key,
            case.clipped_key,
            case.base_rgba,
            case.clipped_rgba,
        );
        let mut clipping_provider = clipping_run_provider(
            case.base_key,
            case.clipped_key,
            case.base_rgba,
            case.clipped_rgba,
        );

        let direct = renderer
            .draw_normal_stack_with_provider_to_rgba8(
                CanvasSize::new(4, 4),
                &direct_sources,
                &mut direct_provider,
            )
            .unwrap_or_else(|error| panic!("{}: draw direct base: {error:?}", case.name));
        let clipping = renderer
            .draw_normal_stack_with_provider_to_rgba8(
                CanvasSize::new(4, 4),
                &clipping_sources,
                &mut clipping_provider,
            )
            .unwrap_or_else(|error| {
                panic!(
                    "{}: draw clipping run with ineffective sibling: {error:?}",
                    case.name
                )
            });

        assert_eq!(clipping.pixels, direct.pixels, "{}", case.name);
        if let Some(expected) = &case.expected {
            assert_eq!(&clipping.pixels, expected, "{}", case.name);
        }
        assert_eq!(
            clipping_provider.raster_request_count(case.base_key),
            1,
            "{}",
            case.name
        );
        assert_eq!(
            clipping_provider.raster_request_count(case.clipped_key),
            0,
            "{}",
            case.name
        );
    }
}

fn clipping_run_provider(
    base_key: GpuRasterResourceKey,
    clipped_key: GpuRasterResourceKey,
    base_rgba: [u8; 4],
    clipped_rgba: [u8; 4],
) -> InlineProvider {
    InlineProvider::new(vec![
        (
            base_key,
            InlineRaster {
                render_node_id: RenderNodeId(base_key.render_mipmap_id),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: base_rgba.repeat(4),
            },
        ),
        (
            clipped_key,
            InlineRaster {
                render_node_id: RenderNodeId(clipped_key.render_mipmap_id),
                size: CanvasSize::new(2, 2),
                offset: (1, 1),
                pixels: clipped_rgba.repeat(4),
            },
        ),
    ])
}
