use clip_model::CanvasSize;

use super::pass_pipeline::{
    clear_rgba8_texture, create_rgba8_texture, encode_normal_source_pass, mask_texture_view,
    raster_texture_view, source_mask_texture_view,
};
use crate::blend::clipped_source_pipeline;
use crate::source_params::{
    generated_raster_source_uniform_bytes_with_blend, raster_source_uniform_bytes,
};
use crate::types::{
    GpuClippedStackSource, GpuNormalRasterSource, GpuNormalStackSource, GpuRasterBlendMode,
};
use crate::{
    GpuMaskResourceCache, GpuMaskResourceKey, GpuRasterResourceCache, GpuRenderError, GpuRenderer,
};

impl GpuRenderer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_clipping_run_source(
        &self,
        cache: &GpuRasterResourceCache,
        mask_cache: Option<&GpuMaskResourceCache>,
        output_size: CanvasSize,
        base: GpuNormalRasterSource,
        clipped: &[GpuClippedStackSource],
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
            self.encode_clipped_stack_source(
                cache,
                mask_cache,
                output_size,
                clipped_source,
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
                &clipping_textures,
                &mut previous_index,
                &mut next_index,
                encoder,
            )?;
        }

        Ok(clipping_textures[previous_index].create_view(&wgpu::TextureViewDescriptor::default()))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_container_clipping_run_source(
        &self,
        cache: &GpuRasterResourceCache,
        mask_cache: Option<&GpuMaskResourceCache>,
        output_size: CanvasSize,
        children: &[GpuNormalStackSource],
        opacity: f32,
        mask_key: Option<GpuMaskResourceKey>,
        clipped: &[GpuClippedStackSource],
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

        let clipping_usage =
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let clipping_textures = [
            create_rgba8_texture(
                &self.context.device,
                "rizum_clip_container_clipping_cache_a",
                output_size,
                clipping_usage,
            ),
            create_rgba8_texture(
                &self.context.device,
                "rizum_clip_container_clipping_cache_b",
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
                "rizum_clip_container_clipping_initial_clear",
            );
        }

        {
            let previous_view = clipping_textures[previous_index]
                .create_view(&wgpu::TextureViewDescriptor::default());
            let output_view =
                clipping_textures[next_index].create_view(&wgpu::TextureViewDescriptor::default());
            let mask_view = source_mask_texture_view(mask_cache, mask_key, fallback_texture)?;
            encode_normal_source_pass(
                &self.context.device,
                encoder,
                alpha_pipeline,
                bind_group_layout,
                &container_view,
                &previous_view,
                &mask_view,
                &output_view,
                generated_raster_source_uniform_bytes_with_blend(
                    opacity,
                    mask_key.is_some(),
                    GpuRasterBlendMode::Normal,
                ),
                "rizum_clip_container_clipping_base_pass",
            );
            std::mem::swap(&mut previous_index, &mut next_index);
        }

        for clipped_source in clipped {
            self.encode_clipped_stack_source(
                cache,
                mask_cache,
                output_size,
                clipped_source,
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
                &clipping_textures,
                &mut previous_index,
                &mut next_index,
                encoder,
            )?;
        }

        Ok(clipping_textures[previous_index].create_view(&wgpu::TextureViewDescriptor::default()))
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_clipped_stack_source(
        &self,
        cache: &GpuRasterResourceCache,
        mask_cache: Option<&GpuMaskResourceCache>,
        output_size: CanvasSize,
        clipped_source: &GpuClippedStackSource,
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
        clipping_textures: &[wgpu::Texture; 2],
        previous_index: &mut usize,
        next_index: &mut usize,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<(), GpuRenderError> {
        match clipped_source {
            GpuClippedStackSource::Raster(raster) => {
                let source_view = raster_texture_view(cache, *raster)?;
                let previous_view = clipping_textures[*previous_index]
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let output_view = clipping_textures[*next_index]
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let mask_view = mask_texture_view(mask_cache, *raster, fallback_texture)?;
                encode_normal_source_pass(
                    &self.context.device,
                    encoder,
                    clipped_source_pipeline(
                        raster.blend_mode,
                        clipped_pipeline,
                        clipped_byte_pipeline,
                    ),
                    bind_group_layout,
                    &source_view,
                    &previous_view,
                    &mask_view,
                    &output_view,
                    raster_source_uniform_bytes(*raster),
                    "rizum_clip_clipping_run_clipped_raster_pass",
                );
            }
            GpuClippedStackSource::Container {
                children,
                opacity,
                mask_key,
                blend_mode,
                ..
            } => {
                let source_view = self.render_container_source(
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
                let previous_view = clipping_textures[*previous_index]
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let output_view = clipping_textures[*next_index]
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let mask_view = source_mask_texture_view(mask_cache, *mask_key, fallback_texture)?;
                encode_normal_source_pass(
                    &self.context.device,
                    encoder,
                    clipped_source_pipeline(*blend_mode, clipped_pipeline, clipped_byte_pipeline),
                    bind_group_layout,
                    &source_view,
                    &previous_view,
                    &mask_view,
                    &output_view,
                    generated_raster_source_uniform_bytes_with_blend(
                        *opacity,
                        mask_key.is_some(),
                        *blend_mode,
                    ),
                    "rizum_clip_clipping_run_clipped_container_pass",
                );
            }
        }
        std::mem::swap(previous_index, next_index);
        Ok(())
    }
}
