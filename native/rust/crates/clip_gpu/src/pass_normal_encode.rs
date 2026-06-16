use clip_model::CanvasSize;

use super::pass_pipeline::{
    NormalStackPipelines, encode_normal_source_pass, mask_texture_view, raster_texture_view,
    source_mask_texture_view,
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
    pub(super) fn encode_normal_stack_sources(
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
                        through_pipeline,
                        add_glow_pipeline,
                        color_dodge_pipeline,
                        color_burn_pipeline,
                        glow_dodge_pipeline,
                        standard_blend_pipeline,
                        lut_filter_pipeline,
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
                    let mask_view = accum_textures[previous_index]
                        .create_view(&wgpu::TextureViewDescriptor::default());
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
                        "rizum_clip_normal_container_clipping_resolve_pass",
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
}
