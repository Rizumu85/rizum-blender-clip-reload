use clip_model::CanvasSize;

use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_clipping_tile_silo::clipping_run_silo_is_eligible;
use crate::stream_program::{SegmentCostHint, TileProgramKind};
use crate::stream_program_barriers::{RenderProgramBarrierReason, barrier_reason_for_source};
use crate::stream_tile_filter_silo::{point_filter_silo_run_len, raster_filter_silo_run_len};
use crate::stream_tile_scope_silo_plan::{
    simple_container_scope_event_count, simple_through_scope_event_count,
};
use crate::stream_tile_silo::raster_silo_run_len;
use crate::{GpuClippedStackSource, GpuNormalStackSource};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LoweringDecision {
    TileLocal(TileLocalLowering),
    Barrier(BarrierLowering),
}

impl LoweringDecision {
    pub(crate) fn source_len(self) -> usize {
        match self {
            Self::TileLocal(lowering) => lowering.source_len,
            Self::Barrier(lowering) => lowering.source_len,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TileLocalLowering {
    pub(crate) source_len: usize,
    pub(crate) kind: TileProgramKind,
    pub(crate) reason: TileLocalReason,
    pub(crate) cost_hint: SegmentCostHint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TileLocalReason {
    RasterRun,
    RasterOnlyClippingRun,
    RasterFilterRun,
    PointFilterRun,
    SimpleContainerScope,
    SimpleThroughScope,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BarrierLowering {
    pub(crate) source_len: usize,
    pub(crate) reason: RenderProgramBarrierReason,
    pub(crate) cost_hint: SegmentCostHint,
}

pub(crate) fn lower_source_range<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    sources: &[GpuNormalStackSource],
) -> LoweringDecision
where
    P: GpuNormalStackResourceProvider,
{
    debug_assert!(!sources.is_empty());
    if let GpuNormalStackSource::ClippingRun { base, clipped } = &sources[0]
        && clipping_run_silo_is_eligible(
            provider,
            output_size,
            target_origin,
            target_size,
            *base,
            clipped,
        )
    {
        return LoweringDecision::TileLocal(TileLocalLowering {
            source_len: 1,
            kind: TileProgramKind::RasterClippingRun,
            reason: TileLocalReason::RasterOnlyClippingRun,
            cost_hint: tile_cost_hint(raster_clipping_tile_event_count(clipped)),
        });
    }

    if let Some(event_count) = simple_container_scope_event_count(
        provider,
        output_size,
        target_origin,
        target_size,
        &sources[0],
    ) {
        return LoweringDecision::TileLocal(TileLocalLowering {
            source_len: 1,
            kind: TileProgramKind::SimpleContainerScope,
            reason: TileLocalReason::SimpleContainerScope,
            cost_hint: tile_cost_hint(u32::try_from(event_count).unwrap_or(u32::MAX)),
        });
    }

    if let Some(event_count) = simple_through_scope_event_count(
        provider,
        output_size,
        target_origin,
        target_size,
        &sources[0],
    ) {
        return LoweringDecision::TileLocal(TileLocalLowering {
            source_len: 1,
            kind: TileProgramKind::SimpleThroughScope,
            reason: TileLocalReason::SimpleThroughScope,
            cost_hint: tile_cost_hint(u32::try_from(event_count).unwrap_or(u32::MAX)),
        });
    }

    let raster_filter_run_len =
        raster_filter_silo_run_len(provider, output_size, target_origin, target_size, sources);
    if raster_filter_run_len >= 2 {
        return LoweringDecision::TileLocal(TileLocalLowering {
            source_len: raster_filter_run_len,
            kind: TileProgramKind::RasterFilterRun,
            reason: TileLocalReason::RasterFilterRun,
            cost_hint: tile_cost_hint(u32::try_from(raster_filter_run_len).unwrap_or(u32::MAX)),
        });
    }

    let point_filter_run_len =
        point_filter_silo_run_len(provider, target_origin, target_size, sources);
    if point_filter_run_len > 0 {
        return LoweringDecision::TileLocal(TileLocalLowering {
            source_len: point_filter_run_len,
            kind: TileProgramKind::PointFilterRun,
            reason: TileLocalReason::PointFilterRun,
            cost_hint: tile_cost_hint(u32::try_from(point_filter_run_len).unwrap_or(u32::MAX)),
        });
    }

    let raster_run_len =
        raster_silo_run_len(provider, output_size, target_origin, target_size, sources);
    if raster_run_len > 0 {
        return LoweringDecision::TileLocal(TileLocalLowering {
            source_len: raster_run_len,
            kind: TileProgramKind::RasterRun,
            reason: TileLocalReason::RasterRun,
            cost_hint: tile_cost_hint(u32::try_from(raster_run_len).unwrap_or(u32::MAX)),
        });
    }

    let reason = barrier_reason_for_source(
        provider,
        output_size,
        target_origin,
        target_size,
        &sources[0],
    );
    LoweringDecision::Barrier(BarrierLowering {
        source_len: 1,
        reason,
        cost_hint: SegmentCostHint {
            expected_passes: 1,
            tile_events: 0,
            legacy_sources: 1,
        },
    })
}

fn tile_cost_hint(tile_events: u32) -> SegmentCostHint {
    SegmentCostHint {
        expected_passes: 1,
        tile_events,
        legacy_sources: 0,
    }
}

fn raster_clipping_tile_event_count(clipped: &[GpuClippedStackSource]) -> u32 {
    let clipped_rasters = clipped
        .iter()
        .filter(|source| matches!(source, GpuClippedStackSource::Raster(_)))
        .count();
    u32::try_from(clipped_rasters.saturating_add(1)).unwrap_or(u32::MAX)
}
