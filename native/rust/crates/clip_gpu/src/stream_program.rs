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
