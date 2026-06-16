use clip_model::CanvasSize;

use crate::blend::raster_source_pipeline;
use crate::lut_filter::{create_lut_filter_texture, encode_lut_filter_pass_scissored};
use crate::pass::{
    NormalStackPipelines, encode_normal_source_pass, encode_normal_source_pass_scissored,
};
use crate::source_params::{
    generated_raster_source_uniform_bytes_with_blend_and_origins,
    lut_filter_uniform_bytes_with_target_origin, raster_source_uniform_bytes_with_target_origin,
    solid_source_uniform_bytes,
};
use crate::stream_bounds::CanvasRect;
use crate::stream_groups::{
    render_clipping_run_with_provider, render_container_with_provider,
    render_through_group_with_provider,
};
use crate::stream_resources::{
    known_raster_source_bounds, mask_view_with_provider, pass_bounds_for_change,
    raster_view_with_provider,
};
use crate::stream_state::{StreamingEncoder, StreamingTexturePair};
use crate::{
    GpuLutFilterMode, GpuMaskResourceCache, GpuMaskResourceKey, GpuNormalRasterSource,
    GpuNormalStackSource, GpuRasterResourceCache, GpuRasterStackOutput, GpuRenderError,
    GpuRenderer,
};

pub trait GpuNormalStackResourceProvider {
    type Error: From<GpuRenderError>;

    fn raster_resource(
        &mut self,
        renderer: &GpuRenderer,
        source: GpuNormalRasterSource,
    ) -> Result<GpuRasterResourceCache, Self::Error>;

    fn raster_resource_size(&self, source: GpuNormalRasterSource) -> Option<CanvasSize> {
        let _ = source;
        None
    }

    fn raster_resource_offset(&self, source: GpuNormalRasterSource) -> Option<(i32, i32)> {
        let _ = source;
        None
    }

    fn mask_resource(
        &mut self,
        renderer: &GpuRenderer,
        key: GpuMaskResourceKey,
    ) -> Result<GpuMaskResourceCache, Self::Error>;
}

impl GpuRenderer {
    pub fn draw_normal_stack_with_provider_to_rgba8<P>(
        &self,
        output_size: CanvasSize,
        sources: &[GpuNormalStackSource],
        provider: &mut P,
    ) -> Result<GpuRasterStackOutput, P::Error>
    where
        P: GpuNormalStackResourceProvider,
    {
        if output_size.width == 0 || output_size.height == 0 {
            return Err(P::Error::from(GpuRenderError::InvalidImageSize));
        }
        if sources.is_empty() {
            return Err(P::Error::from(GpuRenderError::EmptyRasterStack));
        }

        let accum_usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC;
        let accum_pair = StreamingTexturePair::new(
            &self.context.device,
            "rizum_clip_provider_normal_accum_a",
            "rizum_clip_provider_normal_accum_b",
            output_size,
            accum_usage,
        );
        let pipelines = NormalStackPipelines::new(&self.context.device);
        let mut state = StreamingEncoder::<P::Error>::new(
            &self.context.device,
            &self.context.queue,
            "rizum_clip_provider_normal_encoder",
        );
        let mut previous_index = 0usize;
        let mut next_index = 1usize;

        state.clear_rgba8_texture_pair(
            accum_pair.view(previous_index),
            accum_pair.view(next_index),
            "rizum_clip_provider_normal_initial_clear",
        );

        let updated = encode_sources_with_provider(
            self,
            provider,
            &mut state,
            output_size,
            sources,
            &accum_pair,
            previous_index,
            next_index,
            &pipelines,
        )?;
        previous_index = updated.0;
        next_index = updated.1;
        let _ = next_index;
        state.flush()?;
        let drawn_resources = state.into_drawn_resources();

        let pixels = self
            .read_texture_rgba8(
                accum_pair.texture(previous_index),
                output_size.width,
                output_size.height,
            )
            .map_err(P::Error::from)?;
        Ok(GpuRasterStackOutput {
            drawn_resources,
            size: output_size,
            pixels,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn encode_sources_with_provider<P>(
    renderer: &GpuRenderer,
    provider: &mut P,
    state: &mut StreamingEncoder<'_, P::Error>,
    output_size: CanvasSize,
    sources: &[GpuNormalStackSource],
    accum_pair: &StreamingTexturePair,
    mut previous_index: usize,
    mut next_index: usize,
    pipelines: &NormalStackPipelines,
) -> Result<(usize, usize), P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let mut dirty_bounds = None;
    for source in sources {
        let did_write = encode_source_with_provider(
            renderer,
            provider,
            state,
            output_size,
            (0, 0),
            source,
            accum_pair.texture(previous_index),
            accum_pair.texture(previous_index),
            accum_pair.view(previous_index),
            accum_pair.view(next_index),
            pipelines,
            &mut dirty_bounds,
        )?;
        if did_write {
            std::mem::swap(&mut previous_index, &mut next_index);
        }
    }
    Ok((previous_index, next_index))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_source_with_provider<P>(
    renderer: &GpuRenderer,
    provider: &mut P,
    state: &mut StreamingEncoder<'_, P::Error>,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    source: &GpuNormalStackSource,
    previous_texture: &wgpu::Texture,
    fallback_texture: &wgpu::Texture,
    previous_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    pipelines: &NormalStackPipelines,
    dirty_bounds: &mut Option<CanvasRect>,
) -> Result<bool, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    match source {
        GpuNormalStackSource::Raster(raster) => {
            let known_source_bounds = known_raster_source_bounds(provider, *raster, output_size);
            if matches!(known_source_bounds, Some(None)) {
                return Ok(false);
            }
            let (raster_cache, source_view, effective_raster, uploaded_source_bounds) =
                raster_view_with_provider(renderer, provider, state, output_size, *raster)?;
            let source_bounds = known_source_bounds.flatten().or(uploaded_source_bounds);
            let Some(pass_bounds) = pass_bounds_for_change(*dirty_bounds, source_bounds) else {
                return Ok(false);
            };
            let (mask_cache, mask_view) = mask_view_with_provider(
                renderer,
                provider,
                state,
                output_size,
                raster.mask_key,
                raster.key.layer_id,
                previous_view,
            )?;
            encode_normal_source_pass_scissored(
                state.device(),
                state.encoder_mut(),
                raster_source_pipeline(
                    raster.blend_mode,
                    &pipelines.alpha_pipeline,
                    &pipelines.add_glow_pipeline,
                    &pipelines.color_dodge_pipeline,
                    &pipelines.color_burn_pipeline,
                    &pipelines.glow_dodge_pipeline,
                    &pipelines.standard_blend_pipeline,
                ),
                &pipelines.bind_group_layout,
                &source_view,
                previous_view,
                mask_view.as_ref(),
                output_view,
                raster_source_uniform_bytes_with_target_origin(effective_raster, target_origin),
                "rizum_clip_provider_normal_raster_pass",
                local_pass_bounds(pass_bounds, target_origin),
            );
            state.retain_raster_cache(raster_cache);
            state.retain_optional_mask_cache(mask_cache);
            state.finish_pass()?;
            *dirty_bounds = Some(pass_bounds);
            Ok(true)
        }
        GpuNormalStackSource::ClippingRun { base, clipped } => {
            let clipping_cache = render_clipping_run_with_provider(
                renderer,
                provider,
                state,
                output_size,
                *base,
                clipped,
                pipelines,
            )?;
            let Some(pass_bounds) = pass_bounds_for_change(*dirty_bounds, clipping_cache.bounds())
            else {
                return Ok(false);
            };
            encode_normal_source_pass_scissored(
                state.device(),
                state.encoder_mut(),
                raster_source_pipeline(
                    base.blend_mode,
                    &pipelines.alpha_pipeline,
                    &pipelines.add_glow_pipeline,
                    &pipelines.color_dodge_pipeline,
                    &pipelines.color_burn_pipeline,
                    &pipelines.glow_dodge_pipeline,
                    &pipelines.standard_blend_pipeline,
                ),
                &pipelines.bind_group_layout,
                clipping_cache.view(),
                previous_view,
                previous_view,
                output_view,
                generated_raster_source_uniform_bytes_with_blend_and_origins(
                    1.0,
                    false,
                    base.blend_mode,
                    clipping_cache.texture_origin(),
                    target_origin,
                ),
                "rizum_clip_provider_clipping_resolve_pass",
                local_pass_bounds(pass_bounds, target_origin),
            );
            state.retain_intermediate_cache(clipping_cache);
            state.finish_pass()?;
            *dirty_bounds = Some(pass_bounds);
            Ok(true)
        }
        GpuNormalStackSource::Container {
            children,
            opacity,
            mask_key,
            blend_mode,
        } => {
            let container_cache = render_container_with_provider(
                renderer,
                provider,
                state,
                output_size,
                children,
                fallback_texture,
                pipelines,
            )?;
            let Some(pass_bounds) = pass_bounds_for_change(*dirty_bounds, container_cache.bounds())
            else {
                return Ok(false);
            };
            let (mask_cache, mask_view) = mask_view_with_provider(
                renderer,
                provider,
                state,
                output_size,
                *mask_key,
                mask_key
                    .map(|key| key.layer_id)
                    .unwrap_or(clip_model::LayerId(0)),
                previous_view,
            )?;
            encode_normal_source_pass_scissored(
                state.device(),
                state.encoder_mut(),
                raster_source_pipeline(
                    *blend_mode,
                    &pipelines.alpha_pipeline,
                    &pipelines.add_glow_pipeline,
                    &pipelines.color_dodge_pipeline,
                    &pipelines.color_burn_pipeline,
                    &pipelines.glow_dodge_pipeline,
                    &pipelines.standard_blend_pipeline,
                ),
                &pipelines.bind_group_layout,
                container_cache.view(),
                previous_view,
                mask_view.as_ref(),
                output_view,
                generated_raster_source_uniform_bytes_with_blend_and_origins(
                    *opacity,
                    mask_key.is_some(),
                    *blend_mode,
                    container_cache.texture_origin(),
                    target_origin,
                ),
                "rizum_clip_provider_container_resolve_pass",
                local_pass_bounds(pass_bounds, target_origin),
            );
            state.retain_intermediate_cache(container_cache);
            state.retain_optional_mask_cache(mask_cache);
            state.finish_pass()?;
            *dirty_bounds = Some(pass_bounds);
            Ok(true)
        }
        GpuNormalStackSource::ThroughGroup {
            children,
            opacity,
            mask_key,
        } => render_through_group_with_provider(
            renderer,
            provider,
            state,
            output_size,
            target_origin,
            children,
            *opacity,
            *mask_key,
            previous_texture,
            previous_view,
            fallback_texture,
            output_view,
            pipelines,
            dirty_bounds,
        ),
        GpuNormalStackSource::SolidColor { color, opacity } => {
            let Some(full_bounds) = CanvasRect::full(output_size) else {
                return Ok(false);
            };
            encode_normal_source_pass(
                state.device(),
                state.encoder_mut(),
                &pipelines.alpha_pipeline,
                &pipelines.bind_group_layout,
                previous_view,
                previous_view,
                previous_view,
                output_view,
                solid_source_uniform_bytes(*color, *opacity),
                "rizum_clip_provider_solid_pass",
            );
            state.finish_pass()?;
            *dirty_bounds = Some(full_bounds);
            Ok(true)
        }
        GpuNormalStackSource::LutFilter {
            lut_rgba,
            opacity,
            mask_key,
            filter_mode,
        } => {
            let pass_bounds = match *dirty_bounds {
                Some(bounds) => bounds,
                None => {
                    let Some(full_bounds) = CanvasRect::full(output_size) else {
                        return Ok(false);
                    };
                    full_bounds
                }
            };
            let (mask_cache, mask_view) = mask_view_with_provider(
                renderer,
                provider,
                state,
                output_size,
                *mask_key,
                mask_key
                    .map(|key| key.layer_id)
                    .unwrap_or(clip_model::LayerId(0)),
                previous_view,
            )?;
            let lut_texture = create_lut_filter_texture(
                state.device(),
                renderer_context_queue(renderer),
                lut_rgba,
            )
            .map_err(P::Error::from)?;
            let lut_view = lut_texture.create_view(&wgpu::TextureViewDescriptor::default());
            encode_lut_filter_pass_scissored(
                state.device(),
                state.encoder_mut(),
                &pipelines.lut_filter_pipeline,
                &pipelines.bind_group_layout,
                previous_view,
                mask_view.as_ref(),
                &lut_view,
                output_view,
                lut_filter_uniform_bytes_with_target_origin(
                    *opacity,
                    mask_key.is_some(),
                    *filter_mode,
                    target_origin,
                ),
                lut_filter_label(*filter_mode),
                local_pass_bounds(pass_bounds, target_origin),
            );
            state.retain_optional_mask_cache(mask_cache);
            state.retain_lut_texture(lut_texture);
            state.finish_pass()?;
            *dirty_bounds = Some(pass_bounds);
            Ok(true)
        }
    }
}

fn renderer_context_queue(renderer: &GpuRenderer) -> &wgpu::Queue {
    &renderer.context.queue
}

fn lut_filter_label(filter_mode: GpuLutFilterMode) -> &'static str {
    match filter_mode {
        GpuLutFilterMode::ToneCurveRgb => "rizum_clip_provider_tone_curve_pass",
        GpuLutFilterMode::GradientMapLum => "rizum_clip_provider_gradient_map_pass",
    }
}

fn local_pass_bounds(pass_bounds: CanvasRect, target_origin: (i32, i32)) -> CanvasRect {
    pass_bounds
        .translate_to_local(target_origin)
        .expect("global pass bounds must fit inside the streaming target")
}
