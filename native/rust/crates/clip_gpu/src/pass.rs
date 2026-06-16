use clip_model::CanvasSize;

use crate::blend::{clipped_source_pipeline, raster_source_pipeline};
use crate::lut_filter::{create_lut_filter_texture, encode_lut_filter_pass};
use crate::resource::GpuRasterResource;
use crate::shaders::*;
use crate::source_params::{
    generated_raster_source_uniform_bytes, generated_raster_source_uniform_bytes_with_blend,
    lut_filter_uniform_bytes, raster_source_uniform_bytes, solid_source_uniform_bytes,
};
use crate::stream_bounds::CanvasRect;
use crate::types::{
    GpuNormalRasterSource, GpuNormalStackChunk, GpuNormalStackSource, GpuRasterBlendMode,
    GpuRasterDrawOutput, GpuRasterStackOutput,
};
use crate::validation::validate_normal_stack_sources;
use crate::{
    GpuMaskResourceCache, GpuMaskResourceKey, GpuRasterResourceCache, GpuRasterResourceKey,
    GpuRenderError, GpuRenderer,
};

impl GpuRenderer {
    pub fn draw_raster_resource_to_rgba8(
        &self,
        cache: &GpuRasterResourceCache,
        key: GpuRasterResourceKey,
    ) -> Result<GpuRasterDrawOutput, GpuRenderError> {
        let output = self.draw_raster_stack_to_rgba8(cache, &[key])?;
        let info = output.drawn_resources[0];
        Ok(GpuRasterDrawOutput {
            resource_info: info,
            size: output.size,
            pixels: output.pixels,
        })
    }

    pub fn draw_raster_stack_to_rgba8(
        &self,
        cache: &GpuRasterResourceCache,
        keys: &[GpuRasterResourceKey],
    ) -> Result<GpuRasterStackOutput, GpuRenderError> {
        let StackResources {
            size: output_size,
            resources,
        } = stack_resources(cache, keys)?;

        let output_texture = self
            .context
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("rizum_clip_single_resource_output_texture"),
                size: wgpu::Extent3d {
                    width: output_size.width,
                    height: output_size.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
        let output_view = output_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group_layout =
            self.context
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("rizum_clip_copy_raster_bind_group_layout"),
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    }],
                });
        let pipeline_layout =
            self.context
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("rizum_clip_copy_raster_pipeline_layout"),
                    bind_group_layouts: &[Some(&bind_group_layout)],
                    immediate_size: 0,
                });
        let shader = self
            .context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("rizum_clip_copy_raster_shader"),
                source: wgpu::ShaderSource::Wgsl(COPY_RASTER_SHADER.into()),
            });
        let pipeline =
            self.context
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("rizum_clip_copy_raster_pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        ..wgpu::PrimitiveState::default()
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                });

        let mut encoder =
            self.context
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("rizum_clip_copy_raster_encoder"),
                });
        for (index, resource) in resources.iter().enumerate() {
            let source_view = resource
                .texture()
                .create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = self
                .context
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("rizum_clip_copy_raster_bind_group"),
                    layout: &bind_group_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&source_view),
                    }],
                });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rizum_clip_copy_raster_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if index == 0 {
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.context.queue.submit([encoder.finish()]);

        let pixels =
            self.read_texture_rgba8(&output_texture, output_size.width, output_size.height)?;
        Ok(GpuRasterStackOutput {
            drawn_resources: resources.iter().map(|resource| resource.info()).collect(),
            size: output_size,
            pixels,
        })
    }

    pub fn draw_normal_raster_stack_to_rgba8(
        &self,
        cache: &GpuRasterResourceCache,
        keys: &[GpuRasterResourceKey],
    ) -> Result<GpuRasterStackOutput, GpuRenderError> {
        let StackResources {
            size: output_size,
            resources: _,
        } = stack_resources(cache, keys)?;
        let sources: Vec<_> = keys
            .iter()
            .copied()
            .map(|key| {
                GpuNormalStackSource::Raster(GpuNormalRasterSource {
                    key,
                    opacity: 1.0,
                    mask_key: None,
                    offset_x: 0,
                    offset_y: 0,
                    blend_mode: GpuRasterBlendMode::Normal,
                })
            })
            .collect();
        self.draw_normal_stack_to_rgba8(cache, None, output_size, &sources)
    }

    pub fn draw_normal_stack_to_rgba8(
        &self,
        cache: &GpuRasterResourceCache,
        mask_cache: Option<&GpuMaskResourceCache>,
        output_size: CanvasSize,
        sources: &[GpuNormalStackSource],
    ) -> Result<GpuRasterStackOutput, GpuRenderError> {
        if sources.is_empty() {
            return Err(GpuRenderError::EmptyRasterStack);
        }
        if output_size.width == 0 || output_size.height == 0 {
            return Err(GpuRenderError::InvalidImageSize);
        }
        let drawn_resources =
            validate_normal_stack_sources(cache, mask_cache, output_size, sources)?;

        let accum_usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC;
        let accum_textures = [
            create_rgba8_texture(
                &self.context.device,
                "rizum_clip_normal_alpha_accum_a",
                output_size,
                accum_usage,
            ),
            create_rgba8_texture(
                &self.context.device,
                "rizum_clip_normal_alpha_accum_b",
                output_size,
                accum_usage,
            ),
        ];
        let pipelines = NormalStackPipelines::new(&self.context.device);

        let mut encoder =
            self.context
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("rizum_clip_normal_alpha_encoder"),
                });
        let previous_index = 0usize;
        let next_index = 1usize;
        {
            let initial_view =
                accum_textures[previous_index].create_view(&wgpu::TextureViewDescriptor::default());
            clear_rgba8_texture(
                &mut encoder,
                &initial_view,
                "rizum_clip_normal_alpha_initial_clear",
            );
        }
        let (previous_index, _next_index) = self.encode_normal_stack_sources(
            cache,
            mask_cache,
            output_size,
            sources,
            &accum_textures,
            previous_index,
            next_index,
            &pipelines,
            &mut encoder,
        )?;
        self.context.queue.submit([encoder.finish()]);

        let pixels = self.read_texture_rgba8(
            &accum_textures[previous_index],
            output_size.width,
            output_size.height,
        )?;
        Ok(GpuRasterStackOutput {
            drawn_resources,
            size: output_size,
            pixels,
        })
    }

    pub fn draw_normal_stack_streamed_to_rgba8<E, F>(
        &self,
        output_size: CanvasSize,
        mut next_chunk: F,
    ) -> Result<GpuRasterStackOutput, E>
    where
        E: From<GpuRenderError>,
        F: FnMut(usize) -> Result<Option<GpuNormalStackChunk>, E>,
    {
        if output_size.width == 0 || output_size.height == 0 {
            return Err(E::from(GpuRenderError::InvalidImageSize));
        }

        let accum_usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC;
        let accum_textures = [
            create_rgba8_texture(
                &self.context.device,
                "rizum_clip_stream_normal_accum_a",
                output_size,
                accum_usage,
            ),
            create_rgba8_texture(
                &self.context.device,
                "rizum_clip_stream_normal_accum_b",
                output_size,
                accum_usage,
            ),
        ];
        let pipelines = NormalStackPipelines::new(&self.context.device);

        let mut encoder =
            self.context
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("rizum_clip_stream_normal_encoder"),
                });
        let mut previous_index = 0usize;
        let mut next_index = 1usize;
        {
            let initial_view =
                accum_textures[previous_index].create_view(&wgpu::TextureViewDescriptor::default());
            clear_rgba8_texture(
                &mut encoder,
                &initial_view,
                "rizum_clip_stream_normal_initial_clear",
            );
        }

        let mut chunk_index = 0usize;
        let mut has_sources = false;
        let mut drawn_resources = Vec::new();
        while let Some(chunk) = next_chunk(chunk_index)? {
            if chunk.sources.is_empty() {
                chunk_index += 1;
                continue;
            }
            let chunk_drawn = validate_normal_stack_sources(
                &chunk.raster_cache,
                chunk.mask_cache.as_ref(),
                output_size,
                &chunk.sources,
            )
            .map_err(E::from)?;
            drawn_resources.extend(chunk_drawn);

            let (updated_previous, updated_next) = self
                .encode_normal_stack_sources(
                    &chunk.raster_cache,
                    chunk.mask_cache.as_ref(),
                    output_size,
                    &chunk.sources,
                    &accum_textures,
                    previous_index,
                    next_index,
                    &pipelines,
                    &mut encoder,
                )
                .map_err(E::from)?;
            previous_index = updated_previous;
            next_index = updated_next;
            has_sources = true;

            self.context.queue.submit([encoder.finish()]);
            self.context
                .device
                .poll(wgpu::PollType::wait_indefinitely())
                .map_err(|err| E::from(GpuRenderError::PollFailed(err.to_string())))?;
            drop(chunk);

            encoder = self
                .context
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("rizum_clip_stream_normal_encoder"),
                });
            chunk_index += 1;
        }

        if !has_sources {
            return Err(E::from(GpuRenderError::EmptyRasterStack));
        }

        let pixels = self
            .read_texture_rgba8(
                &accum_textures[previous_index],
                output_size.width,
                output_size.height,
            )
            .map_err(E::from)?;
        Ok(GpuRasterStackOutput {
            drawn_resources,
            size: output_size,
            pixels,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_normal_stack_sources(
        &self,
        cache: &GpuRasterResourceCache,
        mask_cache: Option<&GpuMaskResourceCache>,
        output_size: CanvasSize,
        sources: &[GpuNormalStackSource],
        accum_textures: &[wgpu::Texture; 2],
        mut previous_index: usize,
        mut next_index: usize,
        pipelines: &NormalStackPipelines,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<(usize, usize), GpuRenderError> {
        let bind_group_layout = &pipelines.bind_group_layout;
        let alpha_pipeline = &pipelines.alpha_pipeline;
        let clipped_pipeline = &pipelines.clipped_pipeline;
        let clipped_byte_pipeline = &pipelines.clipped_byte_pipeline;
        let through_pipeline = &pipelines.through_pipeline;
        let add_glow_pipeline = &pipelines.add_glow_pipeline;
        let color_dodge_pipeline = &pipelines.color_dodge_pipeline;
        let color_burn_pipeline = &pipelines.color_burn_pipeline;
        let glow_dodge_pipeline = &pipelines.glow_dodge_pipeline;
        let standard_blend_pipeline = &pipelines.standard_blend_pipeline;
        let lut_filter_pipeline = &pipelines.lut_filter_pipeline;
        for source in sources {
            let previous_view =
                accum_textures[previous_index].create_view(&wgpu::TextureViewDescriptor::default());
            let output_view =
                accum_textures[next_index].create_view(&wgpu::TextureViewDescriptor::default());
            match source {
                GpuNormalStackSource::Raster(raster) => {
                    let source_view = raster_texture_view(cache, *raster)?;
                    let mask_view =
                        mask_texture_view(mask_cache, *raster, &accum_textures[previous_index])?;
                    encode_normal_source_pass(
                        &self.context.device,
                        encoder,
                        raster_source_pipeline(
                            raster.blend_mode,
                            alpha_pipeline,
                            add_glow_pipeline,
                            color_dodge_pipeline,
                            color_burn_pipeline,
                            glow_dodge_pipeline,
                            standard_blend_pipeline,
                        ),
                        bind_group_layout,
                        &source_view,
                        &previous_view,
                        &mask_view,
                        &output_view,
                        raster_source_uniform_bytes(*raster),
                        "rizum_clip_normal_alpha_pass",
                    );
                }
                GpuNormalStackSource::ClippingRun { base, clipped } => {
                    let clipping_view = self.render_clipping_run_source(
                        cache,
                        mask_cache,
                        output_size,
                        *base,
                        clipped,
                        &accum_textures[previous_index],
                        bind_group_layout,
                        alpha_pipeline,
                        clipped_pipeline,
                        clipped_byte_pipeline,
                        encoder,
                    )?;
                    let mask_view = accum_textures[previous_index]
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    encode_normal_source_pass(
                        &self.context.device,
                        encoder,
                        raster_source_pipeline(
                            base.blend_mode,
                            alpha_pipeline,
                            add_glow_pipeline,
                            color_dodge_pipeline,
                            color_burn_pipeline,
                            glow_dodge_pipeline,
                            standard_blend_pipeline,
                        ),
                        bind_group_layout,
                        &clipping_view,
                        &previous_view,
                        &mask_view,
                        &output_view,
                        generated_raster_source_uniform_bytes_with_blend(
                            1.0,
                            false,
                            base.blend_mode,
                        ),
                        "rizum_clip_normal_alpha_clipping_resolve_pass",
                    );
                }
                GpuNormalStackSource::Container {
                    children,
                    opacity,
                    mask_key,
                    blend_mode,
                } => {
                    let container_view = self.render_container_source(
                        cache,
                        mask_cache,
                        output_size,
                        children,
                        &accum_textures[previous_index],
                        bind_group_layout,
                        alpha_pipeline,
                        clipped_pipeline,
                        clipped_byte_pipeline,
                        through_pipeline,
                        add_glow_pipeline,
                        color_dodge_pipeline,
                        color_burn_pipeline,
                        glow_dodge_pipeline,
                        standard_blend_pipeline,
                        lut_filter_pipeline,
                        encoder,
                    )?;
                    let mask_view = source_mask_texture_view(
                        mask_cache,
                        *mask_key,
                        &accum_textures[previous_index],
                    )?;
                    encode_normal_source_pass(
                        &self.context.device,
                        encoder,
                        raster_source_pipeline(
                            *blend_mode,
                            alpha_pipeline,
                            add_glow_pipeline,
                            color_dodge_pipeline,
                            color_burn_pipeline,
                            glow_dodge_pipeline,
                            standard_blend_pipeline,
                        ),
                        bind_group_layout,
                        &container_view,
                        &previous_view,
                        &mask_view,
                        &output_view,
                        generated_raster_source_uniform_bytes_with_blend(
                            *opacity,
                            mask_key.is_some(),
                            *blend_mode,
                        ),
                        "rizum_clip_normal_alpha_container_resolve_pass",
                    );
                }
                GpuNormalStackSource::ThroughGroup {
                    children,
                    opacity,
                    mask_key,
                } => {
                    self.render_through_group_to_output(
                        cache,
                        mask_cache,
                        output_size,
                        children,
                        *opacity,
                        *mask_key,
                        &accum_textures[previous_index],
                        &accum_textures[previous_index],
                        &output_view,
                        bind_group_layout,
                        alpha_pipeline,
                        clipped_pipeline,
                        clipped_byte_pipeline,
                        through_pipeline,
                        add_glow_pipeline,
                        color_dodge_pipeline,
                        color_burn_pipeline,
                        glow_dodge_pipeline,
                        standard_blend_pipeline,
                        lut_filter_pipeline,
                        encoder,
                    )?;
                }
                GpuNormalStackSource::SolidColor { color, opacity } => {
                    let source_view = accum_textures[previous_index]
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    let mask_view = accum_textures[previous_index]
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    encode_normal_source_pass(
                        &self.context.device,
                        encoder,
                        alpha_pipeline,
                        bind_group_layout,
                        &source_view,
                        &previous_view,
                        &mask_view,
                        &output_view,
                        solid_source_uniform_bytes(*color, *opacity),
                        "rizum_clip_normal_alpha_solid_pass",
                    );
                }
                GpuNormalStackSource::LutFilter {
                    lut_rgba,
                    opacity,
                    mask_key,
                    filter_mode,
                } => {
                    let mask_view = source_mask_texture_view(
                        mask_cache,
                        *mask_key,
                        &accum_textures[previous_index],
                    )?;
                    let lut_texture = create_lut_filter_texture(
                        &self.context.device,
                        &self.context.queue,
                        lut_rgba,
                    )?;
                    let lut_view = lut_texture.create_view(&wgpu::TextureViewDescriptor::default());
                    encode_lut_filter_pass(
                        &self.context.device,
                        encoder,
                        lut_filter_pipeline,
                        bind_group_layout,
                        &previous_view,
                        &mask_view,
                        &lut_view,
                        &output_view,
                        lut_filter_uniform_bytes(*opacity, mask_key.is_some(), *filter_mode),
                        "rizum_clip_lut_filter_pass",
                    );
                }
            }
            std::mem::swap(&mut previous_index, &mut next_index);
        }
        Ok((previous_index, next_index))
    }

    #[allow(clippy::too_many_arguments)]
    fn render_clipping_run_source(
        &self,
        cache: &GpuRasterResourceCache,
        mask_cache: Option<&GpuMaskResourceCache>,
        output_size: CanvasSize,
        base: GpuNormalRasterSource,
        clipped: &[GpuNormalRasterSource],
        fallback_texture: &wgpu::Texture,
        bind_group_layout: &wgpu::BindGroupLayout,
        alpha_pipeline: &wgpu::RenderPipeline,
        clipped_pipeline: &wgpu::RenderPipeline,
        clipped_byte_pipeline: &wgpu::RenderPipeline,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<wgpu::TextureView, GpuRenderError> {
        let clipping_usage =
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let clipping_textures = [
            create_rgba8_texture(
                &self.context.device,
                "rizum_clip_clipping_run_cache_a",
                output_size,
                clipping_usage,
            ),
            create_rgba8_texture(
                &self.context.device,
                "rizum_clip_clipping_run_cache_b",
                output_size,
                clipping_usage,
            ),
        ];
        let mut previous_index = 0usize;
        let mut next_index = 1usize;

        {
            let initial_view = clipping_textures[previous_index]
                .create_view(&wgpu::TextureViewDescriptor::default());
            clear_rgba8_texture(
                encoder,
                &initial_view,
                "rizum_clip_clipping_run_initial_clear",
            );
        }

        {
            let source_view = raster_texture_view(cache, base)?;
            let previous_view = clipping_textures[previous_index]
                .create_view(&wgpu::TextureViewDescriptor::default());
            let output_view =
                clipping_textures[next_index].create_view(&wgpu::TextureViewDescriptor::default());
            let mask_view = mask_texture_view(mask_cache, base, fallback_texture)?;
            encode_normal_source_pass(
                &self.context.device,
                encoder,
                alpha_pipeline,
                bind_group_layout,
                &source_view,
                &previous_view,
                &mask_view,
                &output_view,
                raster_source_uniform_bytes(base),
                "rizum_clip_clipping_run_base_pass",
            );
            std::mem::swap(&mut previous_index, &mut next_index);
        }

        for clipped_source in clipped {
            let source_view = raster_texture_view(cache, *clipped_source)?;
            let previous_view = clipping_textures[previous_index]
                .create_view(&wgpu::TextureViewDescriptor::default());
            let output_view =
                clipping_textures[next_index].create_view(&wgpu::TextureViewDescriptor::default());
            let mask_view = mask_texture_view(mask_cache, *clipped_source, fallback_texture)?;
            encode_normal_source_pass(
                &self.context.device,
                encoder,
                clipped_source_pipeline(
                    clipped_source.blend_mode,
                    clipped_pipeline,
                    clipped_byte_pipeline,
                ),
                bind_group_layout,
                &source_view,
                &previous_view,
                &mask_view,
                &output_view,
                raster_source_uniform_bytes(*clipped_source),
                "rizum_clip_clipping_run_clipped_pass",
            );
            std::mem::swap(&mut previous_index, &mut next_index);
        }

        Ok(clipping_textures[previous_index].create_view(&wgpu::TextureViewDescriptor::default()))
    }

    #[allow(clippy::too_many_arguments)]
    fn render_container_source(
        &self,
        cache: &GpuRasterResourceCache,
        mask_cache: Option<&GpuMaskResourceCache>,
        output_size: CanvasSize,
        children: &[GpuNormalStackSource],
        fallback_texture: &wgpu::Texture,
        bind_group_layout: &wgpu::BindGroupLayout,
        alpha_pipeline: &wgpu::RenderPipeline,
        clipped_pipeline: &wgpu::RenderPipeline,
        clipped_byte_pipeline: &wgpu::RenderPipeline,
        through_pipeline: &wgpu::RenderPipeline,
        add_glow_pipeline: &wgpu::RenderPipeline,
        color_dodge_pipeline: &wgpu::RenderPipeline,
        color_burn_pipeline: &wgpu::RenderPipeline,
        glow_dodge_pipeline: &wgpu::RenderPipeline,
        standard_blend_pipeline: &wgpu::RenderPipeline,
        lut_filter_pipeline: &wgpu::RenderPipeline,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<wgpu::TextureView, GpuRenderError> {
        let container_usage =
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let container_textures = [
            create_rgba8_texture(
                &self.context.device,
                "rizum_clip_container_cache_a",
                output_size,
                container_usage,
            ),
            create_rgba8_texture(
                &self.context.device,
                "rizum_clip_container_cache_b",
                output_size,
                container_usage,
            ),
        ];
        let mut previous_index = 0usize;
        let mut next_index = 1usize;

        {
            let initial_view = container_textures[previous_index]
                .create_view(&wgpu::TextureViewDescriptor::default());
            clear_rgba8_texture(encoder, &initial_view, "rizum_clip_container_initial_clear");
        }

        for child in children {
            let previous_view = container_textures[previous_index]
                .create_view(&wgpu::TextureViewDescriptor::default());
            let output_view =
                container_textures[next_index].create_view(&wgpu::TextureViewDescriptor::default());
            match child {
                GpuNormalStackSource::Raster(raster) => {
                    let source_view = raster_texture_view(cache, *raster)?;
                    let mask_view = mask_texture_view(mask_cache, *raster, fallback_texture)?;
                    encode_normal_source_pass(
                        &self.context.device,
                        encoder,
                        raster_source_pipeline(
                            raster.blend_mode,
                            alpha_pipeline,
                            add_glow_pipeline,
                            color_dodge_pipeline,
                            color_burn_pipeline,
                            glow_dodge_pipeline,
                            standard_blend_pipeline,
                        ),
                        bind_group_layout,
                        &source_view,
                        &previous_view,
                        &mask_view,
                        &output_view,
                        raster_source_uniform_bytes(*raster),
                        "rizum_clip_container_raster_pass",
                    );
                }
                GpuNormalStackSource::ClippingRun { base, clipped } => {
                    let clipping_view = self.render_clipping_run_source(
                        cache,
                        mask_cache,
                        output_size,
                        *base,
                        clipped,
                        fallback_texture,
                        bind_group_layout,
                        alpha_pipeline,
                        clipped_pipeline,
                        clipped_byte_pipeline,
                        encoder,
                    )?;
                    let mask_view =
                        fallback_texture.create_view(&wgpu::TextureViewDescriptor::default());
                    encode_normal_source_pass(
                        &self.context.device,
                        encoder,
                        raster_source_pipeline(
                            base.blend_mode,
                            alpha_pipeline,
                            add_glow_pipeline,
                            color_dodge_pipeline,
                            color_burn_pipeline,
                            glow_dodge_pipeline,
                            standard_blend_pipeline,
                        ),
                        bind_group_layout,
                        &clipping_view,
                        &previous_view,
                        &mask_view,
                        &output_view,
                        generated_raster_source_uniform_bytes_with_blend(
                            1.0,
                            false,
                            base.blend_mode,
                        ),
                        "rizum_clip_container_clipping_resolve_pass",
                    );
                }
                GpuNormalStackSource::Container {
                    children,
                    opacity,
                    mask_key,
                    blend_mode,
                } => {
                    let container_view = self.render_container_source(
                        cache,
                        mask_cache,
                        output_size,
                        children,
                        fallback_texture,
                        bind_group_layout,
                        alpha_pipeline,
                        clipped_pipeline,
                        clipped_byte_pipeline,
                        through_pipeline,
                        add_glow_pipeline,
                        color_dodge_pipeline,
                        color_burn_pipeline,
                        glow_dodge_pipeline,
                        standard_blend_pipeline,
                        lut_filter_pipeline,
                        encoder,
                    )?;
                    let mask_view =
                        source_mask_texture_view(mask_cache, *mask_key, fallback_texture)?;
                    encode_normal_source_pass(
                        &self.context.device,
                        encoder,
                        raster_source_pipeline(
                            *blend_mode,
                            alpha_pipeline,
                            add_glow_pipeline,
                            color_dodge_pipeline,
                            color_burn_pipeline,
                            glow_dodge_pipeline,
                            standard_blend_pipeline,
                        ),
                        bind_group_layout,
                        &container_view,
                        &previous_view,
                        &mask_view,
                        &output_view,
                        generated_raster_source_uniform_bytes_with_blend(
                            *opacity,
                            mask_key.is_some(),
                            *blend_mode,
                        ),
                        "rizum_clip_container_nested_resolve_pass",
                    );
                }
                GpuNormalStackSource::ThroughGroup {
                    children,
                    opacity,
                    mask_key,
                } => {
                    self.render_through_group_to_output(
                        cache,
                        mask_cache,
                        output_size,
                        children,
                        *opacity,
                        *mask_key,
                        &container_textures[previous_index],
                        fallback_texture,
                        &output_view,
                        bind_group_layout,
                        alpha_pipeline,
                        clipped_pipeline,
                        clipped_byte_pipeline,
                        through_pipeline,
                        add_glow_pipeline,
                        color_dodge_pipeline,
                        color_burn_pipeline,
                        glow_dodge_pipeline,
                        standard_blend_pipeline,
                        lut_filter_pipeline,
                        encoder,
                    )?;
                }
                GpuNormalStackSource::SolidColor { color, opacity } => {
                    let source_view =
                        fallback_texture.create_view(&wgpu::TextureViewDescriptor::default());
                    let mask_view =
                        fallback_texture.create_view(&wgpu::TextureViewDescriptor::default());
                    encode_normal_source_pass(
                        &self.context.device,
                        encoder,
                        alpha_pipeline,
                        bind_group_layout,
                        &source_view,
                        &previous_view,
                        &mask_view,
                        &output_view,
                        solid_source_uniform_bytes(*color, *opacity),
                        "rizum_clip_container_solid_pass",
                    );
                }
                GpuNormalStackSource::LutFilter {
                    lut_rgba,
                    opacity,
                    mask_key,
                    filter_mode,
                } => {
                    let mask_view =
                        source_mask_texture_view(mask_cache, *mask_key, fallback_texture)?;
                    let lut_texture = create_lut_filter_texture(
                        &self.context.device,
                        &self.context.queue,
                        lut_rgba,
                    )?;
                    let lut_view = lut_texture.create_view(&wgpu::TextureViewDescriptor::default());
                    encode_lut_filter_pass(
                        &self.context.device,
                        encoder,
                        lut_filter_pipeline,
                        bind_group_layout,
                        &previous_view,
                        &mask_view,
                        &lut_view,
                        &output_view,
                        lut_filter_uniform_bytes(*opacity, mask_key.is_some(), *filter_mode),
                        "rizum_clip_container_lut_filter_pass",
                    );
                }
            }
            std::mem::swap(&mut previous_index, &mut next_index);
        }

        Ok(container_textures[previous_index].create_view(&wgpu::TextureViewDescriptor::default()))
    }

    #[allow(clippy::too_many_arguments)]
    fn render_through_group_to_output(
        &self,
        cache: &GpuRasterResourceCache,
        mask_cache: Option<&GpuMaskResourceCache>,
        output_size: CanvasSize,
        children: &[GpuNormalStackSource],
        opacity: f32,
        mask_key: Option<GpuMaskResourceKey>,
        before_texture: &wgpu::Texture,
        fallback_texture: &wgpu::Texture,
        output_view: &wgpu::TextureView,
        bind_group_layout: &wgpu::BindGroupLayout,
        alpha_pipeline: &wgpu::RenderPipeline,
        clipped_pipeline: &wgpu::RenderPipeline,
        clipped_byte_pipeline: &wgpu::RenderPipeline,
        through_pipeline: &wgpu::RenderPipeline,
        add_glow_pipeline: &wgpu::RenderPipeline,
        color_dodge_pipeline: &wgpu::RenderPipeline,
        color_burn_pipeline: &wgpu::RenderPipeline,
        glow_dodge_pipeline: &wgpu::RenderPipeline,
        standard_blend_pipeline: &wgpu::RenderPipeline,
        lut_filter_pipeline: &wgpu::RenderPipeline,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<(), GpuRenderError> {
        let through_usage =
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let through_textures = [
            create_rgba8_texture(
                &self.context.device,
                "rizum_clip_through_group_after_a",
                output_size,
                through_usage,
            ),
            create_rgba8_texture(
                &self.context.device,
                "rizum_clip_through_group_after_b",
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
            let previous_view =
                previous_texture.create_view(&wgpu::TextureViewDescriptor::default());
            let child_output_view =
                through_textures[next_index].create_view(&wgpu::TextureViewDescriptor::default());
            match child {
                GpuNormalStackSource::Raster(raster) => {
                    let source_view = raster_texture_view(cache, *raster)?;
                    let mask_view = mask_texture_view(mask_cache, *raster, fallback_texture)?;
                    encode_normal_source_pass(
                        &self.context.device,
                        encoder,
                        raster_source_pipeline(
                            raster.blend_mode,
                            alpha_pipeline,
                            add_glow_pipeline,
                            color_dodge_pipeline,
                            color_burn_pipeline,
                            glow_dodge_pipeline,
                            standard_blend_pipeline,
                        ),
                        bind_group_layout,
                        &source_view,
                        &previous_view,
                        &mask_view,
                        &child_output_view,
                        raster_source_uniform_bytes(*raster),
                        "rizum_clip_through_group_raster_pass",
                    );
                }
                GpuNormalStackSource::ClippingRun { base, clipped } => {
                    let clipping_view = self.render_clipping_run_source(
                        cache,
                        mask_cache,
                        output_size,
                        *base,
                        clipped,
                        fallback_texture,
                        bind_group_layout,
                        alpha_pipeline,
                        clipped_pipeline,
                        clipped_byte_pipeline,
                        encoder,
                    )?;
                    let mask_view =
                        fallback_texture.create_view(&wgpu::TextureViewDescriptor::default());
                    encode_normal_source_pass(
                        &self.context.device,
                        encoder,
                        raster_source_pipeline(
                            base.blend_mode,
                            alpha_pipeline,
                            add_glow_pipeline,
                            color_dodge_pipeline,
                            color_burn_pipeline,
                            glow_dodge_pipeline,
                            standard_blend_pipeline,
                        ),
                        bind_group_layout,
                        &clipping_view,
                        &previous_view,
                        &mask_view,
                        &child_output_view,
                        generated_raster_source_uniform_bytes_with_blend(
                            1.0,
                            false,
                            base.blend_mode,
                        ),
                        "rizum_clip_through_group_clipping_resolve_pass",
                    );
                }
                GpuNormalStackSource::Container {
                    children,
                    opacity,
                    mask_key,
                    blend_mode,
                } => {
                    let container_view = self.render_container_source(
                        cache,
                        mask_cache,
                        output_size,
                        children,
                        fallback_texture,
                        bind_group_layout,
                        alpha_pipeline,
                        clipped_pipeline,
                        clipped_byte_pipeline,
                        through_pipeline,
                        add_glow_pipeline,
                        color_dodge_pipeline,
                        color_burn_pipeline,
                        glow_dodge_pipeline,
                        standard_blend_pipeline,
                        lut_filter_pipeline,
                        encoder,
                    )?;
                    let mask_view =
                        source_mask_texture_view(mask_cache, *mask_key, fallback_texture)?;
                    encode_normal_source_pass(
                        &self.context.device,
                        encoder,
                        raster_source_pipeline(
                            *blend_mode,
                            alpha_pipeline,
                            add_glow_pipeline,
                            color_dodge_pipeline,
                            color_burn_pipeline,
                            glow_dodge_pipeline,
                            standard_blend_pipeline,
                        ),
                        bind_group_layout,
                        &container_view,
                        &previous_view,
                        &mask_view,
                        &child_output_view,
                        generated_raster_source_uniform_bytes_with_blend(
                            *opacity,
                            mask_key.is_some(),
                            *blend_mode,
                        ),
                        "rizum_clip_through_group_container_resolve_pass",
                    );
                }
                GpuNormalStackSource::ThroughGroup {
                    children,
                    opacity,
                    mask_key,
                } => {
                    self.render_through_group_to_output(
                        cache,
                        mask_cache,
                        output_size,
                        children,
                        *opacity,
                        *mask_key,
                        previous_texture,
                        fallback_texture,
                        &child_output_view,
                        bind_group_layout,
                        alpha_pipeline,
                        clipped_pipeline,
                        clipped_byte_pipeline,
                        through_pipeline,
                        add_glow_pipeline,
                        color_dodge_pipeline,
                        color_burn_pipeline,
                        glow_dodge_pipeline,
                        standard_blend_pipeline,
                        lut_filter_pipeline,
                        encoder,
                    )?;
                }
                GpuNormalStackSource::SolidColor { color, opacity } => {
                    let source_view =
                        fallback_texture.create_view(&wgpu::TextureViewDescriptor::default());
                    let mask_view =
                        fallback_texture.create_view(&wgpu::TextureViewDescriptor::default());
                    encode_normal_source_pass(
                        &self.context.device,
                        encoder,
                        alpha_pipeline,
                        bind_group_layout,
                        &source_view,
                        &previous_view,
                        &mask_view,
                        &child_output_view,
                        solid_source_uniform_bytes(*color, *opacity),
                        "rizum_clip_through_group_solid_pass",
                    );
                }
                GpuNormalStackSource::LutFilter {
                    lut_rgba,
                    opacity,
                    mask_key,
                    filter_mode,
                } => {
                    let mask_view =
                        source_mask_texture_view(mask_cache, *mask_key, fallback_texture)?;
                    let lut_texture = create_lut_filter_texture(
                        &self.context.device,
                        &self.context.queue,
                        lut_rgba,
                    )?;
                    let lut_view = lut_texture.create_view(&wgpu::TextureViewDescriptor::default());
                    encode_lut_filter_pass(
                        &self.context.device,
                        encoder,
                        lut_filter_pipeline,
                        bind_group_layout,
                        &previous_view,
                        &mask_view,
                        &lut_view,
                        &child_output_view,
                        lut_filter_uniform_bytes(*opacity, mask_key.is_some(), *filter_mode),
                        "rizum_clip_through_group_lut_filter_pass",
                    );
                }
            }
            std::mem::swap(&mut previous_index, &mut next_index);
        }

        let before_view = before_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let after_view = if children.is_empty() {
            before_texture.create_view(&wgpu::TextureViewDescriptor::default())
        } else {
            through_textures[previous_index].create_view(&wgpu::TextureViewDescriptor::default())
        };
        let mask_view = source_mask_texture_view(mask_cache, mask_key, fallback_texture)?;
        encode_normal_source_pass(
            &self.context.device,
            encoder,
            through_pipeline,
            bind_group_layout,
            &after_view,
            &before_view,
            &mask_view,
            output_view,
            generated_raster_source_uniform_bytes(opacity, mask_key.is_some()),
            "rizum_clip_through_group_resolve_pass",
        );
        Ok(())
    }
}

pub(crate) struct NormalStackPipelines {
    pub(crate) bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) alpha_pipeline: wgpu::RenderPipeline,
    pub(crate) clipped_pipeline: wgpu::RenderPipeline,
    pub(crate) clipped_byte_pipeline: wgpu::RenderPipeline,
    pub(crate) through_pipeline: wgpu::RenderPipeline,
    pub(crate) add_glow_pipeline: wgpu::RenderPipeline,
    pub(crate) color_dodge_pipeline: wgpu::RenderPipeline,
    pub(crate) color_burn_pipeline: wgpu::RenderPipeline,
    pub(crate) glow_dodge_pipeline: wgpu::RenderPipeline,
    pub(crate) standard_blend_pipeline: wgpu::RenderPipeline,
    pub(crate) lut_filter_pipeline: wgpu::RenderPipeline,
}

impl NormalStackPipelines {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let bind_group_layout = create_normal_source_bind_group_layout(device);
        let alpha_pipeline = create_normal_source_pipeline(
            device,
            &bind_group_layout,
            NORMAL_ALPHA_OVER_SHADER,
            "rizum_clip_normal_alpha_shader",
            "rizum_clip_normal_alpha_pipeline",
            "rizum_clip_normal_alpha_pipeline_layout",
        );
        let clipped_pipeline = create_normal_source_pipeline(
            device,
            &bind_group_layout,
            CLIPPED_NORMAL_PRESERVE_SHADER,
            "rizum_clip_clipped_normal_preserve_shader",
            "rizum_clip_clipped_normal_preserve_pipeline",
            "rizum_clip_clipped_normal_preserve_pipeline_layout",
        );
        let clipped_byte_pipeline = create_normal_source_pipeline(
            device,
            &bind_group_layout,
            CLIPPED_BYTE_PRESERVE_SHADER,
            "rizum_clip_clipped_byte_preserve_shader",
            "rizum_clip_clipped_byte_preserve_pipeline",
            "rizum_clip_clipped_byte_preserve_pipeline_layout",
        );
        let through_pipeline = create_normal_source_pipeline(
            device,
            &bind_group_layout,
            THROUGH_GROUP_RESOLVE_SHADER,
            "rizum_clip_through_group_resolve_shader",
            "rizum_clip_through_group_resolve_pipeline",
            "rizum_clip_through_group_resolve_pipeline_layout",
        );
        let add_glow_pipeline = create_normal_source_pipeline(
            device,
            &bind_group_layout,
            ADD_GLOW_SHADER,
            "rizum_clip_add_glow_shader",
            "rizum_clip_add_glow_pipeline",
            "rizum_clip_add_glow_pipeline_layout",
        );
        let color_dodge_pipeline = create_normal_source_pipeline(
            device,
            &bind_group_layout,
            COLOR_DODGE_SHADER,
            "rizum_clip_color_dodge_shader",
            "rizum_clip_color_dodge_pipeline",
            "rizum_clip_color_dodge_pipeline_layout",
        );
        let color_burn_pipeline = create_normal_source_pipeline(
            device,
            &bind_group_layout,
            COLOR_BURN_SHADER,
            "rizum_clip_color_burn_shader",
            "rizum_clip_color_burn_pipeline",
            "rizum_clip_color_burn_pipeline_layout",
        );
        let glow_dodge_pipeline = create_normal_source_pipeline(
            device,
            &bind_group_layout,
            GLOW_DODGE_SHADER,
            "rizum_clip_glow_dodge_shader",
            "rizum_clip_glow_dodge_pipeline",
            "rizum_clip_glow_dodge_pipeline_layout",
        );
        let standard_blend_pipeline = create_normal_source_pipeline(
            device,
            &bind_group_layout,
            STANDARD_BLEND_SHADER,
            "rizum_clip_standard_blend_shader",
            "rizum_clip_standard_blend_pipeline",
            "rizum_clip_standard_blend_pipeline_layout",
        );
        let lut_filter_pipeline = create_normal_source_pipeline(
            device,
            &bind_group_layout,
            LUT_FILTER_SHADER,
            "rizum_clip_lut_filter_shader",
            "rizum_clip_lut_filter_pipeline",
            "rizum_clip_lut_filter_pipeline_layout",
        );
        Self {
            bind_group_layout,
            alpha_pipeline,
            clipped_pipeline,
            clipped_byte_pipeline,
            through_pipeline,
            add_glow_pipeline,
            color_dodge_pipeline,
            color_burn_pipeline,
            glow_dodge_pipeline,
            standard_blend_pipeline,
            lut_filter_pipeline,
        }
    }
}

pub(crate) const WHITE_TRANSPARENT: wgpu::Color = wgpu::Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.0,
};

fn create_normal_source_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rizum_clip_normal_source_bind_group_layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    })
}

fn create_normal_source_pipeline(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    shader_source: &'static str,
    shader_label: &'static str,
    pipeline_label: &'static str,
    pipeline_layout_label: &'static str,
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(pipeline_layout_label),
        bind_group_layouts: &[Some(bind_group_layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(shader_label),
        source: wgpu::ShaderSource::Wgsl(shader_source.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(pipeline_label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..wgpu::PrimitiveState::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

pub(crate) fn create_rgba8_texture(
    device: &wgpu::Device,
    label: &'static str,
    size: CanvasSize,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage,
        view_formats: &[],
    })
}

fn raster_texture_view(
    cache: &GpuRasterResourceCache,
    source: GpuNormalRasterSource,
) -> Result<wgpu::TextureView, GpuRenderError> {
    Ok(cache
        .resource(source.key)
        .ok_or(GpuRenderError::MissingRasterResource {
            layer_id: source.key.layer_id,
            render_mipmap_id: source.key.render_mipmap_id,
        })?
        .texture()
        .create_view(&wgpu::TextureViewDescriptor::default()))
}

fn mask_texture_view(
    mask_cache: Option<&GpuMaskResourceCache>,
    source: GpuNormalRasterSource,
    fallback_texture: &wgpu::Texture,
) -> Result<wgpu::TextureView, GpuRenderError> {
    source_mask_texture_view(mask_cache, source.mask_key, fallback_texture)
}

fn source_mask_texture_view(
    mask_cache: Option<&GpuMaskResourceCache>,
    mask_key: Option<GpuMaskResourceKey>,
    fallback_texture: &wgpu::Texture,
) -> Result<wgpu::TextureView, GpuRenderError> {
    let Some(mask_key) = mask_key else {
        return Ok(fallback_texture.create_view(&wgpu::TextureViewDescriptor::default()));
    };
    Ok(mask_cache
        .ok_or(GpuRenderError::MissingMaskResource {
            layer_id: mask_key.layer_id,
            mask_mipmap_id: mask_key.mask_mipmap_id,
        })?
        .resource(mask_key)
        .ok_or(GpuRenderError::MissingMaskResource {
            layer_id: mask_key.layer_id,
            mask_mipmap_id: mask_key.mask_mipmap_id,
        })?
        .texture()
        .create_view(&wgpu::TextureViewDescriptor::default()))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_normal_source_pass(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind_group_layout: &wgpu::BindGroupLayout,
    source_view: &wgpu::TextureView,
    dest_view: &wgpu::TextureView,
    mask_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    uniform_bytes: [u8; 48],
    label: &'static str,
) {
    encode_normal_source_pass_with_load(
        device,
        encoder,
        pipeline,
        bind_group_layout,
        source_view,
        dest_view,
        mask_view,
        output_view,
        uniform_bytes,
        label,
        wgpu::LoadOp::Clear(WHITE_TRANSPARENT),
        None,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_normal_source_pass_scissored(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind_group_layout: &wgpu::BindGroupLayout,
    source_view: &wgpu::TextureView,
    dest_view: &wgpu::TextureView,
    mask_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    uniform_bytes: [u8; 48],
    label: &'static str,
    scissor: CanvasRect,
) {
    encode_normal_source_pass_with_load(
        device,
        encoder,
        pipeline,
        bind_group_layout,
        source_view,
        dest_view,
        mask_view,
        output_view,
        uniform_bytes,
        label,
        wgpu::LoadOp::Load,
        Some(scissor),
    );
}

#[allow(clippy::too_many_arguments)]
fn encode_normal_source_pass_with_load(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind_group_layout: &wgpu::BindGroupLayout,
    source_view: &wgpu::TextureView,
    dest_view: &wgpu::TextureView,
    mask_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    uniform_bytes: [u8; 48],
    label: &'static str,
    load: wgpu::LoadOp<wgpu::Color>,
    scissor: Option<CanvasRect>,
) {
    let uniform = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rizum_clip_normal_source_uniform"),
        size: uniform_bytes.len() as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::UNIFORM,
        mapped_at_creation: true,
    });
    {
        let mut mapped = uniform.slice(..).get_mapped_range_mut();
        mapped.copy_from_slice(&uniform_bytes);
    }
    uniform.unmap();
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("rizum_clip_normal_source_bind_group"),
        layout: bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(dest_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(mask_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: uniform.as_entire_binding(),
            },
        ],
    });
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: output_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        timestamp_writes: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &bind_group, &[]);
    if let Some(scissor) = scissor {
        pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
    }
    pass.draw(0..3, 0..1);
}

pub(crate) fn clear_rgba8_texture(
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    label: &'static str,
) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(WHITE_TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        timestamp_writes: None,
        multiview_mask: None,
    });
}

struct StackResources<'a> {
    size: CanvasSize,
    resources: Vec<&'a GpuRasterResource>,
}

fn stack_resources<'a>(
    cache: &'a GpuRasterResourceCache,
    keys: &[GpuRasterResourceKey],
) -> Result<StackResources<'a>, GpuRenderError> {
    let Some(first_key) = keys.first().copied() else {
        return Err(GpuRenderError::EmptyRasterStack);
    };
    let first_resource =
        cache
            .resource(first_key)
            .ok_or(GpuRenderError::MissingRasterResource {
                layer_id: first_key.layer_id,
                render_mipmap_id: first_key.render_mipmap_id,
            })?;
    let output_size = first_resource.info().size;
    let mut resources = Vec::with_capacity(keys.len());
    for key in keys {
        let resource = cache
            .resource(*key)
            .ok_or(GpuRenderError::MissingRasterResource {
                layer_id: key.layer_id,
                render_mipmap_id: key.render_mipmap_id,
            })?;
        let info = resource.info();
        if info.size != output_size {
            return Err(GpuRenderError::RasterResourceSizeMismatch {
                layer_id: info.key.layer_id,
                expected: output_size,
                actual: info.size,
            });
        }
        resources.push(resource);
    }
    Ok(StackResources {
        size: output_size,
        resources,
    })
}

#[cfg(test)]
mod tests {
    use clip_graph::RenderNodeId;
    use clip_model::{CanvasSize, LayerId, Rgba8};

    use crate::{
        GpuDeviceConfig, GpuNormalRasterSource, GpuNormalStackSource, GpuRasterBlendMode,
        GpuRasterResourceCache, GpuRasterResourceKey, GpuRasterUpload, GpuRenderer,
    };

    #[test]
    fn container_source_resolves_child_cache_with_opacity() {
        let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
        let sources = [GpuNormalStackSource::Container {
            children: vec![GpuNormalStackSource::SolidColor {
                color: Rgba8 {
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 255,
                },
                opacity: 1.0,
            }],
            opacity: 0.5,
            mask_key: None,
            blend_mode: GpuRasterBlendMode::Normal,
        }];

        let output = renderer
            .draw_normal_stack_to_rgba8(
                &GpuRasterResourceCache::empty(),
                None,
                CanvasSize::new(2, 2),
                &sources,
            )
            .expect("draw container source");

        assert_eq!(output.size, CanvasSize::new(2, 2));
        for pixel in output.pixels.chunks_exact(4) {
            assert_eq!(pixel[0], 255);
            assert_eq!(pixel[1], 0);
            assert_eq!(pixel[2], 0);
            assert!((127..=128).contains(&pixel[3]));
        }
    }

    #[test]
    fn raster_source_samples_with_canvas_offset() {
        let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
        let key = GpuRasterResourceKey {
            layer_id: LayerId(30),
            render_mipmap_id: 40,
        };
        let uploads = [GpuRasterUpload {
            layer_id: key.layer_id,
            render_node_id: RenderNodeId(30),
            render_mipmap_id: key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &[10, 20, 30, 255],
        }];
        let cache = renderer
            .upload_raster_resources(&uploads)
            .expect("upload source-sized raster");
        let sources = [GpuNormalStackSource::Raster(GpuNormalRasterSource {
            key,
            opacity: 1.0,
            mask_key: None,
            offset_x: 1,
            offset_y: 1,
            blend_mode: GpuRasterBlendMode::Normal,
        })];

        let output = renderer
            .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(3, 3), &sources)
            .expect("draw source-sized raster with canvas offset");

        let mut expected = vec![255, 255, 255, 0].repeat(9);
        expected[16..20].copy_from_slice(&[10, 20, 30, 255]);
        assert_eq!(output.pixels, expected);
    }

    #[test]
    fn container_source_resolves_child_cache_with_blend_mode() {
        let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
        let sources = [
            GpuNormalStackSource::SolidColor {
                color: Rgba8 {
                    r: 200,
                    g: 160,
                    b: 120,
                    a: 255,
                },
                opacity: 1.0,
            },
            GpuNormalStackSource::Container {
                children: vec![GpuNormalStackSource::SolidColor {
                    color: Rgba8 {
                        r: 128,
                        g: 64,
                        b: 255,
                        a: 255,
                    },
                    opacity: 1.0,
                }],
                opacity: 1.0,
                mask_key: None,
                blend_mode: GpuRasterBlendMode::Multiply,
            },
        ];

        let output = renderer
            .draw_normal_stack_to_rgba8(
                &GpuRasterResourceCache::empty(),
                None,
                CanvasSize::new(1, 1),
                &sources,
            )
            .expect("draw multiply container source");

        assert_eq!(output.pixels, [100, 40, 120, 255]);
    }

    #[test]
    fn through_group_resolves_before_after_delta_with_opacity() {
        let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
        let sources = [
            GpuNormalStackSource::SolidColor {
                color: Rgba8 {
                    r: 0,
                    g: 0,
                    b: 255,
                    a: 255,
                },
                opacity: 1.0,
            },
            GpuNormalStackSource::ThroughGroup {
                children: vec![GpuNormalStackSource::SolidColor {
                    color: Rgba8 {
                        r: 255,
                        g: 0,
                        b: 0,
                        a: 255,
                    },
                    opacity: 1.0,
                }],
                opacity: 0.5,
                mask_key: None,
            },
        ];

        let output = renderer
            .draw_normal_stack_to_rgba8(
                &GpuRasterResourceCache::empty(),
                None,
                CanvasSize::new(2, 2),
                &sources,
            )
            .expect("draw through group source");

        assert_eq!(output.size, CanvasSize::new(2, 2));
        for pixel in output.pixels.chunks_exact(4) {
            assert!((127..=128).contains(&pixel[0]));
            assert_eq!(pixel[1], 0);
            assert!((127..=128).contains(&pixel[2]));
            assert_eq!(pixel[3], 255);
        }
    }

    #[test]
    fn add_raster_source_uses_standard_blend_formula() {
        let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
        let key = GpuRasterResourceKey {
            layer_id: LayerId(9),
            render_mipmap_id: 19,
        };
        let pixels = [100u8, 50, 100, 128];
        let uploads = [GpuRasterUpload {
            layer_id: key.layer_id,
            render_node_id: RenderNodeId(0),
            render_mipmap_id: key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &pixels,
        }];
        let cache = renderer
            .upload_raster_resources(&uploads)
            .expect("upload Add source");
        let sources = [
            GpuNormalStackSource::SolidColor {
                color: Rgba8 {
                    r: 0,
                    g: 0,
                    b: 200,
                    a: 255,
                },
                opacity: 1.0,
            },
            GpuNormalStackSource::Raster(GpuNormalRasterSource {
                key,
                opacity: 1.0,
                mask_key: None,
                offset_x: 0,
                offset_y: 0,
                blend_mode: GpuRasterBlendMode::Add,
            }),
        ];

        let output = renderer
            .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
            .expect("draw Add source");

        assert_eq!(output.pixels, [50, 25, 228, 255]);
    }

    #[test]
    fn add_glow_raster_source_uses_byte_domain_formula() {
        let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
        let key = GpuRasterResourceKey {
            layer_id: LayerId(10),
            render_mipmap_id: 20,
        };
        let pixels = [100u8, 50, 0, 128];
        let uploads = [GpuRasterUpload {
            layer_id: key.layer_id,
            render_node_id: RenderNodeId(1),
            render_mipmap_id: key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &pixels,
        }];
        let cache = renderer
            .upload_raster_resources(&uploads)
            .expect("upload AddGlow source");
        let sources = [
            GpuNormalStackSource::SolidColor {
                color: Rgba8 {
                    r: 0,
                    g: 0,
                    b: 200,
                    a: 255,
                },
                opacity: 1.0,
            },
            GpuNormalStackSource::Raster(GpuNormalRasterSource {
                key,
                opacity: 1.0,
                mask_key: None,
                offset_x: 0,
                offset_y: 0,
                blend_mode: GpuRasterBlendMode::AddGlow,
            }),
        ];

        let output = renderer
            .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
            .expect("draw AddGlow source");

        assert_eq!(output.pixels, [50, 25, 200, 255]);
    }

    #[test]
    fn clipping_run_resolves_cache_with_base_blend_and_clipped_multiply() {
        let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
        let base_key = GpuRasterResourceKey {
            layer_id: LayerId(30),
            render_mipmap_id: 40,
        };
        let clipped_key = GpuRasterResourceKey {
            layer_id: LayerId(31),
            render_mipmap_id: 41,
        };
        let base_pixels = [100u8, 50, 0, 128];
        let clipped_pixels = [128u8, 128, 128, 128];
        let uploads = [
            GpuRasterUpload {
                layer_id: base_key.layer_id,
                render_node_id: RenderNodeId(30),
                render_mipmap_id: base_key.render_mipmap_id,
                size: CanvasSize::new(1, 1),
                pixels: &base_pixels,
            },
            GpuRasterUpload {
                layer_id: clipped_key.layer_id,
                render_node_id: RenderNodeId(31),
                render_mipmap_id: clipped_key.render_mipmap_id,
                size: CanvasSize::new(1, 1),
                pixels: &clipped_pixels,
            },
        ];
        let cache = renderer
            .upload_raster_resources(&uploads)
            .expect("upload clipping run sources");
        let sources = [
            GpuNormalStackSource::SolidColor {
                color: Rgba8 {
                    r: 20,
                    g: 40,
                    b: 60,
                    a: 255,
                },
                opacity: 1.0,
            },
            GpuNormalStackSource::ClippingRun {
                base: GpuNormalRasterSource {
                    key: base_key,
                    opacity: 1.0,
                    mask_key: None,
                    offset_x: 0,
                    offset_y: 0,
                    blend_mode: GpuRasterBlendMode::AddGlow,
                },
                clipped: vec![GpuNormalRasterSource {
                    key: clipped_key,
                    opacity: 1.0,
                    mask_key: None,
                    offset_x: 0,
                    offset_y: 0,
                    blend_mode: GpuRasterBlendMode::Multiply,
                }],
            },
        ];

        let output = renderer
            .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
            .expect("draw clipping run");

        assert_eq!(output.pixels, [57, 58, 60, 255]);
    }

    #[test]
    fn clipping_run_preserves_base_alpha_with_byte_domain_clipped_blends() {
        let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
        let base_key = GpuRasterResourceKey {
            layer_id: LayerId(32),
            render_mipmap_id: 42,
        };
        let clipped_key = GpuRasterResourceKey {
            layer_id: LayerId(33),
            render_mipmap_id: 43,
        };
        let base_pixels = [50u8, 50, 50, 128];
        let clipped_pixels = [128u8, 0, 0, 128];
        let uploads = [
            GpuRasterUpload {
                layer_id: base_key.layer_id,
                render_node_id: RenderNodeId(32),
                render_mipmap_id: base_key.render_mipmap_id,
                size: CanvasSize::new(1, 1),
                pixels: &base_pixels,
            },
            GpuRasterUpload {
                layer_id: clipped_key.layer_id,
                render_node_id: RenderNodeId(33),
                render_mipmap_id: clipped_key.render_mipmap_id,
                size: CanvasSize::new(1, 1),
                pixels: &clipped_pixels,
            },
        ];
        let cache = renderer
            .upload_raster_resources(&uploads)
            .expect("upload clipping run sources");

        for (blend_mode, expected) in [
            (GpuRasterBlendMode::AddGlow, [114, 50, 50, 128]),
            (GpuRasterBlendMode::ColorDodge, [75, 50, 50, 128]),
            (GpuRasterBlendMode::ColorBurn, [25, 25, 25, 128]),
            (GpuRasterBlendMode::GlowDodge, [66, 50, 50, 128]),
        ] {
            let sources = [GpuNormalStackSource::ClippingRun {
                base: GpuNormalRasterSource {
                    key: base_key,
                    opacity: 1.0,
                    mask_key: None,
                    offset_x: 0,
                    offset_y: 0,
                    blend_mode: GpuRasterBlendMode::Normal,
                },
                clipped: vec![GpuNormalRasterSource {
                    key: clipped_key,
                    opacity: 1.0,
                    mask_key: None,
                    offset_x: 0,
                    offset_y: 0,
                    blend_mode,
                }],
            }];

            let output = renderer
                .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
                .expect("draw byte-domain clipping run");

            assert_eq!(output.pixels, expected, "blend mode {blend_mode:?}");
        }
    }

    #[test]
    fn color_dodge_raster_source_uses_byte_domain_formula() {
        let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
        let key = GpuRasterResourceKey {
            layer_id: LayerId(11),
            render_mipmap_id: 21,
        };
        let pixels = [100u8, 200, 250, 128];
        let uploads = [GpuRasterUpload {
            layer_id: key.layer_id,
            render_node_id: RenderNodeId(2),
            render_mipmap_id: key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &pixels,
        }];
        let cache = renderer
            .upload_raster_resources(&uploads)
            .expect("upload ColorDodge source");
        let sources = [
            GpuNormalStackSource::SolidColor {
                color: Rgba8 {
                    r: 100,
                    g: 50,
                    b: 200,
                    a: 255,
                },
                opacity: 1.0,
            },
            GpuNormalStackSource::Raster(GpuNormalRasterSource {
                key,
                opacity: 1.0,
                mask_key: None,
                offset_x: 0,
                offset_y: 0,
                blend_mode: GpuRasterBlendMode::ColorDodge,
            }),
        ];

        let output = renderer
            .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
            .expect("draw ColorDodge source");

        assert_eq!(output.pixels, [132, 141, 228, 255]);
    }

    #[test]
    fn color_burn_raster_source_uses_byte_domain_formula() {
        let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
        let key = GpuRasterResourceKey {
            layer_id: LayerId(13),
            render_mipmap_id: 23,
        };
        let pixels = [100u8, 200, 250, 128];
        let uploads = [GpuRasterUpload {
            layer_id: key.layer_id,
            render_node_id: RenderNodeId(4),
            render_mipmap_id: key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &pixels,
        }];
        let cache = renderer
            .upload_raster_resources(&uploads)
            .expect("upload ColorBurn source");
        let sources = [
            GpuNormalStackSource::SolidColor {
                color: Rgba8 {
                    r: 100,
                    g: 50,
                    b: 200,
                    a: 255,
                },
                opacity: 1.0,
            },
            GpuNormalStackSource::Raster(GpuNormalRasterSource {
                key,
                opacity: 1.0,
                mask_key: None,
                offset_x: 0,
                offset_y: 0,
                blend_mode: GpuRasterBlendMode::ColorBurn,
            }),
        ];

        let output = renderer
            .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
            .expect("draw ColorBurn source");

        assert_eq!(output.pixels, [50, 25, 199, 255]);
    }

    #[test]
    fn vivid_light_raster_source_uses_standard_blend_formula() {
        let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
        let key = GpuRasterResourceKey {
            layer_id: LayerId(14),
            render_mipmap_id: 24,
        };
        let pixels = [100u8, 200, 250, 128];
        let uploads = [GpuRasterUpload {
            layer_id: key.layer_id,
            render_node_id: RenderNodeId(5),
            render_mipmap_id: key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &pixels,
        }];
        let cache = renderer
            .upload_raster_resources(&uploads)
            .expect("upload VividLight source");
        let sources = [
            GpuNormalStackSource::SolidColor {
                color: Rgba8 {
                    r: 100,
                    g: 50,
                    b: 200,
                    a: 255,
                },
                opacity: 1.0,
            },
            GpuNormalStackSource::Raster(GpuNormalRasterSource {
                key,
                opacity: 1.0,
                mask_key: None,
                offset_x: 0,
                offset_y: 0,
                blend_mode: GpuRasterBlendMode::VividLight,
            }),
        ];

        let output = renderer
            .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
            .expect("draw VividLight source");

        assert_eq!(output.pixels, [78, 83, 228, 255]);
    }

    #[test]
    fn hard_mix_raster_source_uses_standard_blend_formula() {
        let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
        let key = GpuRasterResourceKey {
            layer_id: LayerId(15),
            render_mipmap_id: 25,
        };
        let pixels = [100u8, 200, 250, 128];
        let uploads = [GpuRasterUpload {
            layer_id: key.layer_id,
            render_node_id: RenderNodeId(6),
            render_mipmap_id: key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &pixels,
        }];
        let cache = renderer
            .upload_raster_resources(&uploads)
            .expect("upload HardMix source");
        let sources = [
            GpuNormalStackSource::SolidColor {
                color: Rgba8 {
                    r: 100,
                    g: 50,
                    b: 200,
                    a: 255,
                },
                opacity: 1.0,
            },
            GpuNormalStackSource::Raster(GpuNormalRasterSource {
                key,
                opacity: 1.0,
                mask_key: None,
                offset_x: 0,
                offset_y: 0,
                blend_mode: GpuRasterBlendMode::HardMix,
            }),
        ];

        let output = renderer
            .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
            .expect("draw HardMix source");

        assert_eq!(output.pixels, [50, 25, 228, 255]);
    }

    fn draw_one_pixel_standard_blend(blend_mode: GpuRasterBlendMode) -> Vec<u8> {
        draw_one_pixel_standard_blend_with_colors(
            blend_mode,
            [100, 200, 250, 128],
            Rgba8 {
                r: 100,
                g: 50,
                b: 200,
                a: 255,
            },
        )
    }

    fn draw_one_pixel_standard_blend_with_colors(
        blend_mode: GpuRasterBlendMode,
        pixels: [u8; 4],
        dest: Rgba8,
    ) -> Vec<u8> {
        let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
        let key = GpuRasterResourceKey {
            layer_id: LayerId(20),
            render_mipmap_id: 30,
        };
        let uploads = [GpuRasterUpload {
            layer_id: key.layer_id,
            render_node_id: RenderNodeId(20),
            render_mipmap_id: key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &pixels,
        }];
        let cache = renderer
            .upload_raster_resources(&uploads)
            .expect("upload standard blend source");
        let sources = [
            GpuNormalStackSource::SolidColor {
                color: dest,
                opacity: 1.0,
            },
            GpuNormalStackSource::Raster(GpuNormalRasterSource {
                key,
                opacity: 1.0,
                mask_key: None,
                offset_x: 0,
                offset_y: 0,
                blend_mode,
            }),
        ];

        renderer
            .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
            .expect("draw standard blend source")
            .pixels
    }

    #[test]
    fn darken_raster_source_uses_standard_blend_formula() {
        assert_eq!(
            draw_one_pixel_standard_blend(GpuRasterBlendMode::Darken),
            [100, 50, 200, 255]
        );
    }

    #[test]
    fn multiply_raster_source_uses_w3c_blend_formula() {
        assert_eq!(
            draw_one_pixel_standard_blend(GpuRasterBlendMode::Multiply),
            [69, 44, 198, 255]
        );
    }

    #[test]
    fn linear_burn_raster_source_uses_standard_blend_formula() {
        assert_eq!(
            draw_one_pixel_standard_blend(GpuRasterBlendMode::LinearBurn),
            [50, 25, 197, 255]
        );
    }

    #[test]
    fn darker_color_raster_source_uses_standard_blend_formula() {
        assert_eq!(
            draw_one_pixel_standard_blend(GpuRasterBlendMode::DarkerColor),
            [100, 50, 200, 255]
        );
    }

    #[test]
    fn darker_color_raster_source_uses_rec709_luma_compare() {
        assert_eq!(
            draw_one_pixel_standard_blend_with_colors(
                GpuRasterBlendMode::DarkerColor,
                [84, 51, 250, 255],
                Rgba8 {
                    r: 84,
                    g: 56,
                    b: 222,
                    a: 255,
                },
            ),
            [84, 51, 250, 255]
        );
    }

    #[test]
    fn lighten_raster_source_uses_standard_blend_formula() {
        assert_eq!(
            draw_one_pixel_standard_blend(GpuRasterBlendMode::Lighten),
            [100, 125, 225, 255]
        );
    }

    #[test]
    fn screen_raster_source_uses_standard_blend_formula() {
        assert_eq!(
            draw_one_pixel_standard_blend(GpuRasterBlendMode::Screen),
            [131, 131, 227, 255]
        );
    }

    #[test]
    fn lighter_color_raster_source_uses_standard_blend_formula() {
        assert_eq!(
            draw_one_pixel_standard_blend(GpuRasterBlendMode::LighterColor),
            [100, 125, 225, 255]
        );
    }

    #[test]
    fn lighter_color_raster_source_uses_rec709_luma_compare() {
        assert_eq!(
            draw_one_pixel_standard_blend_with_colors(
                GpuRasterBlendMode::LighterColor,
                [84, 51, 250, 255],
                Rgba8 {
                    r: 84,
                    g: 56,
                    b: 222,
                    a: 255,
                },
            ),
            [84, 56, 222, 255]
        );
    }

    #[test]
    fn overlay_raster_source_uses_w3c_blend_formula() {
        assert_eq!(
            draw_one_pixel_standard_blend(GpuRasterBlendMode::Overlay),
            [89, 64, 227, 255]
        );
    }

    #[test]
    fn hard_light_raster_source_uses_w3c_blend_formula() {
        assert_eq!(
            draw_one_pixel_standard_blend(GpuRasterBlendMode::HardLight),
            [89, 109, 227, 255]
        );
    }

    #[test]
    fn linear_light_raster_source_uses_standard_blend_formula() {
        assert_eq!(
            draw_one_pixel_standard_blend(GpuRasterBlendMode::LinearLight),
            [72, 123, 228, 255]
        );
    }

    #[test]
    fn pin_light_raster_source_uses_standard_blend_formula() {
        assert_eq!(
            draw_one_pixel_standard_blend(GpuRasterBlendMode::PinLight),
            [100, 98, 223, 255]
        );
    }

    #[test]
    fn subtract_raster_source_uses_standard_blend_formula() {
        assert_eq!(
            draw_one_pixel_standard_blend(GpuRasterBlendMode::Subtract),
            [50, 25, 100, 255]
        );
    }

    #[test]
    fn difference_raster_source_uses_standard_blend_formula() {
        assert_eq!(
            draw_one_pixel_standard_blend(GpuRasterBlendMode::Difference),
            [50, 100, 125, 255]
        );
    }

    #[test]
    fn exclusion_raster_source_uses_standard_blend_formula() {
        assert_eq!(
            draw_one_pixel_standard_blend(GpuRasterBlendMode::Exclusion),
            [111, 111, 129, 255]
        );
    }

    #[test]
    fn brightness_raster_source_uses_hsl_blend_formula() {
        assert_eq!(
            draw_one_pixel_standard_blend(GpuRasterBlendMode::Brightness),
            [144, 102, 228, 255]
        );
    }

    #[test]
    fn divide_raster_source_uses_standard_blend_formula() {
        assert_eq!(
            draw_one_pixel_standard_blend(GpuRasterBlendMode::Divide),
            [178, 57, 202, 255]
        );
    }

    #[test]
    fn soft_light_raster_source_uses_w3c_blend_formula() {
        let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
        let key = GpuRasterResourceKey {
            layer_id: LayerId(19),
            render_mipmap_id: 29,
        };
        let pixels = [100u8, 200, 250, 128];
        let uploads = [GpuRasterUpload {
            layer_id: key.layer_id,
            render_node_id: RenderNodeId(10),
            render_mipmap_id: key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &pixels,
        }];
        let cache = renderer
            .upload_raster_resources(&uploads)
            .expect("upload SoftLight source");
        let sources = [
            GpuNormalStackSource::SolidColor {
                color: Rgba8 {
                    r: 100,
                    g: 50,
                    b: 200,
                    a: 255,
                },
                opacity: 1.0,
            },
            GpuNormalStackSource::Raster(GpuNormalRasterSource {
                key,
                opacity: 1.0,
                mask_key: None,
                offset_x: 0,
                offset_y: 0,
                blend_mode: GpuRasterBlendMode::SoftLight,
            }),
        ];

        let output = renderer
            .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
            .expect("draw SoftLight source");

        assert_eq!(output.pixels, [93, 68, 213, 255]);
    }

    #[test]
    fn hue_raster_source_uses_hsl_blend_formula() {
        let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
        let key = GpuRasterResourceKey {
            layer_id: LayerId(16),
            render_mipmap_id: 26,
        };
        let pixels = [100u8, 200, 250, 128];
        let uploads = [GpuRasterUpload {
            layer_id: key.layer_id,
            render_node_id: RenderNodeId(7),
            render_mipmap_id: key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &pixels,
        }];
        let cache = renderer
            .upload_raster_resources(&uploads)
            .expect("upload Hue source");
        let sources = [
            GpuNormalStackSource::SolidColor {
                color: Rgba8 {
                    r: 100,
                    g: 50,
                    b: 200,
                    a: 255,
                },
                opacity: 1.0,
            },
            GpuNormalStackSource::Raster(GpuNormalRasterSource {
                key,
                opacity: 1.0,
                mask_key: None,
                offset_x: 0,
                offset_y: 0,
                blend_mode: GpuRasterBlendMode::Hue,
            }),
        ];

        let output = renderer
            .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
            .expect("draw Hue source");

        assert_eq!(output.pixels, [53, 78, 178, 255]);
    }

    #[test]
    fn saturation_raster_source_uses_hsl_blend_formula() {
        let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
        let key = GpuRasterResourceKey {
            layer_id: LayerId(17),
            render_mipmap_id: 27,
        };
        let pixels = [100u8, 200, 250, 128];
        let uploads = [GpuRasterUpload {
            layer_id: key.layer_id,
            render_node_id: RenderNodeId(8),
            render_mipmap_id: key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &pixels,
        }];
        let cache = renderer
            .upload_raster_resources(&uploads)
            .expect("upload Saturation source");
        let sources = [
            GpuNormalStackSource::SolidColor {
                color: Rgba8 {
                    r: 100,
                    g: 50,
                    b: 200,
                    a: 255,
                },
                opacity: 1.0,
            },
            GpuNormalStackSource::Raster(GpuNormalRasterSource {
                key,
                opacity: 1.0,
                mask_key: None,
                offset_x: 0,
                offset_y: 0,
                blend_mode: GpuRasterBlendMode::Saturation,
            }),
        ];

        let output = renderer
            .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
            .expect("draw Saturation source");

        assert_eq!(output.pixels, [100, 50, 200, 255]);
    }

    #[test]
    fn saturation_raster_source_quantizes_tiny_base_span() {
        assert_eq!(
            draw_one_pixel_standard_blend_with_colors(
                GpuRasterBlendMode::Saturation,
                [84, 51, 250, 255],
                Rgba8 {
                    r: 225,
                    g: 224,
                    b: 226,
                    a: 255,
                },
            ),
            [221, 221, 255, 255]
        );
    }

    #[test]
    fn color_raster_source_uses_hsl_blend_formula() {
        let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
        let key = GpuRasterResourceKey {
            layer_id: LayerId(18),
            render_mipmap_id: 28,
        };
        let pixels = [100u8, 200, 250, 128];
        let uploads = [GpuRasterUpload {
            layer_id: key.layer_id,
            render_node_id: RenderNodeId(9),
            render_mipmap_id: key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &pixels,
        }];
        let cache = renderer
            .upload_raster_resources(&uploads)
            .expect("upload Color source");
        let sources = [
            GpuNormalStackSource::SolidColor {
                color: Rgba8 {
                    r: 100,
                    g: 50,
                    b: 200,
                    a: 255,
                },
                opacity: 1.0,
            },
            GpuNormalStackSource::Raster(GpuNormalRasterSource {
                key,
                opacity: 1.0,
                mask_key: None,
                offset_x: 0,
                offset_y: 0,
                blend_mode: GpuRasterBlendMode::Color,
            }),
        ];

        let output = renderer
            .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
            .expect("draw Color source");

        assert_eq!(output.pixels, [53, 78, 178, 255]);
    }

    #[test]
    fn glow_dodge_raster_source_uses_byte_domain_formula() {
        let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer");
        let key = GpuRasterResourceKey {
            layer_id: LayerId(12),
            render_mipmap_id: 22,
        };
        let pixels = [100u8, 200, 250, 128];
        let uploads = [GpuRasterUpload {
            layer_id: key.layer_id,
            render_node_id: RenderNodeId(3),
            render_mipmap_id: key.render_mipmap_id,
            size: CanvasSize::new(1, 1),
            pixels: &pixels,
        }];
        let cache = renderer
            .upload_raster_resources(&uploads)
            .expect("upload GlowDodge source");
        let sources = [
            GpuNormalStackSource::SolidColor {
                color: Rgba8 {
                    r: 100,
                    g: 50,
                    b: 200,
                    a: 255,
                },
                opacity: 1.0,
            },
            GpuNormalStackSource::Raster(GpuNormalRasterSource {
                key,
                opacity: 1.0,
                mask_key: None,
                offset_x: 0,
                offset_y: 0,
                blend_mode: GpuRasterBlendMode::GlowDodge,
            }),
        ];

        let output = renderer
            .draw_normal_stack_to_rgba8(&cache, None, CanvasSize::new(1, 1), &sources)
            .expect("draw GlowDodge source");

        assert_eq!(output.pixels, [124, 82, 255, 255]);
    }
}
