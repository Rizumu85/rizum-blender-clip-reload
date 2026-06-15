use std::marker::PhantomData;

use clip_model::CanvasSize;

use crate::blend::{clipped_source_pipeline, raster_source_pipeline};
use crate::pass::{
    NormalStackPipelines, clear_rgba8_texture, create_lut_filter_texture, create_rgba8_texture,
    encode_lut_filter_pass, encode_normal_source_pass,
};
use crate::source_params::{
    generated_raster_source_uniform_bytes, generated_raster_source_uniform_bytes_with_blend,
    lut_filter_uniform_bytes, raster_source_uniform_bytes, solid_source_uniform_bytes,
};
use crate::{
    GpuLutFilterMode, GpuMaskResourceCache, GpuMaskResourceInfo, GpuMaskResourceKey,
    GpuNormalRasterSource, GpuNormalStackSource, GpuRasterResourceCache, GpuRasterResourceInfo,
    GpuRasterStackOutput, GpuRenderError, GpuRenderer,
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
        let accum_textures = [
            create_rgba8_texture(
                &self.context.device,
                "rizum_clip_provider_normal_accum_a",
                output_size,
                accum_usage,
            ),
            create_rgba8_texture(
                &self.context.device,
                "rizum_clip_provider_normal_accum_b",
                output_size,
                accum_usage,
            ),
        ];
        let pipelines = NormalStackPipelines::new(&self.context.device);
        let mut state = StreamingEncoder::<P::Error>::new(
            &self.context.device,
            &self.context.queue,
            "rizum_clip_provider_normal_encoder",
        );
        let mut previous_index = 0usize;
        let mut next_index = 1usize;

        {
            let initial_view =
                accum_textures[previous_index].create_view(&wgpu::TextureViewDescriptor::default());
            clear_rgba8_texture(
                state.encoder_mut(),
                &initial_view,
                "rizum_clip_provider_normal_initial_clear",
            );
        }

        let updated = encode_sources_with_provider(
            self,
            provider,
            &mut state,
            output_size,
            sources,
            &accum_textures,
            previous_index,
            next_index,
            &pipelines,
        )?;
        previous_index = updated.0;
        next_index = updated.1;
        let _ = next_index;
        state.flush()?;

        let pixels = self
            .read_texture_rgba8(
                &accum_textures[previous_index],
                output_size.width,
                output_size.height,
            )
            .map_err(P::Error::from)?;
        Ok(GpuRasterStackOutput {
            drawn_resources: state.drawn_resources,
            size: output_size,
            pixels,
        })
    }
}

struct StreamingEncoder<'a, E> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    label: &'static str,
    encoder: Option<wgpu::CommandEncoder>,
    drawn_resources: Vec<GpuRasterResourceInfo>,
    _error: PhantomData<E>,
}

impl<'a, E> StreamingEncoder<'a, E>
where
    E: From<GpuRenderError>,
{
    fn new(device: &'a wgpu::Device, queue: &'a wgpu::Queue, label: &'static str) -> Self {
        Self {
            device,
            queue,
            label,
            encoder: Some(
                device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) }),
            ),
            drawn_resources: Vec::new(),
            _error: PhantomData,
        }
    }

    fn encoder_mut(&mut self) -> &mut wgpu::CommandEncoder {
        self.encoder
            .as_mut()
            .expect("streaming encoder must exist before finish")
    }

    fn flush(&mut self) -> Result<(), E> {
        let encoder = self
            .encoder
            .take()
            .expect("streaming encoder must exist before flush");
        self.queue.submit([encoder.finish()]);
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|err| E::from(GpuRenderError::PollFailed(err.to_string())))?;
        self.encoder = Some(
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some(self.label),
                }),
        );
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn encode_sources_with_provider<P>(
    renderer: &GpuRenderer,
    provider: &mut P,
    state: &mut StreamingEncoder<'_, P::Error>,
    output_size: CanvasSize,
    sources: &[GpuNormalStackSource],
    accum_textures: &[wgpu::Texture; 2],
    mut previous_index: usize,
    mut next_index: usize,
    pipelines: &NormalStackPipelines,
) -> Result<(usize, usize), P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    for source in sources {
        let previous_view =
            accum_textures[previous_index].create_view(&wgpu::TextureViewDescriptor::default());
        let output_view =
            accum_textures[next_index].create_view(&wgpu::TextureViewDescriptor::default());
        encode_source_with_provider(
            renderer,
            provider,
            state,
            output_size,
            source,
            &accum_textures[previous_index],
            &accum_textures[previous_index],
            &previous_view,
            &output_view,
            pipelines,
        )?;
        std::mem::swap(&mut previous_index, &mut next_index);
    }
    Ok((previous_index, next_index))
}

#[allow(clippy::too_many_arguments)]
fn encode_source_with_provider<P>(
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
) -> Result<(), P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    match source {
        GpuNormalStackSource::Raster(raster) => {
            let (_raster_cache, source_view) =
                raster_view_with_provider(renderer, provider, state, *raster)?;
            let (_mask_cache, mask_view) = mask_view_with_provider(
                renderer,
                provider,
                output_size,
                raster.mask_key,
                raster.key.layer_id,
                previous_view,
            )?;
            encode_normal_source_pass(
                state.device,
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
            );
            state.flush()?;
        }
        GpuNormalStackSource::ClippingRun { base, clipped } => {
            let clipping_view = render_clipping_run_with_provider(
                renderer,
                provider,
                state,
                output_size,
                *base,
                clipped,
                pipelines,
            )?;
            encode_normal_source_pass(
                state.device,
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
                &clipping_view,
                previous_view,
                previous_view,
                output_view,
                generated_raster_source_uniform_bytes_with_blend(1.0, false, base.blend_mode),
                "rizum_clip_provider_clipping_resolve_pass",
            );
            state.flush()?;
        }
        GpuNormalStackSource::Container {
            children,
            opacity,
            mask_key,
            blend_mode,
        } => {
            let container_view = render_container_with_provider(
                renderer,
                provider,
                state,
                output_size,
                children,
                fallback_texture,
                pipelines,
            )?;
            let (_mask_cache, mask_view) = mask_view_with_provider(
                renderer,
                provider,
                output_size,
                *mask_key,
                mask_key
                    .map(|key| key.layer_id)
                    .unwrap_or(clip_model::LayerId(0)),
                previous_view,
            )?;
            encode_normal_source_pass(
                state.device,
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
                &container_view,
                previous_view,
                mask_view.as_ref(),
                output_view,
                generated_raster_source_uniform_bytes_with_blend(
                    *opacity,
                    mask_key.is_some(),
                    *blend_mode,
                ),
                "rizum_clip_provider_container_resolve_pass",
            );
            state.flush()?;
        }
        GpuNormalStackSource::ThroughGroup {
            children,
            opacity,
            mask_key,
        } => {
            render_through_group_with_provider(
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
            )?;
        }
        GpuNormalStackSource::SolidColor { color, opacity } => {
            encode_normal_source_pass(
                state.device,
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
            state.flush()?;
        }
        GpuNormalStackSource::LutFilter {
            lut_rgba,
            opacity,
            mask_key,
            filter_mode,
        } => {
            let (_mask_cache, mask_view) = mask_view_with_provider(
                renderer,
                provider,
                output_size,
                *mask_key,
                mask_key
                    .map(|key| key.layer_id)
                    .unwrap_or(clip_model::LayerId(0)),
                previous_view,
            )?;
            let lut_texture =
                create_lut_filter_texture(state.device, renderer_context_queue(renderer), lut_rgba)
                    .map_err(P::Error::from)?;
            let lut_view = lut_texture.create_view(&wgpu::TextureViewDescriptor::default());
            encode_lut_filter_pass(
                state.device,
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
            state.flush()?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn render_clipping_run_with_provider<P>(
    renderer: &GpuRenderer,
    provider: &mut P,
    state: &mut StreamingEncoder<'_, P::Error>,
    output_size: CanvasSize,
    base: GpuNormalRasterSource,
    clipped: &[GpuNormalRasterSource],
    pipelines: &NormalStackPipelines,
) -> Result<wgpu::TextureView, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let clipping_usage =
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let clipping_textures = [
        create_rgba8_texture(
            state.device,
            "rizum_clip_provider_clipping_cache_a",
            output_size,
            clipping_usage,
        ),
        create_rgba8_texture(
            state.device,
            "rizum_clip_provider_clipping_cache_b",
            output_size,
            clipping_usage,
        ),
    ];
    let mut previous_index = 0usize;
    let mut next_index = 1usize;

    {
        let initial_view =
            clipping_textures[previous_index].create_view(&wgpu::TextureViewDescriptor::default());
        clear_rgba8_texture(
            state.encoder_mut(),
            &initial_view,
            "rizum_clip_provider_clipping_initial_clear",
        );
    }

    {
        let (_raster_cache, source_view) =
            raster_view_with_provider(renderer, provider, state, base)?;
        let previous_view =
            clipping_textures[previous_index].create_view(&wgpu::TextureViewDescriptor::default());
        let output_view =
            clipping_textures[next_index].create_view(&wgpu::TextureViewDescriptor::default());
        let (_mask_cache, mask_view) = mask_view_with_provider(
            renderer,
            provider,
            output_size,
            base.mask_key,
            base.key.layer_id,
            &previous_view,
        )?;
        encode_normal_source_pass(
            state.device,
            state.encoder_mut(),
            &pipelines.alpha_pipeline,
            &pipelines.bind_group_layout,
            &source_view,
            &previous_view,
            mask_view.as_ref(),
            &output_view,
            raster_source_uniform_bytes(base),
            "rizum_clip_provider_clipping_base_pass",
        );
        state.flush()?;
        std::mem::swap(&mut previous_index, &mut next_index);
    }

    for clipped_source in clipped {
        let (_raster_cache, source_view) =
            raster_view_with_provider(renderer, provider, state, *clipped_source)?;
        let previous_view =
            clipping_textures[previous_index].create_view(&wgpu::TextureViewDescriptor::default());
        let output_view =
            clipping_textures[next_index].create_view(&wgpu::TextureViewDescriptor::default());
        let (_mask_cache, mask_view) = mask_view_with_provider(
            renderer,
            provider,
            output_size,
            clipped_source.mask_key,
            clipped_source.key.layer_id,
            &previous_view,
        )?;
        encode_normal_source_pass(
            state.device,
            state.encoder_mut(),
            clipped_source_pipeline(
                clipped_source.blend_mode,
                &pipelines.clipped_pipeline,
                &pipelines.clipped_byte_pipeline,
            ),
            &pipelines.bind_group_layout,
            &source_view,
            &previous_view,
            mask_view.as_ref(),
            &output_view,
            raster_source_uniform_bytes(*clipped_source),
            "rizum_clip_provider_clipping_clipped_pass",
        );
        state.flush()?;
        std::mem::swap(&mut previous_index, &mut next_index);
    }

    Ok(clipping_textures[previous_index].create_view(&wgpu::TextureViewDescriptor::default()))
}

#[allow(clippy::too_many_arguments)]
fn render_container_with_provider<P>(
    renderer: &GpuRenderer,
    provider: &mut P,
    state: &mut StreamingEncoder<'_, P::Error>,
    output_size: CanvasSize,
    children: &[GpuNormalStackSource],
    fallback_texture: &wgpu::Texture,
    pipelines: &NormalStackPipelines,
) -> Result<wgpu::TextureView, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let container_usage =
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let container_textures = [
        create_rgba8_texture(
            state.device,
            "rizum_clip_provider_container_cache_a",
            output_size,
            container_usage,
        ),
        create_rgba8_texture(
            state.device,
            "rizum_clip_provider_container_cache_b",
            output_size,
            container_usage,
        ),
    ];
    let mut previous_index = 0usize;
    let mut next_index = 1usize;

    {
        let initial_view =
            container_textures[previous_index].create_view(&wgpu::TextureViewDescriptor::default());
        clear_rgba8_texture(
            state.encoder_mut(),
            &initial_view,
            "rizum_clip_provider_container_initial_clear",
        );
    }

    for child in children {
        let previous_view =
            container_textures[previous_index].create_view(&wgpu::TextureViewDescriptor::default());
        let output_view =
            container_textures[next_index].create_view(&wgpu::TextureViewDescriptor::default());
        encode_source_with_provider(
            renderer,
            provider,
            state,
            output_size,
            child,
            &container_textures[previous_index],
            fallback_texture,
            &previous_view,
            &output_view,
            pipelines,
        )?;
        std::mem::swap(&mut previous_index, &mut next_index);
    }

    Ok(container_textures[previous_index].create_view(&wgpu::TextureViewDescriptor::default()))
}

#[allow(clippy::too_many_arguments)]
fn render_through_group_with_provider<P>(
    renderer: &GpuRenderer,
    provider: &mut P,
    state: &mut StreamingEncoder<'_, P::Error>,
    output_size: CanvasSize,
    children: &[GpuNormalStackSource],
    opacity: f32,
    mask_key: Option<GpuMaskResourceKey>,
    before_texture: &wgpu::Texture,
    fallback_texture: &wgpu::Texture,
    output_view: &wgpu::TextureView,
    pipelines: &NormalStackPipelines,
) -> Result<(), P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let through_usage =
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let through_textures = [
        create_rgba8_texture(
            state.device,
            "rizum_clip_provider_through_after_a",
            output_size,
            through_usage,
        ),
        create_rgba8_texture(
            state.device,
            "rizum_clip_provider_through_after_b",
            output_size,
            through_usage,
        ),
    ];
    let mut previous_index = 0usize;
    let mut next_index = 1usize;

    for (child_index, child) in children.iter().enumerate() {
        let previous_texture = if child_index == 0 {
            before_texture
        } else {
            &through_textures[previous_index]
        };
        let previous_view = previous_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let child_output_view =
            through_textures[next_index].create_view(&wgpu::TextureViewDescriptor::default());
        encode_source_with_provider(
            renderer,
            provider,
            state,
            output_size,
            child,
            previous_texture,
            fallback_texture,
            &previous_view,
            &child_output_view,
            pipelines,
        )?;
        std::mem::swap(&mut previous_index, &mut next_index);
    }

    let before_view = before_texture.create_view(&wgpu::TextureViewDescriptor::default());
    let after_view = if children.is_empty() {
        before_texture.create_view(&wgpu::TextureViewDescriptor::default())
    } else {
        through_textures[previous_index].create_view(&wgpu::TextureViewDescriptor::default())
    };
    let (_mask_cache, mask_view) = mask_view_with_provider(
        renderer,
        provider,
        output_size,
        mask_key,
        mask_key
            .map(|key| key.layer_id)
            .unwrap_or(clip_model::LayerId(0)),
        &before_view,
    )?;
    encode_normal_source_pass(
        state.device,
        state.encoder_mut(),
        &pipelines.through_pipeline,
        &pipelines.bind_group_layout,
        &after_view,
        &before_view,
        mask_view.as_ref(),
        output_view,
        generated_raster_source_uniform_bytes(opacity, mask_key.is_some()),
        "rizum_clip_provider_through_resolve_pass",
    );
    state.flush()?;
    Ok(())
}

fn raster_view_with_provider<P>(
    renderer: &GpuRenderer,
    provider: &mut P,
    state: &mut StreamingEncoder<'_, P::Error>,
    source: GpuNormalRasterSource,
) -> Result<(GpuRasterResourceCache, wgpu::TextureView), P::Error>
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
    state.drawn_resources.push(resource.info());
    let view = resource
        .texture()
        .create_view(&wgpu::TextureViewDescriptor::default());
    Ok((cache, view))
}

fn mask_view_with_provider<'a, P>(
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

enum MaskTextureView<'a> {
    Borrowed(&'a wgpu::TextureView),
    Owned(wgpu::TextureView),
}

impl MaskTextureView<'_> {
    fn as_ref(&self) -> &wgpu::TextureView {
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
