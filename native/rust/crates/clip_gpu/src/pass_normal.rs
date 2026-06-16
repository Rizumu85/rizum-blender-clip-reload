use clip_model::CanvasSize;

use super::pass_pipeline::{NormalStackPipelines, clear_rgba8_texture, create_rgba8_texture};
use super::{StackResources, stack_resources};
use crate::types::{
    GpuNormalRasterSource, GpuNormalStackChunk, GpuNormalStackSource, GpuRasterBlendMode,
    GpuRasterStackOutput,
};
use crate::validation::validate_normal_stack_sources;
use crate::{
    GpuMaskResourceCache, GpuRasterResourceCache, GpuRasterResourceKey, GpuRenderError, GpuRenderer,
};

impl GpuRenderer {
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
}
