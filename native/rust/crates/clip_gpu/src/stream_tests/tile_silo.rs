use std::collections::HashMap;

use clip_graph::RenderNodeId;
use clip_model::CanvasSize;

use super::common::*;
use crate::stream_tile_silo::raster_silo_run_len;
use crate::{
    GpuClippedStackSource, GpuMaskAtlasSource, GpuMaskAtlasTileChunk, GpuMaskResourceCache,
    GpuMaskResourceKey, GpuNormalRasterSource, GpuNormalStackResourceProvider,
    GpuNormalStackSource, GpuRasterAtlasPixels, GpuRasterAtlasSource, GpuRasterAtlasTileChunk,
    GpuRasterAtlasTilePixels, GpuRasterBlendMode, GpuRasterResourceCache, GpuRasterResourceInfo,
    GpuRasterResourceKey, GpuRenderError, GpuRenderer,
};

#[test]
fn streamed_tile_silo_collapses_opaque_normal_raster_run() {
    let renderer = crate::test_support::shared_renderer();
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
    let renderer = crate::test_support::shared_renderer();
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
fn streamed_tile_silo_applies_leading_filter_before_raster_like_legacy_pass() {
    let renderer = crate::test_support::shared_renderer();
    let child_key = raster_key(149);
    let sources = vec![
        GpuNormalStackSource::LutFilter {
            lut_rgba: inverted_tone_curve_lut(),
            opacity: 1.0,
            mask_key: None,
            filter_mode: lut_mode(),
        },
        GpuNormalStackSource::Raster(raster_source_at(child_key, 1, 1)),
    ];

    let mut reference_provider = InlineProvider::new(vec![(
        child_key,
        InlineRaster {
            render_node_id: RenderNodeId(149),
            size: CanvasSize::new(1, 1),
            offset: (1, 1),
            pixels: vec![64, 128, 192, 255],
        },
    )]);
    let reference = renderer
        .draw_normal_stack_with_provider_to_rgba8(
            CanvasSize::new(3, 3),
            &sources,
            &mut reference_provider,
        )
        .expect("draw legacy leading filter reference");

    let mut provider = AtlasInlineProvider::new(vec![(
        child_key,
        AtlasInlineRaster {
            render_node_id: RenderNodeId(149),
            size: CanvasSize::new(1, 1),
            offset: (1, 1),
            pixels: vec![64, 128, 192, 255],
        },
    )]);
    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(3, 3), &sources, &mut provider)
        .expect("draw provider-backed leading filter tile event");

    assert_eq!(output.pixels, reference.pixels);
    assert_eq!(provider.atlas_requests, 1);
    assert_eq!(provider.raster_requests, 0);
}

#[test]
fn streamed_tile_silo_applies_filter_only_run_after_legacy_source() {
    let renderer = crate::test_support::shared_renderer();
    let sources = vec![
        GpuNormalStackSource::SolidColor {
            color: clip_model::Rgba8 {
                r: 10,
                g: 20,
                b: 30,
                a: 255,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::LutFilter {
            lut_rgba: inverted_tone_curve_lut(),
            opacity: 1.0,
            mask_key: None,
            filter_mode: lut_mode(),
        },
    ];
    let mut provider = InlineProvider::new(Vec::new());

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(3, 3), &sources, &mut provider)
        .expect("draw filter-only tile event after legacy source");

    assert_eq!(output.pixels, [245, 235, 225, 255].repeat(9));
}

#[test]
fn streamed_tile_silo_applies_masked_filter_only_run_like_legacy_pass() {
    let renderer = crate::test_support::shared_renderer();
    let filter_mask_key = mask_key(168);
    let sources = vec![
        GpuNormalStackSource::SolidColor {
            color: clip_model::Rgba8 {
                r: 64,
                g: 128,
                b: 192,
                a: 255,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::LutFilter {
            lut_rgba: inverted_tone_curve_lut(),
            opacity: 1.0,
            mask_key: Some(filter_mask_key),
            filter_mode: lut_mode(),
        },
    ];

    let mut reference_provider = InlineProvider::new(Vec::new()).with_masks(vec![(
        filter_mask_key,
        InlineMask {
            render_node_id: RenderNodeId(268),
            size: CanvasSize::new(1, 1),
            origin: (1, 1),
            fill_value: 0,
            pixels: vec![128],
        },
    )]);
    let reference = renderer
        .draw_normal_stack_with_provider_to_rgba8(
            CanvasSize::new(3, 3),
            &sources,
            &mut reference_provider,
        )
        .expect("draw legacy masked filter-only reference");

    let mut provider = AtlasInlineProvider::new(Vec::new()).with_masks(vec![(
        filter_mask_key,
        AtlasInlineMask {
            size: CanvasSize::new(1, 1),
            origin: (1, 1),
            fill_value: 0,
            pixels: vec![128],
        },
    )]);
    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(3, 3), &sources, &mut provider)
        .expect("draw provider-backed masked filter-only tile event");

    assert_eq!(output.pixels, reference.pixels);
    assert_eq!(provider.atlas_requests, 0);
    assert_eq!(provider.raster_requests, 0);
}

#[test]
fn streamed_tile_silo_lowers_simple_container_as_isolated_scope() {
    let renderer = crate::test_support::shared_renderer();
    let multiply_key = raster_key(142);
    let mut multiply = raster_source_at(multiply_key, 1, 1);
    multiply.blend_mode = GpuRasterBlendMode::Multiply;
    let mut provider = AtlasInlineProvider::new(vec![(
        multiply_key,
        AtlasInlineRaster {
            render_node_id: RenderNodeId(142),
            size: CanvasSize::new(1, 1),
            offset: (1, 1),
            pixels: vec![128, 0, 0, 255],
        },
    )]);
    let sources = [
        GpuNormalStackSource::SolidColor {
            color: clip_model::Rgba8 {
                r: 128,
                g: 128,
                b: 128,
                a: 255,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::Container {
            children: vec![GpuNormalStackSource::Raster(multiply)],
            opacity: 1.0,
            mask_key: None,
            blend_mode: GpuRasterBlendMode::Normal,
        },
    ];

    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(3, 3), &sources, &mut provider)
        .expect("draw provider-backed simple container scope");

    let mut expected = [128, 128, 128, 255].repeat(9);
    expected[((1 * 3 + 1) * 4) as usize..((1 * 3 + 1) * 4 + 4) as usize]
        .copy_from_slice(&[128, 0, 0, 255]);
    assert_eq!(output.pixels, expected);
    assert_eq!(provider.atlas_requests, 1);
    assert_eq!(provider.raster_requests, 0);
}

// Legacy-parity tables.
//
// Every case below renders the same `sources` twice on a 3x3 canvas: once through
// `InlineProvider` (the legacy per-raster upload path, used as the reference) and
// once through `AtlasInlineProvider` (the tile path). The tile path must produce
// identical pixels from exactly one atlas request and no per-raster request.

/// A 1x1 raster placed at canvas offset (1, 1), keyed by `raster_key(id)`.
struct ScenePixel {
    id: u32,
    rgba: [u8; 4],
}

/// A 1x1 mask placed at canvas origin (1, 1), keyed by `mask_key(id)`.
struct SceneMask {
    id: u32,
    fill_value: u8,
    value: u8,
}

struct ParityCase {
    name: &'static str,
    sources: Vec<GpuNormalStackSource>,
    rasters: Vec<ScenePixel>,
    masks: Vec<SceneMask>,
    /// Extra hand-derived check on the (1, 1) pixel of the tile-path output.
    expected_center: Option<[u8; 4]>,
}

fn px(id: u32, rgba: [u8; 4]) -> ScenePixel {
    ScenePixel { id, rgba }
}

fn mask_px(id: u32, fill_value: u8, value: u8) -> SceneMask {
    SceneMask {
        id,
        fill_value,
        value,
    }
}

fn solid(r: u8, g: u8, b: u8) -> GpuNormalStackSource {
    GpuNormalStackSource::SolidColor {
        color: clip_model::Rgba8 { r, g, b, a: 255 },
        opacity: 1.0,
    }
}

fn raster(id: u32) -> GpuNormalStackSource {
    GpuNormalStackSource::Raster(raster_source_at(raster_key(id), 1, 1))
}

fn blended_raster(id: u32, opacity: f32, blend_mode: GpuRasterBlendMode) -> GpuNormalRasterSource {
    GpuNormalRasterSource {
        opacity,
        blend_mode,
        ..raster_source_at(raster_key(id), 1, 1)
    }
}

fn container(
    children: Vec<GpuNormalStackSource>,
    opacity: f32,
    blend_mode: GpuRasterBlendMode,
) -> GpuNormalStackSource {
    GpuNormalStackSource::Container {
        children,
        opacity,
        mask_key: None,
        blend_mode,
    }
}

fn masked_container(
    children: Vec<GpuNormalStackSource>,
    opacity: f32,
    mask_id: u32,
) -> GpuNormalStackSource {
    GpuNormalStackSource::Container {
        children,
        opacity,
        mask_key: Some(mask_key(mask_id)),
        blend_mode: GpuRasterBlendMode::Normal,
    }
}

fn through(children: Vec<GpuNormalStackSource>, opacity: f32) -> GpuNormalStackSource {
    GpuNormalStackSource::ThroughGroup {
        children,
        opacity,
        mask_key: None,
    }
}

fn masked_through(
    children: Vec<GpuNormalStackSource>,
    opacity: f32,
    mask_id: u32,
) -> GpuNormalStackSource {
    GpuNormalStackSource::ThroughGroup {
        children,
        opacity,
        mask_key: Some(mask_key(mask_id)),
    }
}

fn clipping_run(
    base: GpuNormalRasterSource,
    clipped: Vec<GpuClippedStackSource>,
) -> GpuNormalStackSource {
    GpuNormalStackSource::ClippingRun { base, clipped }
}

fn clipped_container(
    layer_id: u32,
    children: Vec<GpuNormalStackSource>,
    opacity: f32,
    blend_mode: GpuRasterBlendMode,
) -> GpuClippedStackSource {
    GpuClippedStackSource::Container {
        layer_id: clip_model::LayerId(layer_id),
        children,
        opacity,
        mask_key: None,
        blend_mode,
    }
}

fn lut_filter(opacity: f32, mask_key: Option<GpuMaskResourceKey>) -> GpuNormalStackSource {
    GpuNormalStackSource::LutFilter {
        lut_rgba: inverted_tone_curve_lut(),
        opacity,
        mask_key,
        filter_mode: lut_mode(),
    }
}

fn legacy_provider(case: &ParityCase) -> InlineProvider {
    InlineProvider::new(
        case.rasters
            .iter()
            .map(|raster| {
                (
                    raster_key(raster.id),
                    InlineRaster {
                        render_node_id: RenderNodeId(raster.id),
                        size: CanvasSize::new(1, 1),
                        offset: (1, 1),
                        pixels: raster.rgba.to_vec(),
                    },
                )
            })
            .collect(),
    )
    .with_masks(
        case.masks
            .iter()
            .map(|mask| {
                (
                    mask_key(mask.id),
                    InlineMask {
                        render_node_id: RenderNodeId(mask.id + 100),
                        size: CanvasSize::new(1, 1),
                        origin: (1, 1),
                        fill_value: mask.fill_value,
                        pixels: vec![mask.value],
                    },
                )
            })
            .collect(),
    )
}

fn atlas_provider(case: &ParityCase) -> AtlasInlineProvider {
    AtlasInlineProvider::new(
        case.rasters
            .iter()
            .map(|raster| {
                (
                    raster_key(raster.id),
                    AtlasInlineRaster {
                        render_node_id: RenderNodeId(raster.id),
                        size: CanvasSize::new(1, 1),
                        offset: (1, 1),
                        pixels: raster.rgba.to_vec(),
                    },
                )
            })
            .collect(),
    )
    .with_masks(
        case.masks
            .iter()
            .map(|mask| {
                (
                    mask_key(mask.id),
                    AtlasInlineMask {
                        size: CanvasSize::new(1, 1),
                        origin: (1, 1),
                        fill_value: mask.fill_value,
                        pixels: vec![mask.value],
                    },
                )
            })
            .collect(),
    )
}

fn assert_tile_path_matches_legacy(renderer: &GpuRenderer, case: &ParityCase) {
    let canvas = CanvasSize::new(3, 3);

    // `InlineProvider` has no atlas request counter: it never overrides the
    // atlas hooks of `GpuNormalStackResourceProvider`, so the renderer cannot
    // take the tile path for it and the reference is guaranteed to be the
    // legacy per-raster pass.
    let mut reference_provider = legacy_provider(case);
    let reference = renderer
        .draw_normal_stack_with_provider_to_rgba8(canvas, &case.sources, &mut reference_provider)
        .unwrap_or_else(|error| panic!("{}: legacy reference render failed: {error:?}", case.name));

    let mut provider = atlas_provider(case);
    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(canvas, &case.sources, &mut provider)
        .unwrap_or_else(|error| panic!("{}: tile path render failed: {error:?}", case.name));

    assert_eq!(output.pixels, reference.pixels, "{}", case.name);
    if let Some(center) = case.expected_center {
        assert_eq!(&output.pixels[16..20], &center, "{}", case.name);
    }
    assert_eq!(provider.atlas_requests, 1, "{}", case.name);
    assert_eq!(provider.raster_requests, 0, "{}", case.name);
}

/// Solid backdrop plus one container holding a single raster child.
fn container_over_solid(
    name: &'static str,
    backdrop: (u8, u8, u8),
    child: ScenePixel,
    opacity: f32,
    blend_mode: GpuRasterBlendMode,
) -> ParityCase {
    let sources = vec![
        solid(backdrop.0, backdrop.1, backdrop.2),
        container(vec![raster(child.id)], opacity, blend_mode),
    ];
    ParityCase {
        name,
        sources,
        rasters: vec![child],
        masks: Vec::new(),
        expected_center: None,
    }
}

/// Grey backdrop, an outer container with a plain raster followed by a
/// multiply raster wrapped in `depth` nested Normal/1.0 containers.
fn nested_container_case(
    name: &'static str,
    outer_id: u32,
    inner_id: u32,
    depth: usize,
) -> ParityCase {
    let mut inner =
        GpuNormalStackSource::Raster(blended_raster(inner_id, 1.0, GpuRasterBlendMode::Multiply));
    for _ in 0..depth {
        inner = container(vec![inner], 1.0, GpuRasterBlendMode::Normal);
    }
    ParityCase {
        name,
        sources: vec![
            solid(180, 180, 180),
            container(
                vec![raster(outer_id), inner],
                1.0,
                GpuRasterBlendMode::Normal,
            ),
        ],
        rasters: vec![
            px(outer_id, [0, 200, 0, 255]),
            px(inner_id, [128, 0, 0, 255]),
        ],
        masks: Vec::new(),
        expected_center: Some([128, 0, 0, 255]),
    }
}

fn container_scope_cases() -> Vec<ParityCase> {
    let normal = GpuRasterBlendMode::Normal;
    let multiply = GpuRasterBlendMode::Multiply;
    vec![
        // {Normal, Multiply} x {1.0, 0.5} container blend/opacity grid.
        container_over_solid(
            "normal container at full opacity",
            (20, 40, 80),
            px(143, [220, 100, 40, 255]),
            1.0,
            normal,
        ),
        container_over_solid(
            "normal container at half opacity",
            (20, 40, 80),
            px(143, [220, 100, 40, 255]),
            0.5,
            normal,
        ),
        container_over_solid(
            "multiply container at full opacity",
            (200, 100, 50),
            px(144, [128, 128, 128, 255]),
            1.0,
            multiply,
        ),
        container_over_solid(
            "multiply container at half opacity",
            (200, 100, 50),
            px(145, [128, 128, 128, 255]),
            0.5,
            multiply,
        ),
        ParityCase {
            name: "masked container",
            sources: vec![
                solid(20, 40, 80),
                masked_container(vec![raster(157)], 1.0, 157),
            ],
            rasters: vec![px(157, [220, 100, 40, 255])],
            masks: vec![mask_px(157, 0, 128)],
            expected_center: None,
        },
        ParityCase {
            name: "masked filter inside container",
            sources: vec![
                solid(20, 40, 80),
                container(
                    vec![raster(167), lut_filter(1.0, Some(mask_key(167)))],
                    1.0,
                    normal,
                ),
            ],
            rasters: vec![px(167, [64, 128, 192, 255])],
            masks: vec![mask_px(167, 0, 128)],
            expected_center: None,
        },
        ParityCase {
            name: "through inside container",
            sources: vec![
                solid(20, 40, 80),
                container(vec![through(vec![raster(169)], 0.5)], 1.0, normal),
            ],
            rasters: vec![px(169, [220, 100, 40, 255])],
            masks: Vec::new(),
            expected_center: None,
        },
        ParityCase {
            name: "through inside nested container",
            sources: vec![
                solid(20, 40, 80),
                container(
                    vec![container(
                        vec![through(vec![raster(170)], 0.5)],
                        1.0,
                        normal,
                    )],
                    1.0,
                    normal,
                ),
            ],
            rasters: vec![px(170, [220, 100, 40, 255])],
            masks: Vec::new(),
            expected_center: None,
        },
        ParityCase {
            name: "initial container with through child",
            sources: vec![container(
                vec![through(vec![raster(171)], 1.0), raster(172)],
                1.0,
                normal,
            )],
            rasters: vec![px(171, [79, 75, 87, 255]), px(172, [8, 10, 15, 255])],
            masks: Vec::new(),
            expected_center: None,
        },
        ParityCase {
            name: "masked clipping run inside container",
            sources: vec![
                solid(40, 80, 120),
                container(
                    vec![clipping_run(
                        GpuNormalRasterSource {
                            mask_key: Some(mask_key(171)),
                            ..blended_raster(171, 0.8, multiply)
                        },
                        vec![GpuClippedStackSource::Raster(GpuNormalRasterSource {
                            mask_key: Some(mask_key(172)),
                            ..blended_raster(172, 1.0, multiply)
                        })],
                    )],
                    0.75,
                    normal,
                ),
            ],
            rasters: vec![px(171, [200, 120, 80, 160]), px(172, [80, 200, 255, 255])],
            masks: vec![mask_px(171, 0, 180), mask_px(172, 0, 220)],
            expected_center: None,
        },
        nested_container_case("two-deep container", 150, 151, 1),
        nested_container_case("three-deep container", 152, 153, 2),
        nested_container_case("four-deep container", 177, 178, 3),
    ]
}

#[test]
fn streamed_tile_silo_container_scope_equals_legacy() {
    let renderer = crate::test_support::shared_renderer();
    for case in container_scope_cases() {
        assert_tile_path_matches_legacy(&renderer, &case);
    }
}

/// Backdrop, then a container holding one clipping run whose clipped sibling
/// is a container scope.
fn clipped_sibling_case(
    name: &'static str,
    outer_base: GpuNormalRasterSource,
    clipped: GpuClippedStackSource,
    outer_opacity: f32,
    rasters: Vec<ScenePixel>,
) -> ParityCase {
    ParityCase {
        name,
        sources: vec![
            solid(40, 80, 120),
            container(
                vec![clipping_run(outer_base, vec![clipped])],
                outer_opacity,
                GpuRasterBlendMode::Normal,
            ),
        ],
        rasters,
        masks: Vec::new(),
        expected_center: None,
    }
}

fn clipped_container_sibling_cases() -> Vec<ParityCase> {
    let normal = GpuRasterBlendMode::Normal;
    let multiply = GpuRasterBlendMode::Multiply;
    let screen = GpuRasterBlendMode::Screen;
    vec![
        clipped_sibling_case(
            "plain clipped container sibling",
            blended_raster(177, 0.8, multiply),
            clipped_container(
                178,
                vec![GpuNormalStackSource::Raster(blended_raster(
                    178, 1.0, screen,
                ))],
                0.6,
                multiply,
            ),
            0.75,
            vec![px(177, [200, 120, 80, 160]), px(178, [80, 200, 255, 255])],
        ),
        clipped_sibling_case(
            "clipped container with filter",
            blended_raster(188, 0.8, multiply),
            clipped_container(
                189,
                vec![
                    GpuNormalStackSource::Raster(blended_raster(189, 1.0, screen)),
                    lut_filter(0.5, None),
                ],
                0.6,
                multiply,
            ),
            0.75,
            vec![px(188, [200, 120, 80, 160]), px(189, [80, 200, 255, 255])],
        ),
        clipped_sibling_case(
            "clipped container with nested child container",
            blended_raster(190, 0.85, normal),
            clipped_container(
                191,
                vec![container(vec![raster(191)], 0.6, normal)],
                0.65,
                multiply,
            ),
            0.8,
            vec![px(190, [200, 120, 80, 180]), px(191, [80, 200, 255, 210])],
        ),
        clipped_sibling_case(
            "clipped container with child clipping run",
            blended_raster(192, 0.85, normal),
            clipped_container(
                192,
                vec![clipping_run(
                    blended_raster(193, 0.7, normal),
                    vec![GpuClippedStackSource::Raster(blended_raster(
                        194, 0.8, screen,
                    ))],
                )],
                0.65,
                multiply,
            ),
            0.8,
            vec![
                px(192, [200, 120, 80, 180]),
                px(193, [120, 180, 90, 210]),
                px(194, [80, 200, 255, 230]),
            ],
        ),
        clipped_sibling_case(
            "clipped container with through child",
            blended_raster(195, 0.85, normal),
            clipped_container(
                195,
                vec![through(
                    vec![GpuNormalStackSource::Raster(blended_raster(
                        196, 0.7, screen,
                    ))],
                    0.65,
                )],
                0.75,
                multiply,
            ),
            0.8,
            vec![px(195, [200, 120, 80, 180]), px(196, [80, 200, 255, 210])],
        ),
        clipped_sibling_case(
            "clipped container with nested through scope",
            blended_raster(204, 0.85, normal),
            clipped_container(
                204,
                vec![through(
                    vec![through(
                        vec![GpuNormalStackSource::Raster(blended_raster(
                            205, 0.7, screen,
                        ))],
                        0.6,
                    )],
                    0.65,
                )],
                0.75,
                multiply,
            ),
            0.8,
            vec![px(204, [200, 120, 80, 180]), px(205, [80, 200, 255, 210])],
        ),
        clipped_sibling_case(
            "clipped container with through child and filter",
            blended_raster(199, 0.85, normal),
            clipped_container(
                199,
                vec![
                    through(
                        vec![GpuNormalStackSource::Raster(blended_raster(
                            200, 0.7, screen,
                        ))],
                        0.65,
                    ),
                    lut_filter(0.5, None),
                ],
                0.75,
                multiply,
            ),
            0.8,
            vec![px(199, [200, 120, 80, 180]), px(200, [80, 200, 255, 210])],
        ),
        clipped_sibling_case(
            "clipped container with through clipping run child",
            blended_raster(201, 0.85, normal),
            clipped_container(
                201,
                vec![through(
                    vec![clipping_run(
                        blended_raster(202, 0.8, multiply),
                        vec![GpuClippedStackSource::Raster(blended_raster(
                            203, 0.7, screen,
                        ))],
                    )],
                    0.65,
                )],
                0.75,
                multiply,
            ),
            0.8,
            vec![
                px(201, [200, 120, 80, 180]),
                px(202, [100, 160, 200, 190]),
                px(203, [80, 200, 255, 210]),
            ],
        ),
        clipped_sibling_case(
            "clipped container with nested through child",
            blended_raster(197, 0.85, normal),
            clipped_container(
                197,
                vec![container(
                    vec![through(
                        vec![GpuNormalStackSource::Raster(blended_raster(
                            198, 0.7, screen,
                        ))],
                        0.65,
                    )],
                    0.7,
                    normal,
                )],
                0.75,
                multiply,
            ),
            0.8,
            vec![px(197, [200, 120, 80, 180]), px(198, [80, 200, 255, 210])],
        ),
    ]
}

#[test]
fn streamed_tile_silo_clipped_container_sibling_equals_legacy() {
    let renderer = crate::test_support::shared_renderer();
    for case in clipped_container_sibling_cases() {
        assert_tile_path_matches_legacy(&renderer, &case);
    }
}

fn through_scope_case(
    name: &'static str,
    backdrop: (u8, u8, u8),
    scope: GpuNormalStackSource,
    rasters: Vec<ScenePixel>,
    masks: Vec<SceneMask>,
) -> ParityCase {
    ParityCase {
        name,
        sources: vec![solid(backdrop.0, backdrop.1, backdrop.2), scope],
        rasters,
        masks,
        expected_center: None,
    }
}

fn through_scope_cases() -> Vec<ParityCase> {
    let normal = GpuRasterBlendMode::Normal;
    let multiply = GpuRasterBlendMode::Multiply;
    let screen = GpuRasterBlendMode::Screen;
    let clip_pixels = |base_id: u32, clipped_id: u32| {
        vec![
            px(base_id, [200, 120, 80, 160]),
            px(clipped_id, [80, 200, 255, 220]),
        ]
    };
    let clip_run = |base_id: u32, clipped_id: u32| {
        clipping_run(
            blended_raster(base_id, 0.8, multiply),
            vec![GpuClippedStackSource::Raster(blended_raster(
                clipped_id, 1.0, screen,
            ))],
        )
    };
    vec![
        through_scope_case(
            "direct clipping run inside through",
            (40, 80, 120),
            through(vec![clip_run(173, 174)], 0.5),
            clip_pixels(173, 174),
            Vec::new(),
        ),
        through_scope_case(
            "nested through clipping run",
            (40, 80, 120),
            through(vec![through(vec![clip_run(175, 176)], 0.6)], 0.5),
            clip_pixels(175, 176),
            Vec::new(),
        ),
        through_scope_case(
            "through container clipping run",
            (40, 80, 120),
            through(vec![container(vec![clip_run(177, 178)], 1.0, normal)], 0.5),
            clip_pixels(177, 178),
            Vec::new(),
        ),
        through_scope_case(
            "through scope opacity",
            (0, 0, 255),
            through(vec![raster(146)], 0.5),
            vec![px(146, [255, 0, 0, 255])],
            Vec::new(),
        ),
        through_scope_case(
            "opaque masked through",
            (0, 0, 255),
            masked_through(vec![raster(156)], 0.5, 156),
            vec![px(156, [255, 0, 0, 255])],
            vec![mask_px(156, 255, 255)],
        ),
        through_scope_case(
            "masked through",
            (0, 0, 255),
            masked_through(vec![raster(158)], 0.75, 158),
            vec![px(158, [255, 0, 0, 255])],
            vec![mask_px(158, 0, 128)],
        ),
        through_scope_case(
            "child blend inside through",
            (200, 100, 50),
            through(
                vec![GpuNormalStackSource::Raster(blended_raster(
                    147, 1.0, multiply,
                ))],
                1.0,
            ),
            vec![px(147, [128, 128, 128, 255])],
            Vec::new(),
        ),
        through_scope_case(
            "fractional nested through",
            (0, 0, 255),
            through(vec![raster(154), through(vec![raster(155)], 0.5)], 0.75),
            vec![px(154, [255, 0, 0, 255]), px(155, [0, 255, 0, 255])],
            Vec::new(),
        ),
        through_scope_case(
            "container inside through",
            (200, 100, 50),
            through(
                vec![container(
                    vec![GpuNormalStackSource::Raster(blended_raster(
                        148, 1.0, multiply,
                    ))],
                    1.0,
                    normal,
                )],
                1.0,
            ),
            vec![px(148, [128, 0, 0, 255])],
            Vec::new(),
        ),
    ]
}

#[test]
fn streamed_tile_silo_through_scope_equals_legacy() {
    let renderer = crate::test_support::shared_renderer();
    for case in through_scope_cases() {
        assert_tile_path_matches_legacy(&renderer, &case);
    }
}

#[test]
fn streamed_tile_silo_accepts_provider_backed_masked_normal_atlas_pixels() {
    let renderer = crate::test_support::shared_renderer();
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
    let renderer = crate::test_support::shared_renderer();
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
    let renderer = crate::test_support::shared_renderer();
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
fn streamed_tile_silo_applies_byte_domain_special_blends() {
    let renderer = crate::test_support::shared_renderer();
    let cases = [
        (
            GpuRasterBlendMode::AddGlow,
            [0, 0, 200, 255],
            [100, 50, 0, 128],
            [50, 25, 200, 255],
        ),
        (
            GpuRasterBlendMode::ColorDodge,
            [100, 50, 200, 255],
            [100, 200, 250, 128],
            [132, 141, 228, 255],
        ),
        (
            GpuRasterBlendMode::ColorBurn,
            [100, 50, 200, 255],
            [100, 200, 250, 128],
            [50, 25, 199, 255],
        ),
        (
            GpuRasterBlendMode::GlowDodge,
            [100, 50, 200, 255],
            [100, 200, 250, 128],
            [124, 82, 255, 255],
        ),
    ];

    for (index, (blend_mode, base_pixel, source_pixel, expected_pixel)) in
        cases.into_iter().enumerate()
    {
        let base_key = raster_key(500 + index as u32 * 2);
        let blend_key = raster_key(501 + index as u32 * 2);
        let mut blend_source = raster_source_at(blend_key, 1, 1);
        blend_source.blend_mode = blend_mode;
        let sources = [
            GpuNormalStackSource::Raster(raster_source_at(base_key, 1, 1)),
            GpuNormalStackSource::Raster(blend_source),
        ];
        let mut provider = InlineProvider::new(vec![
            (
                base_key,
                InlineRaster {
                    render_node_id: RenderNodeId(base_key.render_mipmap_id),
                    size: CanvasSize::new(1, 1),
                    offset: (1, 1),
                    pixels: base_pixel.to_vec(),
                },
            ),
            (
                blend_key,
                InlineRaster {
                    render_node_id: RenderNodeId(blend_key.render_mipmap_id),
                    size: CanvasSize::new(1, 1),
                    offset: (1, 1),
                    pixels: source_pixel.to_vec(),
                },
            ),
        ]);

        assert_eq!(
            raster_silo_run_len(
                &provider,
                CanvasSize::new(3, 3),
                (0, 0),
                CanvasSize::new(3, 3),
                &sources,
            ),
            2
        );

        let output = renderer
            .draw_normal_stack_with_provider_to_rgba8(
                CanvasSize::new(3, 3),
                &sources,
                &mut provider,
            )
            .expect("draw tile-silo byte-domain special blend");

        let mut expected = [255, 255, 255, 0].repeat(9);
        expected[((1 * 3 + 1) * 4) as usize..((1 * 3 + 1) * 4 + 4) as usize]
            .copy_from_slice(&expected_pixel);
        assert_eq!(output.pixels, expected);
    }
}

#[test]
fn streamed_tile_silo_accepts_provider_backed_masked_multiply_atlas_tiles() {
    let renderer = crate::test_support::shared_renderer();
    let base_key = raster_key(340);
    let multiply_key = raster_key(341);
    let multiply_mask_key = mask_key(341);
    let mut multiply = raster_source_at_with_mask(multiply_key, multiply_mask_key, 1, 1);
    multiply.blend_mode = GpuRasterBlendMode::Multiply;
    let sources = [
        GpuNormalStackSource::Raster(raster_source_at(base_key, 1, 1)),
        GpuNormalStackSource::Raster(multiply),
    ];

    let mut reference_provider = InlineProvider::new(vec![
        (
            base_key,
            InlineRaster {
                render_node_id: RenderNodeId(340),
                size: CanvasSize::new(1, 1),
                offset: (1, 1),
                pixels: vec![200, 100, 50, 255],
            },
        ),
        (
            multiply_key,
            InlineRaster {
                render_node_id: RenderNodeId(341),
                size: CanvasSize::new(1, 1),
                offset: (1, 1),
                pixels: vec![128, 128, 128, 255],
            },
        ),
    ])
    .with_masks(vec![(
        multiply_mask_key,
        InlineMask {
            render_node_id: RenderNodeId(1341),
            size: CanvasSize::new(1, 1),
            origin: (1, 1),
            fill_value: 0,
            pixels: vec![128],
        },
    )]);
    let reference = renderer
        .draw_normal_stack_with_provider_to_rgba8(
            CanvasSize::new(3, 3),
            &sources,
            &mut reference_provider,
        )
        .expect("draw masked multiply reference");

    let mut provider = AtlasInlineProvider::new(vec![
        (
            base_key,
            AtlasInlineRaster {
                render_node_id: RenderNodeId(340),
                size: CanvasSize::new(1, 1),
                offset: (1, 1),
                pixels: vec![200, 100, 50, 255],
            },
        ),
        (
            multiply_key,
            AtlasInlineRaster {
                render_node_id: RenderNodeId(341),
                size: CanvasSize::new(1, 1),
                offset: (1, 1),
                pixels: vec![128, 128, 128, 255],
            },
        ),
    ])
    .with_masks(vec![(
        multiply_mask_key,
        AtlasInlineMask {
            size: CanvasSize::new(1, 1),
            origin: (1, 1),
            fill_value: 0,
            pixels: vec![128],
        },
    )]);
    assert_eq!(
        raster_silo_run_len(
            &provider,
            CanvasSize::new(3, 3),
            (0, 0),
            CanvasSize::new(3, 3),
            &sources,
        ),
        2
    );
    let output = renderer
        .draw_normal_stack_with_provider_to_rgba8(CanvasSize::new(3, 3), &sources, &mut provider)
        .expect("draw provider-backed masked multiply atlas tile-silo run");

    assert_eq!(output.pixels, reference.pixels);
    assert_eq!(provider.atlas_requests, 1);
    assert_eq!(provider.raster_requests, 0);
}

#[test]
fn tile_silo_planner_accepts_special_blends_but_stops_at_unsupported_masks() {
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
        2
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

    fn raster_run_atlas_supports_masks(&self) -> bool {
        !self.masks.is_empty()
    }

    fn mask_is_fully_opaque(&self, key: GpuMaskResourceKey) -> Option<bool> {
        let mask = self.masks.get(&key)?;
        Some(mask.fill_value == 255 && mask.pixels.iter().all(|value| *value == 255))
    }

    fn mask_atlas_tiles_supported(&self) -> bool {
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

    fn raster_run_atlas_tile_pixels(
        &mut self,
        sources: &[GpuRasterAtlasSource],
        atlas_size: CanvasSize,
    ) -> Result<Option<GpuRasterAtlasTilePixels>, Self::Error> {
        if !sources
            .iter()
            .any(|source| source.source.mask_key.is_some())
        {
            return Ok(None);
        }
        self.atlas_requests += 1;
        let mut chunks = Vec::with_capacity(sources.len());
        let mut mask_chunks = Vec::new();
        let mut resources = Vec::with_capacity(sources.len());
        for request in sources {
            let raster = self.rasters.get(&request.source.key).ok_or(
                GpuRenderError::MissingRasterResource {
                    layer_id: request.source.key.layer_id,
                    render_mipmap_id: request.source.key.render_mipmap_id,
                },
            )?;
            if let Some(mask_key) = request.source.mask_key {
                let mask =
                    self.masks
                        .get(&mask_key)
                        .ok_or(GpuRenderError::MissingMaskResource {
                            layer_id: mask_key.layer_id,
                            mask_mipmap_id: mask_key.mask_mipmap_id,
                        })?;
                let mut mask_pixels =
                    Vec::with_capacity((raster.size.width * raster.size.height) as usize);
                for y in 0..raster.size.height {
                    for x in 0..raster.size.width {
                        mask_pixels.push(atlas_mask_value(
                            mask,
                            request.offset_x + x as i32,
                            request.offset_y + y as i32,
                        ));
                    }
                }
                mask_chunks.push(GpuMaskAtlasTileChunk {
                    atlas_x: request.atlas_x,
                    atlas_y: request.atlas_y,
                    size: raster.size,
                    pixels: mask_pixels,
                });
            }
            chunks.push(GpuRasterAtlasTileChunk {
                source: request.source,
                atlas_x: request.atlas_x,
                atlas_y: request.atlas_y,
                mask_atlas_x: request.source.mask_key.map(|_| request.atlas_x),
                mask_atlas_y: request.source.mask_key.map(|_| request.atlas_y),
                size: raster.size,
                offset_x: request.offset_x,
                offset_y: request.offset_y,
                pixels: raster.pixels.clone(),
            });
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
            mask_chunks,
            resources,
        }))
    }

    fn mask_atlas_tile_pixels(
        &mut self,
        sources: &[GpuMaskAtlasSource],
        atlas_size: CanvasSize,
    ) -> Result<Option<Vec<GpuMaskAtlasTileChunk>>, Self::Error> {
        let mut chunks = Vec::with_capacity(sources.len());
        for request in sources {
            let right = request
                .atlas_x
                .checked_add(request.canvas_bounds.width)
                .ok_or(GpuRenderError::TextureSizeOverflow)?;
            let bottom = request
                .atlas_y
                .checked_add(request.canvas_bounds.height)
                .ok_or(GpuRenderError::TextureSizeOverflow)?;
            if right > atlas_size.width || bottom > atlas_size.height {
                return Ok(None);
            }
            let mask = self
                .masks
                .get(&request.key)
                .ok_or(GpuRenderError::MissingMaskResource {
                    layer_id: request.key.layer_id,
                    mask_mipmap_id: request.key.mask_mipmap_id,
                })?;
            let mut pixels = Vec::with_capacity(
                (request.canvas_bounds.width * request.canvas_bounds.height) as usize,
            );
            for y in 0..request.canvas_bounds.height {
                for x in 0..request.canvas_bounds.width {
                    let global_x = request
                        .canvas_bounds
                        .x
                        .checked_add(x)
                        .ok_or(GpuRenderError::TextureSizeOverflow)?;
                    let global_y = request
                        .canvas_bounds
                        .y
                        .checked_add(y)
                        .ok_or(GpuRenderError::TextureSizeOverflow)?;
                    pixels.push(atlas_mask_value(
                        mask,
                        i32::try_from(global_x).map_err(|_| GpuRenderError::TextureSizeOverflow)?,
                        i32::try_from(global_y).map_err(|_| GpuRenderError::TextureSizeOverflow)?,
                    ));
                }
            }
            chunks.push(GpuMaskAtlasTileChunk {
                atlas_x: request.atlas_x,
                atlas_y: request.atlas_y,
                size: CanvasSize::new(request.canvas_bounds.width, request.canvas_bounds.height),
                pixels,
            });
        }
        Ok(Some(chunks))
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
