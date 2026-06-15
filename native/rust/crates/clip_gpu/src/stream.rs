use clip_model::CanvasSize;

use crate::blend::raster_source_pipeline;
use crate::pass::{
    NormalStackPipelines, create_lut_filter_texture, encode_lut_filter_pass,
    encode_normal_source_pass, encode_normal_source_pass_scissored,
};
use crate::source_params::{
    generated_raster_source_uniform_bytes_with_blend, lut_filter_uniform_bytes,
    raster_source_uniform_bytes, solid_source_uniform_bytes,
};
use crate::stream_bounds::{CanvasRect, union_optional};
use crate::stream_groups::{
    render_clipping_run_with_provider, render_container_with_provider,
    render_through_group_with_provider,
};
use crate::stream_state::{StreamingEncoder, StreamingTexturePair};
use crate::{
    GpuLutFilterMode, GpuMaskResourceCache, GpuMaskResourceInfo, GpuMaskResourceKey,
    GpuNormalRasterSource, GpuNormalStackSource, GpuRasterResourceCache, GpuRasterStackOutput,
    GpuRenderError, GpuRenderer,
};

pub trait GpuNormalStackResourceProvider {
    type Error: From<GpuRenderError>;

    fn raster_resource(
        &mut self,
        renderer: &GpuRenderer,
        source: GpuNormalRasterSource,
    ) -> Result<GpuRasterResourceCache, Self::Error>;

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

        state.clear_rgba8_texture(
            accum_pair.view(previous_index),
            "rizum_clip_provider_normal_initial_clear",
        );
        state.clear_rgba8_texture(
            accum_pair.view(next_index),
            "rizum_clip_provider_normal_spare_clear",
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
            let (raster_cache, source_view, source_bounds) =
                raster_view_with_provider(renderer, provider, state, output_size, *raster)?;
            let Some(pass_bounds) = pass_bounds_for_change(*dirty_bounds, source_bounds) else {
                return Ok(false);
            };
            let (mask_cache, mask_view) = mask_view_with_provider(
                renderer,
                provider,
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
                raster_source_uniform_bytes(*raster),
                "rizum_clip_provider_normal_raster_pass",
                pass_bounds,
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
                generated_raster_source_uniform_bytes_with_blend(1.0, false, base.blend_mode),
                "rizum_clip_provider_clipping_resolve_pass",
                pass_bounds,
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
                generated_raster_source_uniform_bytes_with_blend(
                    *opacity,
                    mask_key.is_some(),
                    *blend_mode,
                ),
                "rizum_clip_provider_container_resolve_pass",
                pass_bounds,
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
            children,
            *opacity,
            *mask_key,
            previous_texture,
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
            let Some(full_bounds) = CanvasRect::full(output_size) else {
                return Ok(false);
            };
            let (mask_cache, mask_view) = mask_view_with_provider(
                renderer,
                provider,
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
            encode_lut_filter_pass(
                state.device(),
                state.encoder_mut(),
                &pipelines.lut_filter_pipeline,
                &pipelines.bind_group_layout,
                previous_view,
                mask_view.as_ref(),
                &lut_view,
                output_view,
                lut_filter_uniform_bytes(*opacity, mask_key.is_some(), *filter_mode),
                lut_filter_label(*filter_mode),
            );
            state.retain_optional_mask_cache(mask_cache);
            state.retain_lut_texture(lut_texture);
            state.finish_pass()?;
            *dirty_bounds = Some(full_bounds);
            Ok(true)
        }
    }
}

pub(crate) fn raster_view_with_provider<P>(
    renderer: &GpuRenderer,
    provider: &mut P,
    state: &mut StreamingEncoder<'_, P::Error>,
    output_size: CanvasSize,
    source: GpuNormalRasterSource,
) -> Result<
    (
        GpuRasterResourceCache,
        wgpu::TextureView,
        Option<CanvasRect>,
    ),
    P::Error,
>
where
    P: GpuNormalStackResourceProvider,
{
    let cache = provider.raster_resource(renderer, source)?;
    let resource = cache
        .resource(source.key)
        .ok_or(GpuRenderError::MissingRasterResource {
            layer_id: source.key.layer_id,
            render_mipmap_id: source.key.render_mipmap_id,
        })
        .map_err(P::Error::from)?;
    let info = resource.info();
    state.push_drawn_resource(info);
    let bounds = CanvasRect::from_source(source.offset_x, source.offset_y, info.size, output_size);
    let view = resource
        .texture()
        .create_view(&wgpu::TextureViewDescriptor::default());
    Ok((cache, view, bounds))
}

pub(crate) fn pass_bounds_for_change(
    dirty_bounds: Option<CanvasRect>,
    change_bounds: Option<CanvasRect>,
) -> Option<CanvasRect> {
    change_bounds.map(|change_bounds| {
        union_optional(dirty_bounds, Some(change_bounds)).expect("change bounds must be present")
    })
}

pub(crate) fn mask_view_with_provider<'a, P>(
    renderer: &GpuRenderer,
    provider: &mut P,
    output_size: CanvasSize,
    mask_key: Option<GpuMaskResourceKey>,
    owner_layer_id: clip_model::LayerId,
    fallback_view: &'a wgpu::TextureView,
) -> Result<(Option<GpuMaskResourceCache>, MaskTextureView<'a>), P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let Some(mask_key) = mask_key else {
        return Ok((None, MaskTextureView::Borrowed(fallback_view)));
    };
    let cache = provider.mask_resource(renderer, mask_key)?;
    let resource = cache
        .resource(mask_key)
        .ok_or(GpuRenderError::MissingMaskResource {
            layer_id: mask_key.layer_id,
            mask_mipmap_id: mask_key.mask_mipmap_id,
        })
        .map_err(P::Error::from)?;
    let info: GpuMaskResourceInfo = resource.info();
    if info.size != output_size {
        return Err(P::Error::from(GpuRenderError::MaskResourceSizeMismatch {
            layer_id: owner_layer_id,
            expected: output_size,
            actual: info.size,
        }));
    }
    let view = resource
        .texture()
        .create_view(&wgpu::TextureViewDescriptor::default());
    Ok((Some(cache), MaskTextureView::Owned(view)))
}

fn renderer_context_queue(renderer: &GpuRenderer) -> &wgpu::Queue {
    &renderer.context.queue
}

pub(crate) enum MaskTextureView<'a> {
    Borrowed(&'a wgpu::TextureView),
    Owned(wgpu::TextureView),
}

impl MaskTextureView<'_> {
    pub(crate) fn as_ref(&self) -> &wgpu::TextureView {
        match self {
            Self::Borrowed(view) => view,
            Self::Owned(view) => view,
        }
    }
}

fn lut_filter_label(filter_mode: GpuLutFilterMode) -> &'static str {
    match filter_mode {
        GpuLutFilterMode::ToneCurveRgb => "rizum_clip_provider_tone_curve_pass",
        GpuLutFilterMode::GradientMapLum => "rizum_clip_provider_gradient_map_pass",
    }
}
