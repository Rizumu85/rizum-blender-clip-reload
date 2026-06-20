use std::time::Instant;

use crate::GpuNormalStackSource;
use crate::render_profile;
use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::CanvasRect;
use crate::stream_clipping_tile_silo::encode_clipping_run_silo_with_provider;
use crate::stream_context::StreamingExecutionContext;
use crate::stream_legacy_segment::encode_legacy_segment;
use crate::stream_program::{
    BarrierProgramKind, RenderSegmentKind, TileProgramKind, plan_render_program,
};
use crate::stream_segment_profile::{
    SegmentAtlasStatus, record_profiled_segment, record_profiled_segment_with_atlas_status,
};
use crate::stream_state::StreamingTexturePair;
use crate::stream_tile_filter_silo::{
    encode_point_filter_silo_run_with_provider, encode_raster_filter_silo_run_with_provider,
};
use crate::stream_tile_scope_silo::{
    encode_simple_container_scope_silo_with_provider,
    encode_simple_through_scope_silo_with_provider,
};
use crate::stream_tile_silo::encode_raster_silo_run_with_provider;

pub(crate) fn encode_source_sequence_with_provider<P>(
    context: &mut StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    sources: &[GpuNormalStackSource],
    texture_pair: &StreamingTexturePair,
    mut previous_index: usize,
    mut next_index: usize,
    fallback_texture: Option<&wgpu::Texture>,
    dirty_bounds: &mut Option<CanvasRect>,
) -> Result<(usize, usize), P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let planning_start = Instant::now();
    let program = plan_render_program(
        &*context.provider,
        context.output_size,
        target_origin,
        texture_pair.size(),
        sources,
    );
    render_profile::record_render_program_planning(planning_start.elapsed());
    let _program_stats = program.stats();

    for segment in program.segments() {
        debug_assert!(segment.cost_hint.expected_passes > 0);
        match segment.kind {
            RenderSegmentKind::TileLocal(TileProgramKind::RasterClippingRun) => {
                let source_index = segment.source_range.start;
                let GpuNormalStackSource::ClippingRun { base, clipped } = &sources[source_index]
                else {
                    encode_legacy_segment(
                        context,
                        target_origin,
                        sources,
                        segment.source_range.clone(),
                        texture_pair,
                        &mut previous_index,
                        &mut next_index,
                        fallback_texture,
                        dirty_bounds,
                    )?;
                    continue;
                };
                let segment_counters_start = render_profile::segment_counters();
                let segment_start = Instant::now();
                let wrote_silo = encode_clipping_run_silo_with_provider(
                    context,
                    target_origin,
                    texture_pair.size(),
                    *base,
                    clipped,
                    texture_pair.view(previous_index),
                    texture_pair.view(next_index),
                    dirty_bounds,
                )?;
                let segment_elapsed = segment_start.elapsed();
                let segment_counters =
                    render_profile::segment_counters().saturating_sub(segment_counters_start);
                render_profile::record_tile_local_segment(segment_elapsed);
                record_profiled_segment(
                    &*context.provider,
                    context.output_size,
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    segment_elapsed,
                    segment_counters,
                    "RasterClippingRun",
                    None,
                );
                if wrote_silo {
                    std::mem::swap(&mut previous_index, &mut next_index);
                    continue;
                }
                let fallback_counters_start = render_profile::segment_counters();
                let fallback_start = Instant::now();
                encode_legacy_segment(
                    context,
                    target_origin,
                    sources,
                    segment.source_range.clone(),
                    texture_pair,
                    &mut previous_index,
                    &mut next_index,
                    fallback_texture,
                    dirty_bounds,
                )?;
                let fallback_elapsed = fallback_start.elapsed();
                let fallback_counters =
                    render_profile::segment_counters().saturating_sub(fallback_counters_start);
                render_profile::record_legacy_fallback_segment(fallback_elapsed);
                record_profiled_segment(
                    &*context.provider,
                    context.output_size,
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    fallback_elapsed,
                    fallback_counters,
                    "LegacyFallback",
                    None,
                );
            }
            RenderSegmentKind::TileLocal(TileProgramKind::RasterRun) => {
                let segment_counters_start = render_profile::segment_counters();
                let segment_start = Instant::now();
                let silo_outcome = encode_raster_silo_run_with_provider(
                    context,
                    target_origin,
                    texture_pair.size(),
                    &sources[segment.source_range.clone()],
                    texture_pair.view(previous_index),
                    texture_pair.view(next_index),
                    dirty_bounds,
                )?;
                let segment_elapsed = segment_start.elapsed();
                let segment_counters =
                    render_profile::segment_counters().saturating_sub(segment_counters_start);
                render_profile::record_tile_local_segment(segment_elapsed);
                record_profiled_segment_with_atlas_status(
                    &*context.provider,
                    context.output_size,
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    segment_elapsed,
                    segment_counters,
                    "RasterRun",
                    None,
                    Some(SegmentAtlasStatus {
                        uses_sparse_resident_atlas: silo_outcome.uses_sparse_resident_atlas,
                        uses_per_run_atlas: silo_outcome.uses_per_run_atlas,
                        gpu_batches: silo_outcome.gpu_batches,
                    }),
                );
                if silo_outcome.wrote {
                    std::mem::swap(&mut previous_index, &mut next_index);
                    continue;
                }
                let fallback_counters_start = render_profile::segment_counters();
                let fallback_start = Instant::now();
                encode_legacy_segment(
                    context,
                    target_origin,
                    sources,
                    segment.source_range.clone(),
                    texture_pair,
                    &mut previous_index,
                    &mut next_index,
                    fallback_texture,
                    dirty_bounds,
                )?;
                let fallback_elapsed = fallback_start.elapsed();
                let fallback_counters =
                    render_profile::segment_counters().saturating_sub(fallback_counters_start);
                render_profile::record_legacy_fallback_segment(fallback_elapsed);
                record_profiled_segment(
                    &*context.provider,
                    context.output_size,
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    fallback_elapsed,
                    fallback_counters,
                    "LegacyFallback",
                    None,
                );
            }
            RenderSegmentKind::TileLocal(TileProgramKind::RasterFilterRun) => {
                let segment_counters_start = render_profile::segment_counters();
                let segment_start = Instant::now();
                let wrote_silo = encode_raster_filter_silo_run_with_provider(
                    context,
                    target_origin,
                    texture_pair.size(),
                    &sources[segment.source_range.clone()],
                    texture_pair.view(previous_index),
                    texture_pair.view(next_index),
                    dirty_bounds,
                )?;
                let segment_elapsed = segment_start.elapsed();
                let segment_counters =
                    render_profile::segment_counters().saturating_sub(segment_counters_start);
                render_profile::record_tile_local_segment(segment_elapsed);
                record_profiled_segment(
                    &*context.provider,
                    context.output_size,
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    segment_elapsed,
                    segment_counters,
                    "RasterFilterRun",
                    None,
                );
                if wrote_silo {
                    std::mem::swap(&mut previous_index, &mut next_index);
                    continue;
                }
                let fallback_counters_start = render_profile::segment_counters();
                let fallback_start = Instant::now();
                encode_legacy_segment(
                    context,
                    target_origin,
                    sources,
                    segment.source_range.clone(),
                    texture_pair,
                    &mut previous_index,
                    &mut next_index,
                    fallback_texture,
                    dirty_bounds,
                )?;
                let fallback_elapsed = fallback_start.elapsed();
                let fallback_counters =
                    render_profile::segment_counters().saturating_sub(fallback_counters_start);
                render_profile::record_legacy_fallback_segment(fallback_elapsed);
                record_profiled_segment(
                    &*context.provider,
                    context.output_size,
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    fallback_elapsed,
                    fallback_counters,
                    "LegacyFallback",
                    None,
                );
            }
            RenderSegmentKind::TileLocal(TileProgramKind::PointFilterRun) => {
                let segment_counters_start = render_profile::segment_counters();
                let segment_start = Instant::now();
                let wrote_silo = encode_point_filter_silo_run_with_provider(
                    context,
                    target_origin,
                    texture_pair.size(),
                    &sources[segment.source_range.clone()],
                    texture_pair.view(previous_index),
                    texture_pair.view(next_index),
                    dirty_bounds,
                )?;
                let segment_elapsed = segment_start.elapsed();
                let segment_counters =
                    render_profile::segment_counters().saturating_sub(segment_counters_start);
                render_profile::record_tile_local_segment(segment_elapsed);
                record_profiled_segment(
                    &*context.provider,
                    context.output_size,
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    segment_elapsed,
                    segment_counters,
                    "PointFilterRun",
                    None,
                );
                if wrote_silo {
                    std::mem::swap(&mut previous_index, &mut next_index);
                    continue;
                }
                let fallback_counters_start = render_profile::segment_counters();
                let fallback_start = Instant::now();
                encode_legacy_segment(
                    context,
                    target_origin,
                    sources,
                    segment.source_range.clone(),
                    texture_pair,
                    &mut previous_index,
                    &mut next_index,
                    fallback_texture,
                    dirty_bounds,
                )?;
                let fallback_elapsed = fallback_start.elapsed();
                let fallback_counters =
                    render_profile::segment_counters().saturating_sub(fallback_counters_start);
                render_profile::record_legacy_fallback_segment(fallback_elapsed);
                record_profiled_segment(
                    &*context.provider,
                    context.output_size,
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    fallback_elapsed,
                    fallback_counters,
                    "LegacyFallback",
                    None,
                );
            }
            RenderSegmentKind::TileLocal(TileProgramKind::SimpleContainerScope) => {
                let source_index = segment.source_range.start;
                let segment_counters_start = render_profile::segment_counters();
                let segment_start = Instant::now();
                let silo_result = encode_simple_container_scope_silo_with_provider(
                    context,
                    target_origin,
                    texture_pair.size(),
                    &sources[source_index],
                    texture_pair.view(previous_index),
                    texture_pair.view(next_index),
                    dirty_bounds,
                )?;
                let segment_elapsed = segment_start.elapsed();
                let segment_counters =
                    render_profile::segment_counters().saturating_sub(segment_counters_start);
                render_profile::record_tile_local_segment(segment_elapsed);
                record_profiled_segment(
                    &*context.provider,
                    context.output_size,
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    segment_elapsed,
                    segment_counters,
                    "SimpleContainerScope",
                    None,
                );
                if silo_result.wrote() {
                    std::mem::swap(&mut previous_index, &mut next_index);
                    continue;
                }
                let fallback_counters_start = render_profile::segment_counters();
                let fallback_start = Instant::now();
                encode_legacy_segment(
                    context,
                    target_origin,
                    sources,
                    segment.source_range.clone(),
                    texture_pair,
                    &mut previous_index,
                    &mut next_index,
                    fallback_texture,
                    dirty_bounds,
                )?;
                let fallback_elapsed = fallback_start.elapsed();
                let fallback_counters =
                    render_profile::segment_counters().saturating_sub(fallback_counters_start);
                render_profile::record_legacy_fallback_segment(fallback_elapsed);
                record_profiled_segment(
                    &*context.provider,
                    context.output_size,
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    fallback_elapsed,
                    fallback_counters,
                    "LegacyFallback",
                    silo_result.fallback_reason(),
                );
            }
            RenderSegmentKind::TileLocal(TileProgramKind::SimpleThroughScope) => {
                let source_index = segment.source_range.start;
                let segment_counters_start = render_profile::segment_counters();
                let segment_start = Instant::now();
                let silo_result = encode_simple_through_scope_silo_with_provider(
                    context,
                    target_origin,
                    texture_pair.size(),
                    &sources[source_index],
                    texture_pair.view(previous_index),
                    texture_pair.view(next_index),
                    dirty_bounds,
                )?;
                let segment_elapsed = segment_start.elapsed();
                let segment_counters =
                    render_profile::segment_counters().saturating_sub(segment_counters_start);
                render_profile::record_tile_local_segment(segment_elapsed);
                record_profiled_segment(
                    &*context.provider,
                    context.output_size,
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    segment_elapsed,
                    segment_counters,
                    "SimpleThroughScope",
                    None,
                );
                if silo_result.wrote() {
                    std::mem::swap(&mut previous_index, &mut next_index);
                    continue;
                }
                let fallback_counters_start = render_profile::segment_counters();
                let fallback_start = Instant::now();
                encode_legacy_segment(
                    context,
                    target_origin,
                    sources,
                    segment.source_range.clone(),
                    texture_pair,
                    &mut previous_index,
                    &mut next_index,
                    fallback_texture,
                    dirty_bounds,
                )?;
                let fallback_elapsed = fallback_start.elapsed();
                let fallback_counters =
                    render_profile::segment_counters().saturating_sub(fallback_counters_start);
                render_profile::record_legacy_fallback_segment(fallback_elapsed);
                record_profiled_segment(
                    &*context.provider,
                    context.output_size,
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    fallback_elapsed,
                    fallback_counters,
                    "LegacyFallback",
                    silo_result.fallback_reason(),
                );
            }
            RenderSegmentKind::Barrier(BarrierProgramKind::LegacySource(reason)) => {
                let segment_counters_start = render_profile::segment_counters();
                let segment_start = Instant::now();
                encode_legacy_segment(
                    context,
                    target_origin,
                    sources,
                    segment.source_range.clone(),
                    texture_pair,
                    &mut previous_index,
                    &mut next_index,
                    fallback_texture,
                    dirty_bounds,
                )?;
                let segment_elapsed = segment_start.elapsed();
                let segment_counters =
                    render_profile::segment_counters().saturating_sub(segment_counters_start);
                render_profile::record_legacy_barrier_segment(segment_elapsed);
                record_profiled_segment(
                    &*context.provider,
                    context.output_size,
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    segment_elapsed,
                    segment_counters,
                    "LegacySource",
                    Some(reason.as_str()),
                );
            }
        }
    }

    Ok((previous_index, next_index))
}
