use clip_model::CanvasSize;

use crate::blend::clipped_source_pipeline;
use crate::pass::{NormalStackPipelines, encode_normal_source_pass_scissored};
use crate::source_params::{
    generated_raster_source_uniform_bytes, raster_source_uniform_bytes_with_target_origin,
};
use crate::stream::{GpuNormalStackResourceProvider, encode_source_with_provider};
use crate::stream_bounds::CanvasRect;
use crate::stream_extents::{KnownStackBounds, known_stack_bounds};
use crate::stream_resources::{
    known_clipping_run_activity, known_raster_source_bounds, known_stack_activity,
    mask_view_with_provider, pass_bounds_for_change, preserving_pass_bounds_for_change,
    raster_view_with_provider,
};
use crate::stream_state::{RenderedStreamingCache, StreamingEncoder, StreamingTexturePair};
use crate::{GpuMaskResourceKey, GpuNormalRasterSource, GpuRenderer};

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_clipping_run_with_provider<P>(
    renderer: &GpuRenderer,
    provider: &mut P,
    state: &mut StreamingEncoder<'_, P::Error>,
    output_size: CanvasSize,
    base: GpuNormalRasterSource,
    clipped: &[GpuNormalRasterSource],
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
            output_size,
            base.mask_key,
            base.key.layer_id,
            clipping_pair.view(previous_index),
        )?;
        encode_normal_source_pass_scissored(
            state.device(),
            state.encoder_mut(),
            &pipelines.alpha_pipeline,
            &pipelines.bind_group_layout,
            &source_view,
            clipping_pair.view(previous_index),
            mask_view.as_ref(),
            clipping_pair.view(next_index),
            raster_source_uniform_bytes_with_target_origin(effective_base, cache_origin),
            "rizum_clip_provider_clipping_base_pass",
            local_cache_bounds,
        );
        state.retain_raster_cache(raster_cache);
        state.retain_optional_mask_cache(mask_cache);
        state.finish_pass()?;
        std::mem::swap(&mut previous_index, &mut next_index);
    }

    for clipped_source in clipped {
        let known_source_bounds =
            known_raster_source_bounds(provider, *clipped_source, output_size);
        if matches!(known_source_bounds, Some(None)) {
            continue;
        }
        if let Some(source_bounds) = known_source_bounds {
            if preserving_pass_bounds_for_change(dirty_bounds, source_bounds).is_none() {
                continue;
            }
        }
        let (raster_cache, source_view, effective_clipped_source, uploaded_source_bounds) =
            raster_view_with_provider(renderer, provider, state, output_size, *clipped_source)?;
        let source_bounds = known_source_bounds.flatten().or(uploaded_source_bounds);
        let Some(global_pass_bounds) =
            preserving_pass_bounds_for_change(dirty_bounds, source_bounds)
        else {
            continue;
        };
        let (mask_cache, mask_view) = mask_view_with_provider(
            renderer,
            provider,
            output_size,
            clipped_source.mask_key,
            clipped_source.key.layer_id,
            clipping_pair.view(previous_index),
        )?;
        encode_normal_source_pass_scissored(
            state.device(),
            state.encoder_mut(),
            clipped_source_pipeline(
                clipped_source.blend_mode,
                &pipelines.clipped_pipeline,
                &pipelines.clipped_byte_pipeline,
            ),
            &pipelines.bind_group_layout,
            &source_view,
            clipping_pair.view(previous_index),
            mask_view.as_ref(),
            clipping_pair.view(next_index),
            raster_source_uniform_bytes_with_target_origin(effective_clipped_source, cache_origin),
            "rizum_clip_provider_clipping_clipped_pass",
            local_cache_bounds,
        );
        state.retain_raster_cache(raster_cache);
        state.retain_optional_mask_cache(mask_cache);
        state.finish_pass()?;
        dirty_bounds = Some(global_pass_bounds);
        std::mem::swap(&mut previous_index, &mut next_index);
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_through_group_with_provider<P>(
    renderer: &GpuRenderer,
    provider: &mut P,
    state: &mut StreamingEncoder<'_, P::Error>,
    output_size: CanvasSize,
    children: &[crate::GpuNormalStackSource],
    opacity: f32,
    mask_key: Option<GpuMaskResourceKey>,
    before_texture: &wgpu::Texture,
    fallback_texture: &wgpu::Texture,
    output_view: &wgpu::TextureView,
    pipelines: &NormalStackPipelines,
    parent_dirty_bounds: &mut Option<CanvasRect>,
) -> Result<bool, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    if known_stack_activity(provider, children, output_size).is_empty() {
        return Ok(false);
    }

    let through_usage =
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let through_pair = StreamingTexturePair::new(
        state.device(),
        "rizum_clip_provider_through_after_a",
        "rizum_clip_provider_through_after_b",
        output_size,
        through_usage,
    );
    let mut previous_index = 0usize;
    let mut next_index = 1usize;
    let before_view = before_texture.create_view(&wgpu::TextureViewDescriptor::default());
    state.clear_rgba8_texture_pair(
        through_pair.view(previous_index),
        through_pair.view(next_index),
        "rizum_clip_provider_through_initial_clear",
    );
    let mut after_dirty_bounds = *parent_dirty_bounds;
    let mut has_child_output = false;

    for (child_index, child) in children.iter().enumerate() {
        let (previous_texture, previous_view) = if child_index == 0 {
            (before_texture, &before_view)
        } else {
            (
                through_pair.texture(previous_index),
                through_pair.view(previous_index),
            )
        };
        let did_write = encode_source_with_provider(
            renderer,
            provider,
            state,
            output_size,
            (0, 0),
            child,
            previous_texture,
            fallback_texture,
            previous_view,
            through_pair.view(next_index),
            pipelines,
            &mut after_dirty_bounds,
        )?;
        if did_write {
            has_child_output = true;
            std::mem::swap(&mut previous_index, &mut next_index);
        }
    }

    if !has_child_output {
        return Ok(false);
    }

    let after_view = if children.is_empty() {
        &before_view
    } else {
        through_pair.view(previous_index)
    };
    let (mask_cache, mask_view) = mask_view_with_provider(
        renderer,
        provider,
        output_size,
        mask_key,
        mask_key
            .map(|key| key.layer_id)
            .unwrap_or(clip_model::LayerId(0)),
        &before_view,
    )?;
    let Some(pass_bounds) = after_dirty_bounds else {
        return Ok(false);
    };
    encode_normal_source_pass_scissored(
        state.device(),
        state.encoder_mut(),
        &pipelines.through_pipeline,
        &pipelines.bind_group_layout,
        after_view,
        &before_view,
        mask_view.as_ref(),
        output_view,
        generated_raster_source_uniform_bytes(opacity, mask_key.is_some()),
        "rizum_clip_provider_through_resolve_pass",
        pass_bounds,
    );
    state.retain_optional_mask_cache(mask_cache);
    state.retain_texture_pair(through_pair);
    state.finish_pass()?;
    *parent_dirty_bounds = Some(pass_bounds);
    Ok(true)
}
