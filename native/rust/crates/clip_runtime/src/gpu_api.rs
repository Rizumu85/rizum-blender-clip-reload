use clip_graph::RenderNodeKind;
use clip_model::LayerId;

use crate::blend::StrictRasterBlendMode;
use crate::gpu_provider::RuntimeGpuResourceProvider;
use crate::stack_plan::{
    GpuRenderStackSelection, PlannedDecodedRaster, StrictRasterStackDraw, StrictRasterStackOptions,
    byte_diff_count, decoded_containers_in_draws, decoded_lut_filters_in_draws,
    decoded_rasters_in_draws, decoded_through_groups_in_draws, gpu_normal_stack_source,
    sample_rgba8, stack_draw_trace_inputs, stack_draw_trace_label,
};
use crate::{
    ClipSession, DrawRasterLayerGpuResult, NormalRasterStackGpuResult,
    NormalRasterStackPixelTraceResult, NormalRasterStackPixelTraceSample, RuntimeError,
    SimpleRasterStackGpuResult,
};

impl ClipSession {
    pub fn read_raster_layer_rgba_via_gpu(
        &self,
        layer_id: LayerId,
    ) -> Result<clip_file::tiles::RgbaTileImage, RuntimeError> {
        let image = clip_file::read_raster_layer_rgba(&self.path, layer_id)?;
        let renderer = clip_gpu::GpuRenderer::new(clip_gpu::GpuDeviceConfig::default())?;
        let pixels = renderer.roundtrip_rgba8(image.width, image.height, &image.pixels)?;
        Ok(clip_file::tiles::RgbaTileImage {
            width: image.width,
            height: image.height,
            pixels,
        })
    }

    pub fn upload_planned_raster_resources_via_gpu(
        &self,
    ) -> Result<Vec<clip_gpu::GpuRasterResourceInfo>, RuntimeError> {
        let mut decoded = Vec::new();
        for node in self
            .render_plan
            .nodes
            .iter()
            .filter(|node| node.kind == RenderNodeKind::Raster)
        {
            let render_mipmap_id =
                node.render_mipmap_id
                    .ok_or(RuntimeError::MissingRasterRenderMipmap {
                        layer_id: node.layer_id,
                    })?;
            let image = clip_file::read_raster_layer_rgba(&self.path, node.layer_id)?;
            decoded.push(PlannedDecodedRaster {
                render_node_id: node.id,
                layer_id: node.layer_id,
                render_mipmap_id,
                image,
                offset_x: 0,
                offset_y: 0,
                opacity: 1.0,
                mask: None,
                blend_mode: StrictRasterBlendMode::Normal,
            });
        }

        let uploads: Vec<_> = decoded
            .iter()
            .map(|decoded| clip_gpu::GpuRasterUpload {
                layer_id: decoded.layer_id,
                render_node_id: decoded.render_node_id,
                render_mipmap_id: decoded.render_mipmap_id,
                size: clip_model::CanvasSize::new(decoded.image.width, decoded.image.height),
                pixels: &decoded.image.pixels,
            })
            .collect();
        let renderer = clip_gpu::GpuRenderer::new(clip_gpu::GpuDeviceConfig::default())?;
        let cache = renderer.upload_raster_resources(&uploads)?;
        Ok(cache.resource_infos().collect())
    }

    pub fn draw_raster_layer_rgba_via_gpu(
        &self,
        layer_id: LayerId,
    ) -> Result<DrawRasterLayerGpuResult, RuntimeError> {
        let node = self
            .render_plan
            .nodes
            .iter()
            .find(|node| node.kind == RenderNodeKind::Raster && node.layer_id == layer_id)
            .ok_or(RuntimeError::MissingPlannedRasterLayer { layer_id })?;
        let render_mipmap_id =
            node.render_mipmap_id
                .ok_or(RuntimeError::MissingRasterRenderMipmap {
                    layer_id: node.layer_id,
                })?;
        let source = clip_file::read_raster_layer_rgba(&self.path, layer_id)?;
        let renderer = clip_gpu::GpuRenderer::new(clip_gpu::GpuDeviceConfig::default())?;
        let upload = clip_gpu::GpuRasterUpload {
            layer_id,
            render_node_id: node.id,
            render_mipmap_id,
            size: clip_model::CanvasSize::new(source.width, source.height),
            pixels: &source.pixels,
        };
        let cache = renderer.upload_raster_resources(&[upload])?;
        let output = renderer.draw_raster_resource_to_rgba8(
            &cache,
            clip_gpu::GpuRasterResourceKey {
                layer_id,
                render_mipmap_id,
            },
        )?;
        let differing_bytes = source
            .pixels
            .iter()
            .zip(output.pixels.iter())
            .filter(|(expected, actual)| expected != actual)
            .count();
        Ok(DrawRasterLayerGpuResult {
            image: clip_file::tiles::RgbaTileImage {
                width: output.size.width,
                height: output.size.height,
                pixels: output.pixels,
            },
            resource_info: output.resource_info,
            differing_bytes,
        })
    }

    pub fn draw_simple_raster_stack_via_gpu(
        &self,
    ) -> Result<SimpleRasterStackGpuResult, RuntimeError> {
        let selection = self.select_strict_normal_raster_stack(StrictRasterStackOptions {
            allow_alpha_compositing: false,
            allow_paper: false,
            allow_layer_opacity: false,
            allow_masks: false,
            allow_clipping_runs: false,
            allow_container_isolation: false,
            allow_through_groups: false,
            allow_add_blend: false,
            allow_add_glow_blend: false,
            allow_color_burn_blend: false,
            allow_color_dodge_blend: false,
            allow_extended_blends: false,
            allow_glow_dodge_blend: false,
            allow_hard_mix_blend: false,
            allow_hsl_blends: false,
            allow_simple_blends: false,
            allow_soft_light_blend: false,
            allow_lut_filters: false,
            allow_vivid_light_blend: false,
            allow_w3c_blends: false,
            allow_initial_terminal_container_elision: false,
        })?;
        let decoded_draws: Vec<_> = selection
            .draws
            .iter()
            .filter_map(StrictRasterStackDraw::as_raster)
            .collect();
        let unsupported = selection.unsupported;

        if decoded_draws.is_empty() {
            return Ok(SimpleRasterStackGpuResult {
                image: None,
                drawn_resources: Vec::new(),
                unsupported,
                differing_bytes_from_last_drawn: None,
            });
        }

        let uploads: Vec<_> = decoded_draws
            .iter()
            .map(|decoded| clip_gpu::GpuRasterUpload {
                layer_id: decoded.layer_id,
                render_node_id: decoded.render_node_id,
                render_mipmap_id: decoded.render_mipmap_id,
                size: clip_model::CanvasSize::new(decoded.image.width, decoded.image.height),
                pixels: &decoded.image.pixels,
            })
            .collect();
        let keys: Vec<_> = decoded_draws
            .iter()
            .map(|decoded| clip_gpu::GpuRasterResourceKey {
                layer_id: decoded.layer_id,
                render_mipmap_id: decoded.render_mipmap_id,
            })
            .collect();
        let renderer = clip_gpu::GpuRenderer::new(clip_gpu::GpuDeviceConfig::default())?;
        let cache = renderer.upload_raster_resources(&uploads)?;
        let output = renderer.draw_raster_stack_to_rgba8(&cache, &keys)?;
        let last_drawn = decoded_draws.last().expect("decoded_draws is not empty");
        let differing_bytes_from_last_drawn =
            byte_diff_count(&last_drawn.image.pixels, &output.pixels);

        Ok(SimpleRasterStackGpuResult {
            image: Some(clip_file::tiles::RgbaTileImage {
                width: output.size.width,
                height: output.size.height,
                pixels: output.pixels,
            }),
            drawn_resources: output.drawn_resources,
            unsupported,
            differing_bytes_from_last_drawn: Some(differing_bytes_from_last_drawn),
        })
    }

    pub fn draw_normal_raster_stack_via_gpu(
        &self,
    ) -> Result<NormalRasterStackGpuResult, RuntimeError> {
        let selection = self.select_gpu_normal_render_stack(StrictRasterStackOptions {
            allow_alpha_compositing: true,
            allow_paper: true,
            allow_layer_opacity: true,
            allow_masks: true,
            allow_clipping_runs: true,
            allow_container_isolation: true,
            allow_through_groups: true,
            allow_add_blend: true,
            allow_add_glow_blend: true,
            allow_color_burn_blend: true,
            allow_color_dodge_blend: true,
            allow_extended_blends: true,
            allow_glow_dodge_blend: true,
            allow_hard_mix_blend: true,
            allow_hsl_blends: true,
            allow_simple_blends: true,
            allow_soft_light_blend: true,
            allow_lut_filters: true,
            allow_vivid_light_blend: true,
            allow_w3c_blends: true,
            allow_initial_terminal_container_elision: true,
        })?;
        let GpuRenderStackSelection {
            sources,
            resource_plan,
            unsupported,
        } = selection;
        let source_count = sources.len();

        if sources.is_empty() {
            return Ok(NormalRasterStackGpuResult {
                image: None,
                source_count,
                drawn_resources: Vec::new(),
                mask_resources: Vec::new(),
                unsupported,
            });
        }

        let renderer = clip_gpu::GpuRenderer::new(clip_gpu::GpuDeviceConfig::default())?;
        let mut provider =
            RuntimeGpuResourceProvider::new(&self.container, self.summary.canvas, resource_plan);
        let output = renderer.draw_normal_stack_with_provider_to_rgba8(
            self.summary.canvas,
            &sources,
            &mut provider,
        )?;

        Ok(NormalRasterStackGpuResult {
            image: Some(clip_file::tiles::RgbaTileImage {
                width: output.size.width,
                height: output.size.height,
                pixels: output.pixels,
            }),
            source_count,
            drawn_resources: output.drawn_resources,
            mask_resources: provider.mask_resources,
            unsupported,
        })
    }

    pub fn trace_normal_raster_stack_pixel_via_gpu(
        &self,
        x: u32,
        y: u32,
    ) -> Result<NormalRasterStackPixelTraceResult, RuntimeError> {
        if x >= self.summary.canvas.width || y >= self.summary.canvas.height {
            return Err(RuntimeError::InvalidRegion);
        }

        let selection = self.select_strict_normal_raster_stack(StrictRasterStackOptions {
            allow_alpha_compositing: true,
            allow_paper: true,
            allow_layer_opacity: true,
            allow_masks: true,
            allow_clipping_runs: true,
            allow_container_isolation: true,
            allow_through_groups: true,
            allow_add_blend: true,
            allow_add_glow_blend: true,
            allow_color_burn_blend: true,
            allow_color_dodge_blend: true,
            allow_extended_blends: true,
            allow_glow_dodge_blend: true,
            allow_hard_mix_blend: true,
            allow_hsl_blends: true,
            allow_simple_blends: true,
            allow_soft_light_blend: true,
            allow_lut_filters: true,
            allow_vivid_light_blend: true,
            allow_w3c_blends: true,
            allow_initial_terminal_container_elision: false,
        })?;
        let source_count = selection.draws.len();
        let unsupported = selection.unsupported;
        if selection.draws.is_empty() {
            return Ok(NormalRasterStackPixelTraceResult {
                source_count,
                samples: Vec::new(),
                unsupported,
            });
        }

        let decoded_rasters = decoded_rasters_in_draws(&selection.draws);
        let uploads: Vec<_> = decoded_rasters
            .iter()
            .map(|decoded| clip_gpu::GpuRasterUpload {
                layer_id: decoded.layer_id,
                render_node_id: decoded.render_node_id,
                render_mipmap_id: decoded.render_mipmap_id,
                size: clip_model::CanvasSize::new(decoded.image.width, decoded.image.height),
                pixels: &decoded.image.pixels,
            })
            .collect();
        let decoded_containers = decoded_containers_in_draws(&selection.draws);
        let decoded_through_groups = decoded_through_groups_in_draws(&selection.draws);
        let decoded_lut_filters = decoded_lut_filters_in_draws(&selection.draws);
        let mask_uploads: Vec<_> = decoded_rasters
            .iter()
            .filter_map(|decoded| {
                decoded
                    .mask
                    .as_ref()
                    .map(|mask| (decoded.render_node_id, decoded.layer_id, mask))
            })
            .chain(decoded_containers.iter().filter_map(|container| {
                container
                    .mask
                    .as_ref()
                    .map(|mask| (container.render_node_id, container.layer_id, mask))
            }))
            .chain(decoded_through_groups.iter().filter_map(|through_group| {
                through_group
                    .mask
                    .as_ref()
                    .map(|mask| (through_group.render_node_id, through_group.layer_id, mask))
            }))
            .chain(decoded_lut_filters.iter().filter_map(|filter| {
                filter
                    .mask
                    .as_ref()
                    .map(|mask| (filter.render_node_id, filter.layer_id, mask))
            }))
            .map(|(render_node_id, layer_id, mask)| clip_gpu::GpuMaskUpload {
                layer_id,
                render_node_id,
                mask_mipmap_id: mask.mask_mipmap_id,
                size: clip_model::CanvasSize::new(mask.image.width, mask.image.height),
                origin_x: 0,
                origin_y: 0,
                fill_value: 0,
                upload_origin_x: 0,
                upload_origin_y: 0,
                upload_size: clip_model::CanvasSize::new(mask.image.width, mask.image.height),
                pixels: &mask.image.pixels,
            })
            .collect();
        let sources: Vec<_> = selection
            .draws
            .iter()
            .map(gpu_normal_stack_source)
            .collect();
        let renderer = clip_gpu::GpuRenderer::new(clip_gpu::GpuDeviceConfig::default())?;
        let cache = renderer.upload_raster_resources(&uploads)?;
        let mask_cache = if mask_uploads.is_empty() {
            None
        } else {
            Some(renderer.upload_mask_resources(&mask_uploads)?)
        };

        let mut samples = Vec::with_capacity(sources.len());
        let mut previous_sample = None;
        for index in 0..sources.len() {
            let output = renderer.draw_normal_stack_to_rgba8(
                &cache,
                mask_cache.as_ref(),
                self.summary.canvas,
                &sources[..=index],
            )?;
            let sample = sample_rgba8(&output.pixels, output.size, x, y)?;
            samples.push(NormalRasterStackPixelTraceSample {
                source_index: index,
                source: stack_draw_trace_label(&selection.draws[index]),
                before_rgba: previous_sample,
                rgba: sample,
                inputs: stack_draw_trace_inputs(&selection.draws[index], x, y)?,
            });
            previous_sample = Some(sample);
        }

        Ok(NormalRasterStackPixelTraceResult {
            source_count,
            samples,
            unsupported,
        })
    }
}
