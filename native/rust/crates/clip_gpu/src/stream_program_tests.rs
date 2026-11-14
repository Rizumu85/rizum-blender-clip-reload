use std::collections::HashMap;

use clip_model::{CanvasSize, LayerId, Rect};

use crate::stream_program::*;
use crate::{
    GpuClippedStackSource, GpuLutFilterMode, GpuMaskResourceCache, GpuMaskResourceKey,
    GpuNormalRasterSource, GpuNormalStackResourceProvider, GpuNormalStackSource,
    GpuRasterBlendMode, GpuRasterResourceCache, GpuRasterResourceKey, GpuRenderError, GpuRenderer,
    RenderProgramBarrierCounts, RenderProgramBarrierReason,
    inspect_normal_stack_render_program_detail,
};

#[test]
fn planner_groups_tile_local_runs_before_barriers() {
    let provider = PlannerProvider::new([
        (raster_key(1), CanvasSize::new(4, 4)),
        (raster_key(2), CanvasSize::new(4, 4)),
        (raster_key(3), CanvasSize::new(4, 4)),
        (raster_key(4), CanvasSize::new(4, 4)),
        (raster_key(5), CanvasSize::new(4, 4)),
        (raster_key(6), CanvasSize::new(4, 4)),
    ]);
    let mut special = raster_source(5);
    special.blend_mode = GpuRasterBlendMode::AddGlow;
    let sources = vec![
        GpuNormalStackSource::Raster(raster_source(1)),
        GpuNormalStackSource::Raster(raster_source(2)),
        GpuNormalStackSource::SolidColor {
            color: clip_model::Rgba8 {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::ClippingRun {
            base: raster_source(3),
            clipped: vec![GpuClippedStackSource::Raster(raster_source(4))],
        },
        GpuNormalStackSource::Raster(special),
        GpuNormalStackSource::Raster(raster_source(6)),
    ];

    let program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &sources,
    );

    assert_eq!(
        program.segments(),
        &[
            RenderSegment {
                source_range: 0..2,
                kind: RenderSegmentKind::TileLocal(TileProgramKind::RasterRun),
                cost_hint: SegmentCostHint {
                    expected_passes: 1,
                    tile_events: 2,
                    legacy_sources: 0,
                },
            },
            RenderSegment {
                source_range: 2..3,
                kind: RenderSegmentKind::Barrier(BarrierProgramKind::LegacySource(
                    RenderProgramBarrierReason::SolidColorNotLowered,
                )),
                cost_hint: SegmentCostHint {
                    expected_passes: 1,
                    tile_events: 0,
                    legacy_sources: 1,
                },
            },
            RenderSegment {
                source_range: 3..4,
                kind: RenderSegmentKind::TileLocal(TileProgramKind::RasterClippingRun),
                cost_hint: SegmentCostHint {
                    expected_passes: 1,
                    tile_events: 2,
                    legacy_sources: 0,
                },
            },
            RenderSegment {
                source_range: 4..6,
                kind: RenderSegmentKind::TileLocal(TileProgramKind::RasterRun),
                cost_hint: SegmentCostHint {
                    expected_passes: 1,
                    tile_events: 2,
                    legacy_sources: 0,
                },
            },
        ]
    );
    assert_eq!(
        program.stats(),
        RenderProgramStats {
            segments: 4,
            tile_local_segments: 3,
            barrier_segments: 1,
            raster_run_segments: 2,
            raster_clipping_run_segments: 1,
            raster_filter_run_segments: 0,
            point_filter_run_segments: 0,
            simple_container_scope_segments: 0,
            simple_through_scope_segments: 0,
            legacy_source_segments: 1,
            planned_tile_events: 6,
            planned_passes: 4,
            barrier_reasons: RenderProgramBarrierCounts {
                solid_color_not_lowered: 1,
                ..RenderProgramBarrierCounts::default()
            },
        }
    );
}

#[test]
fn planner_lowers_single_eligible_raster_as_tile_local() {
    let provider = PlannerProvider::new([(raster_key(1), CanvasSize::new(4, 4))]);
    let sources = vec![GpuNormalStackSource::Raster(raster_source(1))];

    let program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &sources,
    );

    assert_eq!(
        program.segments(),
        &[RenderSegment {
            source_range: 0..1,
            kind: RenderSegmentKind::TileLocal(TileProgramKind::RasterRun),
            cost_hint: SegmentCostHint {
                expected_passes: 1,
                tile_events: 1,
                legacy_sources: 0,
            },
        }]
    );
    assert_eq!(program.stats().barrier_reasons.raster_run_too_short, 0);
}

#[test]
fn inspection_marks_raster_run_checkpoint_candidates() {
    let provider = PlannerProvider::new([
        (raster_key(1), CanvasSize::new(4, 4)),
        (raster_key(2), CanvasSize::new(4, 4)),
        (raster_key(3), CanvasSize::new(4, 4)),
    ]);
    let sources = vec![
        GpuNormalStackSource::Raster(raster_source(1)),
        GpuNormalStackSource::SolidColor {
            color: clip_model::Rgba8 {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
            opacity: 1.0,
        },
        GpuNormalStackSource::Raster(raster_source(2)),
        GpuNormalStackSource::Raster(raster_source(3)),
    ];

    let inspection = inspect_normal_stack_render_program_detail(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &sources,
    );

    assert_eq!(
        inspection
            .segments
            .iter()
            .map(|segment| (segment.kind, segment.checkpoint_before))
            .collect::<Vec<_>>(),
        vec![
            ("RasterRun", true),
            ("LegacySource", false),
            ("RasterRun", true),
        ]
    );
    assert_eq!(inspection.segments[0].checkpoint_priority, 0);
    assert_eq!(inspection.segments[1].checkpoint_priority, 0);
    assert!(inspection.segments[2].checkpoint_priority > 0);
}

#[test]
fn planner_keeps_clipped_container_as_barrier() {
    let provider = PlannerProvider::new([
        (raster_key(1), CanvasSize::new(4, 4)),
        (raster_key(2), CanvasSize::new(4, 4)),
    ]);
    let sources = vec![GpuNormalStackSource::ClippingRun {
        base: raster_source(1),
        clipped: vec![GpuClippedStackSource::Container {
            layer_id: LayerId(8),
            children: vec![GpuNormalStackSource::Raster(raster_source(2))],
            opacity: 1.0,
            mask_key: None,
            blend_mode: GpuRasterBlendMode::Normal,
        }],
    }];

    let program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &sources,
    );

    assert_eq!(
        program.segments()[0].kind,
        RenderSegmentKind::Barrier(BarrierProgramKind::LegacySource(
            RenderProgramBarrierReason::ClippedContainerSiblingNotLowered,
        ))
    );
}

#[test]
fn planner_lowers_unmasked_filter_between_rasters() {
    let provider = PlannerProvider::new([
        (raster_key(1), CanvasSize::new(4, 4)),
        (raster_key(2), CanvasSize::new(4, 4)),
    ]);
    let sources = vec![
        GpuNormalStackSource::Raster(raster_source(1)),
        GpuNormalStackSource::LutFilter {
            lut_rgba: identity_lut(),
            opacity: 1.0,
            mask_key: None,
            filter_mode: GpuLutFilterMode::ToneCurveRgb,
        },
        GpuNormalStackSource::Raster(raster_source(2)),
    ];

    let program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &sources,
    );

    assert_eq!(
        program.segments(),
        &[RenderSegment {
            source_range: 0..3,
            kind: RenderSegmentKind::TileLocal(TileProgramKind::RasterFilterRun),
            cost_hint: SegmentCostHint {
                expected_passes: 1,
                tile_events: 3,
                legacy_sources: 0,
            },
        }]
    );
    assert_eq!(program.stats().raster_filter_run_segments, 1);
    assert_eq!(program.stats().barrier_reasons.filter_not_lowered, 0);
}

#[test]
fn planner_lowers_unmasked_leading_filter_before_raster() {
    let provider = PlannerProvider::new([(raster_key(1), CanvasSize::new(4, 4))]);
    let sources = vec![
        GpuNormalStackSource::LutFilter {
            lut_rgba: identity_lut(),
            opacity: 1.0,
            mask_key: None,
            filter_mode: GpuLutFilterMode::ToneCurveRgb,
        },
        GpuNormalStackSource::Raster(raster_source(1)),
    ];

    let program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &sources,
    );

    assert_eq!(
        program.segments(),
        &[RenderSegment {
            source_range: 0..2,
            kind: RenderSegmentKind::TileLocal(TileProgramKind::RasterFilterRun),
            cost_hint: SegmentCostHint {
                expected_passes: 1,
                tile_events: 2,
                legacy_sources: 0,
            },
        }]
    );
    assert_eq!(program.stats().raster_filter_run_segments, 1);
    assert_eq!(program.stats().barrier_reasons.filter_not_lowered, 0);
}

#[test]
fn planner_lowers_filter_only_run() {
    let provider = PlannerProvider::new([]);
    let sources = vec![
        GpuNormalStackSource::LutFilter {
            lut_rgba: identity_lut(),
            opacity: 1.0,
            mask_key: None,
            filter_mode: GpuLutFilterMode::ToneCurveRgb,
        },
        GpuNormalStackSource::LutFilter {
            lut_rgba: identity_lut(),
            opacity: 0.5,
            mask_key: None,
            filter_mode: GpuLutFilterMode::ToneCurveRgb,
        },
    ];

    let program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &sources,
    );

    assert_eq!(
        program.segments(),
        &[RenderSegment {
            source_range: 0..2,
            kind: RenderSegmentKind::TileLocal(TileProgramKind::PointFilterRun),
            cost_hint: SegmentCostHint {
                expected_passes: 1,
                tile_events: 2,
                legacy_sources: 0,
            },
        }]
    );
    assert_eq!(program.stats().point_filter_run_segments, 1);
    assert_eq!(program.stats().barrier_reasons.filter_not_lowered, 0);
}

#[test]
fn planner_lowers_simple_normal_container_scope() {
    let provider = PlannerProvider::new([
        (raster_key(1), CanvasSize::new(4, 4)),
        (raster_key(2), CanvasSize::new(4, 4)),
    ]);
    let sources = vec![GpuNormalStackSource::Container {
        children: vec![
            GpuNormalStackSource::Raster(raster_source(1)),
            GpuNormalStackSource::LutFilter {
                lut_rgba: identity_lut(),
                opacity: 1.0,
                mask_key: None,
                filter_mode: GpuLutFilterMode::ToneCurveRgb,
            },
            GpuNormalStackSource::Raster(raster_source(2)),
        ],
        opacity: 1.0,
        mask_key: None,
        blend_mode: GpuRasterBlendMode::Normal,
    }];

    let program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &sources,
    );

    assert_eq!(
        program.segments(),
        &[RenderSegment {
            source_range: 0..1,
            kind: RenderSegmentKind::TileLocal(TileProgramKind::SimpleContainerScope),
            cost_hint: SegmentCostHint {
                expected_passes: 1,
                tile_events: 5,
                legacy_sources: 0,
            },
        }]
    );
    assert_eq!(program.stats().simple_container_scope_segments, 1);
    assert_eq!(
        program
            .stats()
            .barrier_reasons
            .isolated_container_requires_intermediate,
        0
    );
}

#[test]
fn planner_lowers_simple_container_scope_with_opacity_and_blend() {
    let provider = PlannerProvider::new([(raster_key(1), CanvasSize::new(4, 4))]);
    let sources = vec![GpuNormalStackSource::Container {
        children: vec![GpuNormalStackSource::Raster(raster_source(1))],
        opacity: 0.5,
        mask_key: None,
        blend_mode: GpuRasterBlendMode::Multiply,
    }];

    let program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &sources,
    );

    assert_eq!(
        program.segments(),
        &[RenderSegment {
            source_range: 0..1,
            kind: RenderSegmentKind::TileLocal(TileProgramKind::SimpleContainerScope),
            cost_hint: SegmentCostHint {
                expected_passes: 1,
                tile_events: 3,
                legacy_sources: 0,
            },
        }]
    );
    assert_eq!(program.stats().simple_container_scope_segments, 1);
    assert_eq!(
        program
            .stats()
            .barrier_reasons
            .isolated_container_requires_intermediate,
        0
    );
}

#[test]
fn planner_lowers_solid_color_inside_simple_container_scope() {
    let provider = PlannerProvider::new([]);
    let sources = vec![GpuNormalStackSource::Container {
        children: vec![GpuNormalStackSource::SolidColor {
            color: clip_model::Rgba8 {
                r: 10,
                g: 20,
                b: 30,
                a: 255,
            },
            opacity: 1.0,
        }],
        opacity: 1.0,
        mask_key: None,
        blend_mode: GpuRasterBlendMode::Normal,
    }];

    let program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &sources,
    );

    assert_eq!(
        program.segments(),
        &[RenderSegment {
            source_range: 0..1,
            kind: RenderSegmentKind::TileLocal(TileProgramKind::SimpleContainerScope),
            cost_hint: SegmentCostHint {
                expected_passes: 1,
                tile_events: 3,
                legacy_sources: 0,
            },
        }]
    );
    assert_eq!(program.stats().simple_container_scope_segments, 1);
    assert_eq!(program.stats().barrier_reasons.solid_color_not_lowered, 0);
}

#[test]
fn planner_lowers_fully_opaque_masked_simple_scopes() {
    let opaque_mask = mask_key(9);
    let provider = PlannerProvider::new([(raster_key(1), CanvasSize::new(4, 4))])
        .with_mask_opacity(opaque_mask, true);

    let container_sources = vec![GpuNormalStackSource::Container {
        children: vec![GpuNormalStackSource::Raster(raster_source(1))],
        opacity: 1.0,
        mask_key: Some(opaque_mask),
        blend_mode: GpuRasterBlendMode::Normal,
    }];
    let container_program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &container_sources,
    );

    assert_eq!(
        container_program.segments()[0].kind,
        RenderSegmentKind::TileLocal(TileProgramKind::SimpleContainerScope)
    );

    let through_sources = vec![GpuNormalStackSource::ThroughGroup {
        children: vec![GpuNormalStackSource::Raster(raster_source(1))],
        opacity: 1.0,
        mask_key: Some(opaque_mask),
    }];
    let through_program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &through_sources,
    );

    assert_eq!(
        through_program.segments()[0].kind,
        RenderSegmentKind::TileLocal(TileProgramKind::SimpleThroughScope)
    );
}

#[test]
fn planner_keeps_unknown_masked_simple_scopes_as_barriers() {
    let unknown_mask = mask_key(10);
    let provider = PlannerProvider::new([(raster_key(1), CanvasSize::new(4, 4))]);

    let container_sources = vec![GpuNormalStackSource::Container {
        children: vec![GpuNormalStackSource::Raster(raster_source(1))],
        opacity: 1.0,
        mask_key: Some(unknown_mask),
        blend_mode: GpuRasterBlendMode::Normal,
    }];
    let container_program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &container_sources,
    );

    assert_eq!(
        container_program.segments()[0].kind,
        RenderSegmentKind::Barrier(BarrierProgramKind::LegacySource(
            RenderProgramBarrierReason::ScopeMaskNotLowered,
        ))
    );
    assert_eq!(
        container_program
            .stats()
            .barrier_reasons
            .scope_mask_not_lowered,
        1
    );

    let through_sources = vec![GpuNormalStackSource::ThroughGroup {
        children: vec![GpuNormalStackSource::Raster(raster_source(1))],
        opacity: 1.0,
        mask_key: Some(unknown_mask),
    }];
    let through_program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &through_sources,
    );

    assert_eq!(
        through_program.segments()[0].kind,
        RenderSegmentKind::Barrier(BarrierProgramKind::LegacySource(
            RenderProgramBarrierReason::ScopeMaskNotLowered,
        ))
    );
    assert_eq!(
        through_program
            .stats()
            .barrier_reasons
            .scope_mask_not_lowered,
        1
    );
}

#[test]
fn planner_keeps_provider_unavailable_masked_filter_inside_scope_as_filter_barrier() {
    let filter_mask = mask_key(11);
    let provider = PlannerProvider::new([(raster_key(1), CanvasSize::new(4, 4))])
        .with_mask_opacity(filter_mask, false);
    let sources = vec![GpuNormalStackSource::Container {
        children: vec![
            GpuNormalStackSource::Raster(raster_source(1)),
            GpuNormalStackSource::LutFilter {
                lut_rgba: identity_lut(),
                opacity: 1.0,
                mask_key: Some(filter_mask),
                filter_mode: GpuLutFilterMode::ToneCurveRgb,
            },
        ],
        opacity: 1.0,
        mask_key: None,
        blend_mode: GpuRasterBlendMode::Normal,
    }];

    let program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &sources,
    );

    assert_eq!(
        program.segments()[0].kind,
        RenderSegmentKind::Barrier(BarrierProgramKind::LegacySource(
            RenderProgramBarrierReason::FilterNotLowered,
        ))
    );
    assert_eq!(program.stats().barrier_reasons.filter_not_lowered, 1);
}

#[test]
fn planner_lowers_provider_backed_masked_filter_inside_scope() {
    let filter_mask = mask_key(11);
    let provider = PlannerProvider::new([(raster_key(1), CanvasSize::new(4, 4))])
        .with_mask_opacity(filter_mask, false)
        .with_mask_atlas_tiles_supported();
    let sources = vec![GpuNormalStackSource::Container {
        children: vec![
            GpuNormalStackSource::Raster(raster_source(1)),
            GpuNormalStackSource::LutFilter {
                lut_rgba: identity_lut(),
                opacity: 1.0,
                mask_key: Some(filter_mask),
                filter_mode: GpuLutFilterMode::ToneCurveRgb,
            },
        ],
        opacity: 1.0,
        mask_key: None,
        blend_mode: GpuRasterBlendMode::Normal,
    }];

    let program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &sources,
    );

    assert_eq!(
        program.segments()[0].kind,
        RenderSegmentKind::TileLocal(TileProgramKind::SimpleContainerScope)
    );
    assert_eq!(program.stats().simple_container_scope_segments, 1);
    assert_eq!(program.stats().barrier_reasons.filter_not_lowered, 0);
}

/// `(name, scope builder, nesting depth, lowered tile events)`: a single raster
/// wrapped `depth` times in the given scope kind. `Some(n)` expects one lowered
/// `SimpleContainerScope` segment with `n` tile events; `None` expects the
/// planner to keep the stack as a `ScopeDepthLimitExceeded` barrier.
type ScopeDepthCase = (&'static str, ScopeBuilder, usize, Option<u32>);

const SCOPE_DEPTH_CASES: &[ScopeDepthCase] = &[
    (
        "container_inside_simple_container_scope",
        container,
        2,
        Some(5),
    ),
    ("three_deep_container_scope", container, 3, Some(7)),
    ("four_deep_container_scope", container, 4, Some(9)),
    ("five_deep_container_scope_as_barrier", container, 5, None),
    (
        "through_beyond_scope_depth_limit_as_barrier",
        through,
        3,
        None,
    ),
];

#[test]
fn planner_lowers_nested_scopes_up_to_depth_limit() {
    for &(name, wrap, depth, lowered_tile_events) in SCOPE_DEPTH_CASES {
        let provider = PlannerProvider::new([(raster_key(1), CanvasSize::new(4, 4))]);
        let mut source = raster(1);
        for _ in 0..depth {
            source = wrap(vec![source], 1.0);
        }
        let sources = vec![source];

        let program = plan_sixteen(&provider, &sources);
        let reasons = program.stats().barrier_reasons;

        match lowered_tile_events {
            Some(tile_events) => {
                assert_eq!(
                    program.segments(),
                    &[tile_local_segment(
                        TileProgramKind::SimpleContainerScope,
                        tile_events
                    )],
                    "{name}"
                );
                assert_eq!(program.stats().simple_container_scope_segments, 1, "{name}");
                assert_eq!(
                    reasons.isolated_container_requires_intermediate, 0,
                    "{name}"
                );
                assert_eq!(reasons.scope_depth_limit_exceeded, 0, "{name}");
            }
            None => {
                assert_eq!(
                    program.segments()[0].kind,
                    RenderSegmentKind::Barrier(BarrierProgramKind::LegacySource(
                        RenderProgramBarrierReason::ScopeDepthLimitExceeded,
                    )),
                    "{name}"
                );
                assert_eq!(reasons.scope_depth_limit_exceeded, 1, "{name}");
            }
        }
    }
}

#[test]
fn planner_lowers_direct_through_inside_simple_container_scope() {
    let provider = PlannerProvider::new([(raster_key(1), CanvasSize::new(4, 4))]);
    let sources = vec![GpuNormalStackSource::Container {
        children: vec![GpuNormalStackSource::ThroughGroup {
            children: vec![GpuNormalStackSource::Raster(raster_source(1))],
            opacity: 0.5,
            mask_key: None,
        }],
        opacity: 1.0,
        mask_key: None,
        blend_mode: GpuRasterBlendMode::Normal,
    }];

    let program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &sources,
    );

    assert_eq!(
        program.segments(),
        &[RenderSegment {
            source_range: 0..1,
            kind: RenderSegmentKind::TileLocal(TileProgramKind::SimpleContainerScope),
            cost_hint: SegmentCostHint {
                expected_passes: 1,
                tile_events: 5,
                legacy_sources: 0,
            },
        }]
    );
    assert_eq!(program.stats().simple_container_scope_segments, 1);
    assert_eq!(program.stats().barrier_reasons.through_group_not_lowered, 0);
}

#[test]
fn planner_lowers_through_inside_nested_container_scope() {
    let provider = PlannerProvider::new([(raster_key(1), CanvasSize::new(4, 4))]);
    let sources = vec![GpuNormalStackSource::Container {
        children: vec![GpuNormalStackSource::Container {
            children: vec![GpuNormalStackSource::ThroughGroup {
                children: vec![GpuNormalStackSource::Raster(raster_source(1))],
                opacity: 0.5,
                mask_key: None,
            }],
            opacity: 1.0,
            mask_key: None,
            blend_mode: GpuRasterBlendMode::Normal,
        }],
        opacity: 1.0,
        mask_key: None,
        blend_mode: GpuRasterBlendMode::Normal,
    }];

    let program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &sources,
    );

    assert_eq!(
        program.segments(),
        &[RenderSegment {
            source_range: 0..1,
            kind: RenderSegmentKind::TileLocal(TileProgramKind::SimpleContainerScope),
            cost_hint: SegmentCostHint {
                expected_passes: 1,
                tile_events: 7,
                legacy_sources: 0,
            },
        }]
    );
    assert_eq!(program.stats().simple_container_scope_segments, 1);
    assert_eq!(program.stats().barrier_reasons.through_group_not_lowered, 0);
}

#[test]
fn planner_lowers_clipping_run_inside_simple_container_scope() {
    let provider = PlannerProvider::new([
        (raster_key(1), CanvasSize::new(4, 4)),
        (raster_key(2), CanvasSize::new(4, 4)),
    ]);
    let sources = vec![GpuNormalStackSource::Container {
        children: vec![GpuNormalStackSource::ClippingRun {
            base: raster_source(1),
            clipped: vec![GpuClippedStackSource::Raster(raster_source(2))],
        }],
        opacity: 1.0,
        mask_key: None,
        blend_mode: GpuRasterBlendMode::Normal,
    }];

    let program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &sources,
    );

    assert_eq!(
        program.segments(),
        &[RenderSegment {
            source_range: 0..1,
            kind: RenderSegmentKind::TileLocal(TileProgramKind::SimpleContainerScope),
            cost_hint: SegmentCostHint {
                expected_passes: 1,
                tile_events: 6,
                legacy_sources: 0,
            },
        }]
    );
    assert_eq!(program.stats().simple_container_scope_segments, 1);
}

/// `(name, clipped container children, lowered tile events)` for the stack
/// `Container { ClippingRun { base: raster 1, clipped: [Container(layer 8) {
/// children }] } }`, expected to lower into one `SimpleContainerScope` segment.
/// Rasters `1..=5` are registered with the planner provider for every row.
type ClippedContainerSiblingCase = (&'static str, ChildrenBuilder, u32);

const CLIPPED_CONTAINER_SIBLING_CASES: &[ClippedContainerSiblingCase] = &[
    ("inside_simple_container_scope", || vec![raster(2)], 8),
    ("with_point_filter", || vec![raster(2), lut_filter()], 9),
    (
        "with_nested_container",
        || vec![container(vec![raster(2)], 1.0)],
        10,
    ),
    ("with_child_clipping_run", || vec![clipping(2, 3)], 11),
    (
        "with_through_child",
        || vec![through(vec![raster(2)], 0.5)],
        10,
    ),
    (
        "with_nested_through_child",
        || vec![through(vec![through(vec![raster(2)], 0.75)], 0.5)],
        12,
    ),
    (
        "with_through_child_then_filter",
        || vec![through(vec![raster(2)], 0.5), lut_filter()],
        11,
    ),
    (
        "with_through_child_clipping_run",
        || vec![through(vec![clipping(2, 3)], 0.5)],
        13,
    ),
    NESTED_CONTAINER_THROUGH_CHILD_CASE,
    (
        "mixed_with_child_clipping_run",
        || vec![container(vec![raster(2)], 1.0), clipping(3, 4)],
        14,
    ),
];

/// Nested THROUGH regression guard; also runs as a row of the table above.
const NESTED_CONTAINER_THROUGH_CHILD_CASE: ClippedContainerSiblingCase = (
    "with_nested_container_through_child",
    || vec![container(vec![through(vec![raster(2)], 0.5)], 0.75)],
    12,
);

fn assert_clipped_container_sibling_lowers(
    &(name, children, tile_events): &ClippedContainerSiblingCase,
) {
    let provider = planner_provider_with_rasters(5);
    let sources = vec![container(
        vec![GpuNormalStackSource::ClippingRun {
            base: raster_source(1),
            clipped: vec![GpuClippedStackSource::Container {
                layer_id: LayerId(8),
                children: children(),
                opacity: 1.0,
                mask_key: None,
                blend_mode: GpuRasterBlendMode::Normal,
            }],
        }],
        1.0,
    )];

    let program = plan_sixteen(&provider, &sources);

    assert_eq!(
        program.segments(),
        &[tile_local_segment(
            TileProgramKind::SimpleContainerScope,
            tile_events
        )],
        "{name}"
    );
    assert_eq!(program.stats().simple_container_scope_segments, 1, "{name}");
}

#[test]
fn planner_lowers_clipped_container_siblings_inside_simple_container_scope() {
    for case in CLIPPED_CONTAINER_SIBLING_CASES {
        assert_clipped_container_sibling_lowers(case);
    }
}

#[test]
fn planner_lowers_clipped_container_sibling_with_nested_container_through_child() {
    assert_clipped_container_sibling_lowers(&NESTED_CONTAINER_THROUGH_CHILD_CASE);
}

/// `(name, through-group children, through-group opacity, lowered tile events)`
/// for a top-level `ThroughGroup` expected to lower into one
/// `SimpleThroughScope` segment. Rasters `1..=5` are registered for every row.
type ThroughScopeCase = (&'static str, ChildrenBuilder, f32, u32);

const THROUGH_SCOPE_CASES: &[ThroughScopeCase] = &[
    ("direct_clipping_run", || vec![clipping(1, 2)], 0.5, 6),
    (
        "nested_container_clipping_run",
        || vec![container(vec![clipping(1, 2)], 1.0)],
        0.5,
        8,
    ),
    (
        "nested_through_direct_clipping_run",
        || vec![through(vec![clipping(1, 2)], 1.0)],
        0.5,
        8,
    ),
    (
        "raster_filter_raster",
        || vec![raster(1), lut_filter(), raster(2)],
        0.5,
        5,
    ),
    (
        "container_child",
        || vec![container(vec![raster(1)], 1.0)],
        1.0,
        5,
    ),
    (
        "nested_through_sibling",
        || vec![raster(1), through(vec![raster(2)], 1.0)],
        1.0,
        6,
    ),
    (
        "fractional_nested_through",
        || vec![through(vec![raster(1)], 0.5)],
        1.0,
        5,
    ),
];

#[test]
fn planner_lowers_simple_through_scopes() {
    for &(name, children, opacity, tile_events) in THROUGH_SCOPE_CASES {
        let provider = planner_provider_with_rasters(5);
        let sources = vec![through(children(), opacity)];

        let program = plan_sixteen(&provider, &sources);

        assert_eq!(
            program.segments(),
            &[tile_local_segment(
                TileProgramKind::SimpleThroughScope,
                tile_events
            )],
            "{name}"
        );
        assert_eq!(program.stats().simple_through_scope_segments, 1, "{name}");
        assert_eq!(
            program.stats().barrier_reasons.through_group_not_lowered,
            0,
            "{name}"
        );
    }
}

#[test]
fn planner_keeps_through_child_container_as_legacy_barrier() {
    let provider = PlannerProvider::new([(raster_key(1), CanvasSize::new(4, 4))]);
    let sources = vec![GpuNormalStackSource::Container {
        children: vec![GpuNormalStackSource::Container {
            children: vec![GpuNormalStackSource::Container {
                children: vec![GpuNormalStackSource::ThroughGroup {
                    children: vec![GpuNormalStackSource::Container {
                        children: vec![GpuNormalStackSource::Raster(raster_source(1))],
                        opacity: 1.0,
                        mask_key: None,
                        blend_mode: GpuRasterBlendMode::Normal,
                    }],
                    opacity: 0.5,
                    mask_key: None,
                }],
                opacity: 1.0,
                mask_key: None,
                blend_mode: GpuRasterBlendMode::Normal,
            }],
            opacity: 1.0,
            mask_key: None,
            blend_mode: GpuRasterBlendMode::Normal,
        }],
        opacity: 1.0,
        mask_key: None,
        blend_mode: GpuRasterBlendMode::Normal,
    }];

    let program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &sources,
    );

    assert_eq!(
        program.segments(),
        &[RenderSegment {
            source_range: 0..1,
            kind: RenderSegmentKind::Barrier(BarrierProgramKind::LegacySource(
                RenderProgramBarrierReason::IsolatedContainerRequiresIntermediate,
            )),
            cost_hint: SegmentCostHint {
                expected_passes: 1,
                tile_events: 0,
                legacy_sources: 1,
            },
        }]
    );
    assert_eq!(program.stats().simple_container_scope_segments, 0);
    assert_eq!(program.stats().barrier_segments, 1);
    assert_eq!(
        program
            .stats()
            .barrier_reasons
            .isolated_container_requires_intermediate,
        1
    );
}

#[test]
fn planner_reports_scope_tile_event_limit_as_barrier() {
    let ids: Vec<u32> = (1..=255).collect();
    let provider = PlannerProvider::from_sizes(
        ids.iter()
            .copied()
            .map(|id| (raster_key(id), CanvasSize::new(4, 4))),
    );
    let sources = vec![GpuNormalStackSource::Container {
        children: ids
            .iter()
            .copied()
            .map(raster_source)
            .map(GpuNormalStackSource::Raster)
            .collect(),
        opacity: 1.0,
        mask_key: None,
        blend_mode: GpuRasterBlendMode::Normal,
    }];

    let program = plan_render_program(
        &provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        &sources,
    );

    assert_eq!(
        program.segments()[0].kind,
        RenderSegmentKind::Barrier(BarrierProgramKind::LegacySource(
            RenderProgramBarrierReason::TileEventLimitExceeded,
        ))
    );
    assert_eq!(program.stats().barrier_reasons.tile_event_limit_exceeded, 1);
}

struct PlannerProvider {
    sizes: HashMap<GpuRasterResourceKey, CanvasSize>,
    opaque_masks: HashMap<GpuMaskResourceKey, bool>,
    mask_atlas_tiles_supported: bool,
}

impl PlannerProvider {
    fn new<const N: usize>(sizes: [(GpuRasterResourceKey, CanvasSize); N]) -> Self {
        Self::from_sizes(sizes)
    }

    fn from_sizes(sizes: impl IntoIterator<Item = (GpuRasterResourceKey, CanvasSize)>) -> Self {
        Self {
            sizes: sizes.into_iter().collect(),
            opaque_masks: HashMap::new(),
            mask_atlas_tiles_supported: false,
        }
    }

    fn with_mask_opacity(mut self, key: GpuMaskResourceKey, opaque: bool) -> Self {
        self.opaque_masks.insert(key, opaque);
        self
    }

    fn with_mask_atlas_tiles_supported(mut self) -> Self {
        self.mask_atlas_tiles_supported = true;
        self
    }
}

impl GpuNormalStackResourceProvider for PlannerProvider {
    type Error = GpuRenderError;

    fn raster_resource(
        &mut self,
        _renderer: &GpuRenderer,
        _source: GpuNormalRasterSource,
    ) -> Result<GpuRasterResourceCache, Self::Error> {
        unimplemented!("planner tests do not upload raster resources")
    }

    fn raster_resource_region(
        &mut self,
        _renderer: &GpuRenderer,
        _source: GpuNormalRasterSource,
        _render_bounds: Rect,
    ) -> Result<GpuRasterResourceCache, Self::Error> {
        unimplemented!("planner tests do not upload raster resources")
    }

    fn raster_resource_size(&self, source: GpuNormalRasterSource) -> Option<CanvasSize> {
        self.sizes.get(&source.key).copied()
    }

    fn raster_run_atlas_supports_masks(&self) -> bool {
        true
    }

    fn mask_is_fully_opaque(&self, key: GpuMaskResourceKey) -> Option<bool> {
        self.opaque_masks.get(&key).copied()
    }

    fn mask_atlas_tiles_supported(&self) -> bool {
        self.mask_atlas_tiles_supported
    }

    fn mask_resource(
        &mut self,
        _renderer: &GpuRenderer,
        _key: GpuMaskResourceKey,
    ) -> Result<GpuMaskResourceCache, Self::Error> {
        unimplemented!("planner tests do not upload mask resources")
    }
}

fn raster_source(id: u32) -> GpuNormalRasterSource {
    GpuNormalRasterSource {
        key: raster_key(id),
        opacity: 1.0,
        mask_key: None,
        offset_x: 0,
        offset_y: 0,
        blend_mode: GpuRasterBlendMode::Normal,
    }
}

fn raster_key(id: u32) -> GpuRasterResourceKey {
    GpuRasterResourceKey {
        layer_id: LayerId(id),
        render_mipmap_id: id,
    }
}

fn mask_key(id: u32) -> GpuMaskResourceKey {
    GpuMaskResourceKey {
        layer_id: LayerId(id),
        mask_mipmap_id: id,
    }
}

fn identity_lut() -> Vec<u8> {
    let mut lut = Vec::with_capacity(256 * 4);
    for value in 0..=255u8 {
        lut.extend_from_slice(&[value, value, value, 255]);
    }
    lut
}

type ChildrenBuilder = fn() -> Vec<GpuNormalStackSource>;
type ScopeBuilder = fn(Vec<GpuNormalStackSource>, f32) -> GpuNormalStackSource;

fn planner_provider_with_rasters(count: u32) -> PlannerProvider {
    PlannerProvider::from_sizes((1..=count).map(|id| (raster_key(id), CanvasSize::new(4, 4))))
}

/// Plans a 16x16 canvas rendered into a 16x16 target at the origin, the
/// configuration shared by every table-driven planner case in this file.
fn plan_sixteen(provider: &PlannerProvider, sources: &[GpuNormalStackSource]) -> RenderProgram {
    plan_render_program(
        provider,
        CanvasSize::new(16, 16),
        (0, 0),
        CanvasSize::new(16, 16),
        sources,
    )
}

fn tile_local_segment(kind: TileProgramKind, tile_events: u32) -> RenderSegment {
    RenderSegment {
        source_range: 0..1,
        kind: RenderSegmentKind::TileLocal(kind),
        cost_hint: SegmentCostHint {
            expected_passes: 1,
            tile_events,
            legacy_sources: 0,
        },
    }
}

fn raster(id: u32) -> GpuNormalStackSource {
    GpuNormalStackSource::Raster(raster_source(id))
}

fn container(children: Vec<GpuNormalStackSource>, opacity: f32) -> GpuNormalStackSource {
    GpuNormalStackSource::Container {
        children,
        opacity,
        mask_key: None,
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

fn clipping(base_id: u32, clipped_id: u32) -> GpuNormalStackSource {
    GpuNormalStackSource::ClippingRun {
        base: raster_source(base_id),
        clipped: vec![GpuClippedStackSource::Raster(raster_source(clipped_id))],
    }
}

fn lut_filter() -> GpuNormalStackSource {
    GpuNormalStackSource::LutFilter {
        lut_rgba: identity_lut(),
        opacity: 1.0,
        mask_key: None,
        filter_mode: GpuLutFilterMode::ToneCurveRgb,
    }
}
