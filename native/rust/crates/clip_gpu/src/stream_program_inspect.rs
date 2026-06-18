use clip_model::CanvasSize;

use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_program::{RenderProgramStats, plan_render_program};
use crate::{GpuClippedStackSource, GpuNormalStackSource};

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
    let program = plan_render_program(provider, output_size, target_origin, target_size, sources);
    let mut stats = program.stats();
    for source in sources {
        add_nested_source_stats(
            &mut stats,
            provider,
            output_size,
            target_origin,
            target_size,
            source,
        );
    }
    stats
}

fn add_nested_source_stats<P>(
    stats: &mut RenderProgramStats,
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    source: &GpuNormalStackSource,
) where
    P: GpuNormalStackResourceProvider,
{
    match source {
        GpuNormalStackSource::ClippingRun { clipped, .. } => {
            add_nested_clipped_source_stats(
                stats,
                provider,
                output_size,
                target_origin,
                target_size,
                clipped,
            );
        }
        GpuNormalStackSource::ContainerClippingRun {
            children, clipped, ..
        } => {
            stats.add_assign(inspect_normal_stack_render_program(
                provider,
                output_size,
                target_origin,
                target_size,
                children,
            ));
            add_nested_clipped_source_stats(
                stats,
                provider,
                output_size,
                target_origin,
                target_size,
                clipped,
            );
        }
        GpuNormalStackSource::Container { children, .. }
        | GpuNormalStackSource::ThroughGroup { children, .. } => {
            stats.add_assign(inspect_normal_stack_render_program(
                provider,
                output_size,
                target_origin,
                target_size,
                children,
            ));
        }
        GpuNormalStackSource::Raster(_)
        | GpuNormalStackSource::SolidColor { .. }
        | GpuNormalStackSource::LutFilter { .. } => {}
    }
}

fn add_nested_clipped_source_stats<P>(
    stats: &mut RenderProgramStats,
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    clipped: &[GpuClippedStackSource],
) where
    P: GpuNormalStackResourceProvider,
{
    for clipped_source in clipped {
        if let GpuClippedStackSource::Container { children, .. } = clipped_source {
            stats.add_assign(inspect_normal_stack_render_program(
                provider,
                output_size,
                target_origin,
                target_size,
                children,
            ));
        }
    }
}
