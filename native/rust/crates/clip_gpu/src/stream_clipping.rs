use crate::GpuClippedStackSource;
use crate::pass::encode_normal_source_pass_scissored;
use crate::source_params::{
    generated_raster_source_uniform_bytes_with_blend_origins_and_mask,
    raster_source_uniform_bytes_with_target_origin_and_mask,
};
use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::CanvasRect;
use crate::stream_context::StreamingExecutionContext;
use crate::stream_effects::clipped_stack_source_can_affect_output;
use crate::stream_groups::render_container_with_provider;
use crate::stream_resources::{
    known_clipped_stack_source_bounds, mask_view_with_provider, preserving_pass_bounds_for_change,
    raster_view_with_provider,
};
use crate::stream_state::StreamingTexturePair;

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_clipped_stack_source_with_provider<P>(
    context: &mut StreamingExecutionContext<'_, '_, P>,
    clipped_source: &GpuClippedStackSource,
    fallback_texture: &wgpu::Texture,
    clipping_pair: &StreamingTexturePair,
    previous_index: &mut usize,
    next_index: &mut usize,
    dirty_bounds: &mut Option<CanvasRect>,
    cache_origin: (i32, i32),
    local_cache_bounds: CanvasRect,
) -> Result<(), P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    if !clipped_stack_source_can_affect_output(clipped_source) {
        return Ok(());
    }
    let output_size = context.output_size;
    let known_source_bounds =
        known_clipped_stack_source_bounds(&*context.provider, clipped_source, output_size);
    if matches!(known_source_bounds, Some(None)) {
        return Ok(());
    }
    if let Some(source_bounds) = known_source_bounds {
        if preserving_pass_bounds_for_change(*dirty_bounds, source_bounds).is_none() {
            return Ok(());
        }
    }

    match clipped_source {
        GpuClippedStackSource::Raster(raster) => {
            let (raster_cache, source_view, effective_source, uploaded_source_bounds) =
                raster_view_with_provider(
                    context.renderer,
                    &mut *context.provider,
                    &mut context.state,
                    output_size,
                    *raster,
                )?;
            let source_bounds = known_source_bounds.flatten().or(uploaded_source_bounds);
            let Some(global_pass_bounds) =
                preserving_pass_bounds_for_change(*dirty_bounds, source_bounds)
            else {
                return Ok(());
            };
            let (mask_cache, mask_view) = mask_view_with_provider(
                context.renderer,
                &mut *context.provider,
                &mut context.state,
                output_size,
                raster.mask_key,
                raster.key.layer_id,
                clipping_pair.view(*previous_index),
            )?;
            let pipeline = context
                .pipelines
                .clipped_source_pipeline(context.state.device(), raster.blend_mode);
            encode_normal_source_pass_scissored(
                context.state.device(),
                context.state.encoder_mut(),
                pipeline,
                &context.pipelines.bind_group_layout,
                &source_view,
                clipping_pair.view(*previous_index),
                mask_view.view(),
                clipping_pair.view(*next_index),
                raster_source_uniform_bytes_with_target_origin_and_mask(
                    effective_source,
                    cache_origin,
                    mask_view.sampling(),
                ),
                "rizum_clip_provider_clipping_clipped_raster_pass",
                local_cache_bounds,
            );
            context.state.retain_raster_cache(raster_cache);
            context.state.retain_optional_mask_cache(mask_cache);
            context.state.finish_pass()?;
            *dirty_bounds = Some(global_pass_bounds);
            std::mem::swap(previous_index, next_index);
        }
        GpuClippedStackSource::Container {
            layer_id,
            children,
            opacity,
            mask_key,
            blend_mode,
        } => {
            let source_cache = render_container_with_provider(context, children, fallback_texture)?;
            let source_bounds = known_source_bounds.flatten().or(source_cache.bounds());
            let Some(global_pass_bounds) =
                preserving_pass_bounds_for_change(*dirty_bounds, source_bounds)
            else {
                return Ok(());
            };
            let (mask_cache, mask_view) = mask_view_with_provider(
                context.renderer,
                &mut *context.provider,
                &mut context.state,
                output_size,
                *mask_key,
                *layer_id,
                clipping_pair.view(*previous_index),
            )?;
            let pipeline = context
                .pipelines
                .clipped_source_pipeline(context.state.device(), *blend_mode);
            encode_normal_source_pass_scissored(
                context.state.device(),
                context.state.encoder_mut(),
                pipeline,
                &context.pipelines.bind_group_layout,
                source_cache.view(),
                clipping_pair.view(*previous_index),
                mask_view.view(),
                clipping_pair.view(*next_index),
                generated_raster_source_uniform_bytes_with_blend_origins_and_mask(
                    *opacity,
                    mask_key.is_some(),
                    *blend_mode,
                    source_cache.texture_origin(),
                    cache_origin,
                    mask_view.sampling(),
                ),
                "rizum_clip_provider_clipping_clipped_container_pass",
                local_cache_bounds,
            );
            context.state.retain_intermediate_cache(source_cache);
            context.state.retain_optional_mask_cache(mask_cache);
            context.state.finish_pass()?;
            *dirty_bounds = Some(global_pass_bounds);
            std::mem::swap(previous_index, next_index);
        }
    }
    Ok(())
}
