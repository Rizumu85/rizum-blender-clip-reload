use clip_model::CanvasSize;

use super::pass_pipeline::{
    create_rgba8_texture, encode_normal_source_pass, mask_texture_view, raster_texture_view,
    source_mask_texture_view,
};
use crate::blend::raster_source_pipeline;
use crate::lut_filter::{create_lut_filter_texture, encode_lut_filter_pass};
use crate::source_params::{
    generated_raster_source_uniform_bytes, generated_raster_source_uniform_bytes_with_blend,
    lut_filter_uniform_bytes, raster_source_uniform_bytes, solid_source_uniform_bytes,
};
use crate::types::GpuNormalStackSource;
use crate::{
    GpuMaskResourceCache, GpuMaskResourceKey, GpuRasterResourceCache, GpuRenderError, GpuRenderer,
};

impl GpuRenderer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_through_group_to_output(
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
                        &child_output_view,
                        generated_raster_source_uniform_bytes_with_blend(
                            1.0,
                            false,
                            base.blend_mode,
                        ),
                        "rizum_clip_through_group_clipping_resolve_pass",
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
                        &child_output_view,
                        generated_raster_source_uniform_bytes_with_blend(1.0, false, *blend_mode),
                        "rizum_clip_through_group_container_clipping_resolve_pass",
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
