use clip_model::CanvasSize;

use crate::pass::encode_normal_source_pass_scissored;
use crate::source_params::{
    generated_raster_source_uniform_bytes_with_blend_origins_and_mask,
    raster_source_uniform_bytes_with_target_origin_and_mask,
};
use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::CanvasRect;
use crate::stream_clipped_tile_silo::{
    clipped_raster_silo_run_len, encode_clipped_raster_silo_run_with_provider,
};
use crate::stream_clipping::encode_clipped_stack_source_with_provider;
use crate::stream_context::StreamingExecutionContext;
use crate::stream_extents::{KnownStackBounds, known_stack_bounds};
use crate::stream_resources::{
    known_clipping_run_activity, known_raster_source_bounds, mask_view_with_provider,
    pass_bounds_for_change, raster_view_with_provider,
};
use crate::stream_sequence::encode_source_sequence_with_provider;
use crate::stream_state::RenderedStreamingCache;
use crate::{GpuClippedStackSource, GpuMaskResourceKey, GpuNormalRasterSource, GpuRasterBlendMode};

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_clipping_run_with_provider<P>(
    context: &mut StreamingExecutionContext<'_, '_, P>,
    base: GpuNormalRasterSource,
    clipped: &[GpuClippedStackSource],
    fallback_texture: &wgpu::Texture,
) -> Result<RenderedStreamingCache, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let output_size = context.output_size;
    if known_clipping_run_activity(&*context.provider, base, output_size).is_empty() {
        return Ok(RenderedStreamingCache::empty());
    }

    let known_source_bounds = known_raster_source_bounds(&*context.provider, base, output_size);
    if matches!(known_source_bounds, Some(None)) {
        return Ok(RenderedStreamingCache::empty());
    }
    if let Some(Some(bounds)) = known_source_bounds
        && context
            .state
            .clip_pass_bounds(pass_bounds_for_change(None, Some(bounds)))
            .is_none()
    {
        return Ok(RenderedStreamingCache::empty());
    }
    let (raster_cache, source_view, effective_base, uploaded_source_bounds) =
        raster_view_with_provider(
            context.renderer,
            &mut *context.provider,
            &mut context.state,
            output_size,
            base,
        )?;
    let source_bounds = known_source_bounds.flatten().or(uploaded_source_bounds);
    let Some(pass_bounds) = context
        .state
        .clip_pass_bounds(pass_bounds_for_change(None, source_bounds))
    else {
        return Ok(RenderedStreamingCache::empty());
    };
    let cache_size = CanvasSize::new(pass_bounds.width, pass_bounds.height);
    let cache_origin = pass_bounds.origin_i32();
    let local_cache_bounds =
        CanvasRect::full(cache_size).expect("non-empty clipping bounds must create local bounds");

    let clipping_usage =
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let clipping_pair = context.texture_pair(
        "rizum_clip_provider_clipping_cache_a",
        "rizum_clip_provider_clipping_cache_b",
        cache_size,
        clipping_usage,
    );
    let mut previous_index = 0usize;
    let mut next_index = 1usize;
    let mut dirty_bounds = Some(pass_bounds);

    context.clear_texture_pair(
        &clipping_pair,
        previous_index,
        next_index,
        "rizum_clip_provider_clipping_initial_clear",
    );

    {
        let (mask_cache, mask_view) = mask_view_with_provider(
            context.renderer,
            &mut *context.provider,
            &mut context.state,
            output_size,
            base.mask_key,
            base.key.layer_id,
            clipping_pair.view(previous_index),
        )?;
        let pipeline = context.pipelines.alpha_pipeline(context.state.device());
        encode_normal_source_pass_scissored(
            context.state.device(),
            context.state.encoder_mut(),
            pipeline,
            &context.pipelines.bind_group_layout,
            &source_view,
            clipping_pair.view(previous_index),
            mask_view.view(),
            clipping_pair.view(next_index),
            raster_source_uniform_bytes_with_target_origin_and_mask(
                effective_base,
                cache_origin,
                mask_view.sampling(),
            ),
            "rizum_clip_provider_clipping_base_pass",
            local_cache_bounds,
        );
        context.state.retain_raster_cache(raster_cache);
        context.state.retain_optional_mask_cache(mask_cache);
        context.state.finish_pass()?;
        std::mem::swap(&mut previous_index, &mut next_index);
    }

    encode_clipped_stack_sources_with_provider(
        context,
        clipped,
        fallback_texture,
        &clipping_pair,
        &mut previous_index,
        &mut next_index,
        &mut dirty_bounds,
        cache_origin,
        local_cache_bounds,
    )?;

    Ok(RenderedStreamingCache::new_with_origin(
        clipping_pair,
        previous_index,
        dirty_bounds,
        cache_origin,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_container_clipping_run_with_provider<P>(
    context: &mut StreamingExecutionContext<'_, '_, P>,
    children: &[crate::GpuNormalStackSource],
    opacity: f32,
    mask_key: Option<GpuMaskResourceKey>,
    clipped: &[GpuClippedStackSource],
    fallback_texture: &wgpu::Texture,
) -> Result<RenderedStreamingCache, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    if !crate::stream_effects::stack_can_affect_output(children) || opacity <= 0.0 {
        return Ok(RenderedStreamingCache::empty());
    }

    let base_cache = render_container_with_provider(context, children, fallback_texture)?;
    let Some(pass_bounds) = base_cache.bounds() else {
        return Ok(RenderedStreamingCache::empty());
    };
    let cache_size = CanvasSize::new(pass_bounds.width, pass_bounds.height);
    let cache_origin = pass_bounds.origin_i32();
    let local_cache_bounds =
        CanvasRect::full(cache_size).expect("non-empty clipping bounds must create local bounds");

    let clipping_usage =
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let clipping_pair = context.texture_pair(
        "rizum_clip_provider_container_clipping_cache_a",
        "rizum_clip_provider_container_clipping_cache_b",
        cache_size,
        clipping_usage,
    );
    let mut previous_index = 0usize;
    let mut next_index = 1usize;
    let mut dirty_bounds = Some(pass_bounds);

    context.clear_texture_pair(
        &clipping_pair,
        previous_index,
        next_index,
        "rizum_clip_provider_container_clipping_initial_clear",
    );

    {
        let (mask_cache, mask_view) = mask_view_with_provider(
            context.renderer,
            &mut *context.provider,
            &mut context.state,
            context.output_size,
            mask_key,
            mask_key
                .map(|key| key.layer_id)
                .unwrap_or(clip_model::LayerId(0)),
            clipping_pair.view(previous_index),
        )?;
        let pipeline = context.pipelines.alpha_pipeline(context.state.device());
        encode_normal_source_pass_scissored(
            context.state.device(),
            context.state.encoder_mut(),
            pipeline,
            &context.pipelines.bind_group_layout,
            base_cache.view(),
            clipping_pair.view(previous_index),
            mask_view.view(),
            clipping_pair.view(next_index),
            generated_raster_source_uniform_bytes_with_blend_origins_and_mask(
                opacity,
                mask_key.is_some(),
                GpuRasterBlendMode::Normal,
                base_cache.texture_origin(),
                cache_origin,
                mask_view.sampling(),
            ),
            "rizum_clip_provider_container_clipping_base_pass",
            local_cache_bounds,
        );
        context.state.retain_intermediate_cache(base_cache);
        context.state.retain_optional_mask_cache(mask_cache);
        context.state.finish_pass()?;
        std::mem::swap(&mut previous_index, &mut next_index);
    }

    encode_clipped_stack_sources_with_provider(
        context,
        clipped,
        fallback_texture,
        &clipping_pair,
        &mut previous_index,
        &mut next_index,
        &mut dirty_bounds,
        cache_origin,
        local_cache_bounds,
    )?;

    Ok(RenderedStreamingCache::new_with_origin(
        clipping_pair,
        previous_index,
        dirty_bounds,
        cache_origin,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_container_with_provider<P>(
    context: &mut StreamingExecutionContext<'_, '_, P>,
    children: &[crate::GpuNormalStackSource],
    fallback_texture: &wgpu::Texture,
) -> Result<RenderedStreamingCache, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let output_size = context.output_size;
    let (cache_size, cache_origin) = match clipped_known_stack_bounds(
        context.state.render_bounds(),
        known_stack_bounds(&*context.provider, children, output_size),
        output_size,
    ) {
        KnownStackBounds::Empty => return Ok(RenderedStreamingCache::empty()),
        KnownStackBounds::Bounded(bounds) => (
            CanvasSize::new(bounds.width, bounds.height),
            bounds.origin_i32(),
        ),
        KnownStackBounds::Unknown => (output_size, (0, 0)),
    };

    let container_usage =
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let container_pair = context.texture_pair(
        "rizum_clip_provider_container_cache_a",
        "rizum_clip_provider_container_cache_b",
        cache_size,
        container_usage,
    );
    let mut previous_index = 0usize;
    let mut next_index = 1usize;

    context.clear_texture_pair(
        &container_pair,
        previous_index,
        next_index,
        "rizum_clip_provider_container_initial_clear",
    );
    let mut dirty_bounds = None;

    let updated = encode_source_sequence_with_provider(
        context,
        cache_origin,
        children,
        &container_pair,
        previous_index,
        next_index,
        Some(fallback_texture),
        &mut dirty_bounds,
    )?;
    previous_index = updated.0;
    next_index = updated.1;
    let _ = next_index;

    Ok(RenderedStreamingCache::new_with_origin(
        container_pair,
        previous_index,
        dirty_bounds,
        cache_origin,
    ))
}

fn clipped_known_stack_bounds(
    render_bounds: Option<CanvasRect>,
    stack_bounds: KnownStackBounds,
    output_size: CanvasSize,
) -> KnownStackBounds {
    let Some(render_bounds) = render_bounds else {
        return stack_bounds;
    };
    match stack_bounds {
        KnownStackBounds::Empty => KnownStackBounds::Empty,
        KnownStackBounds::Bounded(bounds) => bounds
            .intersection(render_bounds)
            .map(KnownStackBounds::Bounded)
            .unwrap_or(KnownStackBounds::Empty),
        KnownStackBounds::Unknown => CanvasRect::full(output_size)
            .and_then(|bounds| bounds.intersection(render_bounds))
            .map(KnownStackBounds::Bounded)
            .unwrap_or(KnownStackBounds::Empty),
    }
}

#[allow(clippy::too_many_arguments)]
fn encode_clipped_stack_sources_with_provider<P>(
    context: &mut StreamingExecutionContext<'_, '_, P>,
    clipped: &[GpuClippedStackSource],
    fallback_texture: &wgpu::Texture,
    clipping_pair: &crate::stream_state::StreamingTexturePair,
    previous_index: &mut usize,
    next_index: &mut usize,
    dirty_bounds: &mut Option<CanvasRect>,
    cache_origin: (i32, i32),
    local_cache_bounds: CanvasRect,
) -> Result<(), P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let mut clipped_index = 0usize;
    while clipped_index < clipped.len() {
        let run_len = clipped_raster_silo_run_len(
            &*context.provider,
            context.output_size,
            cache_origin,
            clipping_pair.size(),
            &clipped[clipped_index..],
        );
        if run_len >= 2 {
            let wrote_silo = encode_clipped_raster_silo_run_with_provider(
                context,
                cache_origin,
                clipping_pair.size(),
                &clipped[clipped_index..clipped_index + run_len],
                clipping_pair.view(*previous_index),
                clipping_pair.view(*next_index),
                dirty_bounds,
            )?;
            if wrote_silo {
                std::mem::swap(previous_index, next_index);
                clipped_index += run_len;
                continue;
            }
        }

        encode_clipped_stack_source_with_provider(
            context,
            &clipped[clipped_index],
            fallback_texture,
            clipping_pair,
            previous_index,
            next_index,
            dirty_bounds,
            cache_origin,
            local_cache_bounds,
        )?;
        clipped_index += 1;
    }
    Ok(())
}
