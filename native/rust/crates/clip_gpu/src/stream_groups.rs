use clip_model::CanvasSize;

use crate::pass::{NormalStackPipelines, encode_normal_source_pass_scissored};
use crate::source_params::{
    generated_raster_source_uniform_bytes_with_blend_origins_and_mask,
    raster_source_uniform_bytes_with_target_origin_and_mask,
};
use crate::stream::{GpuNormalStackResourceProvider, encode_source_with_provider};
use crate::stream_bounds::CanvasRect;
use crate::stream_clipping::encode_clipped_stack_source_with_provider;
use crate::stream_extents::{KnownStackBounds, known_stack_bounds};
use crate::stream_resources::{
    known_clipping_run_activity, known_raster_source_bounds, mask_view_with_provider,
    pass_bounds_for_change, raster_view_with_provider,
};
use crate::stream_state::{RenderedStreamingCache, StreamingEncoder, StreamingTexturePair};
use crate::{
    GpuClippedStackSource, GpuMaskResourceKey, GpuNormalRasterSource, GpuRasterBlendMode,
    GpuRenderer,
};

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_clipping_run_with_provider<P>(
    renderer: &GpuRenderer,
    provider: &mut P,
    state: &mut StreamingEncoder<'_, P::Error>,
    output_size: CanvasSize,
    base: GpuNormalRasterSource,
    clipped: &[GpuClippedStackSource],
    fallback_texture: &wgpu::Texture,
    pipelines: &NormalStackPipelines,
) -> Result<RenderedStreamingCache, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    if known_clipping_run_activity(provider, base, output_size).is_empty() {
        return Ok(RenderedStreamingCache::empty());
    }

    let known_source_bounds = known_raster_source_bounds(provider, base, output_size);
    if matches!(known_source_bounds, Some(None)) {
        return Ok(RenderedStreamingCache::empty());
    }
    let (raster_cache, source_view, effective_base, uploaded_source_bounds) =
        raster_view_with_provider(renderer, provider, state, output_size, base)?;
    let source_bounds = known_source_bounds.flatten().or(uploaded_source_bounds);
    let Some(pass_bounds) = pass_bounds_for_change(None, source_bounds) else {
        return Ok(RenderedStreamingCache::empty());
    };
    let cache_size = CanvasSize::new(pass_bounds.width, pass_bounds.height);
    let cache_origin = pass_bounds.origin_i32();
    let local_cache_bounds =
        CanvasRect::full(cache_size).expect("non-empty clipping bounds must create local bounds");

    let clipping_usage =
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let clipping_pair = StreamingTexturePair::new(
        state.device(),
        "rizum_clip_provider_clipping_cache_a",
        "rizum_clip_provider_clipping_cache_b",
        cache_size,
        clipping_usage,
    );
    let mut previous_index = 0usize;
    let mut next_index = 1usize;
    let mut dirty_bounds = Some(pass_bounds);

    state.clear_rgba8_texture_pair(
        clipping_pair.view(previous_index),
        clipping_pair.view(next_index),
        "rizum_clip_provider_clipping_initial_clear",
    );

    {
        let (mask_cache, mask_view) = mask_view_with_provider(
            renderer,
            provider,
            state,
            output_size,
            base.mask_key,
            base.key.layer_id,
            clipping_pair.view(previous_index),
        )?;
        let pipeline = pipelines.alpha_pipeline(state.device());
        encode_normal_source_pass_scissored(
            state.device(),
            state.encoder_mut(),
            pipeline,
            &pipelines.bind_group_layout,
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
        state.retain_raster_cache(raster_cache);
        state.retain_optional_mask_cache(mask_cache);
        state.finish_pass()?;
        std::mem::swap(&mut previous_index, &mut next_index);
    }

    for clipped_source in clipped {
        encode_clipped_stack_source_with_provider(
            renderer,
            provider,
            state,
            output_size,
            clipped_source,
            fallback_texture,
            pipelines,
            &clipping_pair,
            &mut previous_index,
            &mut next_index,
            &mut dirty_bounds,
            cache_origin,
            local_cache_bounds,
        )?;
    }

    Ok(RenderedStreamingCache::new_with_origin(
        clipping_pair,
        previous_index,
        dirty_bounds,
        cache_origin,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_container_clipping_run_with_provider<P>(
    renderer: &GpuRenderer,
    provider: &mut P,
    state: &mut StreamingEncoder<'_, P::Error>,
    output_size: CanvasSize,
    children: &[crate::GpuNormalStackSource],
    opacity: f32,
    mask_key: Option<GpuMaskResourceKey>,
    clipped: &[GpuClippedStackSource],
    fallback_texture: &wgpu::Texture,
    pipelines: &NormalStackPipelines,
) -> Result<RenderedStreamingCache, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    if !crate::stream_effects::stack_can_affect_output(children) || opacity <= 0.0 {
        return Ok(RenderedStreamingCache::empty());
    }

    let base_cache = render_container_with_provider(
        renderer,
        provider,
        state,
        output_size,
        children,
        fallback_texture,
        pipelines,
    )?;
    let Some(pass_bounds) = base_cache.bounds() else {
        return Ok(RenderedStreamingCache::empty());
    };
    let cache_size = CanvasSize::new(pass_bounds.width, pass_bounds.height);
    let cache_origin = pass_bounds.origin_i32();
    let local_cache_bounds =
        CanvasRect::full(cache_size).expect("non-empty clipping bounds must create local bounds");

    let clipping_usage =
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let clipping_pair = StreamingTexturePair::new(
        state.device(),
        "rizum_clip_provider_container_clipping_cache_a",
        "rizum_clip_provider_container_clipping_cache_b",
        cache_size,
        clipping_usage,
    );
    let mut previous_index = 0usize;
    let mut next_index = 1usize;
    let mut dirty_bounds = Some(pass_bounds);

    state.clear_rgba8_texture_pair(
        clipping_pair.view(previous_index),
        clipping_pair.view(next_index),
        "rizum_clip_provider_container_clipping_initial_clear",
    );

    {
        let (mask_cache, mask_view) = mask_view_with_provider(
            renderer,
            provider,
            state,
            output_size,
            mask_key,
            mask_key
                .map(|key| key.layer_id)
                .unwrap_or(clip_model::LayerId(0)),
            clipping_pair.view(previous_index),
        )?;
        let pipeline = pipelines.alpha_pipeline(state.device());
        encode_normal_source_pass_scissored(
            state.device(),
            state.encoder_mut(),
            pipeline,
            &pipelines.bind_group_layout,
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
        state.retain_intermediate_cache(base_cache);
        state.retain_optional_mask_cache(mask_cache);
        state.finish_pass()?;
        std::mem::swap(&mut previous_index, &mut next_index);
    }

    for clipped_source in clipped {
        encode_clipped_stack_source_with_provider(
            renderer,
            provider,
            state,
            output_size,
            clipped_source,
            fallback_texture,
            pipelines,
            &clipping_pair,
            &mut previous_index,
            &mut next_index,
            &mut dirty_bounds,
            cache_origin,
            local_cache_bounds,
        )?;
    }

    Ok(RenderedStreamingCache::new_with_origin(
        clipping_pair,
        previous_index,
        dirty_bounds,
        cache_origin,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_container_with_provider<P>(
    renderer: &GpuRenderer,
    provider: &mut P,
    state: &mut StreamingEncoder<'_, P::Error>,
    output_size: CanvasSize,
    children: &[crate::GpuNormalStackSource],
    fallback_texture: &wgpu::Texture,
    pipelines: &NormalStackPipelines,
) -> Result<RenderedStreamingCache, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let (cache_size, cache_origin) = match known_stack_bounds(provider, children, output_size) {
        KnownStackBounds::Empty => return Ok(RenderedStreamingCache::empty()),
        KnownStackBounds::Bounded(bounds) => (
            CanvasSize::new(bounds.width, bounds.height),
            bounds.origin_i32(),
        ),
        KnownStackBounds::Unknown => (output_size, (0, 0)),
    };

    let container_usage =
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let container_pair = StreamingTexturePair::new(
        state.device(),
        "rizum_clip_provider_container_cache_a",
        "rizum_clip_provider_container_cache_b",
        cache_size,
        container_usage,
    );
    let mut previous_index = 0usize;
    let mut next_index = 1usize;

    state.clear_rgba8_texture_pair(
        container_pair.view(previous_index),
        container_pair.view(next_index),
        "rizum_clip_provider_container_initial_clear",
    );
    let mut dirty_bounds = None;

    for child in children {
        let did_write = encode_source_with_provider(
            renderer,
            provider,
            state,
            output_size,
            cache_origin,
            child,
            container_pair.texture(previous_index),
            fallback_texture,
            container_pair.view(previous_index),
            container_pair.view(next_index),
            pipelines,
            &mut dirty_bounds,
        )?;
        if did_write {
            std::mem::swap(&mut previous_index, &mut next_index);
        }
    }

    Ok(RenderedStreamingCache::new_with_origin(
        container_pair,
        previous_index,
        dirty_bounds,
        cache_origin,
    ))
}
