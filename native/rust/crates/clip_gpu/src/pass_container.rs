use clip_model::CanvasSize;

use super::pass_pipeline::{
    clear_rgba8_texture, create_rgba8_texture, encode_normal_source_pass, mask_texture_view,
    raster_texture_view, source_mask_texture_view,
};
use crate::blend::raster_source_pipeline;
use crate::lut_filter::{create_lut_filter_texture, encode_lut_filter_pass};
use crate::source_params::{
    generated_raster_source_uniform_bytes_with_blend, lut_filter_uniform_bytes,
    raster_source_uniform_bytes, solid_source_uniform_bytes,
};
use crate::types::GpuNormalStackSource;
use crate::{GpuMaskResourceCache, GpuRasterResourceCache, GpuRenderError, GpuRenderer};

impl GpuRenderer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_container_source(
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
                GpuNormalStackSource::ContainerClippingRun {
                    children,
                    opacity,
                    mask_key,
                    blend_mode,
                    clipped,
                } => {
                    let clipping_view = self.render_container_clipping_run_source(
                        cache,
                        mask_cache,
                        output_size,
                        children,
                        *opacity,
                        *mask_key,
                        clipped,
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
                        fallback_texture.create_view(&wgpu::TextureViewDescriptor::default());
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
                        &clipping_view,
                        &previous_view,
                        &mask_view,
                        &output_view,
                        generated_raster_source_uniform_bytes_with_blend(1.0, false, *blend_mode),
                        "rizum_clip_container_container_clipping_resolve_pass",
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
}
