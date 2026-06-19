use clip_model::CanvasSize;

use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_program::{
    BarrierProgramKind, RenderProgramStats, RenderSegment, RenderSegmentKind, TileProgramKind,
    plan_render_program,
};
use crate::stream_program_barriers::RenderProgramBarrierReason;
use crate::{GpuClippedStackSource, GpuNormalStackSource};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderProgramInspection {
    pub stats: RenderProgramStats,
    pub segments: Vec<RenderProgramSegmentInfo>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderProgramSegmentInfo {
    pub ordinal: u32,
    pub depth: u16,
    pub source_start: u32,
    pub source_end: u32,
    pub kind: &'static str,
    pub barrier_reason: Option<RenderProgramBarrierReason>,
    pub expected_passes: u32,
    pub tile_events: u32,
    pub legacy_sources: u32,
}

pub fn inspect_normal_stack_render_program<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    sources: &[GpuNormalStackSource],
) -> RenderProgramStats
where
    P: GpuNormalStackResourceProvider,
{
    inspect_normal_stack_render_program_detail(
        provider,
        output_size,
        target_origin,
        target_size,
        sources,
    )
    .stats
}

pub fn inspect_normal_stack_render_program_detail<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    sources: &[GpuNormalStackSource],
) -> RenderProgramInspection
where
    P: GpuNormalStackResourceProvider,
{
    let mut next_ordinal = 0u32;
    inspect_stack_render_program_detail(
        provider,
        output_size,
        target_origin,
        target_size,
        sources,
        0,
        &mut next_ordinal,
    )
}

fn inspect_stack_render_program_detail<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    sources: &[GpuNormalStackSource],
    depth: u16,
    next_ordinal: &mut u32,
) -> RenderProgramInspection
where
    P: GpuNormalStackResourceProvider,
{
    let program = plan_render_program(provider, output_size, target_origin, target_size, sources);
    let mut inspection = RenderProgramInspection {
        stats: program.stats(),
        segments: Vec::with_capacity(program.segments().len()),
    };
    for segment in program.segments() {
        inspection
            .segments
            .push(render_segment_info(segment, depth, *next_ordinal));
        *next_ordinal = next_ordinal.saturating_add(1);
    }
    for source in sources {
        add_nested_source_inspection(
            &mut inspection,
            provider,
            output_size,
            target_origin,
            target_size,
            source,
            depth.saturating_add(1),
            next_ordinal,
        );
    }
    inspection
}

fn render_segment_info(
    segment: &RenderSegment,
    depth: u16,
    ordinal: u32,
) -> RenderProgramSegmentInfo {
    let (kind, barrier_reason) = match segment.kind {
        RenderSegmentKind::TileLocal(kind) => (tile_program_kind_name(kind), None),
        RenderSegmentKind::Barrier(BarrierProgramKind::LegacySource(reason)) => {
            ("LegacySource", Some(reason))
        }
    };
    RenderProgramSegmentInfo {
        ordinal,
        depth,
        source_start: usize_to_u32(segment.source_range.start),
        source_end: usize_to_u32(segment.source_range.end),
        kind,
        barrier_reason,
        expected_passes: segment.cost_hint.expected_passes,
        tile_events: segment.cost_hint.tile_events,
        legacy_sources: segment.cost_hint.legacy_sources,
    }
}

fn add_nested_source_inspection<P>(
    inspection: &mut RenderProgramInspection,
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    source: &GpuNormalStackSource,
    depth: u16,
    next_ordinal: &mut u32,
) where
    P: GpuNormalStackResourceProvider,
{
    match source {
        GpuNormalStackSource::ClippingRun { clipped, .. } => {
            add_nested_clipped_source_inspection(
                inspection,
                provider,
                output_size,
                target_origin,
                target_size,
                clipped,
                depth,
                next_ordinal,
            );
        }
        GpuNormalStackSource::ContainerClippingRun {
            children, clipped, ..
        } => {
            add_nested_stack_inspection(
                inspection,
                provider,
                output_size,
                target_origin,
                target_size,
                children,
                depth,
                next_ordinal,
            );
            add_nested_clipped_source_inspection(
                inspection,
                provider,
                output_size,
                target_origin,
                target_size,
                clipped,
                depth,
                next_ordinal,
            );
        }
        GpuNormalStackSource::Container { children, .. }
        | GpuNormalStackSource::ThroughGroup { children, .. } => {
            add_nested_stack_inspection(
                inspection,
                provider,
                output_size,
                target_origin,
                target_size,
                children,
                depth,
                next_ordinal,
            );
        }
        GpuNormalStackSource::Raster(_)
        | GpuNormalStackSource::SolidColor { .. }
        | GpuNormalStackSource::LutFilter { .. } => {}
    }
}

fn add_nested_clipped_source_inspection<P>(
    inspection: &mut RenderProgramInspection,
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    clipped: &[GpuClippedStackSource],
    depth: u16,
    next_ordinal: &mut u32,
) where
    P: GpuNormalStackResourceProvider,
{
    for clipped_source in clipped {
        if let GpuClippedStackSource::Container { children, .. } = clipped_source {
            add_nested_stack_inspection(
                inspection,
                provider,
                output_size,
                target_origin,
                target_size,
                children,
                depth,
                next_ordinal,
            );
        }
    }
}

fn add_nested_stack_inspection<P>(
    inspection: &mut RenderProgramInspection,
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    children: &[GpuNormalStackSource],
    depth: u16,
    next_ordinal: &mut u32,
) where
    P: GpuNormalStackResourceProvider,
{
    let nested = inspect_stack_render_program_detail(
        provider,
        output_size,
        target_origin,
        target_size,
        children,
        depth,
        next_ordinal,
    );
    inspection.stats.add_assign(nested.stats);
    inspection.segments.extend(nested.segments);
}

fn tile_program_kind_name(kind: TileProgramKind) -> &'static str {
    match kind {
        TileProgramKind::RasterRun => "RasterRun",
        TileProgramKind::RasterClippingRun => "RasterClippingRun",
        TileProgramKind::RasterFilterRun => "RasterFilterRun",
        TileProgramKind::PointFilterRun => "PointFilterRun",
        TileProgramKind::SimpleContainerScope => "SimpleContainerScope",
        TileProgramKind::SimpleThroughScope => "SimpleThroughScope",
    }
}

fn usize_to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
