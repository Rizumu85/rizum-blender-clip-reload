use clip_model::CanvasSize;

use crate::pass::encode_normal_source_pass_scissored;
use crate::source_params::{
    generated_raster_source_uniform_bytes_with_blend_and_origins,
    generated_raster_source_uniform_bytes_with_blend_origins_and_mask,
};
use crate::stream::{GpuNormalStackResourceProvider, encode_source_with_provider};
use crate::stream_bounds::{CanvasRect, union_optional};
use crate::stream_context::StreamingExecutionContext;
use crate::stream_extents::{KnownStackBounds, known_stack_bounds};
use crate::stream_resources::{known_stack_activity, mask_view_with_provider};
use crate::{GpuMaskResourceKey, GpuRasterBlendMode};

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_through_group_with_provider<P>(
    context: &mut StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    children: &[crate::GpuNormalStackSource],
    opacity: f32,
    mask_key: Option<GpuMaskResourceKey>,
    before_texture: &wgpu::Texture,
    before_view: &wgpu::TextureView,
    fallback_texture: &wgpu::Texture,
    output_view: &wgpu::TextureView,
    parent_dirty_bounds: &mut Option<CanvasRect>,
) -> Result<bool, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let output_size = context.output_size;
    let through_bounds = through_cache_bounds(
        &*context.provider,
        children,
        output_size,
        context.state.render_bounds(),
    );
    let Some((cache_size, cache_origin, cache_global_bounds)) = through_bounds else {
        return Ok(false);
    };

    let through_usage =
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let through_pair = context.texture_pair(
        "rizum_clip_provider_through_after_a",
        "rizum_clip_provider_through_after_b",
        cache_size,
        through_usage,
    );
    let mut previous_index = 0usize;
    let mut next_index = 1usize;
    context.clear_texture_pair(
        &through_pair,
        previous_index,
        next_index,
        "rizum_clip_provider_through_initial_clear",
    );
    let mut after_dirty_bounds = if cache_global_bounds.is_some() {
        cache_global_bounds
    } else {
        *parent_dirty_bounds
    };
    let mut has_child_output = false;

    if let Some(global_cache_bounds) = cache_global_bounds {
        let local_cache_bounds = CanvasRect::full(cache_size)
            .expect("non-empty through bounds must create local bounds");
        let pipeline = context.pipelines.alpha_pipeline(context.state.device());
        encode_normal_source_pass_scissored(
            context.state.device(),
            context.state.encoder_mut(),
            pipeline,
            &context.pipelines.bind_group_layout,
            before_view,
            through_pair.view(previous_index),
            through_pair.view(previous_index),
            through_pair.view(next_index),
            generated_raster_source_uniform_bytes_with_blend_and_origins(
                1.0,
                false,
                GpuRasterBlendMode::Normal,
                target_origin,
                cache_origin,
            ),
            "rizum_clip_provider_through_seed_pass",
            local_cache_bounds,
        );
        context.state.finish_pass()?;
        after_dirty_bounds = Some(global_cache_bounds);
        std::mem::swap(&mut previous_index, &mut next_index);
    }

    for (child_index, child) in children.iter().enumerate() {
        let (previous_texture, previous_view) = if cache_global_bounds.is_none() && child_index == 0
        {
            (before_texture, before_view)
        } else {
            (
                through_pair.texture(previous_index),
                through_pair.view(previous_index),
            )
        };
        let did_write = encode_source_with_provider(
            context,
            cache_origin,
            child,
            previous_texture,
            fallback_texture,
            previous_view,
            through_pair.view(next_index),
            &mut after_dirty_bounds,
        )?;
        if did_write {
            has_child_output = true;
            std::mem::swap(&mut previous_index, &mut next_index);
        }
    }

    if !has_child_output {
        context.state.retain_texture_pair(through_pair);
        return Ok(false);
    }

    let after_view = through_pair.view(previous_index);
    let (mask_cache, mask_view) = mask_view_with_provider(
        context.renderer,
        &mut *context.provider,
        &mut context.state,
        output_size,
        mask_key,
        mask_key
            .map(|key| key.layer_id)
            .unwrap_or(clip_model::LayerId(0)),
        before_view,
    )?;
    let Some(pass_bounds) = through_resolve_bounds(*parent_dirty_bounds, after_dirty_bounds) else {
        return Ok(false);
    };
    let pipeline = context.pipelines.through_pipeline(context.state.device());
    encode_normal_source_pass_scissored(
        context.state.device(),
        context.state.encoder_mut(),
        pipeline,
        &context.pipelines.bind_group_layout,
        after_view,
        before_view,
        mask_view.view(),
        output_view,
        generated_raster_source_uniform_bytes_with_blend_origins_and_mask(
            opacity,
            mask_key.is_some(),
            GpuRasterBlendMode::Normal,
            cache_origin,
            target_origin,
            mask_view.sampling(),
        ),
        "rizum_clip_provider_through_resolve_pass",
        pass_bounds
            .translate_to_local(target_origin)
            .expect("through resolve bounds must fit inside the target"),
    );
    context.state.retain_optional_mask_cache(mask_cache);
    context.state.retain_texture_pair(through_pair);
    context.state.finish_pass()?;
    *parent_dirty_bounds = Some(pass_bounds);
    Ok(true)
}

fn through_cache_bounds<P>(
    provider: &P,
    children: &[crate::GpuNormalStackSource],
    output_size: CanvasSize,
    render_bounds: Option<CanvasRect>,
) -> Option<(CanvasSize, (i32, i32), Option<CanvasRect>)>
where
    P: GpuNormalStackResourceProvider,
{
    let restrict = |bounds: CanvasRect| match render_bounds {
        Some(render_bounds) => bounds.intersection(render_bounds),
        None => Some(bounds),
    };
    match known_stack_bounds(provider, children, output_size) {
        KnownStackBounds::Empty => None,
        KnownStackBounds::Bounded(bounds) if Some(bounds) != CanvasRect::full(output_size) => {
            let bounds = restrict(bounds)?;
            Some((
                CanvasSize::new(bounds.width, bounds.height),
                bounds.origin_i32(),
                Some(bounds),
            ))
        }
        KnownStackBounds::Bounded(_) | KnownStackBounds::Unknown => {
            if known_stack_activity(provider, children, output_size).is_empty() {
                None
            } else {
                match render_bounds {
                    Some(bounds) => Some((
                        CanvasSize::new(bounds.width, bounds.height),
                        bounds.origin_i32(),
                        Some(bounds),
                    )),
                    None => Some((output_size, (0, 0), None)),
                }
            }
        }
    }
}

fn through_resolve_bounds(
    parent_dirty_bounds: Option<CanvasRect>,
    after_dirty_bounds: Option<CanvasRect>,
) -> Option<CanvasRect> {
    union_optional(parent_dirty_bounds, after_dirty_bounds)
}
