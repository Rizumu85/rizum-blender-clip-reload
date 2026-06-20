use std::time::Instant;

use crate::GpuNormalStackSource;
use crate::render_profile;
use crate::stream::{GpuNormalStackResourceProvider, encode_source_with_provider};
use crate::stream_bounds::CanvasRect;
use crate::stream_clipping_tile_silo::encode_clipping_run_silo_with_provider;
use crate::stream_context::StreamingExecutionContext;
use crate::stream_program::{
    BarrierProgramKind, RenderSegment, RenderSegmentKind, TileProgramKind, plan_render_program,
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
                render_profile::record_tile_local_segment(segment_elapsed);
                record_profiled_segment(
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    segment_elapsed,
                    "RasterClippingRun",
                    None,
                );
                if wrote_silo {
                    std::mem::swap(&mut previous_index, &mut next_index);
                    continue;
                }
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
                render_profile::record_legacy_fallback_segment(fallback_elapsed);
                record_profiled_segment(
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    fallback_elapsed,
                    "LegacyFallback",
                    None,
                );
            }
            RenderSegmentKind::TileLocal(TileProgramKind::RasterRun) => {
                let segment_start = Instant::now();
                let wrote_silo = encode_raster_silo_run_with_provider(
                    context,
                    target_origin,
                    texture_pair.size(),
                    &sources[segment.source_range.clone()],
                    texture_pair.view(previous_index),
                    texture_pair.view(next_index),
                    dirty_bounds,
                )?;
                let segment_elapsed = segment_start.elapsed();
                render_profile::record_tile_local_segment(segment_elapsed);
                record_profiled_segment(
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    segment_elapsed,
                    "RasterRun",
                    None,
                );
                if wrote_silo {
                    std::mem::swap(&mut previous_index, &mut next_index);
                    continue;
                }
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
                render_profile::record_legacy_fallback_segment(fallback_elapsed);
                record_profiled_segment(
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    fallback_elapsed,
                    "LegacyFallback",
                    None,
                );
            }
            RenderSegmentKind::TileLocal(TileProgramKind::RasterFilterRun) => {
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
                render_profile::record_tile_local_segment(segment_elapsed);
                record_profiled_segment(
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    segment_elapsed,
                    "RasterFilterRun",
                    None,
                );
                if wrote_silo {
                    std::mem::swap(&mut previous_index, &mut next_index);
                    continue;
                }
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
                render_profile::record_legacy_fallback_segment(fallback_elapsed);
                record_profiled_segment(
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    fallback_elapsed,
                    "LegacyFallback",
                    None,
                );
            }
            RenderSegmentKind::TileLocal(TileProgramKind::PointFilterRun) => {
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
                render_profile::record_tile_local_segment(segment_elapsed);
                record_profiled_segment(
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    segment_elapsed,
                    "PointFilterRun",
                    None,
                );
                if wrote_silo {
                    std::mem::swap(&mut previous_index, &mut next_index);
                    continue;
                }
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
                render_profile::record_legacy_fallback_segment(fallback_elapsed);
                record_profiled_segment(
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    fallback_elapsed,
                    "LegacyFallback",
                    None,
                );
            }
            RenderSegmentKind::TileLocal(TileProgramKind::SimpleContainerScope) => {
                let source_index = segment.source_range.start;
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
                render_profile::record_tile_local_segment(segment_elapsed);
                record_profiled_segment(
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    segment_elapsed,
                    "SimpleContainerScope",
                    None,
                );
                if silo_result.wrote() {
                    std::mem::swap(&mut previous_index, &mut next_index);
                    continue;
                }
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
                render_profile::record_legacy_fallback_segment(fallback_elapsed);
                record_profiled_segment(
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    fallback_elapsed,
                    "LegacyFallback",
                    silo_result.fallback_reason(),
                );
            }
            RenderSegmentKind::TileLocal(TileProgramKind::SimpleThroughScope) => {
                let source_index = segment.source_range.start;
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
                render_profile::record_tile_local_segment(segment_elapsed);
                record_profiled_segment(
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    segment_elapsed,
                    "SimpleThroughScope",
                    None,
                );
                if silo_result.wrote() {
                    std::mem::swap(&mut previous_index, &mut next_index);
                    continue;
                }
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
                render_profile::record_legacy_fallback_segment(fallback_elapsed);
                record_profiled_segment(
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    fallback_elapsed,
                    "LegacyFallback",
                    silo_result.fallback_reason(),
                );
            }
            RenderSegmentKind::Barrier(BarrierProgramKind::LegacySource(reason)) => {
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
                render_profile::record_legacy_barrier_segment(segment_elapsed);
                record_profiled_segment(
                    segment,
                    sources,
                    target_origin,
                    texture_pair.size(),
                    segment_elapsed,
                    "LegacySource",
                    Some(reason.as_str()),
                );
            }
        }
    }

    Ok((previous_index, next_index))
}

fn record_profiled_segment(
    segment: &RenderSegment,
    sources: &[GpuNormalStackSource],
    target_origin: (i32, i32),
    target_size: clip_model::CanvasSize,
    elapsed: std::time::Duration,
    kind: &'static str,
    reason_text: Option<&'static str>,
) {
    if !render_profile::enabled() {
        return;
    }
    render_profile::record_segment(render_profile::RenderProfileSegmentRecord {
        kind,
        source_shape: source_shape_name(sources, segment.source_range.start),
        legacy_reason: reason_text,
        elapsed,
        source_start: segment.source_range.start,
        source_end: segment.source_range.end,
        first_layer_id: first_layer_id_for_range(sources, segment.source_range.clone()),
        target_origin,
        target_size,
        expected_passes: segment.cost_hint.expected_passes,
        tile_events: segment.cost_hint.tile_events,
        legacy_sources: segment.cost_hint.legacy_sources,
    });
}

fn source_shape_name(sources: &[GpuNormalStackSource], source_index: usize) -> &'static str {
    match sources.get(source_index) {
        Some(GpuNormalStackSource::Raster(_)) => "Raster",
        Some(GpuNormalStackSource::ClippingRun { .. }) => "ClippingRun",
        Some(GpuNormalStackSource::ContainerClippingRun { .. }) => "ContainerClippingRun",
        Some(GpuNormalStackSource::Container { .. }) => "Container",
        Some(GpuNormalStackSource::ThroughGroup { .. }) => "ThroughGroup",
        Some(GpuNormalStackSource::SolidColor { .. }) => "SolidColor",
        Some(GpuNormalStackSource::LutFilter { .. }) => "LutFilter",
        None => "Unknown",
    }
}

fn first_layer_id_for_range(
    sources: &[GpuNormalStackSource],
    source_range: std::ops::Range<usize>,
) -> Option<u32> {
    sources
        .get(source_range)
        .and_then(|range| range.iter().find_map(first_layer_id_for_source))
}

fn first_layer_id_for_source(source: &GpuNormalStackSource) -> Option<u32> {
    match source {
        GpuNormalStackSource::Raster(raster) => Some(raster.key.layer_id.0),
        GpuNormalStackSource::ClippingRun { base, .. } => Some(base.key.layer_id.0),
        GpuNormalStackSource::ContainerClippingRun { children, .. }
        | GpuNormalStackSource::Container { children, .. }
        | GpuNormalStackSource::ThroughGroup { children, .. } => {
            children.iter().find_map(first_layer_id_for_source)
        }
        GpuNormalStackSource::SolidColor { .. } | GpuNormalStackSource::LutFilter { .. } => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn encode_legacy_segment<P>(
    context: &mut StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    sources: &[GpuNormalStackSource],
    source_range: std::ops::Range<usize>,
    texture_pair: &StreamingTexturePair,
    previous_index: &mut usize,
    next_index: &mut usize,
    fallback_texture: Option<&wgpu::Texture>,
    dirty_bounds: &mut Option<CanvasRect>,
) -> Result<(), P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    for source_index in source_range {
        let effective_fallback_texture = match fallback_texture {
            Some(texture) => texture,
            None => texture_pair.texture(*previous_index),
        };
        let did_write = encode_source_with_provider(
            context,
            target_origin,
            &sources[source_index],
            texture_pair.texture(*previous_index),
            effective_fallback_texture,
            texture_pair.view(*previous_index),
            texture_pair.view(*next_index),
            dirty_bounds,
        )?;
        if did_write {
            std::mem::swap(previous_index, next_index);
        }
    }

    Ok(())
}
