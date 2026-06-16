use std::collections::HashMap;
use std::path::{Path, PathBuf};

use clip_file::ClipFileSummary;
use clip_graph::{RenderNode, RenderNodeId, RenderNodeKind, RenderPlan};
use clip_model::{CanvasSize, LayerId, LayerKind, LayerOpacity, LayerVisibility, Rgba8};

use super::{ClipSession, StrictRasterStackDraw, StrictRasterStackOptions};

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
fn byte_diff_count_includes_length_mismatch() {
    assert_eq!(super::byte_diff_count(&[1, 2, 3], &[1, 4]), 2);
}

#[test]
fn alpha_is_fully_opaque_checks_every_pixel() {
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
fn gpu_selector_accepts_python_backed_lut_and_luminosity_filters() {
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
    for (source, (input, expected, expected_mode)) in selection.sources.iter().zip([
        (
            64usize,
            [51, 51, 51],
            clip_gpu::GpuLutFilterMode::ToneCurveRgb,
        ),
        (
            64usize,
            [24, 24, 24],
            clip_gpu::GpuLutFilterMode::ToneCurveRgb,
        ),
        (
            64usize,
            [191, 191, 191],
            clip_gpu::GpuLutFilterMode::ToneCurveRgb,
        ),
        (
            64usize,
            [85, 85, 85],
            clip_gpu::GpuLutFilterMode::ToneCurveRgb,
        ),
        (
            128usize,
            [255, 255, 255],
            clip_gpu::GpuLutFilterMode::ThresholdLum,
        ),
        (
            226usize,
            [231, 212, 231],
            clip_gpu::GpuLutFilterMode::ToneCurveRgb,
        ),
        (
            128usize,
            [128, 128, 128],
            clip_gpu::GpuLutFilterMode::Hsl(clip_gpu::GpuHslFilterParams {
                hue_degrees: 30.0,
                saturation: -25.0,
                luminosity: 40.0,
            }),
        ),
    ]) {
        let clip_gpu::GpuNormalStackSource::LutFilter {
            lut_rgba,
            filter_mode,
            ..
        } = source
        else {
            panic!("source was not represented as a LUT filter");
        };
        assert_eq!(*filter_mode, expected_mode);
        assert_eq!(&lut_rgba[input * 4..input * 4 + 3], expected.as_slice());
    }
}

#[test]
fn strict_raster_blend_mode_allows_supported_blends_by_position() {
    let options = StrictRasterStackOptions {
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
    };
    let add_glow = raster_node_with_composite(1, 5, 1, 9, false, super::LAYER_COMPOSITE_ADD_GLOW);
    assert_eq!(
        super::strict_raster_blend_mode(&add_glow, options, false),
        Some(super::StrictRasterBlendMode::AddGlow)
    );

    let clipped_add_glow =
        raster_node_with_composite(2, 6, 1, 10, true, super::LAYER_COMPOSITE_ADD_GLOW);
    assert_eq!(
        super::strict_raster_blend_mode(&clipped_add_glow, options, true),
        Some(super::StrictRasterBlendMode::AddGlow)
    );

    let disabled = StrictRasterStackOptions {
        allow_add_glow_blend: false,
        ..options
    };
    assert_eq!(
        super::strict_raster_blend_mode(&add_glow, disabled, false),
        None
    );

    let color_burn =
        raster_node_with_composite(7, 11, 1, 15, false, super::LAYER_COMPOSITE_COLOR_BURN);
    assert_eq!(
        super::strict_raster_blend_mode(&color_burn, options, false),
        Some(super::StrictRasterBlendMode::ColorBurn)
    );

    let clipped_color_burn =
        raster_node_with_composite(8, 12, 1, 16, true, super::LAYER_COMPOSITE_COLOR_BURN);
    assert_eq!(
        super::strict_raster_blend_mode(&clipped_color_burn, options, true),
        Some(super::StrictRasterBlendMode::ColorBurn)
    );

    let disabled = StrictRasterStackOptions {
        allow_color_burn_blend: false,
        ..options
    };
    assert_eq!(
        super::strict_raster_blend_mode(&color_burn, disabled, false),
        None
    );

    let color_dodge =
        raster_node_with_composite(3, 7, 1, 11, false, super::LAYER_COMPOSITE_COLOR_DODGE);
    assert_eq!(
        super::strict_raster_blend_mode(&color_dodge, options, false),
        Some(super::StrictRasterBlendMode::ColorDodge)
    );

    let clipped_color_dodge =
        raster_node_with_composite(4, 8, 1, 12, true, super::LAYER_COMPOSITE_COLOR_DODGE);
    assert_eq!(
        super::strict_raster_blend_mode(&clipped_color_dodge, options, true),
        Some(super::StrictRasterBlendMode::ColorDodge)
    );

    let disabled = StrictRasterStackOptions {
        allow_color_dodge_blend: false,
        ..options
    };
    assert_eq!(
        super::strict_raster_blend_mode(&color_dodge, disabled, false),
        None
    );

    let glow_dodge =
        raster_node_with_composite(5, 9, 1, 13, false, super::LAYER_COMPOSITE_GLOW_DODGE);
    assert_eq!(
        super::strict_raster_blend_mode(&glow_dodge, options, false),
        Some(super::StrictRasterBlendMode::GlowDodge)
    );

    let clipped_glow_dodge =
        raster_node_with_composite(6, 10, 1, 14, true, super::LAYER_COMPOSITE_GLOW_DODGE);
    assert_eq!(
        super::strict_raster_blend_mode(&clipped_glow_dodge, options, true),
        Some(super::StrictRasterBlendMode::GlowDodge)
    );

    let disabled = StrictRasterStackOptions {
        allow_glow_dodge_blend: false,
        ..options
    };
    assert_eq!(
        super::strict_raster_blend_mode(&glow_dodge, disabled, false),
        None
    );

    let add = raster_node_with_composite(26, 36, 1, 40, false, super::LAYER_COMPOSITE_ADD);
    assert_eq!(
        super::strict_raster_blend_mode(&add, options, false),
        Some(super::StrictRasterBlendMode::Add)
    );

    let clipped_add = raster_node_with_composite(27, 37, 1, 41, true, super::LAYER_COMPOSITE_ADD);
    assert_eq!(
        super::strict_raster_blend_mode(&clipped_add, options, true),
        Some(super::StrictRasterBlendMode::Add)
    );

    let disabled = StrictRasterStackOptions {
        allow_add_blend: false,
        ..options
    };
    assert_eq!(super::strict_raster_blend_mode(&add, disabled, false), None);

    let hard_mix = raster_node_with_composite(9, 13, 1, 17, false, super::LAYER_COMPOSITE_HARD_MIX);
    assert_eq!(
        super::strict_raster_blend_mode(&hard_mix, options, false),
        Some(super::StrictRasterBlendMode::HardMix)
    );

    let clipped_hard_mix =
        raster_node_with_composite(10, 14, 1, 18, true, super::LAYER_COMPOSITE_HARD_MIX);
    assert_eq!(
        super::strict_raster_blend_mode(&clipped_hard_mix, options, true),
        Some(super::StrictRasterBlendMode::HardMix)
    );

    let disabled = StrictRasterStackOptions {
        allow_hard_mix_blend: false,
        ..options
    };
    assert_eq!(
        super::strict_raster_blend_mode(&hard_mix, disabled, false),
        None
    );

    let w3c_blends = [
        (
            40,
            50,
            60,
            super::LAYER_COMPOSITE_MULTIPLY,
            super::StrictRasterBlendMode::Multiply,
        ),
        (
            41,
            51,
            61,
            super::LAYER_COMPOSITE_OVERLAY,
            super::StrictRasterBlendMode::Overlay,
        ),
        (
            42,
            52,
            62,
            super::LAYER_COMPOSITE_HARD_LIGHT,
            super::StrictRasterBlendMode::HardLight,
        ),
    ];
    for (node, layer, mipmap, composite, expected) in w3c_blends {
        let raster = raster_node_with_composite(node, layer, 1, mipmap, false, composite);
        assert_eq!(
            super::strict_raster_blend_mode(&raster, options, false),
            Some(expected)
        );

        let clipped =
            raster_node_with_composite(node + 10, layer + 10, 1, mipmap + 10, true, composite);
        assert_eq!(
            super::strict_raster_blend_mode(&clipped, options, true),
            Some(expected)
        );

        let disabled = StrictRasterStackOptions {
            allow_w3c_blends: false,
            ..options
        };
        assert_eq!(
            super::strict_raster_blend_mode(&raster, disabled, false),
            None
        );
    }

    let simple_blends = [
        (
            21,
            31,
            35,
            super::LAYER_COMPOSITE_DARKEN,
            super::StrictRasterBlendMode::Darken,
        ),
        (
            22,
            32,
            36,
            super::LAYER_COMPOSITE_SUBTRACT,
            super::StrictRasterBlendMode::Subtract,
        ),
        (
            23,
            33,
            37,
            super::LAYER_COMPOSITE_LIGHTEN,
            super::StrictRasterBlendMode::Lighten,
        ),
        (
            24,
            34,
            38,
            super::LAYER_COMPOSITE_SCREEN,
            super::StrictRasterBlendMode::Screen,
        ),
        (
            25,
            35,
            39,
            super::LAYER_COMPOSITE_DIFFERENCE,
            super::StrictRasterBlendMode::Difference,
        ),
    ];
    for (node, layer, mipmap, composite, expected) in simple_blends {
        let raster = raster_node_with_composite(node, layer, 1, mipmap, false, composite);
        assert_eq!(
            super::strict_raster_blend_mode(&raster, options, false),
            Some(expected)
        );

        let clipped =
            raster_node_with_composite(node + 10, layer + 10, 1, mipmap + 10, true, composite);
        assert_eq!(
            super::strict_raster_blend_mode(&clipped, options, true),
            Some(expected)
        );

        let disabled = StrictRasterStackOptions {
            allow_simple_blends: false,
            ..options
        };
        assert_eq!(
            super::strict_raster_blend_mode(&raster, disabled, false),
            None
        );
    }

    let extended_blends = [
        (
            60,
            70,
            80,
            super::LAYER_COMPOSITE_LINEAR_BURN,
            super::StrictRasterBlendMode::LinearBurn,
        ),
        (
            61,
            71,
            81,
            super::LAYER_COMPOSITE_DARKER_COLOR,
            super::StrictRasterBlendMode::DarkerColor,
        ),
        (
            62,
            72,
            82,
            super::LAYER_COMPOSITE_LIGHTER_COLOR,
            super::StrictRasterBlendMode::LighterColor,
        ),
        (
            63,
            73,
            83,
            super::LAYER_COMPOSITE_LINEAR_LIGHT,
            super::StrictRasterBlendMode::LinearLight,
        ),
        (
            64,
            74,
            84,
            super::LAYER_COMPOSITE_PIN_LIGHT,
            super::StrictRasterBlendMode::PinLight,
        ),
        (
            65,
            75,
            85,
            super::LAYER_COMPOSITE_EXCLUSION,
            super::StrictRasterBlendMode::Exclusion,
        ),
        (
            66,
            76,
            86,
            super::LAYER_COMPOSITE_BRIGHTNESS,
            super::StrictRasterBlendMode::Brightness,
        ),
        (
            67,
            77,
            87,
            super::LAYER_COMPOSITE_DIVIDE,
            super::StrictRasterBlendMode::Divide,
        ),
    ];
    for (node, layer, mipmap, composite, expected) in extended_blends {
        let raster = raster_node_with_composite(node, layer, 1, mipmap, false, composite);
        assert_eq!(
            super::strict_raster_blend_mode(&raster, options, false),
            Some(expected)
        );

        let clipped =
            raster_node_with_composite(node + 10, layer + 10, 1, mipmap + 10, true, composite);
        assert_eq!(
            super::strict_raster_blend_mode(&clipped, options, true),
            Some(expected)
        );

        let disabled = StrictRasterStackOptions {
            allow_extended_blends: false,
            ..options
        };
        assert_eq!(
            super::strict_raster_blend_mode(&raster, disabled, false),
            None
        );
    }

    let hue = raster_node_with_composite(13, 17, 1, 21, false, super::LAYER_COMPOSITE_HUE);
    assert_eq!(
        super::strict_raster_blend_mode(&hue, options, false),
        Some(super::StrictRasterBlendMode::Hue)
    );

    let clipped_hue = raster_node_with_composite(14, 18, 1, 22, true, super::LAYER_COMPOSITE_HUE);
    assert_eq!(
        super::strict_raster_blend_mode(&clipped_hue, options, true),
        Some(super::StrictRasterBlendMode::Hue)
    );

    let saturation =
        raster_node_with_composite(15, 19, 1, 23, false, super::LAYER_COMPOSITE_SATURATION);
    assert_eq!(
        super::strict_raster_blend_mode(&saturation, options, false),
        Some(super::StrictRasterBlendMode::Saturation)
    );

    let clipped_saturation =
        raster_node_with_composite(16, 20, 1, 24, true, super::LAYER_COMPOSITE_SATURATION);
    assert_eq!(
        super::strict_raster_blend_mode(&clipped_saturation, options, true),
        Some(super::StrictRasterBlendMode::Saturation)
    );

    let color = raster_node_with_composite(17, 21, 1, 25, false, super::LAYER_COMPOSITE_COLOR);
    assert_eq!(
        super::strict_raster_blend_mode(&color, options, false),
        Some(super::StrictRasterBlendMode::Color)
    );

    let clipped_color =
        raster_node_with_composite(18, 22, 1, 26, true, super::LAYER_COMPOSITE_COLOR);
    assert_eq!(
        super::strict_raster_blend_mode(&clipped_color, options, true),
        Some(super::StrictRasterBlendMode::Color)
    );

    let disabled = StrictRasterStackOptions {
        allow_hsl_blends: false,
        ..options
    };
    assert_eq!(super::strict_raster_blend_mode(&hue, disabled, false), None);
    assert_eq!(
        super::strict_raster_blend_mode(&saturation, disabled, false),
        None
    );
    assert_eq!(
        super::strict_raster_blend_mode(&color, disabled, false),
        None
    );

    let soft_light =
        raster_node_with_composite(19, 23, 1, 27, false, super::LAYER_COMPOSITE_SOFT_LIGHT);
    assert_eq!(
        super::strict_raster_blend_mode(&soft_light, options, false),
        Some(super::StrictRasterBlendMode::SoftLight)
    );

    let clipped_soft_light =
        raster_node_with_composite(20, 24, 1, 28, true, super::LAYER_COMPOSITE_SOFT_LIGHT);
    assert_eq!(
        super::strict_raster_blend_mode(&clipped_soft_light, options, true),
        Some(super::StrictRasterBlendMode::SoftLight)
    );

    let disabled = StrictRasterStackOptions {
        allow_soft_light_blend: false,
        ..options
    };
    assert_eq!(
        super::strict_raster_blend_mode(&soft_light, disabled, false),
        None
    );

    let vivid_light =
        raster_node_with_composite(11, 15, 1, 19, false, super::LAYER_COMPOSITE_VIVID_LIGHT);
    assert_eq!(
        super::strict_raster_blend_mode(&vivid_light, options, false),
        Some(super::StrictRasterBlendMode::VividLight)
    );

    let clipped_vivid_light =
        raster_node_with_composite(12, 16, 1, 20, true, super::LAYER_COMPOSITE_VIVID_LIGHT);
    assert_eq!(
        super::strict_raster_blend_mode(&clipped_vivid_light, options, true),
        Some(super::StrictRasterBlendMode::VividLight)
    );

    let disabled = StrictRasterStackOptions {
        allow_vivid_light_blend: false,
        ..options
    };
    assert_eq!(
        super::strict_raster_blend_mode(&vivid_light, disabled, false),
        None
    );
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
fn gpu_selector_accepts_container_base_clipping_runs_in_aya_fixture() {
    let path = fixture_path_ending("Aya_Live2D.clip");
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
    let path = fixture_path_ending("Kabi_Live2D.clip");
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
fn gpu_selector_folds_off_canvas_zero_fill_mask_to_zero_opacity() {
    let mut session = synthetic_session(vec![
        container_node(0, 2, 0, 0),
        raster_node_with_mask(1, 10, 1, 15, 50, false),
    ]);
    session
        .raster_sources
        .insert(LayerId(10), raster_source(10, 15));
    session
        .mask_sources
        .insert(LayerId(10), off_canvas_mask_source(10, 50, 0));

    let selection = session
        .select_gpu_normal_render_stack(gpu_selector_options())
        .expect("select synthetic masked raster");

    assert!(selection.unsupported.is_empty());
    assert_eq!(selection.resource_plan.mask_resource_count(), 0);
    assert_eq!(selection.sources.len(), 1);
    let clip_gpu::GpuNormalStackSource::Raster(raster) = &selection.sources[0] else {
        panic!("masked raster was not represented as a raster source");
    };
    assert_eq!(raster.opacity, 0.0);
    assert_eq!(raster.mask_key, None);
}

#[test]
fn gpu_selector_elides_off_canvas_opaque_mask_resource() {
    let mut session = synthetic_session(vec![
        container_node(0, 2, 0, 0),
        raster_node_with_mask(1, 10, 1, 15, 50, false),
    ]);
    session
        .raster_sources
        .insert(LayerId(10), raster_source(10, 15));
    session
        .mask_sources
        .insert(LayerId(10), off_canvas_mask_source(10, 50, 255));

    let selection = session
        .select_gpu_normal_render_stack(gpu_selector_options())
        .expect("select synthetic masked raster");

    assert!(selection.unsupported.is_empty());
    assert_eq!(selection.resource_plan.mask_resource_count(), 0);
    assert_eq!(selection.sources.len(), 1);
    let clip_gpu::GpuNormalStackSource::Raster(raster) = &selection.sources[0] else {
        panic!("masked raster was not represented as a raster source");
    };
    assert_eq!(raster.opacity, 1.0);
    assert_eq!(raster.mask_key, None);
}

#[test]
fn gpu_selector_keeps_partial_fill_off_canvas_mask_resource() {
    let mut session = synthetic_session(vec![
        container_node(0, 2, 0, 0),
        raster_node_with_mask(1, 10, 1, 15, 50, false),
    ]);
    session
        .raster_sources
        .insert(LayerId(10), raster_source(10, 15));
    session
        .mask_sources
        .insert(LayerId(10), off_canvas_mask_source(10, 50, 128));

    let selection = session
        .select_gpu_normal_render_stack(gpu_selector_options())
        .expect("select synthetic masked raster");

    assert!(selection.unsupported.is_empty());
    assert_eq!(selection.resource_plan.mask_resource_count(), 1);
    assert_eq!(selection.sources.len(), 1);
    let clip_gpu::GpuNormalStackSource::Raster(raster) = &selection.sources[0] else {
        panic!("masked raster was not represented as a raster source");
    };
    assert_eq!(raster.opacity, 1.0);
    assert!(raster.mask_key.is_some());
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
        rendered_image: None,
    }
}

fn fixture_path_ending(suffix: &str) -> PathBuf {
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
        .unwrap_or_else(|| panic!("fixture ending with {suffix:?} not found"))
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
