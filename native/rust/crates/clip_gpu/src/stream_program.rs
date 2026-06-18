use std::ops::Range;

use clip_model::CanvasSize;

use crate::GpuNormalStackSource;
use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_program_barriers::{RenderProgramBarrierCounts, RenderProgramBarrierReason};
use crate::stream_program_lowering::{
    BarrierLowering, LoweringDecision, TileLocalLowering, lower_source_range,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RenderProgram {
    segments: Vec<RenderSegment>,
    stats: RenderProgramStats,
}

impl RenderProgram {
    pub(crate) fn segments(&self) -> &[RenderSegment] {
        &self.segments
    }

    pub(crate) fn stats(&self) -> RenderProgramStats {
        self.stats
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RenderSegment {
    pub(crate) source_range: Range<usize>,
    pub(crate) kind: RenderSegmentKind,
    pub(crate) cost_hint: SegmentCostHint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RenderSegmentKind {
    TileLocal(TileProgramKind),
    Barrier(BarrierProgramKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TileProgramKind {
    RasterRun,
    RasterClippingRun,
    RasterFilterRun,
    PointFilterRun,
    SimpleContainerScope,
    SimpleThroughScope,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BarrierProgramKind {
    LegacySource(RenderProgramBarrierReason),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct SegmentCostHint {
    pub(crate) expected_passes: u32,
    pub(crate) tile_events: u32,
    pub(crate) legacy_sources: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderProgramStats {
    pub segments: u32,
    pub tile_local_segments: u32,
    pub barrier_segments: u32,
    pub raster_run_segments: u32,
    pub raster_clipping_run_segments: u32,
    pub raster_filter_run_segments: u32,
    pub point_filter_run_segments: u32,
    pub simple_container_scope_segments: u32,
    pub simple_through_scope_segments: u32,
    pub legacy_source_segments: u32,
    pub planned_tile_events: u32,
    pub planned_passes: u32,
    pub barrier_reasons: RenderProgramBarrierCounts,
}

impl RenderProgramStats {
    pub(crate) fn add_assign(&mut self, other: Self) {
        self.segments = self.segments.saturating_add(other.segments);
        self.tile_local_segments = self
            .tile_local_segments
            .saturating_add(other.tile_local_segments);
        self.barrier_segments = self.barrier_segments.saturating_add(other.barrier_segments);
        self.raster_run_segments = self
            .raster_run_segments
            .saturating_add(other.raster_run_segments);
        self.raster_clipping_run_segments = self
            .raster_clipping_run_segments
            .saturating_add(other.raster_clipping_run_segments);
        self.raster_filter_run_segments = self
            .raster_filter_run_segments
            .saturating_add(other.raster_filter_run_segments);
        self.point_filter_run_segments = self
            .point_filter_run_segments
            .saturating_add(other.point_filter_run_segments);
        self.simple_container_scope_segments = self
            .simple_container_scope_segments
            .saturating_add(other.simple_container_scope_segments);
        self.simple_through_scope_segments = self
            .simple_through_scope_segments
            .saturating_add(other.simple_through_scope_segments);
        self.legacy_source_segments = self
            .legacy_source_segments
            .saturating_add(other.legacy_source_segments);
        self.planned_tile_events = self
            .planned_tile_events
            .saturating_add(other.planned_tile_events);
        self.planned_passes = self.planned_passes.saturating_add(other.planned_passes);
        self.barrier_reasons.add_counts(other.barrier_reasons);
    }
}

pub(crate) fn plan_render_program<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    sources: &[GpuNormalStackSource],
) -> RenderProgram
where
    P: GpuNormalStackResourceProvider,
{
    let mut segments = Vec::new();
    let mut stats = RenderProgramStats::default();
    let mut source_index = 0usize;

    while source_index < sources.len() {
        let lowering = lower_source_range(
            provider,
            output_size,
            target_origin,
            target_size,
            &sources[source_index..],
        );
        let source_len = lowering.source_len();
        match lowering {
            LoweringDecision::TileLocal(tile) => push_tile_segment(
                &mut segments,
                &mut stats,
                source_index..source_index + source_len,
                tile,
            ),
            LoweringDecision::Barrier(barrier) => push_barrier_segment(
                &mut segments,
                &mut stats,
                source_index..source_index + source_len,
                barrier,
            ),
        }
        source_index += source_len;
    }

    RenderProgram { segments, stats }
}

fn push_tile_segment(
    segments: &mut Vec<RenderSegment>,
    stats: &mut RenderProgramStats,
    source_range: Range<usize>,
    lowering: TileLocalLowering,
) {
    segments.push(RenderSegment {
        source_range,
        kind: RenderSegmentKind::TileLocal(lowering.kind),
        cost_hint: lowering.cost_hint,
    });
    stats.segments += 1;
    stats.tile_local_segments += 1;
    stats.planned_passes = stats
        .planned_passes
        .saturating_add(lowering.cost_hint.expected_passes);
    stats.planned_tile_events = stats
        .planned_tile_events
        .saturating_add(lowering.cost_hint.tile_events);
    match lowering.kind {
        TileProgramKind::RasterRun => stats.raster_run_segments += 1,
        TileProgramKind::RasterClippingRun => stats.raster_clipping_run_segments += 1,
        TileProgramKind::RasterFilterRun => stats.raster_filter_run_segments += 1,
        TileProgramKind::PointFilterRun => stats.point_filter_run_segments += 1,
        TileProgramKind::SimpleContainerScope => stats.simple_container_scope_segments += 1,
        TileProgramKind::SimpleThroughScope => stats.simple_through_scope_segments += 1,
    }
}

fn push_barrier_segment(
    segments: &mut Vec<RenderSegment>,
    stats: &mut RenderProgramStats,
    source_range: Range<usize>,
    lowering: BarrierLowering,
) {
    segments.push(RenderSegment {
        source_range,
        kind: RenderSegmentKind::Barrier(BarrierProgramKind::LegacySource(lowering.reason)),
        cost_hint: lowering.cost_hint,
    });
    stats.segments += 1;
    stats.barrier_segments += 1;
    stats.legacy_source_segments += 1;
    stats.planned_passes = stats
        .planned_passes
        .saturating_add(lowering.cost_hint.expected_passes);
    stats.barrier_reasons.add(lowering.reason);
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use clip_model::{CanvasSize, LayerId, Rect};

    use super::*;
    use crate::{
        GpuClippedStackSource, GpuLutFilterMode, GpuMaskResourceCache, GpuMaskResourceKey,
        GpuNormalRasterSource, GpuRasterBlendMode, GpuRasterResourceCache, GpuRasterResourceKey,
        GpuRenderError, GpuRenderer,
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
    fn planner_lowers_container_inside_simple_container_scope() {
        let provider = PlannerProvider::new([(raster_key(1), CanvasSize::new(4, 4))]);
        let sources = vec![GpuNormalStackSource::Container {
            children: vec![GpuNormalStackSource::Container {
                children: vec![GpuNormalStackSource::Raster(raster_source(1))],
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
    fn planner_keeps_deeper_nested_container_as_barrier() {
        let provider = PlannerProvider::new([(raster_key(1), CanvasSize::new(4, 4))]);
        let sources = vec![GpuNormalStackSource::Container {
            children: vec![GpuNormalStackSource::Container {
                children: vec![GpuNormalStackSource::Container {
                    children: vec![GpuNormalStackSource::Raster(raster_source(1))],
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
            program.segments()[0].kind,
            RenderSegmentKind::Barrier(BarrierProgramKind::LegacySource(
                RenderProgramBarrierReason::IsolatedContainerRequiresIntermediate,
            ))
        );
    }

    #[test]
    fn planner_lowers_simple_through_scope() {
        let provider = PlannerProvider::new([
            (raster_key(1), CanvasSize::new(4, 4)),
            (raster_key(2), CanvasSize::new(4, 4)),
        ]);
        let sources = vec![GpuNormalStackSource::ThroughGroup {
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
            opacity: 0.5,
            mask_key: None,
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
                kind: RenderSegmentKind::TileLocal(TileProgramKind::SimpleThroughScope),
                cost_hint: SegmentCostHint {
                    expected_passes: 1,
                    tile_events: 5,
                    legacy_sources: 0,
                },
            }]
        );
        assert_eq!(program.stats().simple_through_scope_segments, 1);
        assert_eq!(program.stats().barrier_reasons.through_group_not_lowered, 0);
    }

    #[test]
    fn planner_lowers_container_inside_simple_through_scope() {
        let provider = PlannerProvider::new([(raster_key(1), CanvasSize::new(4, 4))]);
        let sources = vec![GpuNormalStackSource::ThroughGroup {
            children: vec![GpuNormalStackSource::Container {
                children: vec![GpuNormalStackSource::Raster(raster_source(1))],
                opacity: 1.0,
                mask_key: None,
                blend_mode: GpuRasterBlendMode::Normal,
            }],
            opacity: 1.0,
            mask_key: None,
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
                kind: RenderSegmentKind::TileLocal(TileProgramKind::SimpleThroughScope),
                cost_hint: SegmentCostHint {
                    expected_passes: 1,
                    tile_events: 5,
                    legacy_sources: 0,
                },
            }]
        );
        assert_eq!(program.stats().simple_through_scope_segments, 1);
        assert_eq!(program.stats().barrier_reasons.through_group_not_lowered, 0);
    }

    struct PlannerProvider {
        sizes: HashMap<GpuRasterResourceKey, CanvasSize>,
    }

    impl PlannerProvider {
        fn new<const N: usize>(sizes: [(GpuRasterResourceKey, CanvasSize); N]) -> Self {
            Self {
                sizes: sizes.into_iter().collect(),
            }
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

    fn identity_lut() -> Vec<u8> {
        let mut lut = Vec::with_capacity(256 * 4);
        for value in 0..=255u8 {
            lut.extend_from_slice(&[value, value, value, 255]);
        }
        lut
    }
}
