use clip_graph::RenderNodeId;
use clip_model::{CanvasSize, LayerId, Rgba8};

use crate::{
    GpuClippedStackSource, GpuDeviceConfig, GpuNormalRasterSource, GpuNormalStackSource,
    GpuRasterBlendMode, GpuRasterResourceCache, GpuRasterResourceKey, GpuRasterUpload, GpuRenderer,
};

#[test]
fn container_source_resolves_child_cache_with_opacity() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let sources = [GpuNormalStackSource::Container {
        children: vec![GpuNormalStackSource::SolidColor {
            color: Rgba8 {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
            opacity: 1.0,
        }],
        opacity: 0.5,
        mask_key: None,
        blend_mode: GpuRasterBlendMode::Normal,
    }];

    let output = renderer
        .draw_normal_stack_to_rgba8(
            &GpuRasterResourceCache::empty(),
            None,
            CanvasSize::new(2, 2),
            &sources,
        )
        .expect("draw container source");

    assert_eq!(output.size, CanvasSize::new(2, 2));
    for pixel in output.pixels.chunks_exact(4) {
        assert_eq!(pixel[0], 255);
        assert_eq!(pixel[1], 0);
        assert_eq!(pixel[2], 0);
        assert!((127..=128).contains(&pixel[3]));
    }
}

#[test]
fn raster_source_samples_with_canvas_offset() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = GpuRasterResourceKey {
        layer_id: LayerId(30),
        render_mipmap_id: 40,
    };
    let uploads = [GpuRasterUpload {
        layer_id: key.layer_id,
        render_node_id: RenderNodeId(30),
        render_mipmap_id: key.render_mipmap_id,
        size: CanvasSize::new(1, 1),
        pixels: &[10, 20, 30, 255],
    }];
    let cache = renderer
        .upload_raster_resources(&uploads)
        .expect("upload source-sized raster");
    let sources = [GpuNormalStackSource::Raster(GpuNormalRasterSource {
        key,
        opacity: 1.0,
        mask_key: None,
        offset_x: 1,
        offset_y: 1,
        blend_mode: GpuRasterBlendMode::Normal,
    })];

    let output = renderer
        .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(3, 3), &sources)
        .expect("draw source-sized raster with canvas offset");

    let mut expected = vec![255, 255, 255, 0].repeat(9);
    expected[16..20].copy_from_slice(&[10, 20, 30, 255]);
    assert_eq!(output.pixels, expected);
}

#[test]
fn container_source_resolves_child_cache_with_blend_mode() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let sources = [
        GpuNormalStackSource::SolidColor {
            color: Rgba8 {
                r: 200,
                g: 160,
                b: 120,
                a: 255,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::Container {
            children: vec![GpuNormalStackSource::SolidColor {
                color: Rgba8 {
                    r: 128,
                    g: 64,
                    b: 255,
                    a: 255,
                },
                opacity: 1.0,
            }],
            opacity: 1.0,
            mask_key: None,
            blend_mode: GpuRasterBlendMode::Multiply,
        },
    ];

    let output = renderer
        .draw_normal_stack_to_rgba8(
            &GpuRasterResourceCache::empty(),
            None,
            CanvasSize::new(1, 1),
            &sources,
        )
        .expect("draw multiply container source");

    assert_eq!(output.pixels, [100, 40, 120, 255]);
}

#[test]
fn normal_raster_source_uses_byte_domain_alpha_over() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let base_key = GpuRasterResourceKey {
        layer_id: LayerId(7),
        render_mipmap_id: 17,
    };
    let top_key = GpuRasterResourceKey {
        layer_id: LayerId(8),
        render_mipmap_id: 18,
    };
    let uploads = [
        GpuRasterUpload {
            layer_id: base_key.layer_id,
            render_node_id: RenderNodeId(7),
            render_mipmap_id: base_key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &[0, 0, 0, 26],
        },
        GpuRasterUpload {
            layer_id: top_key.layer_id,
            render_node_id: RenderNodeId(8),
            render_mipmap_id: top_key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &[197, 182, 252, 91],
        },
    ];
    let cache = renderer
        .upload_raster_resources(&uploads)
        .expect("upload normal alpha-over sources");
    let sources = [
        GpuNormalStackSource::Raster(GpuNormalRasterSource {
            key: base_key,
            opacity: 1.0,
            mask_key: None,
            offset_x: 0,
            offset_y: 0,
            blend_mode: GpuRasterBlendMode::Normal,
        }),
        GpuNormalStackSource::Raster(GpuNormalRasterSource {
            key: top_key,
            opacity: 1.0,
            mask_key: None,
            offset_x: 0,
            offset_y: 0,
            blend_mode: GpuRasterBlendMode::Normal,
        }),
    ];

    let output = renderer
        .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
        .expect("draw normal alpha-over sources");

    assert_eq!(output.pixels, [168, 155, 214, 107]);
}

#[test]
fn through_group_resolves_before_after_delta_with_opacity() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let sources = [
        GpuNormalStackSource::SolidColor {
            color: Rgba8 {
                r: 0,
                g: 0,
                b: 255,
                a: 255,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::ThroughGroup {
            children: vec![GpuNormalStackSource::SolidColor {
                color: Rgba8 {
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 255,
                },
                opacity: 1.0,
            }],
            opacity: 0.5,
            mask_key: None,
        },
    ];

    let output = renderer
        .draw_normal_stack_to_rgba8(
            &GpuRasterResourceCache::empty(),
            None,
            CanvasSize::new(2, 2),
            &sources,
        )
        .expect("draw through group source");

    assert_eq!(output.size, CanvasSize::new(2, 2));
    for pixel in output.pixels.chunks_exact(4) {
        assert!((127..=128).contains(&pixel[0]));
        assert_eq!(pixel[1], 0);
        assert!((127..=128).contains(&pixel[2]));
        assert_eq!(pixel[3], 255);
    }
}

#[test]
fn add_raster_source_uses_standard_blend_formula() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = GpuRasterResourceKey {
        layer_id: LayerId(9),
        render_mipmap_id: 19,
    };
    let pixels = [100u8, 50, 100, 128];
    let uploads = [GpuRasterUpload {
        layer_id: key.layer_id,
        render_node_id: RenderNodeId(0),
        render_mipmap_id: key.render_mipmap_id,
        size: CanvasSize::new(1, 1),
        pixels: &pixels,
    }];
    let cache = renderer
        .upload_raster_resources(&uploads)
        .expect("upload Add source");
    let sources = [
        GpuNormalStackSource::SolidColor {
            color: Rgba8 {
                r: 0,
                g: 0,
                b: 200,
                a: 255,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::Raster(GpuNormalRasterSource {
            key,
            opacity: 1.0,
            mask_key: None,
            offset_x: 0,
            offset_y: 0,
            blend_mode: GpuRasterBlendMode::Add,
        }),
    ];

    let output = renderer
        .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
        .expect("draw Add source");

    assert_eq!(output.pixels, [50, 25, 228, 255]);
}

#[test]
fn add_glow_raster_source_uses_byte_domain_formula() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = GpuRasterResourceKey {
        layer_id: LayerId(10),
        render_mipmap_id: 20,
    };
    let pixels = [100u8, 50, 0, 128];
    let uploads = [GpuRasterUpload {
        layer_id: key.layer_id,
        render_node_id: RenderNodeId(1),
        render_mipmap_id: key.render_mipmap_id,
        size: CanvasSize::new(1, 1),
        pixels: &pixels,
    }];
    let cache = renderer
        .upload_raster_resources(&uploads)
        .expect("upload AddGlow source");
    let sources = [
        GpuNormalStackSource::SolidColor {
            color: Rgba8 {
                r: 0,
                g: 0,
                b: 200,
                a: 255,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::Raster(GpuNormalRasterSource {
            key,
            opacity: 1.0,
            mask_key: None,
            offset_x: 0,
            offset_y: 0,
            blend_mode: GpuRasterBlendMode::AddGlow,
        }),
    ];

    let output = renderer
        .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
        .expect("draw AddGlow source");

    assert_eq!(output.pixels, [50, 25, 200, 255]);
}

#[test]
fn add_glow_raster_source_rounds_partial_tail_like_csp() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = GpuRasterResourceKey {
        layer_id: LayerId(11),
        render_mipmap_id: 21,
    };
    let pixels = [66u8, 251, 182, 2];
    let uploads = [GpuRasterUpload {
        layer_id: key.layer_id,
        render_node_id: RenderNodeId(2),
        render_mipmap_id: key.render_mipmap_id,
        size: CanvasSize::new(1, 1),
        pixels: &pixels,
    }];
    let cache = renderer
        .upload_raster_resources(&uploads)
        .expect("upload partial AddGlow source");
    let sources = [
        GpuNormalStackSource::SolidColor {
            color: Rgba8 {
                r: 169,
                g: 32,
                b: 253,
                a: 53,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::Raster(GpuNormalRasterSource {
            key,
            opacity: 1.0,
            mask_key: None,
            offset_x: 0,
            offset_y: 0,
            blend_mode: GpuRasterBlendMode::AddGlow,
        }),
    ];

    let output = renderer
        .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
        .expect("draw partial AddGlow source");

    assert_eq!(output.pixels, [167, 49, 252, 55]);
}

#[test]
fn clipping_run_resolves_cache_with_base_blend_and_clipped_multiply() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let base_key = GpuRasterResourceKey {
        layer_id: LayerId(30),
        render_mipmap_id: 40,
    };
    let clipped_key = GpuRasterResourceKey {
        layer_id: LayerId(31),
        render_mipmap_id: 41,
    };
    let base_pixels = [100u8, 50, 0, 128];
    let clipped_pixels = [128u8, 128, 128, 128];
    let uploads = [
        GpuRasterUpload {
            layer_id: base_key.layer_id,
            render_node_id: RenderNodeId(30),
            render_mipmap_id: base_key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &base_pixels,
        },
        GpuRasterUpload {
            layer_id: clipped_key.layer_id,
            render_node_id: RenderNodeId(31),
            render_mipmap_id: clipped_key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &clipped_pixels,
        },
    ];
    let cache = renderer
        .upload_raster_resources(&uploads)
        .expect("upload clipping run sources");
    let sources = [
        GpuNormalStackSource::SolidColor {
            color: Rgba8 {
                r: 20,
                g: 40,
                b: 60,
                a: 255,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::ClippingRun {
            base: GpuNormalRasterSource {
                key: base_key,
                opacity: 1.0,
                mask_key: None,
                offset_x: 0,
                offset_y: 0,
                blend_mode: GpuRasterBlendMode::AddGlow,
            },
            clipped: vec![GpuClippedStackSource::Raster(GpuNormalRasterSource {
                key: clipped_key,
                opacity: 1.0,
                mask_key: None,
                offset_x: 0,
                offset_y: 0,
                blend_mode: GpuRasterBlendMode::Multiply,
            })],
        },
    ];

    let output = renderer
        .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
        .expect("draw clipping run");

    assert_eq!(output.pixels, [58, 59, 60, 255]);
}

#[test]
fn container_clipping_run_matches_equivalent_raster_base_run() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let base_key = GpuRasterResourceKey {
        layer_id: LayerId(34),
        render_mipmap_id: 44,
    };
    let clipped_key = GpuRasterResourceKey {
        layer_id: LayerId(35),
        render_mipmap_id: 45,
    };
    let base_pixels = [100u8, 50, 0, 128];
    let clipped_pixels = [128u8, 128, 128, 128];
    let uploads = [
        GpuRasterUpload {
            layer_id: base_key.layer_id,
            render_node_id: RenderNodeId(34),
            render_mipmap_id: base_key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &base_pixels,
        },
        GpuRasterUpload {
            layer_id: clipped_key.layer_id,
            render_node_id: RenderNodeId(35),
            render_mipmap_id: clipped_key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &clipped_pixels,
        },
    ];
    let cache = renderer
        .upload_raster_resources(&uploads)
        .expect("upload container clipping run sources");
    let parent = GpuNormalStackSource::SolidColor {
        color: Rgba8 {
            r: 20,
            g: 40,
            b: 60,
            a: 255,
        },
        opacity: 1.0,
    };
    let clipped = GpuNormalRasterSource {
        key: clipped_key,
        opacity: 1.0,
        mask_key: None,
        offset_x: 0,
        offset_y: 0,
        blend_mode: GpuRasterBlendMode::Multiply,
    };
    let raster_base_sources = [
        parent.clone(),
        GpuNormalStackSource::ClippingRun {
            base: GpuNormalRasterSource {
                key: base_key,
                opacity: 1.0,
                mask_key: None,
                offset_x: 0,
                offset_y: 0,
                blend_mode: GpuRasterBlendMode::AddGlow,
            },
            clipped: vec![GpuClippedStackSource::Raster(clipped)],
        },
    ];
    let container_base_sources = [
        parent,
        GpuNormalStackSource::ContainerClippingRun {
            children: vec![GpuNormalStackSource::Raster(GpuNormalRasterSource {
                key: base_key,
                opacity: 1.0,
                mask_key: None,
                offset_x: 0,
                offset_y: 0,
                blend_mode: GpuRasterBlendMode::Normal,
            })],
            opacity: 1.0,
            mask_key: None,
            blend_mode: GpuRasterBlendMode::AddGlow,
            clipped: vec![GpuClippedStackSource::Raster(clipped)],
        },
    ];

    let raster_base = renderer
        .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &raster_base_sources)
        .expect("draw equivalent raster-base clipping run");
    let container_base = renderer
        .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &container_base_sources)
        .expect("draw container-base clipping run");

    assert_eq!(container_base.pixels, raster_base.pixels);
    assert_eq!(container_base.pixels, [58, 59, 60, 255]);
}

#[test]
fn clipping_run_preserves_base_alpha_with_byte_domain_clipped_blends() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let base_key = GpuRasterResourceKey {
        layer_id: LayerId(32),
        render_mipmap_id: 42,
    };
    let clipped_key = GpuRasterResourceKey {
        layer_id: LayerId(33),
        render_mipmap_id: 43,
    };
    let base_pixels = [50u8, 50, 50, 128];
    let clipped_pixels = [128u8, 0, 0, 128];
    let uploads = [
        GpuRasterUpload {
            layer_id: base_key.layer_id,
            render_node_id: RenderNodeId(32),
            render_mipmap_id: base_key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &base_pixels,
        },
        GpuRasterUpload {
            layer_id: clipped_key.layer_id,
            render_node_id: RenderNodeId(33),
            render_mipmap_id: clipped_key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &clipped_pixels,
        },
    ];
    let cache = renderer
        .upload_raster_resources(&uploads)
        .expect("upload clipping run sources");

    for (blend_mode, expected) in [
        (GpuRasterBlendMode::AddGlow, [114, 50, 50, 128]),
        (GpuRasterBlendMode::ColorDodge, [75, 50, 50, 128]),
        (GpuRasterBlendMode::ColorBurn, [25, 25, 25, 128]),
        (GpuRasterBlendMode::GlowDodge, [66, 50, 50, 128]),
    ] {
        let sources = [GpuNormalStackSource::ClippingRun {
            base: GpuNormalRasterSource {
                key: base_key,
                opacity: 1.0,
                mask_key: None,
                offset_x: 0,
                offset_y: 0,
                blend_mode: GpuRasterBlendMode::Normal,
            },
            clipped: vec![GpuClippedStackSource::Raster(GpuNormalRasterSource {
                key: clipped_key,
                opacity: 1.0,
                mask_key: None,
                offset_x: 0,
                offset_y: 0,
                blend_mode,
            })],
        }];

        let output = renderer
            .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
            .expect("draw byte-domain clipping run");

        assert_eq!(output.pixels, expected, "blend mode {blend_mode:?}");
    }
}

#[test]
fn color_dodge_raster_source_uses_byte_domain_formula() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = GpuRasterResourceKey {
        layer_id: LayerId(11),
        render_mipmap_id: 21,
    };
    let pixels = [100u8, 200, 250, 128];
    let uploads = [GpuRasterUpload {
        layer_id: key.layer_id,
        render_node_id: RenderNodeId(2),
        render_mipmap_id: key.render_mipmap_id,
        size: CanvasSize::new(1, 1),
        pixels: &pixels,
    }];
    let cache = renderer
        .upload_raster_resources(&uploads)
        .expect("upload ColorDodge source");
    let sources = [
        GpuNormalStackSource::SolidColor {
            color: Rgba8 {
                r: 100,
                g: 50,
                b: 200,
                a: 255,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::Raster(GpuNormalRasterSource {
            key,
            opacity: 1.0,
            mask_key: None,
            offset_x: 0,
            offset_y: 0,
            blend_mode: GpuRasterBlendMode::ColorDodge,
        }),
    ];

    let output = renderer
        .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
        .expect("draw ColorDodge source");

    assert_eq!(output.pixels, [132, 141, 228, 255]);
}

#[test]
fn color_burn_raster_source_uses_byte_domain_formula() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = GpuRasterResourceKey {
        layer_id: LayerId(13),
        render_mipmap_id: 23,
    };
    let pixels = [100u8, 200, 250, 128];
    let uploads = [GpuRasterUpload {
        layer_id: key.layer_id,
        render_node_id: RenderNodeId(4),
        render_mipmap_id: key.render_mipmap_id,
        size: CanvasSize::new(1, 1),
        pixels: &pixels,
    }];
    let cache = renderer
        .upload_raster_resources(&uploads)
        .expect("upload ColorBurn source");
    let sources = [
        GpuNormalStackSource::SolidColor {
            color: Rgba8 {
                r: 100,
                g: 50,
                b: 200,
                a: 255,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::Raster(GpuNormalRasterSource {
            key,
            opacity: 1.0,
            mask_key: None,
            offset_x: 0,
            offset_y: 0,
            blend_mode: GpuRasterBlendMode::ColorBurn,
        }),
    ];

    let output = renderer
        .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
        .expect("draw ColorBurn source");

    assert_eq!(output.pixels, [50, 25, 199, 255]);
}

#[test]
fn vivid_light_raster_source_uses_standard_blend_formula() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = GpuRasterResourceKey {
        layer_id: LayerId(14),
        render_mipmap_id: 24,
    };
    let pixels = [100u8, 200, 250, 128];
    let uploads = [GpuRasterUpload {
        layer_id: key.layer_id,
        render_node_id: RenderNodeId(5),
        render_mipmap_id: key.render_mipmap_id,
        size: CanvasSize::new(1, 1),
        pixels: &pixels,
    }];
    let cache = renderer
        .upload_raster_resources(&uploads)
        .expect("upload VividLight source");
    let sources = [
        GpuNormalStackSource::SolidColor {
            color: Rgba8 {
                r: 100,
                g: 50,
                b: 200,
                a: 255,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::Raster(GpuNormalRasterSource {
            key,
            opacity: 1.0,
            mask_key: None,
            offset_x: 0,
            offset_y: 0,
            blend_mode: GpuRasterBlendMode::VividLight,
        }),
    ];

    let output = renderer
        .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
        .expect("draw VividLight source");

    assert_eq!(output.pixels, [78, 83, 228, 255]);
}

#[test]
fn hard_mix_raster_source_uses_standard_blend_formula() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = GpuRasterResourceKey {
        layer_id: LayerId(15),
        render_mipmap_id: 25,
    };
    let pixels = [100u8, 200, 250, 128];
    let uploads = [GpuRasterUpload {
        layer_id: key.layer_id,
        render_node_id: RenderNodeId(6),
        render_mipmap_id: key.render_mipmap_id,
        size: CanvasSize::new(1, 1),
        pixels: &pixels,
    }];
    let cache = renderer
        .upload_raster_resources(&uploads)
        .expect("upload HardMix source");
    let sources = [
        GpuNormalStackSource::SolidColor {
            color: Rgba8 {
                r: 100,
                g: 50,
                b: 200,
                a: 255,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::Raster(GpuNormalRasterSource {
            key,
            opacity: 1.0,
            mask_key: None,
            offset_x: 0,
            offset_y: 0,
            blend_mode: GpuRasterBlendMode::HardMix,
        }),
    ];

    let output = renderer
        .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
        .expect("draw HardMix source");

    assert_eq!(output.pixels, [50, 25, 228, 255]);
}

fn draw_one_pixel_standard_blend(blend_mode: GpuRasterBlendMode) -> Vec<u8> {
    draw_one_pixel_standard_blend_with_colors(
        blend_mode,
        [100, 200, 250, 128],
        Rgba8 {
            r: 100,
            g: 50,
            b: 200,
            a: 255,
        },
    )
}

fn draw_one_pixel_standard_blend_with_colors(
    blend_mode: GpuRasterBlendMode,
    pixels: [u8; 4],
    dest: Rgba8,
) -> Vec<u8> {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = GpuRasterResourceKey {
        layer_id: LayerId(20),
        render_mipmap_id: 30,
    };
    let uploads = [GpuRasterUpload {
        layer_id: key.layer_id,
        render_node_id: RenderNodeId(20),
        render_mipmap_id: key.render_mipmap_id,
        size: CanvasSize::new(1, 1),
        pixels: &pixels,
    }];
    let cache = renderer
        .upload_raster_resources(&uploads)
        .expect("upload standard blend source");
    let sources = [
        GpuNormalStackSource::SolidColor {
            color: dest,
            opacity: 1.0,
        },
        GpuNormalStackSource::Raster(GpuNormalRasterSource {
            key,
            opacity: 1.0,
            mask_key: None,
            offset_x: 0,
            offset_y: 0,
            blend_mode,
        }),
    ];

    renderer
        .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
        .expect("draw standard blend source")
        .pixels
}

#[test]
fn darken_raster_source_uses_standard_blend_formula() {
    assert_eq!(
        draw_one_pixel_standard_blend(GpuRasterBlendMode::Darken),
        [100, 50, 200, 255]
    );
}

#[test]
fn multiply_raster_source_uses_w3c_blend_formula() {
    assert_eq!(
        draw_one_pixel_standard_blend(GpuRasterBlendMode::Multiply),
        [69, 45, 198, 255]
    );
}

#[test]
fn multiply_raster_source_keeps_unquantized_blend_product() {
    assert_eq!(
        draw_one_pixel_standard_blend_with_colors(
            GpuRasterBlendMode::Multiply,
            [147, 97, 187, 104],
            Rgba8 {
                r: 226,
                g: 226,
                b: 226,
                a: 255,
            },
        ),
        [187, 169, 201, 255]
    );
}

#[test]
fn linear_burn_raster_source_uses_standard_blend_formula() {
    assert_eq!(
        draw_one_pixel_standard_blend(GpuRasterBlendMode::LinearBurn),
        [50, 25, 197, 255]
    );
}

#[test]
fn darker_color_raster_source_uses_standard_blend_formula() {
    assert_eq!(
        draw_one_pixel_standard_blend(GpuRasterBlendMode::DarkerColor),
        [100, 50, 200, 255]
    );
}

#[test]
fn darker_color_raster_source_uses_rec709_luma_compare() {
    assert_eq!(
        draw_one_pixel_standard_blend_with_colors(
            GpuRasterBlendMode::DarkerColor,
            [84, 51, 250, 255],
            Rgba8 {
                r: 84,
                g: 56,
                b: 222,
                a: 255,
            },
        ),
        [84, 51, 250, 255]
    );
}

#[test]
fn lighten_raster_source_uses_standard_blend_formula() {
    assert_eq!(
        draw_one_pixel_standard_blend(GpuRasterBlendMode::Lighten),
        [100, 125, 225, 255]
    );
}

#[test]
fn screen_raster_source_uses_standard_blend_formula() {
    assert_eq!(
        draw_one_pixel_standard_blend(GpuRasterBlendMode::Screen),
        [131, 131, 227, 255]
    );
}

#[test]
fn lighter_color_raster_source_uses_standard_blend_formula() {
    assert_eq!(
        draw_one_pixel_standard_blend(GpuRasterBlendMode::LighterColor),
        [100, 125, 225, 255]
    );
}

#[test]
fn lighter_color_raster_source_uses_rec709_luma_compare() {
    assert_eq!(
        draw_one_pixel_standard_blend_with_colors(
            GpuRasterBlendMode::LighterColor,
            [84, 51, 250, 255],
            Rgba8 {
                r: 84,
                g: 56,
                b: 222,
                a: 255,
            },
        ),
        [84, 56, 222, 255]
    );
}

#[test]
fn overlay_raster_source_uses_w3c_blend_formula() {
    assert_eq!(
        draw_one_pixel_standard_blend(GpuRasterBlendMode::Overlay),
        [89, 64, 227, 255]
    );
}

#[test]
fn hard_light_raster_source_uses_w3c_blend_formula() {
    assert_eq!(
        draw_one_pixel_standard_blend(GpuRasterBlendMode::HardLight),
        [89, 109, 227, 255]
    );
}

#[test]
fn linear_light_raster_source_uses_standard_blend_formula() {
    assert_eq!(
        draw_one_pixel_standard_blend(GpuRasterBlendMode::LinearLight),
        [72, 123, 228, 255]
    );
}

#[test]
fn pin_light_raster_source_uses_standard_blend_formula() {
    assert_eq!(
        draw_one_pixel_standard_blend(GpuRasterBlendMode::PinLight),
        [100, 98, 223, 255]
    );
}

#[test]
fn subtract_raster_source_uses_standard_blend_formula() {
    assert_eq!(
        draw_one_pixel_standard_blend(GpuRasterBlendMode::Subtract),
        [50, 25, 100, 255]
    );
}

#[test]
fn subtract_raster_source_keeps_partial_equal_channel_residue() {
    assert_eq!(
        draw_one_pixel_standard_blend_with_colors(
            GpuRasterBlendMode::Subtract,
            [252, 50, 252, 253],
            Rgba8 {
                r: 252,
                g: 80,
                b: 252,
                a: 255,
            },
        ),
        [3, 30, 3, 255]
    );
}

#[test]
fn difference_raster_source_uses_standard_blend_formula() {
    assert_eq!(
        draw_one_pixel_standard_blend(GpuRasterBlendMode::Difference),
        [50, 100, 125, 255]
    );
}

#[test]
fn exclusion_raster_source_uses_standard_blend_formula() {
    assert_eq!(
        draw_one_pixel_standard_blend(GpuRasterBlendMode::Exclusion),
        [111, 111, 129, 255]
    );
}

#[test]
fn brightness_raster_source_uses_hsl_blend_formula() {
    assert_eq!(
        draw_one_pixel_standard_blend(GpuRasterBlendMode::Brightness),
        [144, 103, 228, 255]
    );
}

#[test]
fn divide_raster_source_uses_standard_blend_formula() {
    assert_eq!(
        draw_one_pixel_standard_blend(GpuRasterBlendMode::Divide),
        [178, 57, 202, 255]
    );
}

#[test]
fn soft_light_raster_source_uses_w3c_blend_formula() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = GpuRasterResourceKey {
        layer_id: LayerId(19),
        render_mipmap_id: 29,
    };
    let pixels = [100u8, 200, 250, 128];
    let uploads = [GpuRasterUpload {
        layer_id: key.layer_id,
        render_node_id: RenderNodeId(10),
        render_mipmap_id: key.render_mipmap_id,
        size: CanvasSize::new(1, 1),
        pixels: &pixels,
    }];
    let cache = renderer
        .upload_raster_resources(&uploads)
        .expect("upload SoftLight source");
    let sources = [
        GpuNormalStackSource::SolidColor {
            color: Rgba8 {
                r: 100,
                g: 50,
                b: 200,
                a: 255,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::Raster(GpuNormalRasterSource {
            key,
            opacity: 1.0,
            mask_key: None,
            offset_x: 0,
            offset_y: 0,
            blend_mode: GpuRasterBlendMode::SoftLight,
        }),
    ];

    let output = renderer
        .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
        .expect("draw SoftLight source");

    assert_eq!(output.pixels, [93, 68, 213, 255]);
}

#[test]
fn hue_raster_source_uses_hsl_blend_formula() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = GpuRasterResourceKey {
        layer_id: LayerId(16),
        render_mipmap_id: 26,
    };
    let pixels = [100u8, 200, 250, 128];
    let uploads = [GpuRasterUpload {
        layer_id: key.layer_id,
        render_node_id: RenderNodeId(7),
        render_mipmap_id: key.render_mipmap_id,
        size: CanvasSize::new(1, 1),
        pixels: &pixels,
    }];
    let cache = renderer
        .upload_raster_resources(&uploads)
        .expect("upload Hue source");
    let sources = [
        GpuNormalStackSource::SolidColor {
            color: Rgba8 {
                r: 100,
                g: 50,
                b: 200,
                a: 255,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::Raster(GpuNormalRasterSource {
            key,
            opacity: 1.0,
            mask_key: None,
            offset_x: 0,
            offset_y: 0,
            blend_mode: GpuRasterBlendMode::Hue,
        }),
    ];

    let output = renderer
        .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
        .expect("draw Hue source");

    assert_eq!(output.pixels, [52, 77, 177, 255]);
}

#[test]
fn hue_raster_source_floors_partial_alpha_writeback() {
    assert_eq!(
        draw_one_pixel_standard_blend_with_colors(
            GpuRasterBlendMode::Hue,
            [84, 51, 250, 77],
            Rgba8 {
                r: 103,
                g: 64,
                b: 15,
                a: 255,
            },
        ),
        [93, 62, 54, 255]
    );
}

#[test]
fn saturation_raster_source_uses_hsl_blend_formula() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = GpuRasterResourceKey {
        layer_id: LayerId(17),
        render_mipmap_id: 27,
    };
    let pixels = [100u8, 200, 250, 128];
    let uploads = [GpuRasterUpload {
        layer_id: key.layer_id,
        render_node_id: RenderNodeId(8),
        render_mipmap_id: key.render_mipmap_id,
        size: CanvasSize::new(1, 1),
        pixels: &pixels,
    }];
    let cache = renderer
        .upload_raster_resources(&uploads)
        .expect("upload Saturation source");
    let sources = [
        GpuNormalStackSource::SolidColor {
            color: Rgba8 {
                r: 100,
                g: 50,
                b: 200,
                a: 255,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::Raster(GpuNormalRasterSource {
            key,
            opacity: 1.0,
            mask_key: None,
            offset_x: 0,
            offset_y: 0,
            blend_mode: GpuRasterBlendMode::Saturation,
        }),
    ];

    let output = renderer
        .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
        .expect("draw Saturation source");

    assert_eq!(output.pixels, [100, 50, 200, 255]);
}

#[test]
fn saturation_raster_source_quantizes_tiny_base_span() {
    assert_eq!(
        draw_one_pixel_standard_blend_with_colors(
            GpuRasterBlendMode::Saturation,
            [84, 51, 250, 255],
            Rgba8 {
                r: 225,
                g: 224,
                b: 226,
                a: 255,
            },
        ),
        [221, 221, 255, 255]
    );
}

#[test]
fn saturation_raster_source_ceils_min_channel_after_high_luminosity_clip_for_non_tiny_base_span() {
    assert_eq!(
        draw_one_pixel_standard_blend_with_colors(
            GpuRasterBlendMode::Saturation,
            [84, 51, 250, 255],
            Rgba8 {
                r: 202,
                g: 196,
                b: 230,
                a: 255,
            },
        ),
        [203, 192, 255, 255]
    );
}

#[test]
fn color_raster_source_uses_hsl_blend_formula() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = GpuRasterResourceKey {
        layer_id: LayerId(18),
        render_mipmap_id: 28,
    };
    let pixels = [100u8, 200, 250, 128];
    let uploads = [GpuRasterUpload {
        layer_id: key.layer_id,
        render_node_id: RenderNodeId(9),
        render_mipmap_id: key.render_mipmap_id,
        size: CanvasSize::new(1, 1),
        pixels: &pixels,
    }];
    let cache = renderer
        .upload_raster_resources(&uploads)
        .expect("upload Color source");
    let sources = [
        GpuNormalStackSource::SolidColor {
            color: Rgba8 {
                r: 100,
                g: 50,
                b: 200,
                a: 255,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::Raster(GpuNormalRasterSource {
            key,
            opacity: 1.0,
            mask_key: None,
            offset_x: 0,
            offset_y: 0,
            blend_mode: GpuRasterBlendMode::Color,
        }),
    ];

    let output = renderer
        .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
        .expect("draw Color source");

    assert_eq!(output.pixels, [52, 78, 177, 255]);
}

#[test]
fn glow_dodge_raster_source_uses_byte_domain_formula() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
    let key = GpuRasterResourceKey {
        layer_id: LayerId(12),
        render_mipmap_id: 22,
    };
    let pixels = [100u8, 200, 250, 128];
    let uploads = [GpuRasterUpload {
        layer_id: key.layer_id,
        render_node_id: RenderNodeId(3),
        render_mipmap_id: key.render_mipmap_id,
        size: CanvasSize::new(1, 1),
        pixels: &pixels,
    }];
    let cache = renderer
        .upload_raster_resources(&uploads)
        .expect("upload GlowDodge source");
    let sources = [
        GpuNormalStackSource::SolidColor {
            color: Rgba8 {
                r: 100,
                g: 50,
                b: 200,
                a: 255,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::Raster(GpuNormalRasterSource {
            key,
            opacity: 1.0,
            mask_key: None,
            offset_x: 0,
            offset_y: 0,
            blend_mode: GpuRasterBlendMode::GlowDodge,
        }),
    ];

    let output = renderer
        .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
        .expect("draw GlowDodge source");

    assert_eq!(output.pixels, [124, 82, 255, 255]);
}
