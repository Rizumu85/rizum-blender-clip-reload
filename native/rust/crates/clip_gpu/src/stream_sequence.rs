use std::time::Instant;

use crate::GpuNormalStackSource;
use crate::render_profile;
use crate::stream::{GpuNormalStackResourceProvider, encode_source_with_provider};
use crate::stream_bounds::CanvasRect;
use crate::stream_clipping_tile_silo::encode_clipping_run_silo_with_provider;
use crate::stream_context::StreamingExecutionContext;
use crate::stream_program::{
    BarrierProgramKind, RenderSegmentKind, TileProgramKind, plan_render_program,
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
                render_profile::record_tile_local_segment(segment_start.elapsed());
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
                render_profile::record_legacy_fallback_segment(fallback_start.elapsed());
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
                render_profile::record_tile_local_segment(segment_start.elapsed());
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
                render_profile::record_legacy_fallback_segment(fallback_start.elapsed());
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
                render_profile::record_tile_local_segment(segment_start.elapsed());
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
                render_profile::record_legacy_fallback_segment(fallback_start.elapsed());
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
                render_profile::record_tile_local_segment(segment_start.elapsed());
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
                render_profile::record_legacy_fallback_segment(fallback_start.elapsed());
            }
            RenderSegmentKind::TileLocal(TileProgramKind::SimpleContainerScope) => {
                let source_index = segment.source_range.start;
                let segment_start = Instant::now();
                let wrote_silo = encode_simple_container_scope_silo_with_provider(
                    context,
                    target_origin,
                    texture_pair.size(),
                    &sources[source_index],
                    texture_pair.view(previous_index),
                    texture_pair.view(next_index),
                    dirty_bounds,
                )?;
                render_profile::record_tile_local_segment(segment_start.elapsed());
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
                render_profile::record_legacy_fallback_segment(fallback_start.elapsed());
            }
            RenderSegmentKind::TileLocal(TileProgramKind::SimpleThroughScope) => {
                let source_index = segment.source_range.start;
                let segment_start = Instant::now();
                let wrote_silo = encode_simple_through_scope_silo_with_provider(
                    context,
                    target_origin,
                    texture_pair.size(),
                    &sources[source_index],
                    texture_pair.view(previous_index),
                    texture_pair.view(next_index),
                    dirty_bounds,
                )?;
                render_profile::record_tile_local_segment(segment_start.elapsed());
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
                render_profile::record_legacy_fallback_segment(fallback_start.elapsed());
            }
            RenderSegmentKind::Barrier(BarrierProgramKind::LegacySource(_)) => {
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
                render_profile::record_legacy_barrier_segment(segment_start.elapsed());
            }
        }
    }

    Ok((previous_index, next_index))
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
