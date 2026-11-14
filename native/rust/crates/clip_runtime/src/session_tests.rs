use std::collections::HashMap;
use std::path::{Path, PathBuf};

use clip_file::ClipFileSummary;
use clip_graph::{RenderNode, RenderNodeId, RenderNodeKind, RenderPlan};
use clip_model::{CanvasSize, LayerId, LayerKind, LayerOpacity, LayerVisibility, Rect, Rgba8};

use super::{
    ClipSession, SimpleRasterStackUnsupportedReason, StrictRasterStackDraw,
    StrictRasterStackOptions,
};

#[test]
fn plans_test_clipping_visible_layer_order() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
    let session = ClipSession::open(path).expect("open Test_Clipping.clip");
    let plan = session.render_plan();

    let nodes: Vec<_> = plan
        .nodes
        .iter()
        .map(|node| (node.layer_id, node.kind, node.depth))
        .collect();
    assert_eq!(
        nodes,
        vec![
            (LayerId(2), RenderNodeKind::Container, 0),
            (LayerId(4), RenderNodeKind::Paper, 1),
            (LayerId(10), RenderNodeKind::Raster, 1),
            (LayerId(11), RenderNodeKind::Raster, 1),
        ],
    );
}

#[test]
fn opens_session_from_memory_bytes() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
    let bytes = std::fs::read(&path).expect("read Test_Clipping.clip bytes");
    let from_path = ClipSession::open(&path).expect("open Test_Clipping.clip from path");
    let from_memory = ClipSession::from_bytes(bytes).expect("open Test_Clipping.clip bytes");

    assert_eq!(from_memory.summary(), from_path.summary());
    let memory_nodes: Vec<_> = from_memory
        .render_plan()
        .nodes
        .iter()
        .map(|node| (node.id, node.layer_id, node.kind, node.depth))
        .collect();
    let path_nodes: Vec<_> = from_path
        .render_plan()
        .nodes
        .iter()
        .map(|node| (node.id, node.layer_id, node.kind, node.depth))
        .collect();
    assert_eq!(memory_nodes, path_nodes);
}

#[test]
fn pixel_buffer_utils_count_byte_diffs_and_check_every_alpha() {
    assert_eq!(super::byte_diff_count(&[1, 2, 3], &[1, 4]), 2);
    assert!(super::alpha_is_fully_opaque(&[1, 2, 3, 255, 4, 5, 6, 255]));
    assert!(!super::alpha_is_fully_opaque(&[1, 2, 3, 255, 4, 5, 6, 254]));
}

#[test]
fn strict_normal_selector_keeps_normal_folder_as_container_source() {
    let session = synthetic_session(vec![
        container_node(0, 2, 0, 0),
        container_node(1, 8, 1, 0),
        paper_node(2, 4, 2),
    ]);

    let selection = session
        .select_strict_normal_raster_stack(StrictRasterStackOptions {
            allow_alpha_compositing: true,
            allow_paper: true,
            allow_layer_opacity: true,
            allow_masks: true,
            allow_clipping_runs: true,
            allow_container_isolation: true,
            allow_through_groups: true,
            allow_add_blend: true,
            allow_add_glow_blend: true,
            allow_color_burn_blend: true,
            allow_color_dodge_blend: true,
            allow_extended_blends: true,
            allow_glow_dodge_blend: true,
            allow_hard_mix_blend: true,
            allow_hsl_blends: true,
            allow_simple_blends: true,
            allow_soft_light_blend: true,
            allow_lut_filters: true,
            allow_vivid_light_blend: true,
            allow_w3c_blends: true,
            allow_initial_terminal_container_elision: false,
        })
        .expect("select synthetic normal folder");

    assert!(selection.unsupported.is_empty());
    assert_eq!(selection.draws.len(), 1);
    let StrictRasterStackDraw::Container(container) = &selection.draws[0] else {
        panic!("normal folder was not represented as a container source");
    };
    assert_eq!(container.layer_id, LayerId(8));
    assert_eq!(container.draws.len(), 1);
    assert!(matches!(
        container.draws[0],
        StrictRasterStackDraw::Paper { .. }
    ));
}

#[test]
fn strict_normal_selector_keeps_through_folder_as_through_group_source() {
    let session = synthetic_session(vec![
        container_node(0, 2, 0, 0),
        container_node(1, 8, 1, super::LAYER_COMPOSITE_THROUGH),
        paper_node(2, 4, 2),
    ]);

    let selection = session
        .select_strict_normal_raster_stack(StrictRasterStackOptions {
            allow_alpha_compositing: true,
            allow_paper: true,
            allow_layer_opacity: true,
            allow_masks: true,
            allow_clipping_runs: true,
            allow_container_isolation: true,
            allow_through_groups: true,
            allow_add_blend: true,
            allow_add_glow_blend: true,
            allow_color_burn_blend: true,
            allow_color_dodge_blend: true,
            allow_extended_blends: true,
            allow_glow_dodge_blend: true,
            allow_hard_mix_blend: true,
            allow_hsl_blends: true,
            allow_simple_blends: true,
            allow_soft_light_blend: true,
            allow_lut_filters: true,
            allow_vivid_light_blend: true,
            allow_w3c_blends: true,
            allow_initial_terminal_container_elision: false,
        })
        .expect("select synthetic through folder");

    assert!(selection.unsupported.is_empty());
    assert_eq!(selection.draws.len(), 1);
    let StrictRasterStackDraw::ThroughGroup(through_group) = &selection.draws[0] else {
        panic!("THROUGH folder was not represented as a through-group source");
    };
    assert_eq!(through_group.layer_id, LayerId(8));
    assert_eq!(through_group.draws.len(), 1);
    assert!(matches!(
        through_group.draws[0],
        StrictRasterStackDraw::Paper { .. }
    ));
}

#[test]
fn strict_normal_selector_clears_clip_base_after_through_group() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
    let session = ClipSession {
        path: path.to_path_buf(),
        container: clip_file::container::ClipContainer::open(&path)
            .expect("open Test_Clipping.clip container"),
        summary: ClipFileSummary {
            canvas: CanvasSize::new(512, 512),
            root_layer_id: LayerId(2),
            layer_count: 5,
            external_data_count: 7,
        },
        render_plan: RenderPlan {
            canvas: CanvasSize::new(512, 512),
            root_layer_id: LayerId(2),
            nodes: vec![
                container_node(0, 2, 0, 0),
                container_node(1, 8, 1, super::LAYER_COMPOSITE_THROUGH),
                paper_node(2, 4, 2),
                raster_node(3, 11, 1, 16, true),
            ],
        },
        raster_sources: HashMap::new(),
        mask_sources: HashMap::new(),
        filter_sources: HashMap::new(),
        text_sources: HashMap::new(),
        rendered_image: None,
    };

    let selection = session
        .select_strict_normal_raster_stack(StrictRasterStackOptions {
            allow_alpha_compositing: true,
            allow_paper: true,
            allow_layer_opacity: true,
            allow_masks: true,
            allow_clipping_runs: true,
            allow_container_isolation: true,
            allow_through_groups: true,
            allow_add_blend: true,
            allow_add_glow_blend: true,
            allow_color_burn_blend: true,
            allow_color_dodge_blend: true,
            allow_extended_blends: true,
            allow_glow_dodge_blend: true,
            allow_hard_mix_blend: true,
            allow_hsl_blends: true,
            allow_simple_blends: true,
            allow_soft_light_blend: true,
            allow_lut_filters: true,
            allow_vivid_light_blend: true,
            allow_w3c_blends: true,
            allow_initial_terminal_container_elision: false,
        })
        .expect("select synthetic through-cleared clipped raster");

    assert!(selection.unsupported.is_empty());
    assert_eq!(selection.draws.len(), 2);
    assert!(matches!(
        selection.draws[0],
        StrictRasterStackDraw::ThroughGroup(_)
    ));
    assert!(matches!(
        selection.draws[1],
        StrictRasterStackDraw::Raster(_)
    ));
}

#[test]
fn gpu_selector_accepts_lut_and_luminosity_filters() {
    use clip_gpu::GpuLutFilterMode::{Hsl, ThresholdLum, ToneCurveRgb};

    let mut session = synthetic_session(vec![
        container_node(0, 2, 0, 0),
        filter_node(1, 10, 1),
        filter_node(2, 11, 1),
        filter_node(3, 12, 1),
        filter_node(4, 13, 1),
        filter_node(5, 14, 1),
        filter_node(6, 15, 1),
        filter_node(7, 16, 1),
    ]);
    session.filter_sources.insert(
        LayerId(10),
        filter_source(10, 1, brightness_contrast_payload(20, -10)),
    );
    session.filter_sources.insert(
        LayerId(11),
        filter_source(11, 2, level_payload([0, 20000, 65535, 0, 65535])),
    );
    session
        .filter_sources
        .insert(LayerId(12), filter_source(12, 6, Vec::new()));
    session.filter_sources.insert(
        LayerId(13),
        filter_source(13, 7, 4i32.to_be_bytes().to_vec()),
    );
    session.filter_sources.insert(
        LayerId(14),
        filter_source(14, 8, 128i32.to_be_bytes().to_vec()),
    );
    session.filter_sources.insert(
        LayerId(15),
        filter_source(
            15,
            5,
            color_balance_payload([1, 0, 0, 0, 43, -48, 48, 0, 0, 0]),
        ),
    );
    session
        .filter_sources
        .insert(LayerId(16), filter_source(16, 4, hsl_payload(30, -25, 40)));

    let selection = session
        .select_gpu_normal_render_stack(gpu_selector_options())
        .expect("select synthetic LUT filters");

    assert!(selection.unsupported.is_empty());
    assert_eq!(selection.sources.len(), 7);
    // LUT contents are covered by filter_lut_tests.rs; this only checks that
    // each filter type reaches the GPU as the expected LUT mode.
    let expected_modes = [
        ("brightness/contrast", ToneCurveRgb),
        ("level correction", ToneCurveRgb),
        ("invert", ToneCurveRgb),
        ("posterization", ToneCurveRgb),
        ("threshold", ThresholdLum),
        ("color balance", ToneCurveRgb),
        (
            "hue/saturation/luminosity",
            Hsl(clip_gpu::GpuHslFilterParams {
                hue_turns: 30.0 / 360.0,
                saturation_delta: -25.0 / 100.0,
                luminosity_delta: 40.0 / 100.0,
            }),
        ),
    ];
    for (source, (name, expected_mode)) in selection.sources.iter().zip(expected_modes) {
        let clip_gpu::GpuNormalStackSource::LutFilter { filter_mode, .. } = source else {
            panic!("{name}: source was not represented as a LUT filter");
        };
        assert_eq!(*filter_mode, expected_mode, "{name}");
    }
}

#[test]
fn strict_raster_blend_mode_allows_every_supported_blend_by_position() {
    use super::StrictRasterBlendMode::*;

    let options = gpu_selector_options();
    let modes = [
        Normal,
        Add,
        AddGlow,
        ColorBurn,
        ColorDodge,
        Darken,
        DarkerColor,
        Difference,
        Divide,
        Exclusion,
        GlowDodge,
        HardMix,
        HardLight,
        Hue,
        Lighten,
        LighterColor,
        LinearBurn,
        LinearLight,
        Multiply,
        Overlay,
        PinLight,
        Saturation,
        Brightness,
        Color,
        Screen,
        SoftLight,
        Subtract,
        VividLight,
    ];

    for (index, mode) in modes.into_iter().enumerate() {
        let (composite, gate) = blend_composite_and_gate(mode);
        let id = u32::try_from(index).expect("small index") + 1;
        let unclipped = raster_node_with_composite(id, 100 + id, 1, 200 + id, false, composite);
        let clipped = raster_node_with_composite(50 + id, 150 + id, 1, 250 + id, true, composite);

        assert_eq!(
            super::strict_raster_blend_mode(&unclipped, options, false),
            Some(mode),
            "{mode:?}: unclipped raster outside a clipping run"
        );
        assert_eq!(
            super::strict_raster_blend_mode(&clipped, options, true),
            Some(mode),
            "{mode:?}: clipped raster inside a clipping run"
        );

        // Composite 0 (Normal) is never position-gated. Every other blend must
        // reject a clip flag outside a clipping run and a clipping-run position
        // without the clip flag.
        let mismatched = (mode == Normal).then_some(Normal);
        assert_eq!(
            super::strict_raster_blend_mode(&clipped, options, false),
            mismatched,
            "{mode:?}: clip flag outside a clipping run"
        );
        assert_eq!(
            super::strict_raster_blend_mode(&unclipped, options, true),
            mismatched,
            "{mode:?}: clipping-run position without clip flag"
        );

        if let Some(disable) = gate {
            let mut disabled = options;
            disable(&mut disabled);
            assert_eq!(
                super::strict_raster_blend_mode(&unclipped, disabled, false),
                None,
                "{mode:?}: gating option disabled"
            );
        }
    }
}

#[test]
fn normal_folder_with_real_test_clipping_children_matches_flat_stack() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
    let flat = ClipSession::open(&path)
        .expect("open Test_Clipping.clip")
        .draw_normal_raster_stack_via_gpu()
        .expect("draw flat Test_Clipping stack");
    assert!(flat.unsupported.is_empty());
    let flat_image = flat.image.expect("flat output image");

    let folder_session_container = clip_file::container::ClipContainer::open(&path)
        .expect("open Test_Clipping.clip container");
    let folder_raster_sources = clip_file::metadata::read_raster_layer_sources_from_sqlite(
        folder_session_container.sqlite_bytes(),
        &[LayerId(10), LayerId(11)],
        CanvasSize::new(512, 512),
    )
    .expect("read Test_Clipping raster sources");
    let folder_session = ClipSession {
        container: folder_session_container,
        path,
        summary: ClipFileSummary {
            canvas: CanvasSize::new(512, 512),
            root_layer_id: LayerId(2),
            layer_count: 5,
            external_data_count: 7,
        },
        render_plan: RenderPlan {
            canvas: CanvasSize::new(512, 512),
            root_layer_id: LayerId(2),
            nodes: vec![
                container_node(0, 2, 0, 0),
                container_node(1, 1000, 1, 0),
                paper_node(2, 4, 2),
                raster_node(3, 10, 2, 15, false),
                raster_node(4, 11, 2, 16, true),
            ],
        },
        raster_sources: folder_raster_sources,
        mask_sources: HashMap::new(),
        filter_sources: HashMap::new(),
        text_sources: HashMap::new(),
        rendered_image: None,
    };

    let folder = folder_session
        .draw_normal_raster_stack_via_gpu()
        .expect("draw synthetic folder Test_Clipping stack");
    assert!(folder.unsupported.is_empty());
    let folder_image = folder.image.expect("folder output image");

    assert_eq!(folder.source_count, 2);
    assert_eq!(folder.drawn_resources.len(), 2);
    assert_eq!(folder_image.pixels, flat_image.pixels);
}

#[test]
fn normal_gpu_result_reuses_support_resource_stats() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
    let session = ClipSession::open(&path).expect("open Test_Clipping.clip");

    let support = session
        .check_normal_raster_stack_support()
        .expect("check support");
    let render = session
        .draw_normal_raster_stack_via_gpu()
        .expect("draw normal stack");

    assert_eq!(render.source_count, support.source_count);
    assert_eq!(render.resource_stats, support.resource_stats);
    assert_eq!(render.unsupported, support.unsupported);
}

#[test]
fn text_sample_is_supported_and_draws_pixels() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Text_1.clip");
    let session = ClipSession::open(&path).expect("open Text_1.clip");

    let support = session
        .check_normal_raster_stack_support()
        .expect("check Text_1 support");
    assert!(support.unsupported.is_empty());

    let render = session
        .draw_normal_raster_stack_via_gpu()
        .expect("draw Text_1 stack");
    assert!(render.unsupported.is_empty());
    let image = render.image.expect("Text_1 output image");
    let background = &image.pixels[0..4];
    let dark_pixel_count = image
        .pixels
        .chunks_exact(4)
        .filter(|pixel| {
            pixel[3] == 255
                && pixel[0] < background[0].saturating_sub(32)
                && pixel[1] < background[1].saturating_sub(32)
                && pixel[2] < background[2].saturating_sub(32)
        })
        .count();
    assert!(dark_pixel_count > 500);
}

#[test]
fn gpu_selector_records_unsupported_layer_kind_without_blocking_sources() {
    let session = synthetic_session(vec![
        container_node(0, 2, 0, 0),
        paper_node(1, 4, 1),
        unsupported_node(2, 20, 1, 9001),
        paper_node(3, 5, 1),
    ]);

    let selection = session
        .select_gpu_normal_render_stack(gpu_selector_options())
        .expect("select synthetic stack with unsupported layer");

    assert_eq!(selection.sources.len(), 2);
    assert_eq!(selection.unsupported.len(), 1);
    assert_eq!(selection.unsupported[0].layer_id, LayerId(20));
    assert!(matches!(
        selection.unsupported[0].reason,
        SimpleRasterStackUnsupportedReason::UnsupportedLayerKind(9001)
    ));
}

#[test]
fn region_read_skips_unsupported_layer_kinds() {
    let mut session = synthetic_session(vec![
        container_node(0, 2, 0, 0),
        paper_node(1, 4, 1),
        unsupported_node(2, 20, 1, 9001),
        paper_node(3, 5, 1),
    ]);

    let support = session
        .check_normal_raster_stack_support()
        .expect("check synthetic support");
    assert_eq!(support.source_count, 2);
    assert_eq!(support.unsupported.len(), 1);

    let mut pixel = [0u8; 4];
    session
        .read_rgba8_region(Rect::new(0, 0, 1, 1), &mut pixel)
        .expect("read region while unsupported layer is skipped");

    assert_eq!(pixel, [226, 226, 226, 255]);
}

#[test]
fn gpu_selector_accepts_container_base_clipping_runs_in_aya_fixture() {
    let Some(path) = fixture_path_ending("Aya_Live2D.clip") else {
        eprintln!("skip: private fixture img/*Aya_Live2D.clip is not present");
        return;
    };
    let session = ClipSession::open(&path).expect("open Aya fixture");

    let support = session
        .check_normal_raster_stack_support()
        .expect("check Aya support");
    assert!(support.unsupported.is_empty());

    let selection = session
        .select_gpu_normal_render_stack(gpu_selector_options())
        .expect("select Aya render stack");

    assert!(selection.unsupported.is_empty());
    assert!(
        selection
            .sources
            .iter()
            .any(stack_contains_container_clipping_run)
    );
}

#[test]
fn gpu_selector_accepts_container_clipped_siblings_in_kabi_fixture() {
    let Some(path) = fixture_path_ending("Kabi_Live2D.clip") else {
        eprintln!("skip: private fixture img/*Kabi_Live2D.clip is not present");
        return;
    };
    let session = ClipSession::open(&path).expect("open Kabi fixture");

    let support = session
        .check_normal_raster_stack_support()
        .expect("check Kabi support");
    assert!(support.unsupported.is_empty());

    let selection = session
        .select_gpu_normal_render_stack(gpu_selector_options())
        .expect("select Kabi render stack");

    assert!(selection.unsupported.is_empty());
    assert!(
        selection
            .sources
            .iter()
            .any(stack_contains_clipped_container)
    );
}

#[test]
fn gpu_selector_resolves_off_canvas_mask_by_empty_fill() {
    struct Case {
        name: &'static str,
        empty_fill: u8,
        mask_resources: usize,
        opacity: f32,
        keeps_mask_key: bool,
    }
    let cases = [
        Case {
            name: "zero fill folds to zero opacity",
            empty_fill: 0,
            mask_resources: 0,
            opacity: 0.0,
            keeps_mask_key: false,
        },
        Case {
            name: "opaque fill elides the mask resource",
            empty_fill: 255,
            mask_resources: 0,
            opacity: 1.0,
            keeps_mask_key: false,
        },
        Case {
            name: "partial fill keeps the mask resource",
            empty_fill: 128,
            mask_resources: 1,
            opacity: 1.0,
            keeps_mask_key: true,
        },
    ];

    for case in cases {
        let mut session = synthetic_session(vec![
            container_node(0, 2, 0, 0),
            raster_node_with_mask(1, 10, 1, 15, 50, false),
        ]);
        session
            .raster_sources
            .insert(LayerId(10), raster_source(10, 15));
        session
            .mask_sources
            .insert(LayerId(10), off_canvas_mask_source(10, 50, case.empty_fill));

        let selection = session
            .select_gpu_normal_render_stack(gpu_selector_options())
            .unwrap_or_else(|error| panic!("{}: selector failed: {error:?}", case.name));

        assert!(
            selection.unsupported.is_empty(),
            "{}: unsupported entries",
            case.name
        );
        assert_eq!(
            selection.resource_plan.mask_resource_count(),
            case.mask_resources,
            "{}: mask resource count",
            case.name
        );
        assert_eq!(selection.sources.len(), 1, "{}: source count", case.name);
        let clip_gpu::GpuNormalStackSource::Raster(raster) = &selection.sources[0] else {
            panic!(
                "{}: masked raster was not represented as a raster source",
                case.name
            );
        };
        assert_eq!(raster.opacity, case.opacity, "{}: opacity", case.name);
        assert_eq!(
            raster.mask_key.is_some(),
            case.keeps_mask_key,
            "{}: mask key presence",
            case.name
        );
    }
}

fn synthetic_session(nodes: Vec<RenderNode>) -> ClipSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
    ClipSession {
        container: clip_file::container::ClipContainer::open(&path)
            .expect("open Test_Clipping.clip container"),
        path: PathBuf::new(),
        summary: ClipFileSummary {
            canvas: CanvasSize::new(8, 8),
            root_layer_id: LayerId(2),
            layer_count: nodes.len(),
            external_data_count: 0,
        },
        render_plan: RenderPlan {
            canvas: CanvasSize::new(8, 8),
            root_layer_id: LayerId(2),
            nodes,
        },
        raster_sources: HashMap::new(),
        mask_sources: HashMap::new(),
        filter_sources: HashMap::new(),
        text_sources: HashMap::new(),
        rendered_image: None,
    }
}

fn fixture_path_ending(suffix: &str) -> Option<PathBuf> {
    let img_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img");
    std::fs::read_dir(&img_dir)
        .expect("read fixture directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(suffix))
        })
}

fn stack_contains_container_clipping_run(source: &clip_gpu::GpuNormalStackSource) -> bool {
    match source {
        clip_gpu::GpuNormalStackSource::ContainerClippingRun { .. } => true,
        clip_gpu::GpuNormalStackSource::Container { children, .. }
        | clip_gpu::GpuNormalStackSource::ThroughGroup { children, .. } => {
            children.iter().any(stack_contains_container_clipping_run)
        }
        clip_gpu::GpuNormalStackSource::Raster(_)
        | clip_gpu::GpuNormalStackSource::ClippingRun { .. }
        | clip_gpu::GpuNormalStackSource::SolidColor { .. }
        | clip_gpu::GpuNormalStackSource::LutFilter { .. } => false,
    }
}

fn stack_contains_clipped_container(source: &clip_gpu::GpuNormalStackSource) -> bool {
    match source {
        clip_gpu::GpuNormalStackSource::ClippingRun { clipped, .. }
        | clip_gpu::GpuNormalStackSource::ContainerClippingRun { clipped, .. } => clipped
            .iter()
            .any(|source| matches!(source, clip_gpu::GpuClippedStackSource::Container { .. })),
        clip_gpu::GpuNormalStackSource::Container { children, .. }
        | clip_gpu::GpuNormalStackSource::ThroughGroup { children, .. } => {
            children.iter().any(stack_contains_clipped_container)
        }
        clip_gpu::GpuNormalStackSource::Raster(_)
        | clip_gpu::GpuNormalStackSource::SolidColor { .. }
        | clip_gpu::GpuNormalStackSource::LutFilter { .. } => false,
    }
}

fn container_node(id: u32, layer_id: u32, depth: u16, composite: u32) -> RenderNode {
    RenderNode {
        id: RenderNodeId(id),
        layer_id: LayerId(layer_id),
        layer_name: String::new(),
        kind: RenderNodeKind::Container,
        depth,
        clip: false,
        opacity: LayerOpacity::MAX,
        composite,
        render_mipmap_id: None,
        mask_mipmap_id: None,
        paper_color: None,
    }
}

fn paper_node(id: u32, layer_id: u32, depth: u16) -> RenderNode {
    RenderNode {
        id: RenderNodeId(id),
        layer_id: LayerId(layer_id),
        layer_name: String::new(),
        kind: RenderNodeKind::Paper,
        depth,
        clip: false,
        opacity: LayerOpacity::MAX,
        composite: 0,
        render_mipmap_id: None,
        mask_mipmap_id: None,
        paper_color: Some(Rgba8 {
            r: 226,
            g: 226,
            b: 226,
            a: 255,
        }),
    }
}

fn filter_node(id: u32, layer_id: u32, depth: u16) -> RenderNode {
    RenderNode {
        id: RenderNodeId(id),
        layer_id: LayerId(layer_id),
        layer_name: String::new(),
        kind: RenderNodeKind::Filter,
        depth,
        clip: false,
        opacity: LayerOpacity::MAX,
        composite: 0,
        render_mipmap_id: None,
        mask_mipmap_id: None,
        paper_color: None,
    }
}

fn unsupported_node(id: u32, layer_id: u32, depth: u16, raw_kind: u32) -> RenderNode {
    RenderNode {
        id: RenderNodeId(id),
        layer_id: LayerId(layer_id),
        layer_name: String::new(),
        kind: RenderNodeKind::Unsupported(raw_kind),
        depth,
        clip: false,
        opacity: LayerOpacity::MAX,
        composite: 0,
        render_mipmap_id: None,
        mask_mipmap_id: None,
        paper_color: None,
    }
}

fn raster_node(
    id: u32,
    layer_id: u32,
    depth: u16,
    render_mipmap_id: u32,
    clip: bool,
) -> RenderNode {
    raster_node_with_composite(id, layer_id, depth, render_mipmap_id, clip, 0)
}

fn raster_node_with_composite(
    id: u32,
    layer_id: u32,
    depth: u16,
    render_mipmap_id: u32,
    clip: bool,
    composite: u32,
) -> RenderNode {
    RenderNode {
        id: RenderNodeId(id),
        layer_id: LayerId(layer_id),
        layer_name: String::new(),
        kind: RenderNodeKind::Raster,
        depth,
        clip,
        opacity: LayerOpacity::MAX,
        composite,
        render_mipmap_id: Some(render_mipmap_id),
        mask_mipmap_id: None,
        paper_color: None,
    }
}

fn raster_node_with_mask(
    id: u32,
    layer_id: u32,
    depth: u16,
    render_mipmap_id: u32,
    mask_mipmap_id: u32,
    clip: bool,
) -> RenderNode {
    let mut node = raster_node(id, layer_id, depth, render_mipmap_id, clip);
    node.mask_mipmap_id = Some(mask_mipmap_id);
    node
}

fn raster_source(layer_id: u32, render_mipmap_id: u32) -> clip_file::metadata::RasterLayerSource {
    clip_file::metadata::RasterLayerSource {
        layer: clip_file::metadata::LayerRecord {
            id: LayerId(layer_id),
            kind: LayerKind::Raster,
            visibility: LayerVisibility(1),
        },
        render_mipmap_id,
        offscreen_id: 1000 + render_mipmap_id,
        external_id: format!("synthetic-raster-{layer_id}"),
        pixel_size: CanvasSize::new(2, 2),
        color_type: None,
        offset_x: 0,
        offset_y: 0,
    }
}

fn filter_source(
    layer_id: u32,
    filter_type: u32,
    payload: Vec<u8>,
) -> clip_file::metadata::FilterLayerSource {
    clip_file::metadata::FilterLayerSource {
        layer_id: LayerId(layer_id),
        filter_type,
        payload,
    }
}

fn brightness_contrast_payload(brightness: i32, contrast: i32) -> Vec<u8> {
    let mut payload = Vec::with_capacity(8);
    payload.extend_from_slice(&brightness.to_be_bytes());
    payload.extend_from_slice(&contrast.to_be_bytes());
    payload
}

fn level_payload(group: [u16; 5]) -> Vec<u8> {
    let mut payload = vec![0u8; 0x40];
    for (index, value) in group.iter().enumerate() {
        payload[index * 2..index * 2 + 2].copy_from_slice(&value.to_be_bytes());
    }
    payload
}

fn color_balance_payload(values: [i32; 10]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(40);
    for value in values {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    payload
}

fn hsl_payload(hue: i32, saturation: i32, luminosity: i32) -> Vec<u8> {
    let mut payload = Vec::with_capacity(12);
    for value in [hue, saturation, luminosity] {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    payload
}

fn off_canvas_mask_source(
    layer_id: u32,
    mask_mipmap_id: u32,
    empty_fill: u8,
) -> clip_file::metadata::MaskLayerSource {
    clip_file::metadata::MaskLayerSource {
        layer_id: LayerId(layer_id),
        mask_mipmap_id,
        offscreen_id: 2000 + mask_mipmap_id,
        external_id: format!("synthetic-mask-{layer_id}"),
        pixel_size: CanvasSize::new(2, 2),
        empty_fill,
        offset_x: 20,
        offset_y: 20,
    }
}

fn gpu_selector_options() -> StrictRasterStackOptions {
    StrictRasterStackOptions {
        allow_alpha_compositing: true,
        allow_paper: true,
        allow_layer_opacity: true,
        allow_masks: true,
        allow_clipping_runs: true,
        allow_container_isolation: true,
        allow_through_groups: true,
        allow_add_blend: true,
        allow_add_glow_blend: true,
        allow_color_burn_blend: true,
        allow_color_dodge_blend: true,
        allow_extended_blends: true,
        allow_glow_dodge_blend: true,
        allow_hard_mix_blend: true,
        allow_hsl_blends: true,
        allow_simple_blends: true,
        allow_soft_light_blend: true,
        allow_lut_filters: true,
        allow_vivid_light_blend: true,
        allow_w3c_blends: true,
        allow_initial_terminal_container_elision: false,
    }
}

type BlendGate = fn(&mut StrictRasterStackOptions);

/// Layer composite id and the option that gates each strict blend mode. The
/// exhaustive match forces this table to grow with the enum.
fn blend_composite_and_gate(mode: super::StrictRasterBlendMode) -> (u32, Option<BlendGate>) {
    use super::StrictRasterBlendMode::*;
    match mode {
        Normal => (0, None),
        Add => (
            super::LAYER_COMPOSITE_ADD,
            Some(|o| o.allow_add_blend = false),
        ),
        AddGlow => (
            super::LAYER_COMPOSITE_ADD_GLOW,
            Some(|o| o.allow_add_glow_blend = false),
        ),
        ColorBurn => (
            super::LAYER_COMPOSITE_COLOR_BURN,
            Some(|o| o.allow_color_burn_blend = false),
        ),
        ColorDodge => (
            super::LAYER_COMPOSITE_COLOR_DODGE,
            Some(|o| o.allow_color_dodge_blend = false),
        ),
        Darken => (
            super::LAYER_COMPOSITE_DARKEN,
            Some(|o| o.allow_simple_blends = false),
        ),
        DarkerColor => (
            super::LAYER_COMPOSITE_DARKER_COLOR,
            Some(|o| o.allow_extended_blends = false),
        ),
        Difference => (
            super::LAYER_COMPOSITE_DIFFERENCE,
            Some(|o| o.allow_simple_blends = false),
        ),
        Divide => (
            super::LAYER_COMPOSITE_DIVIDE,
            Some(|o| o.allow_extended_blends = false),
        ),
        Exclusion => (
            super::LAYER_COMPOSITE_EXCLUSION,
            Some(|o| o.allow_extended_blends = false),
        ),
        GlowDodge => (
            super::LAYER_COMPOSITE_GLOW_DODGE,
            Some(|o| o.allow_glow_dodge_blend = false),
        ),
        HardMix => (
            super::LAYER_COMPOSITE_HARD_MIX,
            Some(|o| o.allow_hard_mix_blend = false),
        ),
        HardLight => (
            super::LAYER_COMPOSITE_HARD_LIGHT,
            Some(|o| o.allow_w3c_blends = false),
        ),
        Hue => (
            super::LAYER_COMPOSITE_HUE,
            Some(|o| o.allow_hsl_blends = false),
        ),
        Lighten => (
            super::LAYER_COMPOSITE_LIGHTEN,
            Some(|o| o.allow_simple_blends = false),
        ),
        LighterColor => (
            super::LAYER_COMPOSITE_LIGHTER_COLOR,
            Some(|o| o.allow_extended_blends = false),
        ),
        LinearBurn => (
            super::LAYER_COMPOSITE_LINEAR_BURN,
            Some(|o| o.allow_extended_blends = false),
        ),
        LinearLight => (
            super::LAYER_COMPOSITE_LINEAR_LIGHT,
            Some(|o| o.allow_extended_blends = false),
        ),
        Multiply => (
            super::LAYER_COMPOSITE_MULTIPLY,
            Some(|o| o.allow_w3c_blends = false),
        ),
        Overlay => (
            super::LAYER_COMPOSITE_OVERLAY,
            Some(|o| o.allow_w3c_blends = false),
        ),
        PinLight => (
            super::LAYER_COMPOSITE_PIN_LIGHT,
            Some(|o| o.allow_extended_blends = false),
        ),
        Saturation => (
            super::LAYER_COMPOSITE_SATURATION,
            Some(|o| o.allow_hsl_blends = false),
        ),
        Brightness => (
            super::LAYER_COMPOSITE_BRIGHTNESS,
            Some(|o| o.allow_extended_blends = false),
        ),
        Color => (
            super::LAYER_COMPOSITE_COLOR,
            Some(|o| o.allow_hsl_blends = false),
        ),
        Screen => (
            super::LAYER_COMPOSITE_SCREEN,
            Some(|o| o.allow_simple_blends = false),
        ),
        SoftLight => (
            super::LAYER_COMPOSITE_SOFT_LIGHT,
            Some(|o| o.allow_soft_light_blend = false),
        ),
        Subtract => (
            super::LAYER_COMPOSITE_SUBTRACT,
            Some(|o| o.allow_simple_blends = false),
        ),
        VividLight => (
            super::LAYER_COMPOSITE_VIVID_LIGHT,
            Some(|o| o.allow_vivid_light_blend = false),
        ),
    }
}
