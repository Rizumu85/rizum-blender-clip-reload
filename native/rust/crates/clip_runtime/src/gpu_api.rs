use std::cell::RefCell;
use std::time::Instant;

use clip_graph::RenderNodeKind;
use clip_model::{LayerId, Rect, Rgba8};

use crate::blend::StrictRasterBlendMode;
use crate::gpu_provider::{
    GpuResourcePlan, RuntimeGpuResourceProvider, atlas_cache::SparseAtlasCache,
    cache::PersistentGpuTextureCache,
};
use crate::stack_plan::{
    GpuRenderStackSelection, PlannedDecodedRaster, StrictRasterStackDraw, StrictRasterStackOptions,
    byte_diff_count, sample_rgba8,
};
use crate::{
    ClipSession, DrawRasterLayerGpuResult, GpuTextureCacheStats, NormalRasterStackGpuPatchResult,
    NormalRasterStackGpuResult, NormalRasterStackPixelTraceResult,
    NormalRasterStackPixelTraceSample, ReloadPatchRect, RuntimeError, SimpleRasterStackGpuResult,
    SimpleRasterStackUnsupported,
};

mod checkpoint;
mod checkpoint_selection;
mod sparse_atlas;

pub struct RuntimeGpuRenderer {
    renderer: clip_gpu::GpuRenderer,
    texture_cache: RefCell<Option<PersistentGpuTextureCache>>,
    sparse_atlas_cache: RefCell<SparseAtlasCache>,
    sparse_atlas_textures: RefCell<clip_gpu::GpuSparseAtlasTexturePool>,
    segment_checkpoint_cache: RefCell<checkpoint::SegmentCheckpointCache>,
}

impl RuntimeGpuRenderer {
    pub fn new() -> Result<Self, RuntimeError> {
        Ok(Self {
            renderer: clip_gpu::GpuRenderer::new(clip_gpu::GpuDeviceConfig::default())?,
            texture_cache: RefCell::new(None),
            sparse_atlas_cache: RefCell::new(SparseAtlasCache::default()),
            sparse_atlas_textures: RefCell::new(clip_gpu::GpuSparseAtlasTexturePool::default()),
            segment_checkpoint_cache: RefCell::new(checkpoint::SegmentCheckpointCache::default()),
        })
    }

    pub fn new_with_texture_cache() -> Result<Self, RuntimeError> {
        Ok(Self {
            renderer: clip_gpu::GpuRenderer::new(clip_gpu::GpuDeviceConfig::default())?,
            texture_cache: RefCell::new(Some(PersistentGpuTextureCache::new())),
            sparse_atlas_cache: RefCell::new(SparseAtlasCache::default()),
            sparse_atlas_textures: RefCell::new(clip_gpu::GpuSparseAtlasTexturePool::default()),
            segment_checkpoint_cache: RefCell::new(checkpoint::SegmentCheckpointCache::default()),
        })
    }

    pub fn draw_normal_raster_stack(
        &self,
        session: &ClipSession,
    ) -> Result<NormalRasterStackGpuResult, RuntimeError> {
        let mut texture_cache = self.texture_cache.borrow_mut();
        session.draw_normal_raster_stack_with_renderer(&self.renderer, texture_cache.as_mut())
    }

    pub fn draw_normal_raster_stack_patches(
        &self,
        session: &ClipSession,
        rects: &[ReloadPatchRect],
    ) -> Result<NormalRasterStackGpuPatchResult, RuntimeError> {
        let mut texture_cache = self.texture_cache.borrow_mut();
        session.draw_normal_raster_stack_patches_with_renderer(
            &self.renderer,
            texture_cache.as_mut(),
            rects,
        )
    }
}

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
        let renderer = clip_gpu::GpuRenderer::new(clip_gpu::GpuDeviceConfig::default())?;
        self.draw_normal_raster_stack_with_renderer(&renderer, None)
    }

    fn draw_normal_raster_stack_with_renderer(
        &self,
        renderer: &clip_gpu::GpuRenderer,
        mut texture_cache: Option<&mut PersistentGpuTextureCache>,
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
                resource_stats: resource_plan.resource_stats(),
                texture_cache_stats: GpuTextureCacheStats::default(),
                drawn_resources: Vec::new(),
                mask_resources: Vec::new(),
                unsupported,
            });
        }

        let resource_stats = resource_plan.resource_stats();
        let mut provider = match texture_cache.as_deref_mut() {
            Some(cache) => {
                cache.begin_frame();
                RuntimeGpuResourceProvider::with_texture_cache(
                    &self.container,
                    self.summary.canvas,
                    resource_plan,
                    cache,
                )?
            }
            None => RuntimeGpuResourceProvider::new(
                &self.container,
                self.summary.canvas,
                resource_plan,
            )?,
        };
        let output = renderer.draw_normal_stack_with_provider_to_rgba8(
            self.summary.canvas,
            &sources,
            &mut provider,
        )?;
        let mask_resources = std::mem::take(&mut provider.mask_resources);
        drop(provider);
        let texture_cache_stats = texture_cache
            .as_deref()
            .map(PersistentGpuTextureCache::frame_stats)
            .unwrap_or_default();

        Ok(NormalRasterStackGpuResult {
            image: Some(clip_file::tiles::RgbaTileImage {
                width: output.size.width,
                height: output.size.height,
                pixels: output.pixels,
            }),
            source_count,
            resource_stats,
            texture_cache_stats,
            drawn_resources: output.drawn_resources,
            mask_resources,
            unsupported,
        })
    }

    fn draw_normal_raster_stack_patches_with_renderer(
        &self,
        renderer: &clip_gpu::GpuRenderer,
        mut texture_cache: Option<&mut PersistentGpuTextureCache>,
        rects: &[ReloadPatchRect],
    ) -> Result<NormalRasterStackGpuPatchResult, RuntimeError> {
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
        let resource_stats = resource_plan.resource_stats();

        if sources.is_empty() || rects.is_empty() {
            return Ok(NormalRasterStackGpuPatchResult {
                payload: Vec::new(),
                source_count,
                resource_stats,
                texture_cache_stats: GpuTextureCacheStats::default(),
                drawn_resources: Vec::new(),
                mask_resources: Vec::new(),
                unsupported,
            });
        }

        let mut provider = match texture_cache.as_deref_mut() {
            Some(cache) => {
                cache.begin_frame();
                RuntimeGpuResourceProvider::with_texture_cache(
                    &self.container,
                    self.summary.canvas,
                    resource_plan,
                    cache,
                )?
            }
            None => RuntimeGpuResourceProvider::new(
                &self.container,
                self.summary.canvas,
                resource_plan,
            )?,
        };
        let mut payload = Vec::new();
        let mut drawn_resources = Vec::new();
        let render_start = Instant::now();
        for rect in rects {
            let output = renderer.draw_normal_stack_region_with_provider_to_rgba8(
                self.summary.canvas,
                clip_model::Rect::new(rect.x, rect.y, rect.width, rect.height),
                &sources,
                &mut provider,
            )?;
            payload.extend_from_slice(&output.pixels);
            drawn_resources.extend(output.drawn_resources);
        }
        clip_file::decode_profile::record_region_patch_render(render_start.elapsed());
        let mask_resources = std::mem::take(&mut provider.mask_resources);
        drop(provider);
        let texture_cache_stats = texture_cache
            .as_deref()
            .map(PersistentGpuTextureCache::frame_stats)
            .unwrap_or_default();

        Ok(NormalRasterStackGpuPatchResult {
            payload,
            source_count,
            resource_stats,
            texture_cache_stats,
            drawn_resources,
            mask_resources,
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

        let selection = self.select_gpu_normal_render_stack(gpu_trace_options())?;
        let GpuRenderStackSelection {
            sources,
            resource_plan,
            unsupported,
        } = selection;
        self.trace_gpu_sources_pixel(sources, resource_plan, unsupported, x, y)
    }

    pub fn trace_layer_stack_pixel_via_gpu(
        &self,
        layer_id: LayerId,
        x: u32,
        y: u32,
    ) -> Result<NormalRasterStackPixelTraceResult, RuntimeError> {
        if x >= self.summary.canvas.width || y >= self.summary.canvas.height {
            return Err(RuntimeError::InvalidRegion);
        }
        let index = self
            .render_plan
            .nodes
            .iter()
            .position(|node| node.layer_id == layer_id)
            .ok_or(RuntimeError::MissingPlannedRasterLayer { layer_id })?;
        let node = &self.render_plan.nodes[index];
        let subtree_end = self.subtree_end(index);
        let (start, end, depth) = if node.kind == RenderNodeKind::Container {
            (index + 1, subtree_end, node.depth + 1)
        } else {
            (index, subtree_end, node.depth)
        };

        let mut unsupported = Vec::new();
        let mut resource_plan = GpuResourcePlan::default();
        let sources = self.collect_gpu_sources_in_range(
            start,
            end,
            depth,
            gpu_trace_options(),
            &mut unsupported,
            &mut resource_plan,
        )?;

        self.trace_gpu_sources_pixel(sources, resource_plan, unsupported, x, y)
    }

    fn trace_gpu_sources_pixel(
        &self,
        sources: Vec<clip_gpu::GpuNormalStackSource>,
        resource_plan: GpuResourcePlan,
        unsupported: Vec<SimpleRasterStackUnsupported>,
        x: u32,
        y: u32,
    ) -> Result<NormalRasterStackPixelTraceResult, RuntimeError> {
        let source_count = sources.len();
        if sources.is_empty() {
            return Ok(NormalRasterStackPixelTraceResult {
                source_count,
                samples: Vec::new(),
                unsupported,
            });
        }

        let renderer = clip_gpu::GpuRenderer::new(clip_gpu::GpuDeviceConfig::default())?;
        let mut provider =
            RuntimeGpuResourceProvider::new(&self.container, self.summary.canvas, resource_plan)?;

        let trace_region = trace_region_for_pixel(self.summary.canvas, x, y);
        let local_x = x - trace_region.x;
        let local_y = y - trace_region.y;
        let mut samples = Vec::with_capacity(sources.len());
        let mut previous_sample = None;
        for index in 0..sources.len() {
            let sample = match renderer.draw_normal_stack_region_with_provider_to_rgba8(
                self.summary.canvas,
                trace_region,
                &sources[..=index],
                &mut provider,
            ) {
                Ok(output) => sample_rgba8(&output.pixels, output.size, local_x, local_y)?,
                Err(RuntimeError::Gpu(clip_gpu::GpuRenderError::InvalidImageSize)) => {
                    previous_sample.unwrap_or(Rgba8::TRANSPARENT_WHITE)
                }
                Err(err) => return Err(err),
            };
            samples.push(NormalRasterStackPixelTraceSample {
                source_index: index,
                source: gpu_source_trace_label(&sources[index]),
                before_rgba: previous_sample,
                rgba: sample,
                inputs: Vec::new(),
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

fn gpu_trace_options() -> StrictRasterStackOptions {
    StrictRasterStackOptions {
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
    }
}

fn trace_region_for_pixel(canvas: clip_model::CanvasSize, x: u32, y: u32) -> Rect {
    const TRACE_FULL_CANVAS_LIMIT: u32 = 8192;
    const TRACE_REGION_SIZE: u32 = 512;

    if canvas.width <= TRACE_FULL_CANVAS_LIMIT && canvas.height <= TRACE_FULL_CANVAS_LIMIT {
        return Rect::new(0, 0, canvas.width, canvas.height);
    }

    let width = canvas.width.min(TRACE_REGION_SIZE);
    let height = canvas.height.min(TRACE_REGION_SIZE);
    let half_width = width / 2;
    let half_height = height / 2;
    let mut x0 = x.saturating_sub(half_width);
    let mut y0 = y.saturating_sub(half_height);
    if x0 + width > canvas.width {
        x0 = canvas.width - width;
    }
    if y0 + height > canvas.height {
        y0 = canvas.height - height;
    }
    Rect::new(x0, y0, width, height)
}

fn gpu_source_trace_label(source: &clip_gpu::GpuNormalStackSource) -> String {
    match source {
        clip_gpu::GpuNormalStackSource::Raster(raster) => {
            format!("raster {}", gpu_raster_trace_label(raster))
        }
        clip_gpu::GpuNormalStackSource::ClippingRun { base, clipped } => format!(
            "clipping-run base={} clipped={}",
            gpu_raster_trace_label(base),
            clipped.len()
        ),
        clip_gpu::GpuNormalStackSource::ContainerClippingRun {
            children,
            opacity,
            mask_key,
            blend_mode,
            clipped,
        } => format!(
            "container-clipping-run children={} opacity={opacity:.3} blend={blend_mode:?} mask={} clipped={}",
            children.len(),
            gpu_mask_trace_label(*mask_key),
            clipped.len()
        ),
        clip_gpu::GpuNormalStackSource::Container {
            children,
            opacity,
            mask_key,
            blend_mode,
        } => format!(
            "container children={} opacity={opacity:.3} blend={blend_mode:?} mask={}",
            children.len(),
            gpu_mask_trace_label(*mask_key)
        ),
        clip_gpu::GpuNormalStackSource::ThroughGroup {
            children,
            opacity,
            mask_key,
        } => format!(
            "through-group children={} opacity={opacity:.3} mask={}",
            children.len(),
            gpu_mask_trace_label(*mask_key)
        ),
        clip_gpu::GpuNormalStackSource::SolidColor { color, opacity } => format!(
            "solid rgba=[{},{},{},{}] opacity={opacity:.3}",
            color.r, color.g, color.b, color.a
        ),
        clip_gpu::GpuNormalStackSource::LutFilter {
            opacity,
            mask_key,
            filter_mode,
            ..
        } => format!(
            "lut-filter mode={filter_mode:?} opacity={opacity:.3} mask={}",
            gpu_mask_trace_label(*mask_key)
        ),
    }
}

fn gpu_raster_trace_label(raster: &clip_gpu::GpuNormalRasterSource) -> String {
    format!(
        "layer={} mip={} blend={:?} opacity={:.3} mask={} offset=({}, {})",
        raster.key.layer_id.0,
        raster.key.render_mipmap_id,
        raster.blend_mode,
        raster.opacity,
        gpu_mask_trace_label(raster.mask_key),
        raster.offset_x,
        raster.offset_y
    )
}

fn gpu_mask_trace_label(mask_key: Option<clip_gpu::GpuMaskResourceKey>) -> String {
    match mask_key {
        Some(key) => format!("layer:{} mip:{}", key.layer_id.0, key.mask_mipmap_id),
        None => "-".to_string(),
    }
}
