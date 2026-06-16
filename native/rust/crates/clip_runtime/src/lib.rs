#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use clip_file::ClipFileSummary;
use clip_graph::{LayerGraphInput, RenderNodeId, RenderNodeKind, RenderPlan};
use clip_model::{CanvasSize, LayerId, LayerOpacity, Rect, Rgba8};

mod filter_lut;
mod gpu_provider;
mod source_crop;
mod support;

use filter_lut::{PlannedLutFilterMode, lut_filter_rgba};
use gpu_provider::{
    GpuResourcePlan, PlannedGpuMaskResource, RuntimeGpuResourceProvider, plan_gpu_mask_resource,
};

const LAYER_COMPOSITE_THROUGH: u32 = 30;
const LAYER_COMPOSITE_DARKEN: u32 = 1;
const LAYER_COMPOSITE_MULTIPLY: u32 = 2;
const LAYER_COMPOSITE_COLOR_BURN: u32 = 3;
const LAYER_COMPOSITE_LINEAR_BURN: u32 = 4;
const LAYER_COMPOSITE_SUBTRACT: u32 = 5;
const LAYER_COMPOSITE_DARKER_COLOR: u32 = 6;
const LAYER_COMPOSITE_LIGHTEN: u32 = 7;
const LAYER_COMPOSITE_SCREEN: u32 = 8;
const LAYER_COMPOSITE_COLOR_DODGE: u32 = 9;
const LAYER_COMPOSITE_GLOW_DODGE: u32 = 10;
const LAYER_COMPOSITE_ADD: u32 = 11;
const LAYER_COMPOSITE_ADD_GLOW: u32 = 12;
const LAYER_COMPOSITE_LIGHTER_COLOR: u32 = 13;
const LAYER_COMPOSITE_OVERLAY: u32 = 14;
const LAYER_COMPOSITE_SOFT_LIGHT: u32 = 15;
const LAYER_COMPOSITE_HARD_LIGHT: u32 = 16;
const LAYER_COMPOSITE_VIVID_LIGHT: u32 = 17;
const LAYER_COMPOSITE_LINEAR_LIGHT: u32 = 18;
const LAYER_COMPOSITE_PIN_LIGHT: u32 = 19;
const LAYER_COMPOSITE_HARD_MIX: u32 = 20;
const LAYER_COMPOSITE_DIFFERENCE: u32 = 21;
const LAYER_COMPOSITE_EXCLUSION: u32 = 22;
const LAYER_COMPOSITE_HUE: u32 = 23;
const LAYER_COMPOSITE_SATURATION: u32 = 24;
const LAYER_COMPOSITE_COLOR: u32 = 25;
const LAYER_COMPOSITE_BRIGHTNESS: u32 = 26;
const LAYER_COMPOSITE_DIVIDE: u32 = 36;

#[derive(Debug)]
pub enum RuntimeError {
    File(clip_file::ClipFileError),
    Graph(clip_graph::RenderPlanError),
    Gpu(clip_gpu::GpuRenderError),
    MissingRasterRenderMipmap {
        layer_id: LayerId,
    },
    MissingPlannedRasterLayer {
        layer_id: LayerId,
    },
    UnsupportedRenderPlan {
        unsupported: Vec<SimpleRasterStackUnsupported>,
    },
    EmptyRenderPlan,
    InvalidRegion,
    OutputBufferTooSmall {
        expected: usize,
        actual: usize,
    },
}

impl From<clip_file::ClipFileError> for RuntimeError {
    fn from(value: clip_file::ClipFileError) -> Self {
        Self::File(value)
    }
}

impl From<clip_graph::RenderPlanError> for RuntimeError {
    fn from(value: clip_graph::RenderPlanError) -> Self {
        Self::Graph(value)
    }
}

impl From<clip_gpu::GpuRenderError> for RuntimeError {
    fn from(value: clip_gpu::GpuRenderError) -> Self {
        Self::Gpu(value)
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(err) => write!(f, "{err}"),
            Self::Graph(err) => write!(f, "{err}"),
            Self::Gpu(err) => write!(f, "{err}"),
            Self::MissingRasterRenderMipmap { layer_id } => {
                write!(
                    f,
                    "planned raster layer {} has no render mipmap",
                    layer_id.0
                )
            }
            Self::MissingPlannedRasterLayer { layer_id } => {
                write!(
                    f,
                    "layer {} is not a visible planned raster layer",
                    layer_id.0
                )
            }
            Self::UnsupportedRenderPlan { unsupported } => write!(
                f,
                "strict native NORMAL renderer does not yet support {} planned nodes",
                unsupported.len(),
            ),
            Self::EmptyRenderPlan => f.write_str("render plan has no drawable native sources"),
            Self::InvalidRegion => f.write_str("requested image region is outside the canvas"),
            Self::OutputBufferTooSmall { expected, actual } => write!(
                f,
                "output buffer too small: expected at least {expected} bytes, got {actual}",
            ),
        }
    }
}

impl Error for RuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::File(err) => Some(err),
            Self::Graph(err) => Some(err),
            Self::Gpu(err) => Some(err),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct ClipSession {
    path: PathBuf,
    container: clip_file::container::ClipContainer,
    summary: ClipFileSummary,
    render_plan: RenderPlan,
    raster_sources: HashMap<LayerId, clip_file::metadata::RasterLayerSource>,
    mask_sources: HashMap<LayerId, clip_file::metadata::MaskLayerSource>,
    filter_sources: HashMap<LayerId, clip_file::metadata::FilterLayerSource>,
    rendered_image: Option<clip_file::tiles::RgbaTileImage>,
}

impl ClipSession {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RuntimeError> {
        let path = path.as_ref().to_path_buf();
        let container = clip_file::container::ClipContainer::open(&path)?;
        let summary = clip_file::metadata::read_summary_from_sqlite(
            container.sqlite_bytes(),
            container.external_data().len(),
        )?;
        let graph_records =
            clip_file::metadata::read_layer_graph_records_from_sqlite(container.sqlite_bytes())?;
        let graph_inputs: Vec<_> = graph_records
            .iter()
            .map(layer_graph_input_from_file)
            .collect();
        let render_plan = RenderPlan::build(summary.canvas, summary.root_layer_id, &graph_inputs)?;
        let raster_layer_ids: Vec<_> = render_plan
            .nodes
            .iter()
            .filter(|node| node.kind == RenderNodeKind::Raster)
            .map(|node| node.layer_id)
            .collect();
        let mask_layer_ids: Vec<_> = render_plan
            .nodes
            .iter()
            .filter(|node| node.mask_mipmap_id.is_some())
            .map(|node| node.layer_id)
            .collect();
        let filter_layer_ids: Vec<_> = render_plan
            .nodes
            .iter()
            .filter(|node| node.kind == RenderNodeKind::Filter)
            .map(|node| node.layer_id)
            .collect();
        let raster_sources = clip_file::metadata::read_raster_layer_sources_from_sqlite(
            container.sqlite_bytes(),
            &raster_layer_ids,
            summary.canvas,
        )?;
        let mask_sources = clip_file::metadata::read_mask_layer_sources_from_sqlite(
            container.sqlite_bytes(),
            &mask_layer_ids,
            summary.canvas,
        )?;
        let filter_sources = clip_file::metadata::read_filter_layer_sources_from_sqlite(
            container.sqlite_bytes(),
            &filter_layer_ids,
        )?;
        Ok(Self {
            path,
            container,
            summary,
            render_plan,
            raster_sources,
            mask_sources,
            filter_sources,
            rendered_image: None,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn summary(&self) -> &ClipFileSummary {
        &self.summary
    }

    pub fn render_plan(&self) -> &RenderPlan {
        &self.render_plan
    }

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

    fn select_strict_normal_raster_stack(
        &self,
        options: StrictRasterStackOptions,
    ) -> Result<StrictRasterStackSelection, RuntimeError> {
        let mut unsupported = Vec::new();
        let draws = if self.render_plan.nodes.first().map(|node| node.layer_id)
            == Some(self.summary.root_layer_id)
        {
            let root_end = self.subtree_end(0);
            self.collect_strict_draws_in_range(1, root_end, 1, options, &mut unsupported)?
        } else {
            self.collect_strict_draws_in_range(
                0,
                self.render_plan.nodes.len(),
                0,
                options,
                &mut unsupported,
            )?
        };

        Ok(StrictRasterStackSelection { draws, unsupported })
    }

    fn select_gpu_normal_render_stack(
        &self,
        options: StrictRasterStackOptions,
    ) -> Result<GpuRenderStackSelection, RuntimeError> {
        let mut unsupported = Vec::new();
        let mut resource_plan = GpuResourcePlan::default();
        let sources = if self.render_plan.nodes.first().map(|node| node.layer_id)
            == Some(self.summary.root_layer_id)
        {
            let root_end = self.subtree_end(0);
            self.collect_gpu_sources_in_range(
                1,
                root_end,
                1,
                options,
                &mut unsupported,
                &mut resource_plan,
            )?
        } else {
            self.collect_gpu_sources_in_range(
                0,
                self.render_plan.nodes.len(),
                0,
                options,
                &mut unsupported,
                &mut resource_plan,
            )?
        };

        Ok(GpuRenderStackSelection {
            sources,
            resource_plan,
            unsupported,
        })
    }

    fn collect_gpu_sources_in_range(
        &self,
        start: usize,
        end: usize,
        depth: u16,
        options: StrictRasterStackOptions,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
        resource_plan: &mut GpuResourcePlan,
    ) -> Result<Vec<clip_gpu::GpuNormalStackSource>, RuntimeError> {
        let mut sources = Vec::new();
        let mut has_drawn_output = false;
        let mut clip_base_state = ClipBaseState::Cleared;
        let mut index = start;

        while index < end {
            let node = &self.render_plan.nodes[index];
            if node.depth < depth {
                break;
            }
            if node.depth > depth {
                unsupported.push(SimpleRasterStackUnsupported {
                    render_node_id: node.id,
                    layer_id: node.layer_id,
                    kind: node.kind,
                    reason: SimpleRasterStackUnsupportedReason::InsideUnsupportedContainer,
                });
                clip_base_state = ClipBaseState::Blocked;
                index += 1;
                continue;
            }

            match node.kind {
                RenderNodeKind::Container => {
                    let subtree_end = self.subtree_end(index);
                    if node.composite == LAYER_COMPOSITE_THROUGH {
                        if let Some(through_group) = self.collect_gpu_through_group_source(
                            index,
                            subtree_end,
                            options,
                            unsupported,
                            resource_plan,
                        )? {
                            has_drawn_output = true;
                            sources.push(through_group);
                        }
                        clip_base_state = ClipBaseState::Cleared;
                    } else if let Some(container) = self.collect_gpu_container_source(
                        index,
                        subtree_end,
                        options,
                        unsupported,
                        resource_plan,
                    )? {
                        if can_elide_initial_terminal_container(
                            options,
                            node,
                            subtree_end,
                            end,
                            has_drawn_output,
                        ) {
                            if let clip_gpu::GpuNormalStackSource::Container { children, .. } =
                                container
                            {
                                has_drawn_output = has_drawn_output || !children.is_empty();
                                sources.extend(children);
                            }
                        } else {
                            has_drawn_output = true;
                            sources.push(container);
                        }
                        clip_base_state = ClipBaseState::Available;
                    } else {
                        clip_base_state = ClipBaseState::Blocked;
                    }
                    index = subtree_end;
                }
                RenderNodeKind::Paper => {
                    if let Some(source) = self.collect_gpu_paper_source(node, options, unsupported)
                    {
                        has_drawn_output = true;
                        sources.push(source);
                        clip_base_state = ClipBaseState::Available;
                    } else {
                        clip_base_state = ClipBaseState::Blocked;
                    }
                    index += 1;
                }
                RenderNodeKind::Raster => {
                    let orphan_clipped = node.clip && clip_base_state == ClipBaseState::Cleared;
                    let Some(raster) = self.plan_gpu_raster_source(
                        node,
                        options,
                        orphan_clipped,
                        unsupported,
                        resource_plan,
                    )?
                    else {
                        clip_base_state = ClipBaseState::Blocked;
                        index += 1;
                        continue;
                    };

                    if options.allow_clipping_runs && !node.clip {
                        let (clipped, next_index) = self.collect_gpu_clipped_siblings(
                            index + 1,
                            node.depth,
                            end,
                            options,
                            unsupported,
                            resource_plan,
                        )?;
                        if !clipped.is_empty() {
                            has_drawn_output = true;
                            sources.push(clip_gpu::GpuNormalStackSource::ClippingRun {
                                base: raster,
                                clipped,
                            });
                            clip_base_state = ClipBaseState::Cleared;
                            index = next_index;
                            continue;
                        }
                        index = next_index;
                    } else {
                        index += 1;
                    }

                    if !options.allow_alpha_compositing && has_drawn_output {
                        unsupported.push(SimpleRasterStackUnsupported {
                            render_node_id: node.id,
                            layer_id: node.layer_id,
                            kind: node.kind,
                            reason: SimpleRasterStackUnsupportedReason::RequiresAlphaCompositing,
                        });
                        continue;
                    }

                    has_drawn_output = true;
                    clip_base_state = if node.clip {
                        ClipBaseState::Cleared
                    } else {
                        ClipBaseState::Available
                    };
                    sources.push(clip_gpu::GpuNormalStackSource::Raster(raster));
                }
                RenderNodeKind::Filter => {
                    if let Some(filter) =
                        self.plan_gpu_lut_filter_source(node, options, unsupported, resource_plan)?
                    {
                        has_drawn_output = true;
                        sources.push(filter);
                        clip_base_state = ClipBaseState::Cleared;
                    } else {
                        clip_base_state = ClipBaseState::Blocked;
                    }
                    index += 1;
                }
                RenderNodeKind::Unsupported(raw_kind) => {
                    unsupported.push(SimpleRasterStackUnsupported {
                        render_node_id: node.id,
                        layer_id: node.layer_id,
                        kind: node.kind,
                        reason: SimpleRasterStackUnsupportedReason::UnsupportedLayerKind(raw_kind),
                    });
                    clip_base_state = ClipBaseState::Blocked;
                    index += 1;
                }
            }
        }

        Ok(sources)
    }

    fn collect_strict_draws_in_range(
        &self,
        start: usize,
        end: usize,
        depth: u16,
        options: StrictRasterStackOptions,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
    ) -> Result<Vec<StrictRasterStackDraw>, RuntimeError> {
        let mut draws = Vec::new();
        let mut has_drawn_output = false;
        let mut clip_base_state = ClipBaseState::Cleared;
        let mut index = start;

        while index < end {
            let node = &self.render_plan.nodes[index];
            if node.depth < depth {
                break;
            }
            if node.depth > depth {
                unsupported.push(SimpleRasterStackUnsupported {
                    render_node_id: node.id,
                    layer_id: node.layer_id,
                    kind: node.kind,
                    reason: SimpleRasterStackUnsupportedReason::InsideUnsupportedContainer,
                });
                clip_base_state = ClipBaseState::Blocked;
                index += 1;
                continue;
            }

            match node.kind {
                RenderNodeKind::Container => {
                    let subtree_end = self.subtree_end(index);
                    if node.composite == LAYER_COMPOSITE_THROUGH {
                        if let Some(through_group) = self.collect_strict_through_group_draw(
                            index,
                            subtree_end,
                            options,
                            unsupported,
                        )? {
                            has_drawn_output = true;
                            draws.push(StrictRasterStackDraw::ThroughGroup(through_group));
                        }
                        clip_base_state = ClipBaseState::Cleared;
                    } else if let Some(container) = self.collect_strict_container_draw(
                        index,
                        subtree_end,
                        options,
                        unsupported,
                    )? {
                        if can_elide_initial_terminal_container(
                            options,
                            node,
                            subtree_end,
                            end,
                            has_drawn_output,
                        ) {
                            has_drawn_output = has_drawn_output || !container.draws.is_empty();
                            draws.extend(container.draws);
                        } else {
                            has_drawn_output = true;
                            draws.push(StrictRasterStackDraw::Container(container));
                        }
                        clip_base_state = ClipBaseState::Available;
                    } else {
                        clip_base_state = ClipBaseState::Blocked;
                    }
                    index = subtree_end;
                }
                RenderNodeKind::Paper => {
                    if let Some(draw) = self.collect_strict_paper_draw(node, options, unsupported) {
                        has_drawn_output = true;
                        draws.push(draw);
                        clip_base_state = ClipBaseState::Available;
                    } else {
                        clip_base_state = ClipBaseState::Blocked;
                    }
                    index += 1;
                }
                RenderNodeKind::Raster => {
                    let orphan_clipped = node.clip && clip_base_state == ClipBaseState::Cleared;
                    let Some(decoded) = self.decode_strict_normal_raster_node(
                        node,
                        options,
                        orphan_clipped,
                        unsupported,
                    )?
                    else {
                        clip_base_state = ClipBaseState::Blocked;
                        index += 1;
                        continue;
                    };

                    if options.allow_clipping_runs && !node.clip {
                        let (clipped, next_index) = self.collect_strict_clipped_siblings(
                            index + 1,
                            node.depth,
                            end,
                            options,
                            unsupported,
                        )?;
                        if !clipped.is_empty() {
                            has_drawn_output = true;
                            draws.push(StrictRasterStackDraw::ClippingRun(PlannedClippingRun {
                                base: decoded,
                                clipped,
                            }));
                            clip_base_state = ClipBaseState::Cleared;
                            index = next_index;
                            continue;
                        }
                        index = next_index;
                    } else {
                        index += 1;
                    }

                    if !options.allow_alpha_compositing
                        && has_drawn_output
                        && !alpha_is_fully_opaque(&decoded.image.pixels)
                    {
                        unsupported.push(SimpleRasterStackUnsupported {
                            render_node_id: node.id,
                            layer_id: node.layer_id,
                            kind: node.kind,
                            reason: SimpleRasterStackUnsupportedReason::RequiresAlphaCompositing,
                        });
                        continue;
                    }

                    has_drawn_output = true;
                    clip_base_state = if node.clip {
                        ClipBaseState::Cleared
                    } else {
                        ClipBaseState::Available
                    };
                    draws.push(StrictRasterStackDraw::Raster(decoded));
                }
                RenderNodeKind::Filter => {
                    if let Some(filter) =
                        self.decode_strict_lut_filter_node(node, options, unsupported)?
                    {
                        has_drawn_output = true;
                        draws.push(StrictRasterStackDraw::LutFilter(filter));
                        clip_base_state = ClipBaseState::Cleared;
                    } else {
                        clip_base_state = ClipBaseState::Blocked;
                    }
                    index += 1;
                }
                RenderNodeKind::Unsupported(raw_kind) => {
                    unsupported.push(SimpleRasterStackUnsupported {
                        render_node_id: node.id,
                        layer_id: node.layer_id,
                        kind: node.kind,
                        reason: SimpleRasterStackUnsupportedReason::UnsupportedLayerKind(raw_kind),
                    });
                    clip_base_state = ClipBaseState::Blocked;
                    index += 1;
                }
            }
        }

        Ok(draws)
    }

    fn collect_strict_paper_draw(
        &self,
        node: &clip_graph::RenderNode,
        options: StrictRasterStackOptions,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
    ) -> Option<StrictRasterStackDraw> {
        if !options.allow_paper {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Paper,
            });
            return None;
        }
        if node.clip || node.composite != 0 || node.mask_mipmap_id.is_some() {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::PaperSemantics,
            });
            return None;
        }
        if !options.allow_layer_opacity && node.opacity != LayerOpacity::MAX {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Opacity(node.opacity.0),
            });
            return None;
        }
        let Some(opacity) = opacity_factor(node.opacity) else {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::OpacityOutOfRange(node.opacity.0),
            });
            return None;
        };
        let Some(color) = node.paper_color else {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::PaperColorMissing,
            });
            return None;
        };
        Some(StrictRasterStackDraw::Paper { color, opacity })
    }

    fn collect_gpu_paper_source(
        &self,
        node: &clip_graph::RenderNode,
        options: StrictRasterStackOptions,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
    ) -> Option<clip_gpu::GpuNormalStackSource> {
        match self.collect_strict_paper_draw(node, options, unsupported)? {
            StrictRasterStackDraw::Paper { color, opacity } => {
                Some(clip_gpu::GpuNormalStackSource::SolidColor { color, opacity })
            }
            _ => None,
        }
    }

    fn collect_gpu_container_source(
        &self,
        index: usize,
        subtree_end: usize,
        options: StrictRasterStackOptions,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
        resource_plan: &mut GpuResourcePlan,
    ) -> Result<Option<clip_gpu::GpuNormalStackSource>, RuntimeError> {
        let node = &self.render_plan.nodes[index];
        if !options.allow_container_isolation || node.clip {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::ContainerSemantics,
                unsupported,
            );
            return Ok(None);
        }
        let Some(blend_mode) = strict_raster_blend_mode(node, options, false) else {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::Composite(node.composite),
                unsupported,
            );
            return Ok(None);
        };
        if !options.allow_layer_opacity && node.opacity != LayerOpacity::MAX {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::Opacity(node.opacity.0),
                unsupported,
            );
            return Ok(None);
        }
        let Some(opacity) = opacity_factor(node.opacity) else {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::OpacityOutOfRange(node.opacity.0),
                unsupported,
            );
            return Ok(None);
        };
        if node.mask_mipmap_id.is_some() && !options.allow_masks {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::Mask,
                unsupported,
            );
            return Ok(None);
        }

        let (mask_key, opacity) = apply_planned_gpu_mask(
            plan_gpu_mask_resource(&self.mask_sources, node, self.summary.canvas, resource_plan)?,
            opacity,
        );
        let children = self.collect_gpu_sources_in_range(
            index + 1,
            subtree_end,
            node.depth + 1,
            options,
            unsupported,
            resource_plan,
        )?;
        if children.is_empty() {
            return Ok(None);
        }
        Ok(Some(clip_gpu::GpuNormalStackSource::Container {
            children,
            opacity,
            mask_key,
            blend_mode: gpu_raster_blend_mode(blend_mode),
        }))
    }

    fn collect_gpu_through_group_source(
        &self,
        index: usize,
        subtree_end: usize,
        options: StrictRasterStackOptions,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
        resource_plan: &mut GpuResourcePlan,
    ) -> Result<Option<clip_gpu::GpuNormalStackSource>, RuntimeError> {
        let node = &self.render_plan.nodes[index];
        if !options.allow_through_groups || node.clip || node.composite != LAYER_COMPOSITE_THROUGH {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::ContainerSemantics,
                unsupported,
            );
            return Ok(None);
        }
        if !options.allow_layer_opacity && node.opacity != LayerOpacity::MAX {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::Opacity(node.opacity.0),
                unsupported,
            );
            return Ok(None);
        }
        let Some(opacity) = opacity_factor(node.opacity) else {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::OpacityOutOfRange(node.opacity.0),
                unsupported,
            );
            return Ok(None);
        };
        if node.mask_mipmap_id.is_some() && !options.allow_masks {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::Mask,
                unsupported,
            );
            return Ok(None);
        }

        let (mask_key, opacity) = apply_planned_gpu_mask(
            plan_gpu_mask_resource(&self.mask_sources, node, self.summary.canvas, resource_plan)?,
            opacity,
        );
        let children = self.collect_gpu_sources_in_range(
            index + 1,
            subtree_end,
            node.depth + 1,
            options,
            unsupported,
            resource_plan,
        )?;
        if children.is_empty() {
            return Ok(None);
        }
        Ok(Some(clip_gpu::GpuNormalStackSource::ThroughGroup {
            children,
            opacity,
            mask_key,
        }))
    }

    fn plan_gpu_raster_source(
        &self,
        node: &clip_graph::RenderNode,
        options: StrictRasterStackOptions,
        allow_clip_flag: bool,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
        resource_plan: &mut GpuResourcePlan,
    ) -> Result<Option<clip_gpu::GpuNormalRasterSource>, RuntimeError> {
        if node.clip && !allow_clip_flag {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Clipping,
            });
            return Ok(None);
        }
        let Some(blend_mode) = strict_raster_blend_mode(node, options, allow_clip_flag) else {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Composite(node.composite),
            });
            return Ok(None);
        };
        if !options.allow_layer_opacity && node.opacity != LayerOpacity::MAX {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Opacity(node.opacity.0),
            });
            return Ok(None);
        }
        let Some(opacity) = opacity_factor(node.opacity) else {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::OpacityOutOfRange(node.opacity.0),
            });
            return Ok(None);
        };
        if node.mask_mipmap_id.is_some() && !options.allow_masks {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Mask,
            });
            return Ok(None);
        }

        let render_mipmap_id =
            node.render_mipmap_id
                .ok_or(RuntimeError::MissingRasterRenderMipmap {
                    layer_id: node.layer_id,
                })?;
        let source = self
            .raster_sources
            .get(&node.layer_id)
            .cloned()
            .ok_or(clip_file::ClipFileError::MissingLayer(node.layer_id))?;
        let key = clip_gpu::GpuRasterResourceKey {
            layer_id: node.layer_id,
            render_mipmap_id,
        };
        resource_plan.insert_raster(
            key,
            node.id,
            node.layer_id,
            render_mipmap_id,
            source.clone(),
        );
        let (mask_key, opacity) = apply_planned_gpu_mask(
            plan_gpu_mask_resource(&self.mask_sources, node, self.summary.canvas, resource_plan)?,
            opacity,
        );
        Ok(Some(clip_gpu::GpuNormalRasterSource {
            key,
            opacity,
            mask_key,
            offset_x: source.offset_x,
            offset_y: source.offset_y,
            blend_mode: gpu_raster_blend_mode(blend_mode),
        }))
    }

    fn plan_gpu_lut_filter_source(
        &self,
        node: &clip_graph::RenderNode,
        options: StrictRasterStackOptions,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
        resource_plan: &mut GpuResourcePlan,
    ) -> Result<Option<clip_gpu::GpuNormalStackSource>, RuntimeError> {
        if !options.allow_lut_filters {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Filter,
            });
            return Ok(None);
        }
        if node.clip {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Clipping,
            });
            return Ok(None);
        }
        if node.composite != 0 {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Composite(node.composite),
            });
            return Ok(None);
        }
        let Some(opacity) = opacity_factor(node.opacity) else {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::OpacityOutOfRange(node.opacity.0),
            });
            return Ok(None);
        };
        if node.mask_mipmap_id.is_some() && !options.allow_masks {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Mask,
            });
            return Ok(None);
        }

        let filter = self.filter_sources.get(&node.layer_id).ok_or(
            clip_file::ClipFileError::LayerHasNoFilterInfo {
                layer_id: node.layer_id,
            },
        )?;
        let Some((_name, mode, lut_rgba)) = lut_filter_rgba(filter.filter_type, &filter.payload)
        else {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Filter,
            });
            return Ok(None);
        };
        let (mask_key, opacity) = apply_planned_gpu_mask(
            plan_gpu_mask_resource(&self.mask_sources, node, self.summary.canvas, resource_plan)?,
            opacity,
        );
        Ok(Some(clip_gpu::GpuNormalStackSource::LutFilter {
            lut_rgba,
            opacity,
            mask_key,
            filter_mode: gpu_lut_filter_mode(mode),
        }))
    }

    fn collect_gpu_clipped_siblings(
        &self,
        mut index: usize,
        base_depth: u16,
        end: usize,
        options: StrictRasterStackOptions,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
        resource_plan: &mut GpuResourcePlan,
    ) -> Result<(Vec<clip_gpu::GpuNormalRasterSource>, usize), RuntimeError> {
        let mut clipped = Vec::new();
        while index < end {
            let node = &self.render_plan.nodes[index];
            if node.depth != base_depth || !node.clip {
                break;
            }
            let subtree_end = self.subtree_end(index).min(end);
            match node.kind {
                RenderNodeKind::Raster => {
                    if let Some(raster) = self.plan_gpu_raster_source(
                        node,
                        options,
                        true,
                        unsupported,
                        resource_plan,
                    )? {
                        clipped.push(raster);
                    }
                    index += 1;
                }
                RenderNodeKind::Container => {
                    self.push_unsupported_subtree(
                        index,
                        subtree_end,
                        SimpleRasterStackUnsupportedReason::ContainerSemantics,
                        unsupported,
                    );
                    index = subtree_end;
                }
                RenderNodeKind::Paper => {
                    unsupported.push(SimpleRasterStackUnsupported {
                        render_node_id: node.id,
                        layer_id: node.layer_id,
                        kind: node.kind,
                        reason: SimpleRasterStackUnsupportedReason::PaperSemantics,
                    });
                    index += 1;
                }
                RenderNodeKind::Filter => {
                    unsupported.push(SimpleRasterStackUnsupported {
                        render_node_id: node.id,
                        layer_id: node.layer_id,
                        kind: node.kind,
                        reason: SimpleRasterStackUnsupportedReason::Filter,
                    });
                    index += 1;
                }
                RenderNodeKind::Unsupported(raw_kind) => {
                    unsupported.push(SimpleRasterStackUnsupported {
                        render_node_id: node.id,
                        layer_id: node.layer_id,
                        kind: node.kind,
                        reason: SimpleRasterStackUnsupportedReason::UnsupportedLayerKind(raw_kind),
                    });
                    index += 1;
                }
            }
        }
        Ok((clipped, index))
    }

    fn collect_strict_container_draw(
        &self,
        index: usize,
        subtree_end: usize,
        options: StrictRasterStackOptions,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
    ) -> Result<Option<PlannedContainerStack>, RuntimeError> {
        let node = &self.render_plan.nodes[index];
        if !options.allow_container_isolation {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::ContainerSemantics,
                unsupported,
            );
            return Ok(None);
        }
        if node.clip {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::ContainerSemantics,
                unsupported,
            );
            return Ok(None);
        }
        let Some(blend_mode) = strict_raster_blend_mode(node, options, false) else {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::Composite(node.composite),
                unsupported,
            );
            return Ok(None);
        };
        if !options.allow_layer_opacity && node.opacity != LayerOpacity::MAX {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::Opacity(node.opacity.0),
                unsupported,
            );
            return Ok(None);
        }
        let Some(opacity) = opacity_factor(node.opacity) else {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::OpacityOutOfRange(node.opacity.0),
                unsupported,
            );
            return Ok(None);
        };
        if node.mask_mipmap_id.is_some() && !options.allow_masks {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::Mask,
                unsupported,
            );
            return Ok(None);
        }

        let mask = if let Some(mask_mipmap_id) = node.mask_mipmap_id {
            let image = clip_file::read_layer_mask_alpha(&self.path, node.layer_id)?;
            if image.width != self.summary.canvas.width
                || image.height != self.summary.canvas.height
            {
                self.push_unsupported_subtree(
                    index,
                    subtree_end,
                    SimpleRasterStackUnsupportedReason::MaskSize {
                        width: image.width,
                        height: image.height,
                    },
                    unsupported,
                );
                return Ok(None);
            }
            Some(PlannedDecodedMask {
                mask_mipmap_id,
                image,
            })
        } else {
            None
        };

        let draws = self.collect_strict_draws_in_range(
            index + 1,
            subtree_end,
            node.depth + 1,
            options,
            unsupported,
        )?;
        if draws.is_empty() {
            return Ok(None);
        }
        Ok(Some(PlannedContainerStack {
            render_node_id: node.id,
            layer_id: node.layer_id,
            opacity,
            mask,
            blend_mode,
            draws,
        }))
    }

    fn collect_strict_through_group_draw(
        &self,
        index: usize,
        subtree_end: usize,
        options: StrictRasterStackOptions,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
    ) -> Result<Option<PlannedThroughGroup>, RuntimeError> {
        let node = &self.render_plan.nodes[index];
        if !options.allow_through_groups {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::ContainerSemantics,
                unsupported,
            );
            return Ok(None);
        }
        if node.clip || node.composite != LAYER_COMPOSITE_THROUGH {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::ContainerSemantics,
                unsupported,
            );
            return Ok(None);
        }
        if !options.allow_layer_opacity && node.opacity != LayerOpacity::MAX {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::Opacity(node.opacity.0),
                unsupported,
            );
            return Ok(None);
        }
        let Some(opacity) = opacity_factor(node.opacity) else {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::OpacityOutOfRange(node.opacity.0),
                unsupported,
            );
            return Ok(None);
        };
        if node.mask_mipmap_id.is_some() && !options.allow_masks {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::Mask,
                unsupported,
            );
            return Ok(None);
        }

        let mask = if let Some(mask_mipmap_id) = node.mask_mipmap_id {
            let image = clip_file::read_layer_mask_alpha(&self.path, node.layer_id)?;
            if image.width != self.summary.canvas.width
                || image.height != self.summary.canvas.height
            {
                self.push_unsupported_subtree(
                    index,
                    subtree_end,
                    SimpleRasterStackUnsupportedReason::MaskSize {
                        width: image.width,
                        height: image.height,
                    },
                    unsupported,
                );
                return Ok(None);
            }
            Some(PlannedDecodedMask {
                mask_mipmap_id,
                image,
            })
        } else {
            None
        };

        let draws = self.collect_strict_draws_in_range(
            index + 1,
            subtree_end,
            node.depth + 1,
            options,
            unsupported,
        )?;
        if draws.is_empty() {
            return Ok(None);
        }
        Ok(Some(PlannedThroughGroup {
            render_node_id: node.id,
            layer_id: node.layer_id,
            opacity,
            mask,
            draws,
        }))
    }

    fn push_unsupported_subtree(
        &self,
        index: usize,
        subtree_end: usize,
        reason: SimpleRasterStackUnsupportedReason,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
    ) {
        let node = &self.render_plan.nodes[index];
        unsupported.push(SimpleRasterStackUnsupported {
            render_node_id: node.id,
            layer_id: node.layer_id,
            kind: node.kind,
            reason,
        });
        for child in &self.render_plan.nodes[index + 1..subtree_end] {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: child.id,
                layer_id: child.layer_id,
                kind: child.kind,
                reason: SimpleRasterStackUnsupportedReason::InsideUnsupportedContainer,
            });
        }
    }

    fn subtree_end(&self, index: usize) -> usize {
        let depth = self.render_plan.nodes[index].depth;
        let mut end = index + 1;
        while end < self.render_plan.nodes.len() && self.render_plan.nodes[end].depth > depth {
            end += 1;
        }
        end
    }

    fn decode_strict_normal_raster_node(
        &self,
        node: &clip_graph::RenderNode,
        options: StrictRasterStackOptions,
        allow_clip_flag: bool,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
    ) -> Result<Option<PlannedDecodedRaster>, RuntimeError> {
        if node.clip && !allow_clip_flag {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Clipping,
            });
            return Ok(None);
        }
        let Some(blend_mode) = strict_raster_blend_mode(node, options, allow_clip_flag) else {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Composite(node.composite),
            });
            return Ok(None);
        };
        if !options.allow_layer_opacity && node.opacity != LayerOpacity::MAX {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Opacity(node.opacity.0),
            });
            return Ok(None);
        }
        let Some(opacity) = opacity_factor(node.opacity) else {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::OpacityOutOfRange(node.opacity.0),
            });
            return Ok(None);
        };
        if node.mask_mipmap_id.is_some() && !options.allow_masks {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Mask,
            });
            return Ok(None);
        }

        let render_mipmap_id =
            node.render_mipmap_id
                .ok_or(RuntimeError::MissingRasterRenderMipmap {
                    layer_id: node.layer_id,
                })?;
        let placed = clip_file::read_raster_layer_source_rgba(&self.path, node.layer_id)?;
        let mask = if let Some(mask_mipmap_id) = node.mask_mipmap_id {
            let image = clip_file::read_layer_mask_alpha(&self.path, node.layer_id)?;
            if image.width != self.summary.canvas.width
                || image.height != self.summary.canvas.height
            {
                unsupported.push(SimpleRasterStackUnsupported {
                    render_node_id: node.id,
                    layer_id: node.layer_id,
                    kind: node.kind,
                    reason: SimpleRasterStackUnsupportedReason::MaskSize {
                        width: image.width,
                        height: image.height,
                    },
                });
                return Ok(None);
            }
            Some(PlannedDecodedMask {
                mask_mipmap_id,
                image,
            })
        } else {
            None
        };

        Ok(Some(PlannedDecodedRaster {
            render_node_id: node.id,
            layer_id: node.layer_id,
            render_mipmap_id,
            image: placed.image,
            offset_x: placed.offset_x,
            offset_y: placed.offset_y,
            opacity,
            mask,
            blend_mode,
        }))
    }

    fn decode_strict_lut_filter_node(
        &self,
        node: &clip_graph::RenderNode,
        options: StrictRasterStackOptions,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
    ) -> Result<Option<PlannedLutFilter>, RuntimeError> {
        if !options.allow_lut_filters {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Filter,
            });
            return Ok(None);
        }
        if node.clip {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Clipping,
            });
            return Ok(None);
        }
        if node.composite != 0 {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Composite(node.composite),
            });
            return Ok(None);
        }
        let Some(opacity) = opacity_factor(node.opacity) else {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::OpacityOutOfRange(node.opacity.0),
            });
            return Ok(None);
        };
        if node.mask_mipmap_id.is_some() && !options.allow_masks {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Mask,
            });
            return Ok(None);
        }
        let filter = self.filter_sources.get(&node.layer_id).ok_or(
            clip_file::ClipFileError::LayerHasNoFilterInfo {
                layer_id: node.layer_id,
            },
        )?;
        let Some((name, mode, lut_rgba)) = lut_filter_rgba(filter.filter_type, &filter.payload)
        else {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Filter,
            });
            return Ok(None);
        };
        let mask = if let Some(mask_mipmap_id) = node.mask_mipmap_id {
            let image = clip_file::read_layer_mask_alpha(&self.path, node.layer_id)?;
            if image.width != self.summary.canvas.width
                || image.height != self.summary.canvas.height
            {
                unsupported.push(SimpleRasterStackUnsupported {
                    render_node_id: node.id,
                    layer_id: node.layer_id,
                    kind: node.kind,
                    reason: SimpleRasterStackUnsupportedReason::MaskSize {
                        width: image.width,
                        height: image.height,
                    },
                });
                return Ok(None);
            }
            Some(PlannedDecodedMask {
                mask_mipmap_id,
                image,
            })
        } else {
            None
        };
        Ok(Some(PlannedLutFilter {
            render_node_id: node.id,
            layer_id: node.layer_id,
            name,
            mode,
            opacity,
            mask,
            lut_rgba,
        }))
    }

    fn collect_strict_clipped_siblings(
        &self,
        mut index: usize,
        base_depth: u16,
        end: usize,
        options: StrictRasterStackOptions,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
    ) -> Result<(Vec<PlannedDecodedRaster>, usize), RuntimeError> {
        let mut clipped = Vec::new();
        while index < end {
            let node = &self.render_plan.nodes[index];
            if node.depth != base_depth || !node.clip {
                break;
            }
            let subtree_end = self.subtree_end(index).min(end);
            match node.kind {
                RenderNodeKind::Raster => {
                    if let Some(decoded) =
                        self.decode_strict_normal_raster_node(node, options, true, unsupported)?
                    {
                        clipped.push(decoded);
                    }
                    index += 1;
                }
                RenderNodeKind::Container => {
                    self.push_unsupported_subtree(
                        index,
                        subtree_end,
                        SimpleRasterStackUnsupportedReason::ContainerSemantics,
                        unsupported,
                    );
                    index = subtree_end;
                }
                RenderNodeKind::Paper => {
                    unsupported.push(SimpleRasterStackUnsupported {
                        render_node_id: node.id,
                        layer_id: node.layer_id,
                        kind: node.kind,
                        reason: SimpleRasterStackUnsupportedReason::PaperSemantics,
                    });
                    index += 1;
                }
                RenderNodeKind::Filter => {
                    unsupported.push(SimpleRasterStackUnsupported {
                        render_node_id: node.id,
                        layer_id: node.layer_id,
                        kind: node.kind,
                        reason: SimpleRasterStackUnsupportedReason::Filter,
                    });
                    index += 1;
                }
                RenderNodeKind::Unsupported(raw_kind) => {
                    unsupported.push(SimpleRasterStackUnsupported {
                        render_node_id: node.id,
                        layer_id: node.layer_id,
                        kind: node.kind,
                        reason: SimpleRasterStackUnsupportedReason::UnsupportedLayerKind(raw_kind),
                    });
                    index += 1;
                }
            }
        }
        Ok((clipped, index))
    }

    pub fn read_rgba8_region(&mut self, region: Rect, out: &mut [u8]) -> Result<(), RuntimeError> {
        let x_end = region
            .x
            .checked_add(region.width)
            .ok_or(RuntimeError::InvalidRegion)?;
        let y_end = region
            .y
            .checked_add(region.height)
            .ok_or(RuntimeError::InvalidRegion)?;
        if x_end > self.summary.canvas.width || y_end > self.summary.canvas.height {
            return Err(RuntimeError::InvalidRegion);
        }

        let expected = u64::from(region.width)
            .checked_mul(u64::from(region.height))
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .ok_or(RuntimeError::InvalidRegion)?;
        if out.len() < expected {
            return Err(RuntimeError::OutputBufferTooSmall {
                expected,
                actual: out.len(),
            });
        }

        let image = self.rendered_image()?;
        let width = usize::try_from(region.width).map_err(|_| RuntimeError::InvalidRegion)?;
        let height = usize::try_from(region.height).map_err(|_| RuntimeError::InvalidRegion)?;
        let image_width = usize::try_from(image.width).map_err(|_| RuntimeError::InvalidRegion)?;
        let x = usize::try_from(region.x).map_err(|_| RuntimeError::InvalidRegion)?;
        let base_y = usize::try_from(region.y).map_err(|_| RuntimeError::InvalidRegion)?;
        for row in 0..height {
            let src_start = ((base_y + row) * image_width + x) * 4;
            let src_end = src_start + width * 4;
            let dst_start = row * width * 4;
            let dst_end = dst_start + width * 4;
            out[dst_start..dst_end].copy_from_slice(&image.pixels[src_start..src_end]);
        }
        Ok(())
    }

    fn rendered_image(&mut self) -> Result<&clip_file::tiles::RgbaTileImage, RuntimeError> {
        if self.rendered_image.is_none() {
            let result = self.draw_normal_raster_stack_via_gpu()?;
            if !result.unsupported.is_empty() {
                return Err(RuntimeError::UnsupportedRenderPlan {
                    unsupported: result.unsupported,
                });
            }
            let image = result.image.ok_or(RuntimeError::EmptyRenderPlan)?;
            self.rendered_image = Some(image);
        }
        Ok(self
            .rendered_image
            .as_ref()
            .expect("rendered image was populated"))
    }
}

#[derive(Debug)]
pub struct SimpleRasterStackGpuResult {
    pub image: Option<clip_file::tiles::RgbaTileImage>,
    pub drawn_resources: Vec<clip_gpu::GpuRasterResourceInfo>,
    pub unsupported: Vec<SimpleRasterStackUnsupported>,
    pub differing_bytes_from_last_drawn: Option<usize>,
}

#[derive(Debug)]
pub struct NormalRasterStackGpuResult {
    pub image: Option<clip_file::tiles::RgbaTileImage>,
    pub source_count: usize,
    pub drawn_resources: Vec<clip_gpu::GpuRasterResourceInfo>,
    pub mask_resources: Vec<clip_gpu::GpuMaskResourceInfo>,
    pub unsupported: Vec<SimpleRasterStackUnsupported>,
}

#[derive(Debug)]
pub struct NormalRasterStackSupportResult {
    pub source_count: usize,
    pub resource_stats: NormalRasterStackResourceStats,
    pub unsupported: Vec<SimpleRasterStackUnsupported>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NormalRasterStackResourceStats {
    pub raster_count: usize,
    pub raster_bytes: u64,
    pub max_raster_layer_id: Option<LayerId>,
    pub max_raster_width: u32,
    pub max_raster_height: u32,
    pub max_raster_bytes: u64,
    pub mask_count: usize,
    pub mask_bytes: u64,
    pub max_mask_layer_id: Option<LayerId>,
    pub max_mask_width: u32,
    pub max_mask_height: u32,
    pub max_mask_bytes: u64,
}

#[derive(Debug)]
pub struct NormalRasterStackPixelTraceResult {
    pub source_count: usize,
    pub samples: Vec<NormalRasterStackPixelTraceSample>,
    pub unsupported: Vec<SimpleRasterStackUnsupported>,
}

#[derive(Debug)]
pub struct NormalRasterStackPixelTraceSample {
    pub source_index: usize,
    pub source: String,
    pub before_rgba: Option<Rgba8>,
    pub rgba: Rgba8,
    pub inputs: Vec<NormalRasterStackPixelTraceInput>,
}

#[derive(Debug)]
pub struct NormalRasterStackPixelTraceInput {
    pub role: String,
    pub render_node_id: Option<u32>,
    pub layer_id: Option<u32>,
    pub blend_mode: Option<String>,
    pub opacity: Option<f32>,
    pub rgba: Option<Rgba8>,
    pub mask_alpha: Option<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimpleRasterStackUnsupported {
    pub render_node_id: RenderNodeId,
    pub layer_id: LayerId,
    pub kind: RenderNodeKind,
    pub reason: SimpleRasterStackUnsupportedReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SimpleRasterStackUnsupportedReason {
    Paper,
    Clipping,
    Composite(u32),
    Opacity(u16),
    OpacityOutOfRange(u16),
    Mask,
    MaskSize { width: u32, height: u32 },
    NonCanvasSizedRaster { width: u32, height: u32 },
    RasterColorType(Option<u32>),
    RequiresAlphaCompositing,
    PaperSemantics,
    PaperColorMissing,
    ContainerSemantics,
    InsideUnsupportedContainer,
    Filter,
    UnsupportedLayerKind(u32),
}

impl fmt::Display for SimpleRasterStackUnsupportedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Paper => f.write_str("paper fill is not in the strict raster stack pass"),
            Self::Clipping => f.write_str("clipping requires clip-base compositing"),
            Self::Composite(composite) => {
                write!(f, "LayerComposite {composite} is not direct copy")
            }
            Self::Opacity(opacity) => write!(f, "LayerOpacity {opacity} requires opacity handling"),
            Self::OpacityOutOfRange(opacity) => {
                write!(
                    f,
                    "LayerOpacity {opacity} is outside the supported 0..256 range"
                )
            }
            Self::Mask => f.write_str("layer mask requires mask sampling"),
            Self::MaskSize { width, height } => {
                write!(f, "mask size {width}x{height} does not match the canvas")
            }
            Self::NonCanvasSizedRaster { width, height } => write!(
                f,
                "raster size {width}x{height} requires placement metadata",
            ),
            Self::RasterColorType(color_type) => {
                write!(f, "raster colour type {color_type:?} is not supported")
            }
            Self::RequiresAlphaCompositing => {
                f.write_str("stacked non-opaque raster requires alpha compositing")
            }
            Self::PaperSemantics => {
                f.write_str("paper layer has unsupported clip, mask, or composite semantics")
            }
            Self::PaperColorMissing => f.write_str("paper layer has no decoded paper colour"),
            Self::ContainerSemantics => {
                f.write_str("container requires folder compositing semantics")
            }
            Self::InsideUnsupportedContainer => {
                f.write_str("node is inside an unsupported container")
            }
            Self::Filter => f.write_str("filter layer is not in the strict raster stack pass"),
            Self::UnsupportedLayerKind(kind) => write!(f, "unsupported layer kind {kind}"),
        }
    }
}

#[derive(Debug)]
pub struct DrawRasterLayerGpuResult {
    pub image: clip_file::tiles::RgbaTileImage,
    pub resource_info: clip_gpu::GpuRasterResourceInfo,
    pub differing_bytes: usize,
}

#[derive(Debug)]
struct PlannedDecodedRaster {
    render_node_id: RenderNodeId,
    layer_id: LayerId,
    render_mipmap_id: u32,
    image: clip_file::tiles::RgbaTileImage,
    offset_x: i32,
    offset_y: i32,
    opacity: f32,
    mask: Option<PlannedDecodedMask>,
    blend_mode: StrictRasterBlendMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StrictRasterBlendMode {
    Normal,
    Add,
    AddGlow,
    ColorBurn,
    ColorDodge,
    Darken,
    DarkerColor,
    Difference,
    Divide,
    Exclusion,
    GlowDodge,
    HardMix,
    HardLight,
    Hue,
    Lighten,
    LighterColor,
    LinearBurn,
    LinearLight,
    Multiply,
    Overlay,
    PinLight,
    Saturation,
    Brightness,
    Color,
    Screen,
    SoftLight,
    Subtract,
    VividLight,
}

#[derive(Debug)]
struct PlannedDecodedMask {
    mask_mipmap_id: u32,
    image: clip_file::tiles::AlphaTileImage,
}

#[derive(Debug)]
struct PlannedClippingRun {
    base: PlannedDecodedRaster,
    clipped: Vec<PlannedDecodedRaster>,
}

#[derive(Debug)]
struct PlannedContainerStack {
    render_node_id: RenderNodeId,
    layer_id: LayerId,
    opacity: f32,
    mask: Option<PlannedDecodedMask>,
    blend_mode: StrictRasterBlendMode,
    draws: Vec<StrictRasterStackDraw>,
}

#[derive(Debug)]
struct PlannedThroughGroup {
    render_node_id: RenderNodeId,
    layer_id: LayerId,
    opacity: f32,
    mask: Option<PlannedDecodedMask>,
    draws: Vec<StrictRasterStackDraw>,
}

#[derive(Debug)]
struct PlannedLutFilter {
    render_node_id: RenderNodeId,
    layer_id: LayerId,
    name: &'static str,
    mode: PlannedLutFilterMode,
    opacity: f32,
    mask: Option<PlannedDecodedMask>,
    lut_rgba: Vec<u8>,
}

#[derive(Debug)]
struct StrictRasterStackSelection {
    draws: Vec<StrictRasterStackDraw>,
    unsupported: Vec<SimpleRasterStackUnsupported>,
}

#[derive(Debug)]
struct GpuRenderStackSelection {
    sources: Vec<clip_gpu::GpuNormalStackSource>,
    resource_plan: GpuResourcePlan,
    unsupported: Vec<SimpleRasterStackUnsupported>,
}

#[derive(Debug)]
enum StrictRasterStackDraw {
    Paper { color: Rgba8, opacity: f32 },
    Raster(PlannedDecodedRaster),
    ClippingRun(PlannedClippingRun),
    Container(PlannedContainerStack),
    ThroughGroup(PlannedThroughGroup),
    LutFilter(PlannedLutFilter),
}

impl StrictRasterStackDraw {
    fn as_raster(&self) -> Option<&PlannedDecodedRaster> {
        match self {
            Self::Raster(decoded) => Some(decoded),
            Self::Paper { .. }
            | Self::ClippingRun(_)
            | Self::Container(_)
            | Self::ThroughGroup(_)
            | Self::LutFilter(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct StrictRasterStackOptions {
    allow_alpha_compositing: bool,
    allow_paper: bool,
    allow_layer_opacity: bool,
    allow_masks: bool,
    allow_clipping_runs: bool,
    allow_container_isolation: bool,
    allow_through_groups: bool,
    allow_add_blend: bool,
    allow_add_glow_blend: bool,
    allow_color_burn_blend: bool,
    allow_color_dodge_blend: bool,
    allow_extended_blends: bool,
    allow_glow_dodge_blend: bool,
    allow_hard_mix_blend: bool,
    allow_hsl_blends: bool,
    allow_simple_blends: bool,
    allow_soft_light_blend: bool,
    allow_lut_filters: bool,
    allow_vivid_light_blend: bool,
    allow_w3c_blends: bool,
    allow_initial_terminal_container_elision: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClipBaseState {
    Available,
    Cleared,
    Blocked,
}

fn opacity_factor(opacity: LayerOpacity) -> Option<f32> {
    if opacity.0 <= LayerOpacity::MAX.0 {
        Some(f32::from(opacity.0) / f32::from(LayerOpacity::MAX.0))
    } else {
        None
    }
}

fn apply_planned_gpu_mask(
    mask: PlannedGpuMaskResource,
    opacity: f32,
) -> (Option<clip_gpu::GpuMaskResourceKey>, f32) {
    match mask {
        PlannedGpuMaskResource::None | PlannedGpuMaskResource::FullyOpaque => (None, opacity),
        PlannedGpuMaskResource::Key(key) => (Some(key), opacity),
        PlannedGpuMaskResource::FullyTransparent => (None, 0.0),
    }
}

fn strict_raster_blend_mode(
    node: &clip_graph::RenderNode,
    options: StrictRasterStackOptions,
    allow_clip_flag: bool,
) -> Option<StrictRasterBlendMode> {
    match node.composite {
        0 => Some(StrictRasterBlendMode::Normal),
        LAYER_COMPOSITE_DARKEN
            if options.allow_simple_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Darken,
                ) =>
        {
            Some(StrictRasterBlendMode::Darken)
        }
        LAYER_COMPOSITE_MULTIPLY
            if options.allow_w3c_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Multiply,
                ) =>
        {
            Some(StrictRasterBlendMode::Multiply)
        }
        LAYER_COMPOSITE_LINEAR_BURN
            if options.allow_extended_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::LinearBurn,
                ) =>
        {
            Some(StrictRasterBlendMode::LinearBurn)
        }
        LAYER_COMPOSITE_ADD_GLOW
            if options.allow_add_glow_blend
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::AddGlow,
                ) =>
        {
            Some(StrictRasterBlendMode::AddGlow)
        }
        LAYER_COMPOSITE_DARKER_COLOR
            if options.allow_extended_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::DarkerColor,
                ) =>
        {
            Some(StrictRasterBlendMode::DarkerColor)
        }
        LAYER_COMPOSITE_SUBTRACT
            if options.allow_simple_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Subtract,
                ) =>
        {
            Some(StrictRasterBlendMode::Subtract)
        }
        LAYER_COMPOSITE_LIGHTEN
            if options.allow_simple_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Lighten,
                ) =>
        {
            Some(StrictRasterBlendMode::Lighten)
        }
        LAYER_COMPOSITE_SCREEN
            if options.allow_simple_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Screen,
                ) =>
        {
            Some(StrictRasterBlendMode::Screen)
        }
        LAYER_COMPOSITE_COLOR_BURN
            if options.allow_color_burn_blend
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::ColorBurn,
                ) =>
        {
            Some(StrictRasterBlendMode::ColorBurn)
        }
        LAYER_COMPOSITE_COLOR_DODGE
            if options.allow_color_dodge_blend
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::ColorDodge,
                ) =>
        {
            Some(StrictRasterBlendMode::ColorDodge)
        }
        LAYER_COMPOSITE_GLOW_DODGE
            if options.allow_glow_dodge_blend
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::GlowDodge,
                ) =>
        {
            Some(StrictRasterBlendMode::GlowDodge)
        }
        LAYER_COMPOSITE_ADD
            if options.allow_add_blend
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Add,
                ) =>
        {
            Some(StrictRasterBlendMode::Add)
        }
        LAYER_COMPOSITE_LIGHTER_COLOR
            if options.allow_extended_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::LighterColor,
                ) =>
        {
            Some(StrictRasterBlendMode::LighterColor)
        }
        LAYER_COMPOSITE_OVERLAY
            if options.allow_w3c_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Overlay,
                ) =>
        {
            Some(StrictRasterBlendMode::Overlay)
        }
        LAYER_COMPOSITE_HARD_MIX
            if options.allow_hard_mix_blend
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::HardMix,
                ) =>
        {
            Some(StrictRasterBlendMode::HardMix)
        }
        LAYER_COMPOSITE_HARD_LIGHT
            if options.allow_w3c_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::HardLight,
                ) =>
        {
            Some(StrictRasterBlendMode::HardLight)
        }
        LAYER_COMPOSITE_LINEAR_LIGHT
            if options.allow_extended_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::LinearLight,
                ) =>
        {
            Some(StrictRasterBlendMode::LinearLight)
        }
        LAYER_COMPOSITE_PIN_LIGHT
            if options.allow_extended_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::PinLight,
                ) =>
        {
            Some(StrictRasterBlendMode::PinLight)
        }
        LAYER_COMPOSITE_HUE
            if options.allow_hsl_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Hue,
                ) =>
        {
            Some(StrictRasterBlendMode::Hue)
        }
        LAYER_COMPOSITE_SATURATION
            if options.allow_hsl_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Saturation,
                ) =>
        {
            Some(StrictRasterBlendMode::Saturation)
        }
        LAYER_COMPOSITE_COLOR
            if options.allow_hsl_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Color,
                ) =>
        {
            Some(StrictRasterBlendMode::Color)
        }
        LAYER_COMPOSITE_SOFT_LIGHT
            if options.allow_soft_light_blend
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::SoftLight,
                ) =>
        {
            Some(StrictRasterBlendMode::SoftLight)
        }
        LAYER_COMPOSITE_DIFFERENCE
            if options.allow_simple_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Difference,
                ) =>
        {
            Some(StrictRasterBlendMode::Difference)
        }
        LAYER_COMPOSITE_EXCLUSION
            if options.allow_extended_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Exclusion,
                ) =>
        {
            Some(StrictRasterBlendMode::Exclusion)
        }
        LAYER_COMPOSITE_BRIGHTNESS
            if options.allow_extended_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Brightness,
                ) =>
        {
            Some(StrictRasterBlendMode::Brightness)
        }
        LAYER_COMPOSITE_VIVID_LIGHT
            if options.allow_vivid_light_blend
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::VividLight,
                ) =>
        {
            Some(StrictRasterBlendMode::VividLight)
        }
        LAYER_COMPOSITE_DIVIDE
            if options.allow_extended_blends
                && raster_blend_allowed_at_position(
                    node,
                    allow_clip_flag,
                    StrictRasterBlendMode::Divide,
                ) =>
        {
            Some(StrictRasterBlendMode::Divide)
        }
        _ => None,
    }
}

fn raster_blend_allowed_at_position(
    node: &clip_graph::RenderNode,
    allow_clip_flag: bool,
    blend_mode: StrictRasterBlendMode,
) -> bool {
    if !node.clip && !allow_clip_flag {
        return true;
    }
    node.clip && allow_clip_flag && clipped_blend_supported(blend_mode)
}

fn clipped_blend_supported(blend_mode: StrictRasterBlendMode) -> bool {
    matches!(
        blend_mode,
        StrictRasterBlendMode::Normal
            | StrictRasterBlendMode::Add
            | StrictRasterBlendMode::AddGlow
            | StrictRasterBlendMode::ColorBurn
            | StrictRasterBlendMode::ColorDodge
            | StrictRasterBlendMode::Darken
            | StrictRasterBlendMode::DarkerColor
            | StrictRasterBlendMode::Difference
            | StrictRasterBlendMode::Divide
            | StrictRasterBlendMode::Exclusion
            | StrictRasterBlendMode::GlowDodge
            | StrictRasterBlendMode::HardMix
            | StrictRasterBlendMode::HardLight
            | StrictRasterBlendMode::Hue
            | StrictRasterBlendMode::Lighten
            | StrictRasterBlendMode::LighterColor
            | StrictRasterBlendMode::LinearBurn
            | StrictRasterBlendMode::LinearLight
            | StrictRasterBlendMode::Multiply
            | StrictRasterBlendMode::Overlay
            | StrictRasterBlendMode::PinLight
            | StrictRasterBlendMode::Saturation
            | StrictRasterBlendMode::Brightness
            | StrictRasterBlendMode::Color
            | StrictRasterBlendMode::SoftLight
            | StrictRasterBlendMode::Screen
            | StrictRasterBlendMode::Subtract
            | StrictRasterBlendMode::VividLight
    )
}

fn alpha_is_fully_opaque(pixels: &[u8]) -> bool {
    pixels.chunks_exact(4).all(|pixel| pixel[3] == 255)
}

fn can_elide_initial_terminal_container(
    options: StrictRasterStackOptions,
    node: &clip_graph::RenderNode,
    subtree_end: usize,
    range_end: usize,
    has_drawn_output: bool,
) -> bool {
    options.allow_initial_terminal_container_elision
        && !has_drawn_output
        && subtree_end == range_end
        && !node.clip
        && node.composite == 0
        && node.opacity == LayerOpacity::MAX
        && node.mask_mipmap_id.is_none()
}

fn decoded_rasters_in_draws(draws: &[StrictRasterStackDraw]) -> Vec<&PlannedDecodedRaster> {
    let mut rasters = Vec::new();
    for draw in draws {
        match draw {
            StrictRasterStackDraw::Raster(decoded) => rasters.push(decoded),
            StrictRasterStackDraw::ClippingRun(run) => {
                rasters.push(&run.base);
                rasters.extend(run.clipped.iter());
            }
            StrictRasterStackDraw::Container(container) => {
                rasters.extend(decoded_rasters_in_draws(&container.draws));
            }
            StrictRasterStackDraw::ThroughGroup(through_group) => {
                rasters.extend(decoded_rasters_in_draws(&through_group.draws));
            }
            StrictRasterStackDraw::Paper { .. } | StrictRasterStackDraw::LutFilter(_) => {}
        }
    }
    rasters
}

fn decoded_containers_in_draws(draws: &[StrictRasterStackDraw]) -> Vec<&PlannedContainerStack> {
    let mut containers = Vec::new();
    for draw in draws {
        match draw {
            StrictRasterStackDraw::Container(container) => {
                containers.push(container);
                containers.extend(decoded_containers_in_draws(&container.draws));
            }
            StrictRasterStackDraw::ThroughGroup(through_group) => {
                containers.extend(decoded_containers_in_draws(&through_group.draws));
            }
            StrictRasterStackDraw::Paper { .. }
            | StrictRasterStackDraw::Raster(_)
            | StrictRasterStackDraw::ClippingRun(_)
            | StrictRasterStackDraw::LutFilter(_) => {}
        }
    }
    containers
}

fn decoded_through_groups_in_draws(draws: &[StrictRasterStackDraw]) -> Vec<&PlannedThroughGroup> {
    let mut through_groups = Vec::new();
    for draw in draws {
        match draw {
            StrictRasterStackDraw::ThroughGroup(through_group) => {
                through_groups.push(through_group);
                through_groups.extend(decoded_through_groups_in_draws(&through_group.draws));
            }
            StrictRasterStackDraw::Container(container) => {
                through_groups.extend(decoded_through_groups_in_draws(&container.draws));
            }
            StrictRasterStackDraw::Paper { .. }
            | StrictRasterStackDraw::Raster(_)
            | StrictRasterStackDraw::ClippingRun(_)
            | StrictRasterStackDraw::LutFilter(_) => {}
        }
    }
    through_groups
}

fn decoded_lut_filters_in_draws(draws: &[StrictRasterStackDraw]) -> Vec<&PlannedLutFilter> {
    let mut filters = Vec::new();
    for draw in draws {
        match draw {
            StrictRasterStackDraw::LutFilter(filter) => filters.push(filter),
            StrictRasterStackDraw::Container(container) => {
                filters.extend(decoded_lut_filters_in_draws(&container.draws));
            }
            StrictRasterStackDraw::ThroughGroup(through_group) => {
                filters.extend(decoded_lut_filters_in_draws(&through_group.draws));
            }
            StrictRasterStackDraw::Paper { .. }
            | StrictRasterStackDraw::Raster(_)
            | StrictRasterStackDraw::ClippingRun(_) => {}
        }
    }
    filters
}

fn sample_rgba8(pixels: &[u8], size: CanvasSize, x: u32, y: u32) -> Result<Rgba8, RuntimeError> {
    let width = usize::try_from(size.width).map_err(|_| RuntimeError::InvalidRegion)?;
    let x = usize::try_from(x).map_err(|_| RuntimeError::InvalidRegion)?;
    let y = usize::try_from(y).map_err(|_| RuntimeError::InvalidRegion)?;
    let pixel_offset = y
        .checked_mul(width)
        .and_then(|row| row.checked_add(x))
        .and_then(|pixel| pixel.checked_mul(4))
        .ok_or(RuntimeError::InvalidRegion)?;
    let pixel = pixels
        .get(pixel_offset..pixel_offset + 4)
        .ok_or(RuntimeError::InvalidRegion)?;
    Ok(Rgba8 {
        r: pixel[0],
        g: pixel[1],
        b: pixel[2],
        a: pixel[3],
    })
}

fn sample_alpha8(pixels: &[u8], size: CanvasSize, x: u32, y: u32) -> Result<u8, RuntimeError> {
    let width = usize::try_from(size.width).map_err(|_| RuntimeError::InvalidRegion)?;
    let x = usize::try_from(x).map_err(|_| RuntimeError::InvalidRegion)?;
    let y = usize::try_from(y).map_err(|_| RuntimeError::InvalidRegion)?;
    let pixel_offset = y
        .checked_mul(width)
        .and_then(|row| row.checked_add(x))
        .ok_or(RuntimeError::InvalidRegion)?;
    pixels
        .get(pixel_offset)
        .copied()
        .ok_or(RuntimeError::InvalidRegion)
}

fn stack_draw_trace_label(draw: &StrictRasterStackDraw) -> String {
    match draw {
        StrictRasterStackDraw::Paper { color, opacity } => {
            format!(
                "paper color=[{},{},{},{}] opacity={opacity:.6}",
                color.r, color.g, color.b, color.a
            )
        }
        StrictRasterStackDraw::Raster(decoded) => format!(
            "raster node={} layer={} blend={:?} opacity={:.6}",
            decoded.render_node_id.0, decoded.layer_id.0, decoded.blend_mode, decoded.opacity
        ),
        StrictRasterStackDraw::ClippingRun(run) => format!(
            "clipping-run base_node={} base_layer={} clipped_count={}",
            run.base.render_node_id.0,
            run.base.layer_id.0,
            run.clipped.len()
        ),
        StrictRasterStackDraw::Container(container) => format!(
            "container node={} layer={} children={} opacity={:.6}",
            container.render_node_id.0,
            container.layer_id.0,
            container.draws.len(),
            container.opacity
        ),
        StrictRasterStackDraw::ThroughGroup(through_group) => format!(
            "through-group node={} layer={} children={} opacity={:.6}",
            through_group.render_node_id.0,
            through_group.layer_id.0,
            through_group.draws.len(),
            through_group.opacity
        ),
        StrictRasterStackDraw::LutFilter(filter) => format!(
            "{} filter node={} layer={} opacity={:.6}",
            filter.name, filter.render_node_id.0, filter.layer_id.0, filter.opacity
        ),
    }
}

fn stack_draw_trace_inputs(
    draw: &StrictRasterStackDraw,
    x: u32,
    y: u32,
) -> Result<Vec<NormalRasterStackPixelTraceInput>, RuntimeError> {
    let mut inputs = Vec::new();
    push_stack_draw_trace_inputs(draw, "", x, y, &mut inputs)?;
    Ok(inputs)
}

fn push_stack_draw_trace_inputs(
    draw: &StrictRasterStackDraw,
    role_prefix: &str,
    x: u32,
    y: u32,
    inputs: &mut Vec<NormalRasterStackPixelTraceInput>,
) -> Result<(), RuntimeError> {
    match draw {
        StrictRasterStackDraw::Paper { color, opacity } => {
            inputs.push(NormalRasterStackPixelTraceInput {
                role: prefixed_trace_role(role_prefix, "paper"),
                render_node_id: None,
                layer_id: None,
                blend_mode: None,
                opacity: Some(*opacity),
                rgba: Some(*color),
                mask_alpha: None,
            });
        }
        StrictRasterStackDraw::Raster(decoded) => {
            push_raster_trace_input("raster", role_prefix, decoded, x, y, inputs)?;
        }
        StrictRasterStackDraw::ClippingRun(run) => {
            push_raster_trace_input("clipping-base", role_prefix, &run.base, x, y, inputs)?;
            for (index, clipped) in run.clipped.iter().enumerate() {
                push_raster_trace_input(
                    &format!("clipped[{index}]"),
                    role_prefix,
                    clipped,
                    x,
                    y,
                    inputs,
                )?;
            }
        }
        StrictRasterStackDraw::Container(container) => {
            inputs.push(NormalRasterStackPixelTraceInput {
                role: prefixed_trace_role(role_prefix, "container"),
                render_node_id: Some(container.render_node_id.0),
                layer_id: Some(container.layer_id.0),
                blend_mode: Some("Normal".to_string()),
                opacity: Some(container.opacity),
                rgba: None,
                mask_alpha: sample_optional_mask_alpha(container.mask.as_ref(), x, y)?,
            });
            let child_prefix = prefixed_trace_role(role_prefix, "container");
            for child in &container.draws {
                push_stack_draw_trace_inputs(child, &child_prefix, x, y, inputs)?;
            }
        }
        StrictRasterStackDraw::ThroughGroup(through_group) => {
            inputs.push(NormalRasterStackPixelTraceInput {
                role: prefixed_trace_role(role_prefix, "through-group"),
                render_node_id: Some(through_group.render_node_id.0),
                layer_id: Some(through_group.layer_id.0),
                blend_mode: Some("Through".to_string()),
                opacity: Some(through_group.opacity),
                rgba: None,
                mask_alpha: sample_optional_mask_alpha(through_group.mask.as_ref(), x, y)?,
            });
            let child_prefix = prefixed_trace_role(role_prefix, "through-group");
            for child in &through_group.draws {
                push_stack_draw_trace_inputs(child, &child_prefix, x, y, inputs)?;
            }
        }
        StrictRasterStackDraw::LutFilter(filter) => {
            inputs.push(NormalRasterStackPixelTraceInput {
                role: prefixed_trace_role(role_prefix, filter.name),
                render_node_id: Some(filter.render_node_id.0),
                layer_id: Some(filter.layer_id.0),
                blend_mode: Some(filter.name.to_string()),
                opacity: Some(filter.opacity),
                rgba: None,
                mask_alpha: sample_optional_mask_alpha(filter.mask.as_ref(), x, y)?,
            });
        }
    }
    Ok(())
}

fn push_raster_trace_input(
    role: &str,
    role_prefix: &str,
    decoded: &PlannedDecodedRaster,
    x: u32,
    y: u32,
    inputs: &mut Vec<NormalRasterStackPixelTraceInput>,
) -> Result<(), RuntimeError> {
    let image_size = CanvasSize::new(decoded.image.width, decoded.image.height);
    inputs.push(NormalRasterStackPixelTraceInput {
        role: prefixed_trace_role(role_prefix, role),
        render_node_id: Some(decoded.render_node_id.0),
        layer_id: Some(decoded.layer_id.0),
        blend_mode: Some(format!("{:?}", decoded.blend_mode)),
        opacity: Some(decoded.opacity),
        rgba: Some(sample_raster_source_rgba(decoded, image_size, x, y)?),
        mask_alpha: sample_optional_mask_alpha(decoded.mask.as_ref(), x, y)?,
    });
    Ok(())
}

fn sample_raster_source_rgba(
    decoded: &PlannedDecodedRaster,
    image_size: CanvasSize,
    x: u32,
    y: u32,
) -> Result<Rgba8, RuntimeError> {
    let source_x = i64::from(x) - i64::from(decoded.offset_x);
    let source_y = i64::from(y) - i64::from(decoded.offset_y);
    if source_x < 0
        || source_y < 0
        || source_x >= i64::from(image_size.width)
        || source_y >= i64::from(image_size.height)
    {
        return Ok(Rgba8 {
            r: 255,
            g: 255,
            b: 255,
            a: 0,
        });
    }
    sample_rgba8(
        &decoded.image.pixels,
        image_size,
        u32::try_from(source_x).map_err(|_| RuntimeError::InvalidRegion)?,
        u32::try_from(source_y).map_err(|_| RuntimeError::InvalidRegion)?,
    )
}

fn sample_optional_mask_alpha(
    mask: Option<&PlannedDecodedMask>,
    x: u32,
    y: u32,
) -> Result<Option<u8>, RuntimeError> {
    match mask {
        Some(mask) => {
            let size = CanvasSize::new(mask.image.width, mask.image.height);
            Ok(Some(sample_alpha8(&mask.image.pixels, size, x, y)?))
        }
        None => Ok(None),
    }
}

fn prefixed_trace_role(prefix: &str, role: &str) -> String {
    if prefix.is_empty() {
        role.to_string()
    } else {
        format!("{prefix}/{role}")
    }
}

fn gpu_normal_stack_source(draw: &StrictRasterStackDraw) -> clip_gpu::GpuNormalStackSource {
    match draw {
        StrictRasterStackDraw::Paper { color, opacity, .. } => {
            clip_gpu::GpuNormalStackSource::SolidColor {
                color: *color,
                opacity: *opacity,
            }
        }
        StrictRasterStackDraw::Raster(decoded) => {
            clip_gpu::GpuNormalStackSource::Raster(gpu_normal_raster_source(decoded))
        }
        StrictRasterStackDraw::ClippingRun(run) => clip_gpu::GpuNormalStackSource::ClippingRun {
            base: gpu_normal_raster_source(&run.base),
            clipped: run.clipped.iter().map(gpu_normal_raster_source).collect(),
        },
        StrictRasterStackDraw::Container(container) => clip_gpu::GpuNormalStackSource::Container {
            children: container
                .draws
                .iter()
                .map(gpu_normal_stack_source)
                .collect(),
            opacity: container.opacity,
            mask_key: container
                .mask
                .as_ref()
                .map(|mask| clip_gpu::GpuMaskResourceKey {
                    layer_id: container.layer_id,
                    mask_mipmap_id: mask.mask_mipmap_id,
                }),
            blend_mode: gpu_raster_blend_mode(container.blend_mode),
        },
        StrictRasterStackDraw::ThroughGroup(through_group) => {
            clip_gpu::GpuNormalStackSource::ThroughGroup {
                children: through_group
                    .draws
                    .iter()
                    .map(gpu_normal_stack_source)
                    .collect(),
                opacity: through_group.opacity,
                mask_key: through_group
                    .mask
                    .as_ref()
                    .map(|mask| clip_gpu::GpuMaskResourceKey {
                        layer_id: through_group.layer_id,
                        mask_mipmap_id: mask.mask_mipmap_id,
                    }),
            }
        }
        StrictRasterStackDraw::LutFilter(filter) => clip_gpu::GpuNormalStackSource::LutFilter {
            lut_rgba: filter.lut_rgba.clone(),
            opacity: filter.opacity,
            mask_key: filter
                .mask
                .as_ref()
                .map(|mask| clip_gpu::GpuMaskResourceKey {
                    layer_id: filter.layer_id,
                    mask_mipmap_id: mask.mask_mipmap_id,
                }),
            filter_mode: match filter.mode {
                PlannedLutFilterMode::ToneCurveRgb => clip_gpu::GpuLutFilterMode::ToneCurveRgb,
                PlannedLutFilterMode::GradientMapLum => clip_gpu::GpuLutFilterMode::GradientMapLum,
                PlannedLutFilterMode::ThresholdLum => clip_gpu::GpuLutFilterMode::ThresholdLum,
            },
        },
    }
}

fn gpu_normal_raster_source(decoded: &PlannedDecodedRaster) -> clip_gpu::GpuNormalRasterSource {
    clip_gpu::GpuNormalRasterSource {
        key: clip_gpu::GpuRasterResourceKey {
            layer_id: decoded.layer_id,
            render_mipmap_id: decoded.render_mipmap_id,
        },
        opacity: decoded.opacity,
        mask_key: decoded
            .mask
            .as_ref()
            .map(|mask| clip_gpu::GpuMaskResourceKey {
                layer_id: decoded.layer_id,
                mask_mipmap_id: mask.mask_mipmap_id,
            }),
        offset_x: decoded.offset_x,
        offset_y: decoded.offset_y,
        blend_mode: gpu_raster_blend_mode(decoded.blend_mode),
    }
}

fn gpu_lut_filter_mode(mode: PlannedLutFilterMode) -> clip_gpu::GpuLutFilterMode {
    match mode {
        PlannedLutFilterMode::ToneCurveRgb => clip_gpu::GpuLutFilterMode::ToneCurveRgb,
        PlannedLutFilterMode::GradientMapLum => clip_gpu::GpuLutFilterMode::GradientMapLum,
        PlannedLutFilterMode::ThresholdLum => clip_gpu::GpuLutFilterMode::ThresholdLum,
    }
}

fn gpu_raster_blend_mode(blend_mode: StrictRasterBlendMode) -> clip_gpu::GpuRasterBlendMode {
    match blend_mode {
        StrictRasterBlendMode::Normal => clip_gpu::GpuRasterBlendMode::Normal,
        StrictRasterBlendMode::Add => clip_gpu::GpuRasterBlendMode::Add,
        StrictRasterBlendMode::AddGlow => clip_gpu::GpuRasterBlendMode::AddGlow,
        StrictRasterBlendMode::ColorBurn => clip_gpu::GpuRasterBlendMode::ColorBurn,
        StrictRasterBlendMode::ColorDodge => clip_gpu::GpuRasterBlendMode::ColorDodge,
        StrictRasterBlendMode::Darken => clip_gpu::GpuRasterBlendMode::Darken,
        StrictRasterBlendMode::DarkerColor => clip_gpu::GpuRasterBlendMode::DarkerColor,
        StrictRasterBlendMode::Difference => clip_gpu::GpuRasterBlendMode::Difference,
        StrictRasterBlendMode::Divide => clip_gpu::GpuRasterBlendMode::Divide,
        StrictRasterBlendMode::Exclusion => clip_gpu::GpuRasterBlendMode::Exclusion,
        StrictRasterBlendMode::GlowDodge => clip_gpu::GpuRasterBlendMode::GlowDodge,
        StrictRasterBlendMode::HardMix => clip_gpu::GpuRasterBlendMode::HardMix,
        StrictRasterBlendMode::HardLight => clip_gpu::GpuRasterBlendMode::HardLight,
        StrictRasterBlendMode::Hue => clip_gpu::GpuRasterBlendMode::Hue,
        StrictRasterBlendMode::Lighten => clip_gpu::GpuRasterBlendMode::Lighten,
        StrictRasterBlendMode::LighterColor => clip_gpu::GpuRasterBlendMode::LighterColor,
        StrictRasterBlendMode::LinearBurn => clip_gpu::GpuRasterBlendMode::LinearBurn,
        StrictRasterBlendMode::LinearLight => clip_gpu::GpuRasterBlendMode::LinearLight,
        StrictRasterBlendMode::Multiply => clip_gpu::GpuRasterBlendMode::Multiply,
        StrictRasterBlendMode::Overlay => clip_gpu::GpuRasterBlendMode::Overlay,
        StrictRasterBlendMode::PinLight => clip_gpu::GpuRasterBlendMode::PinLight,
        StrictRasterBlendMode::Saturation => clip_gpu::GpuRasterBlendMode::Saturation,
        StrictRasterBlendMode::Brightness => clip_gpu::GpuRasterBlendMode::Brightness,
        StrictRasterBlendMode::Color => clip_gpu::GpuRasterBlendMode::Color,
        StrictRasterBlendMode::Screen => clip_gpu::GpuRasterBlendMode::Screen,
        StrictRasterBlendMode::SoftLight => clip_gpu::GpuRasterBlendMode::SoftLight,
        StrictRasterBlendMode::Subtract => clip_gpu::GpuRasterBlendMode::Subtract,
        StrictRasterBlendMode::VividLight => clip_gpu::GpuRasterBlendMode::VividLight,
    }
}

fn byte_diff_count(expected: &[u8], actual: &[u8]) -> usize {
    expected
        .iter()
        .zip(actual.iter())
        .filter(|(expected, actual)| expected != actual)
        .count()
        + expected.len().abs_diff(actual.len())
}

fn layer_graph_input_from_file(record: &clip_file::metadata::LayerGraphRecord) -> LayerGraphInput {
    LayerGraphInput {
        id: record.id,
        kind: record.kind,
        visibility: record.visibility,
        clip: record.clip,
        opacity: record.opacity,
        composite: record.composite,
        next_layer_id: record.next_layer_id,
        first_child_layer_id: record.first_child_layer_id,
        render_mipmap_id: record.render_mipmap_id,
        mask_mipmap_id: record.mask_mipmap_id,
        paper_color: record.paper_color,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use clip_file::ClipFileSummary;
    use clip_graph::{RenderNode, RenderNodeId, RenderNodeKind, RenderPlan};
    use clip_model::{CanvasSize, LayerId, LayerKind, LayerOpacity, LayerVisibility, Rgba8};

    use super::{ClipSession, StrictRasterStackDraw, StrictRasterStackOptions};

    #[test]
    fn plans_test_clipping_visible_layer_order() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
        let session = ClipSession::open(path).expect("open Test_Clipping.clip");
        let plan = session.render_plan();

        let nodes: Vec<_> = plan
            .nodes
            .iter()
            .map(|node| (node.layer_id, node.kind, node.depth))
            .collect();
        assert_eq!(
            nodes,
            vec![
                (LayerId(2), RenderNodeKind::Container, 0),
                (LayerId(4), RenderNodeKind::Paper, 1),
                (LayerId(10), RenderNodeKind::Raster, 1),
                (LayerId(11), RenderNodeKind::Raster, 1),
            ],
        );
    }

    #[test]
    fn byte_diff_count_includes_length_mismatch() {
        assert_eq!(super::byte_diff_count(&[1, 2, 3], &[1, 4]), 2);
    }

    #[test]
    fn alpha_is_fully_opaque_checks_every_pixel() {
        assert!(super::alpha_is_fully_opaque(&[1, 2, 3, 255, 4, 5, 6, 255]));
        assert!(!super::alpha_is_fully_opaque(&[1, 2, 3, 255, 4, 5, 6, 254]));
    }

    #[test]
    fn strict_normal_selector_keeps_normal_folder_as_container_source() {
        let session = synthetic_session(vec![
            container_node(0, 2, 0, 0),
            container_node(1, 8, 1, 0),
            paper_node(2, 4, 2),
        ]);

        let selection = session
            .select_strict_normal_raster_stack(StrictRasterStackOptions {
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
            })
            .expect("select synthetic normal folder");

        assert!(selection.unsupported.is_empty());
        assert_eq!(selection.draws.len(), 1);
        let StrictRasterStackDraw::Container(container) = &selection.draws[0] else {
            panic!("normal folder was not represented as a container source");
        };
        assert_eq!(container.layer_id, LayerId(8));
        assert_eq!(container.draws.len(), 1);
        assert!(matches!(
            container.draws[0],
            StrictRasterStackDraw::Paper { .. }
        ));
    }

    #[test]
    fn strict_normal_selector_keeps_through_folder_as_through_group_source() {
        let session = synthetic_session(vec![
            container_node(0, 2, 0, 0),
            container_node(1, 8, 1, super::LAYER_COMPOSITE_THROUGH),
            paper_node(2, 4, 2),
        ]);

        let selection = session
            .select_strict_normal_raster_stack(StrictRasterStackOptions {
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
            })
            .expect("select synthetic through folder");

        assert!(selection.unsupported.is_empty());
        assert_eq!(selection.draws.len(), 1);
        let StrictRasterStackDraw::ThroughGroup(through_group) = &selection.draws[0] else {
            panic!("THROUGH folder was not represented as a through-group source");
        };
        assert_eq!(through_group.layer_id, LayerId(8));
        assert_eq!(through_group.draws.len(), 1);
        assert!(matches!(
            through_group.draws[0],
            StrictRasterStackDraw::Paper { .. }
        ));
    }

    #[test]
    fn strict_normal_selector_clears_clip_base_after_through_group() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
        let session = ClipSession {
            path: path.to_path_buf(),
            container: clip_file::container::ClipContainer::open(&path)
                .expect("open Test_Clipping.clip container"),
            summary: ClipFileSummary {
                canvas: CanvasSize::new(512, 512),
                root_layer_id: LayerId(2),
                layer_count: 5,
                external_data_count: 7,
            },
            render_plan: RenderPlan {
                canvas: CanvasSize::new(512, 512),
                root_layer_id: LayerId(2),
                nodes: vec![
                    container_node(0, 2, 0, 0),
                    container_node(1, 8, 1, super::LAYER_COMPOSITE_THROUGH),
                    paper_node(2, 4, 2),
                    raster_node(3, 11, 1, 16, true),
                ],
            },
            raster_sources: HashMap::new(),
            mask_sources: HashMap::new(),
            filter_sources: HashMap::new(),
            rendered_image: None,
        };

        let selection = session
            .select_strict_normal_raster_stack(StrictRasterStackOptions {
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
            })
            .expect("select synthetic through-cleared clipped raster");

        assert!(selection.unsupported.is_empty());
        assert_eq!(selection.draws.len(), 2);
        assert!(matches!(
            selection.draws[0],
            StrictRasterStackDraw::ThroughGroup(_)
        ));
        assert!(matches!(
            selection.draws[1],
            StrictRasterStackDraw::Raster(_)
        ));
    }

    #[test]
    fn gpu_selector_accepts_python_backed_lut_and_luminosity_filters() {
        let mut session = synthetic_session(vec![
            container_node(0, 2, 0, 0),
            filter_node(1, 10, 1),
            filter_node(2, 11, 1),
            filter_node(3, 12, 1),
            filter_node(4, 13, 1),
            filter_node(5, 14, 1),
            filter_node(6, 15, 1),
        ]);
        session.filter_sources.insert(
            LayerId(10),
            filter_source(10, 1, brightness_contrast_payload(20, -10)),
        );
        session.filter_sources.insert(
            LayerId(11),
            filter_source(11, 2, level_payload([0, 20000, 65535, 0, 65535])),
        );
        session
            .filter_sources
            .insert(LayerId(12), filter_source(12, 6, Vec::new()));
        session.filter_sources.insert(
            LayerId(13),
            filter_source(13, 7, 4i32.to_be_bytes().to_vec()),
        );
        session.filter_sources.insert(
            LayerId(14),
            filter_source(14, 8, 128i32.to_be_bytes().to_vec()),
        );
        session.filter_sources.insert(
            LayerId(15),
            filter_source(
                15,
                5,
                color_balance_payload([1, 0, 0, 0, 43, -48, 48, 0, 0, 0]),
            ),
        );

        let selection = session
            .select_gpu_normal_render_stack(gpu_selector_options())
            .expect("select synthetic LUT filters");

        assert!(selection.unsupported.is_empty());
        assert_eq!(selection.sources.len(), 6);
        for (source, (input, expected, expected_mode)) in selection.sources.iter().zip([
            (
                64usize,
                [51, 51, 51],
                clip_gpu::GpuLutFilterMode::ToneCurveRgb,
            ),
            (
                64usize,
                [24, 24, 24],
                clip_gpu::GpuLutFilterMode::ToneCurveRgb,
            ),
            (
                64usize,
                [191, 191, 191],
                clip_gpu::GpuLutFilterMode::ToneCurveRgb,
            ),
            (
                64usize,
                [85, 85, 85],
                clip_gpu::GpuLutFilterMode::ToneCurveRgb,
            ),
            (
                128usize,
                [255, 255, 255],
                clip_gpu::GpuLutFilterMode::ThresholdLum,
            ),
            (
                226usize,
                [231, 212, 231],
                clip_gpu::GpuLutFilterMode::ToneCurveRgb,
            ),
        ]) {
            let clip_gpu::GpuNormalStackSource::LutFilter {
                lut_rgba,
                filter_mode,
                ..
            } = source
            else {
                panic!("source was not represented as a LUT filter");
            };
            assert_eq!(*filter_mode, expected_mode);
            assert_eq!(&lut_rgba[input * 4..input * 4 + 3], expected.as_slice());
        }
    }

    #[test]
    fn strict_raster_blend_mode_allows_supported_blends_by_position() {
        let options = StrictRasterStackOptions {
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
        };
        let add_glow =
            raster_node_with_composite(1, 5, 1, 9, false, super::LAYER_COMPOSITE_ADD_GLOW);
        assert_eq!(
            super::strict_raster_blend_mode(&add_glow, options, false),
            Some(super::StrictRasterBlendMode::AddGlow)
        );

        let clipped_add_glow =
            raster_node_with_composite(2, 6, 1, 10, true, super::LAYER_COMPOSITE_ADD_GLOW);
        assert_eq!(
            super::strict_raster_blend_mode(&clipped_add_glow, options, true),
            Some(super::StrictRasterBlendMode::AddGlow)
        );

        let disabled = StrictRasterStackOptions {
            allow_add_glow_blend: false,
            ..options
        };
        assert_eq!(
            super::strict_raster_blend_mode(&add_glow, disabled, false),
            None
        );

        let color_burn =
            raster_node_with_composite(7, 11, 1, 15, false, super::LAYER_COMPOSITE_COLOR_BURN);
        assert_eq!(
            super::strict_raster_blend_mode(&color_burn, options, false),
            Some(super::StrictRasterBlendMode::ColorBurn)
        );

        let clipped_color_burn =
            raster_node_with_composite(8, 12, 1, 16, true, super::LAYER_COMPOSITE_COLOR_BURN);
        assert_eq!(
            super::strict_raster_blend_mode(&clipped_color_burn, options, true),
            Some(super::StrictRasterBlendMode::ColorBurn)
        );

        let disabled = StrictRasterStackOptions {
            allow_color_burn_blend: false,
            ..options
        };
        assert_eq!(
            super::strict_raster_blend_mode(&color_burn, disabled, false),
            None
        );

        let color_dodge =
            raster_node_with_composite(3, 7, 1, 11, false, super::LAYER_COMPOSITE_COLOR_DODGE);
        assert_eq!(
            super::strict_raster_blend_mode(&color_dodge, options, false),
            Some(super::StrictRasterBlendMode::ColorDodge)
        );

        let clipped_color_dodge =
            raster_node_with_composite(4, 8, 1, 12, true, super::LAYER_COMPOSITE_COLOR_DODGE);
        assert_eq!(
            super::strict_raster_blend_mode(&clipped_color_dodge, options, true),
            Some(super::StrictRasterBlendMode::ColorDodge)
        );

        let disabled = StrictRasterStackOptions {
            allow_color_dodge_blend: false,
            ..options
        };
        assert_eq!(
            super::strict_raster_blend_mode(&color_dodge, disabled, false),
            None
        );

        let glow_dodge =
            raster_node_with_composite(5, 9, 1, 13, false, super::LAYER_COMPOSITE_GLOW_DODGE);
        assert_eq!(
            super::strict_raster_blend_mode(&glow_dodge, options, false),
            Some(super::StrictRasterBlendMode::GlowDodge)
        );

        let clipped_glow_dodge =
            raster_node_with_composite(6, 10, 1, 14, true, super::LAYER_COMPOSITE_GLOW_DODGE);
        assert_eq!(
            super::strict_raster_blend_mode(&clipped_glow_dodge, options, true),
            Some(super::StrictRasterBlendMode::GlowDodge)
        );

        let disabled = StrictRasterStackOptions {
            allow_glow_dodge_blend: false,
            ..options
        };
        assert_eq!(
            super::strict_raster_blend_mode(&glow_dodge, disabled, false),
            None
        );

        let add = raster_node_with_composite(26, 36, 1, 40, false, super::LAYER_COMPOSITE_ADD);
        assert_eq!(
            super::strict_raster_blend_mode(&add, options, false),
            Some(super::StrictRasterBlendMode::Add)
        );

        let clipped_add =
            raster_node_with_composite(27, 37, 1, 41, true, super::LAYER_COMPOSITE_ADD);
        assert_eq!(
            super::strict_raster_blend_mode(&clipped_add, options, true),
            Some(super::StrictRasterBlendMode::Add)
        );

        let disabled = StrictRasterStackOptions {
            allow_add_blend: false,
            ..options
        };
        assert_eq!(super::strict_raster_blend_mode(&add, disabled, false), None);

        let hard_mix =
            raster_node_with_composite(9, 13, 1, 17, false, super::LAYER_COMPOSITE_HARD_MIX);
        assert_eq!(
            super::strict_raster_blend_mode(&hard_mix, options, false),
            Some(super::StrictRasterBlendMode::HardMix)
        );

        let clipped_hard_mix =
            raster_node_with_composite(10, 14, 1, 18, true, super::LAYER_COMPOSITE_HARD_MIX);
        assert_eq!(
            super::strict_raster_blend_mode(&clipped_hard_mix, options, true),
            Some(super::StrictRasterBlendMode::HardMix)
        );

        let disabled = StrictRasterStackOptions {
            allow_hard_mix_blend: false,
            ..options
        };
        assert_eq!(
            super::strict_raster_blend_mode(&hard_mix, disabled, false),
            None
        );

        let w3c_blends = [
            (
                40,
                50,
                60,
                super::LAYER_COMPOSITE_MULTIPLY,
                super::StrictRasterBlendMode::Multiply,
            ),
            (
                41,
                51,
                61,
                super::LAYER_COMPOSITE_OVERLAY,
                super::StrictRasterBlendMode::Overlay,
            ),
            (
                42,
                52,
                62,
                super::LAYER_COMPOSITE_HARD_LIGHT,
                super::StrictRasterBlendMode::HardLight,
            ),
        ];
        for (node, layer, mipmap, composite, expected) in w3c_blends {
            let raster = raster_node_with_composite(node, layer, 1, mipmap, false, composite);
            assert_eq!(
                super::strict_raster_blend_mode(&raster, options, false),
                Some(expected)
            );

            let clipped =
                raster_node_with_composite(node + 10, layer + 10, 1, mipmap + 10, true, composite);
            assert_eq!(
                super::strict_raster_blend_mode(&clipped, options, true),
                Some(expected)
            );

            let disabled = StrictRasterStackOptions {
                allow_w3c_blends: false,
                ..options
            };
            assert_eq!(
                super::strict_raster_blend_mode(&raster, disabled, false),
                None
            );
        }

        let simple_blends = [
            (
                21,
                31,
                35,
                super::LAYER_COMPOSITE_DARKEN,
                super::StrictRasterBlendMode::Darken,
            ),
            (
                22,
                32,
                36,
                super::LAYER_COMPOSITE_SUBTRACT,
                super::StrictRasterBlendMode::Subtract,
            ),
            (
                23,
                33,
                37,
                super::LAYER_COMPOSITE_LIGHTEN,
                super::StrictRasterBlendMode::Lighten,
            ),
            (
                24,
                34,
                38,
                super::LAYER_COMPOSITE_SCREEN,
                super::StrictRasterBlendMode::Screen,
            ),
            (
                25,
                35,
                39,
                super::LAYER_COMPOSITE_DIFFERENCE,
                super::StrictRasterBlendMode::Difference,
            ),
        ];
        for (node, layer, mipmap, composite, expected) in simple_blends {
            let raster = raster_node_with_composite(node, layer, 1, mipmap, false, composite);
            assert_eq!(
                super::strict_raster_blend_mode(&raster, options, false),
                Some(expected)
            );

            let clipped =
                raster_node_with_composite(node + 10, layer + 10, 1, mipmap + 10, true, composite);
            assert_eq!(
                super::strict_raster_blend_mode(&clipped, options, true),
                Some(expected)
            );

            let disabled = StrictRasterStackOptions {
                allow_simple_blends: false,
                ..options
            };
            assert_eq!(
                super::strict_raster_blend_mode(&raster, disabled, false),
                None
            );
        }

        let extended_blends = [
            (
                60,
                70,
                80,
                super::LAYER_COMPOSITE_LINEAR_BURN,
                super::StrictRasterBlendMode::LinearBurn,
            ),
            (
                61,
                71,
                81,
                super::LAYER_COMPOSITE_DARKER_COLOR,
                super::StrictRasterBlendMode::DarkerColor,
            ),
            (
                62,
                72,
                82,
                super::LAYER_COMPOSITE_LIGHTER_COLOR,
                super::StrictRasterBlendMode::LighterColor,
            ),
            (
                63,
                73,
                83,
                super::LAYER_COMPOSITE_LINEAR_LIGHT,
                super::StrictRasterBlendMode::LinearLight,
            ),
            (
                64,
                74,
                84,
                super::LAYER_COMPOSITE_PIN_LIGHT,
                super::StrictRasterBlendMode::PinLight,
            ),
            (
                65,
                75,
                85,
                super::LAYER_COMPOSITE_EXCLUSION,
                super::StrictRasterBlendMode::Exclusion,
            ),
            (
                66,
                76,
                86,
                super::LAYER_COMPOSITE_BRIGHTNESS,
                super::StrictRasterBlendMode::Brightness,
            ),
            (
                67,
                77,
                87,
                super::LAYER_COMPOSITE_DIVIDE,
                super::StrictRasterBlendMode::Divide,
            ),
        ];
        for (node, layer, mipmap, composite, expected) in extended_blends {
            let raster = raster_node_with_composite(node, layer, 1, mipmap, false, composite);
            assert_eq!(
                super::strict_raster_blend_mode(&raster, options, false),
                Some(expected)
            );

            let clipped =
                raster_node_with_composite(node + 10, layer + 10, 1, mipmap + 10, true, composite);
            assert_eq!(
                super::strict_raster_blend_mode(&clipped, options, true),
                Some(expected)
            );

            let disabled = StrictRasterStackOptions {
                allow_extended_blends: false,
                ..options
            };
            assert_eq!(
                super::strict_raster_blend_mode(&raster, disabled, false),
                None
            );
        }

        let hue = raster_node_with_composite(13, 17, 1, 21, false, super::LAYER_COMPOSITE_HUE);
        assert_eq!(
            super::strict_raster_blend_mode(&hue, options, false),
            Some(super::StrictRasterBlendMode::Hue)
        );

        let clipped_hue =
            raster_node_with_composite(14, 18, 1, 22, true, super::LAYER_COMPOSITE_HUE);
        assert_eq!(
            super::strict_raster_blend_mode(&clipped_hue, options, true),
            Some(super::StrictRasterBlendMode::Hue)
        );

        let saturation =
            raster_node_with_composite(15, 19, 1, 23, false, super::LAYER_COMPOSITE_SATURATION);
        assert_eq!(
            super::strict_raster_blend_mode(&saturation, options, false),
            Some(super::StrictRasterBlendMode::Saturation)
        );

        let clipped_saturation =
            raster_node_with_composite(16, 20, 1, 24, true, super::LAYER_COMPOSITE_SATURATION);
        assert_eq!(
            super::strict_raster_blend_mode(&clipped_saturation, options, true),
            Some(super::StrictRasterBlendMode::Saturation)
        );

        let color = raster_node_with_composite(17, 21, 1, 25, false, super::LAYER_COMPOSITE_COLOR);
        assert_eq!(
            super::strict_raster_blend_mode(&color, options, false),
            Some(super::StrictRasterBlendMode::Color)
        );

        let clipped_color =
            raster_node_with_composite(18, 22, 1, 26, true, super::LAYER_COMPOSITE_COLOR);
        assert_eq!(
            super::strict_raster_blend_mode(&clipped_color, options, true),
            Some(super::StrictRasterBlendMode::Color)
        );

        let disabled = StrictRasterStackOptions {
            allow_hsl_blends: false,
            ..options
        };
        assert_eq!(super::strict_raster_blend_mode(&hue, disabled, false), None);
        assert_eq!(
            super::strict_raster_blend_mode(&saturation, disabled, false),
            None
        );
        assert_eq!(
            super::strict_raster_blend_mode(&color, disabled, false),
            None
        );

        let soft_light =
            raster_node_with_composite(19, 23, 1, 27, false, super::LAYER_COMPOSITE_SOFT_LIGHT);
        assert_eq!(
            super::strict_raster_blend_mode(&soft_light, options, false),
            Some(super::StrictRasterBlendMode::SoftLight)
        );

        let clipped_soft_light =
            raster_node_with_composite(20, 24, 1, 28, true, super::LAYER_COMPOSITE_SOFT_LIGHT);
        assert_eq!(
            super::strict_raster_blend_mode(&clipped_soft_light, options, true),
            Some(super::StrictRasterBlendMode::SoftLight)
        );

        let disabled = StrictRasterStackOptions {
            allow_soft_light_blend: false,
            ..options
        };
        assert_eq!(
            super::strict_raster_blend_mode(&soft_light, disabled, false),
            None
        );

        let vivid_light =
            raster_node_with_composite(11, 15, 1, 19, false, super::LAYER_COMPOSITE_VIVID_LIGHT);
        assert_eq!(
            super::strict_raster_blend_mode(&vivid_light, options, false),
            Some(super::StrictRasterBlendMode::VividLight)
        );

        let clipped_vivid_light =
            raster_node_with_composite(12, 16, 1, 20, true, super::LAYER_COMPOSITE_VIVID_LIGHT);
        assert_eq!(
            super::strict_raster_blend_mode(&clipped_vivid_light, options, true),
            Some(super::StrictRasterBlendMode::VividLight)
        );

        let disabled = StrictRasterStackOptions {
            allow_vivid_light_blend: false,
            ..options
        };
        assert_eq!(
            super::strict_raster_blend_mode(&vivid_light, disabled, false),
            None
        );
    }

    #[test]
    fn normal_folder_with_real_test_clipping_children_matches_flat_stack() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
        let flat = ClipSession::open(&path)
            .expect("open Test_Clipping.clip")
            .draw_normal_raster_stack_via_gpu()
            .expect("draw flat Test_Clipping stack");
        assert!(flat.unsupported.is_empty());
        let flat_image = flat.image.expect("flat output image");

        let folder_session_container = clip_file::container::ClipContainer::open(&path)
            .expect("open Test_Clipping.clip container");
        let folder_raster_sources = clip_file::metadata::read_raster_layer_sources_from_sqlite(
            folder_session_container.sqlite_bytes(),
            &[LayerId(10), LayerId(11)],
            CanvasSize::new(512, 512),
        )
        .expect("read Test_Clipping raster sources");
        let folder_session = ClipSession {
            container: folder_session_container,
            path,
            summary: ClipFileSummary {
                canvas: CanvasSize::new(512, 512),
                root_layer_id: LayerId(2),
                layer_count: 5,
                external_data_count: 7,
            },
            render_plan: RenderPlan {
                canvas: CanvasSize::new(512, 512),
                root_layer_id: LayerId(2),
                nodes: vec![
                    container_node(0, 2, 0, 0),
                    container_node(1, 1000, 1, 0),
                    paper_node(2, 4, 2),
                    raster_node(3, 10, 2, 15, false),
                    raster_node(4, 11, 2, 16, true),
                ],
            },
            raster_sources: folder_raster_sources,
            mask_sources: HashMap::new(),
            filter_sources: HashMap::new(),
            rendered_image: None,
        };

        let folder = folder_session
            .draw_normal_raster_stack_via_gpu()
            .expect("draw synthetic folder Test_Clipping stack");
        assert!(folder.unsupported.is_empty());
        let folder_image = folder.image.expect("folder output image");

        assert_eq!(folder.source_count, 2);
        assert_eq!(folder.drawn_resources.len(), 2);
        assert_eq!(folder_image.pixels, flat_image.pixels);
    }

    #[test]
    fn gpu_selector_folds_off_canvas_zero_fill_mask_to_zero_opacity() {
        let mut session = synthetic_session(vec![
            container_node(0, 2, 0, 0),
            raster_node_with_mask(1, 10, 1, 15, 50, false),
        ]);
        session
            .raster_sources
            .insert(LayerId(10), raster_source(10, 15));
        session
            .mask_sources
            .insert(LayerId(10), off_canvas_mask_source(10, 50, 0));

        let selection = session
            .select_gpu_normal_render_stack(gpu_selector_options())
            .expect("select synthetic masked raster");

        assert!(selection.unsupported.is_empty());
        assert_eq!(selection.resource_plan.mask_resource_count(), 0);
        assert_eq!(selection.sources.len(), 1);
        let clip_gpu::GpuNormalStackSource::Raster(raster) = &selection.sources[0] else {
            panic!("masked raster was not represented as a raster source");
        };
        assert_eq!(raster.opacity, 0.0);
        assert_eq!(raster.mask_key, None);
    }

    #[test]
    fn gpu_selector_elides_off_canvas_opaque_mask_resource() {
        let mut session = synthetic_session(vec![
            container_node(0, 2, 0, 0),
            raster_node_with_mask(1, 10, 1, 15, 50, false),
        ]);
        session
            .raster_sources
            .insert(LayerId(10), raster_source(10, 15));
        session
            .mask_sources
            .insert(LayerId(10), off_canvas_mask_source(10, 50, 255));

        let selection = session
            .select_gpu_normal_render_stack(gpu_selector_options())
            .expect("select synthetic masked raster");

        assert!(selection.unsupported.is_empty());
        assert_eq!(selection.resource_plan.mask_resource_count(), 0);
        assert_eq!(selection.sources.len(), 1);
        let clip_gpu::GpuNormalStackSource::Raster(raster) = &selection.sources[0] else {
            panic!("masked raster was not represented as a raster source");
        };
        assert_eq!(raster.opacity, 1.0);
        assert_eq!(raster.mask_key, None);
    }

    #[test]
    fn gpu_selector_keeps_partial_fill_off_canvas_mask_resource() {
        let mut session = synthetic_session(vec![
            container_node(0, 2, 0, 0),
            raster_node_with_mask(1, 10, 1, 15, 50, false),
        ]);
        session
            .raster_sources
            .insert(LayerId(10), raster_source(10, 15));
        session
            .mask_sources
            .insert(LayerId(10), off_canvas_mask_source(10, 50, 128));

        let selection = session
            .select_gpu_normal_render_stack(gpu_selector_options())
            .expect("select synthetic masked raster");

        assert!(selection.unsupported.is_empty());
        assert_eq!(selection.resource_plan.mask_resource_count(), 1);
        assert_eq!(selection.sources.len(), 1);
        let clip_gpu::GpuNormalStackSource::Raster(raster) = &selection.sources[0] else {
            panic!("masked raster was not represented as a raster source");
        };
        assert_eq!(raster.opacity, 1.0);
        assert!(raster.mask_key.is_some());
    }

    fn synthetic_session(nodes: Vec<RenderNode>) -> ClipSession {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
        ClipSession {
            container: clip_file::container::ClipContainer::open(&path)
                .expect("open Test_Clipping.clip container"),
            path: PathBuf::new(),
            summary: ClipFileSummary {
                canvas: CanvasSize::new(8, 8),
                root_layer_id: LayerId(2),
                layer_count: nodes.len(),
                external_data_count: 0,
            },
            render_plan: RenderPlan {
                canvas: CanvasSize::new(8, 8),
                root_layer_id: LayerId(2),
                nodes,
            },
            raster_sources: HashMap::new(),
            mask_sources: HashMap::new(),
            filter_sources: HashMap::new(),
            rendered_image: None,
        }
    }

    fn container_node(id: u32, layer_id: u32, depth: u16, composite: u32) -> RenderNode {
        RenderNode {
            id: RenderNodeId(id),
            layer_id: LayerId(layer_id),
            kind: RenderNodeKind::Container,
            depth,
            clip: false,
            opacity: LayerOpacity::MAX,
            composite,
            render_mipmap_id: None,
            mask_mipmap_id: None,
            paper_color: None,
        }
    }

    fn paper_node(id: u32, layer_id: u32, depth: u16) -> RenderNode {
        RenderNode {
            id: RenderNodeId(id),
            layer_id: LayerId(layer_id),
            kind: RenderNodeKind::Paper,
            depth,
            clip: false,
            opacity: LayerOpacity::MAX,
            composite: 0,
            render_mipmap_id: None,
            mask_mipmap_id: None,
            paper_color: Some(Rgba8 {
                r: 226,
                g: 226,
                b: 226,
                a: 255,
            }),
        }
    }

    fn filter_node(id: u32, layer_id: u32, depth: u16) -> RenderNode {
        RenderNode {
            id: RenderNodeId(id),
            layer_id: LayerId(layer_id),
            kind: RenderNodeKind::Filter,
            depth,
            clip: false,
            opacity: LayerOpacity::MAX,
            composite: 0,
            render_mipmap_id: None,
            mask_mipmap_id: None,
            paper_color: None,
        }
    }

    fn raster_node(
        id: u32,
        layer_id: u32,
        depth: u16,
        render_mipmap_id: u32,
        clip: bool,
    ) -> RenderNode {
        raster_node_with_composite(id, layer_id, depth, render_mipmap_id, clip, 0)
    }

    fn raster_node_with_composite(
        id: u32,
        layer_id: u32,
        depth: u16,
        render_mipmap_id: u32,
        clip: bool,
        composite: u32,
    ) -> RenderNode {
        RenderNode {
            id: RenderNodeId(id),
            layer_id: LayerId(layer_id),
            kind: RenderNodeKind::Raster,
            depth,
            clip,
            opacity: LayerOpacity::MAX,
            composite,
            render_mipmap_id: Some(render_mipmap_id),
            mask_mipmap_id: None,
            paper_color: None,
        }
    }

    fn raster_node_with_mask(
        id: u32,
        layer_id: u32,
        depth: u16,
        render_mipmap_id: u32,
        mask_mipmap_id: u32,
        clip: bool,
    ) -> RenderNode {
        let mut node = raster_node(id, layer_id, depth, render_mipmap_id, clip);
        node.mask_mipmap_id = Some(mask_mipmap_id);
        node
    }

    fn raster_source(
        layer_id: u32,
        render_mipmap_id: u32,
    ) -> clip_file::metadata::RasterLayerSource {
        clip_file::metadata::RasterLayerSource {
            layer: clip_file::metadata::LayerRecord {
                id: LayerId(layer_id),
                kind: LayerKind::Raster,
                visibility: LayerVisibility(1),
            },
            render_mipmap_id,
            offscreen_id: 1000 + render_mipmap_id,
            external_id: format!("synthetic-raster-{layer_id}"),
            pixel_size: CanvasSize::new(2, 2),
            color_type: None,
            offset_x: 0,
            offset_y: 0,
        }
    }

    fn filter_source(
        layer_id: u32,
        filter_type: u32,
        payload: Vec<u8>,
    ) -> clip_file::metadata::FilterLayerSource {
        clip_file::metadata::FilterLayerSource {
            layer_id: LayerId(layer_id),
            filter_type,
            payload,
        }
    }

    fn brightness_contrast_payload(brightness: i32, contrast: i32) -> Vec<u8> {
        let mut payload = Vec::with_capacity(8);
        payload.extend_from_slice(&brightness.to_be_bytes());
        payload.extend_from_slice(&contrast.to_be_bytes());
        payload
    }

    fn level_payload(group: [u16; 5]) -> Vec<u8> {
        let mut payload = vec![0u8; 0x40];
        for (index, value) in group.iter().enumerate() {
            payload[index * 2..index * 2 + 2].copy_from_slice(&value.to_be_bytes());
        }
        payload
    }

    fn color_balance_payload(values: [i32; 10]) -> Vec<u8> {
        let mut payload = Vec::with_capacity(40);
        for value in values {
            payload.extend_from_slice(&value.to_be_bytes());
        }
        payload
    }

    fn off_canvas_mask_source(
        layer_id: u32,
        mask_mipmap_id: u32,
        empty_fill: u8,
    ) -> clip_file::metadata::MaskLayerSource {
        clip_file::metadata::MaskLayerSource {
            layer_id: LayerId(layer_id),
            mask_mipmap_id,
            offscreen_id: 2000 + mask_mipmap_id,
            external_id: format!("synthetic-mask-{layer_id}"),
            pixel_size: CanvasSize::new(2, 2),
            empty_fill,
            offset_x: 20,
            offset_y: 20,
        }
    }

    fn gpu_selector_options() -> StrictRasterStackOptions {
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
            allow_initial_terminal_container_elision: false,
        }
    }
}
