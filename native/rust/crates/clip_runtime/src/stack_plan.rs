mod trace;

pub(super) use trace::{sample_rgba8, stack_draw_trace_inputs, stack_draw_trace_label};

use clip_graph::RenderNodeId;
use clip_model::{LayerId, LayerOpacity, Rgba8};

use crate::blend::{StrictRasterBlendMode, gpu_raster_blend_mode};
use crate::filter_lut::PlannedLutFilterMode;
use crate::gpu_provider::{GpuResourcePlan, PlannedGpuMaskResource};
use crate::results::SimpleRasterStackUnsupported;

#[derive(Debug)]
pub(super) struct PlannedDecodedRaster {
    pub(super) render_node_id: RenderNodeId,
    pub(super) layer_id: LayerId,
    pub(super) render_mipmap_id: u32,
    pub(super) image: clip_file::tiles::RgbaTileImage,
    pub(super) offset_x: i32,
    pub(super) offset_y: i32,
    pub(super) opacity: f32,
    pub(super) mask: Option<PlannedDecodedMask>,
    pub(super) blend_mode: StrictRasterBlendMode,
}

#[derive(Debug)]
pub(super) struct PlannedDecodedMask {
    pub(super) mask_mipmap_id: u32,
    pub(super) image: clip_file::tiles::AlphaTileImage,
}

#[derive(Debug)]
pub(super) struct PlannedClippingRun {
    pub(super) base: PlannedDecodedRaster,
    pub(super) clipped: Vec<PlannedDecodedRaster>,
}

#[derive(Debug)]
pub(super) struct PlannedContainerStack {
    pub(super) render_node_id: RenderNodeId,
    pub(super) layer_id: LayerId,
    pub(super) opacity: f32,
    pub(super) mask: Option<PlannedDecodedMask>,
    pub(super) blend_mode: StrictRasterBlendMode,
    pub(super) draws: Vec<StrictRasterStackDraw>,
}

#[derive(Debug)]
pub(super) struct PlannedThroughGroup {
    pub(super) render_node_id: RenderNodeId,
    pub(super) layer_id: LayerId,
    pub(super) opacity: f32,
    pub(super) mask: Option<PlannedDecodedMask>,
    pub(super) draws: Vec<StrictRasterStackDraw>,
}

#[derive(Debug)]
pub(super) struct PlannedLutFilter {
    pub(super) render_node_id: RenderNodeId,
    pub(super) layer_id: LayerId,
    pub(super) name: &'static str,
    pub(super) mode: PlannedLutFilterMode,
    pub(super) opacity: f32,
    pub(super) mask: Option<PlannedDecodedMask>,
    pub(super) lut_rgba: Vec<u8>,
}

#[derive(Debug)]
pub(super) struct StrictRasterStackSelection {
    pub(super) draws: Vec<StrictRasterStackDraw>,
    pub(super) unsupported: Vec<SimpleRasterStackUnsupported>,
}

#[derive(Debug)]
pub(super) struct GpuRenderStackSelection {
    pub(super) sources: Vec<clip_gpu::GpuNormalStackSource>,
    pub(super) resource_plan: GpuResourcePlan,
    pub(super) unsupported: Vec<SimpleRasterStackUnsupported>,
}

#[derive(Debug)]
pub(super) enum StrictRasterStackDraw {
    Paper { color: Rgba8, opacity: f32 },
    Raster(PlannedDecodedRaster),
    ClippingRun(PlannedClippingRun),
    Container(PlannedContainerStack),
    ThroughGroup(PlannedThroughGroup),
    LutFilter(PlannedLutFilter),
}

impl StrictRasterStackDraw {
    pub(super) fn as_raster(&self) -> Option<&PlannedDecodedRaster> {
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
pub(super) struct StrictRasterStackOptions {
    pub(super) allow_alpha_compositing: bool,
    pub(super) allow_paper: bool,
    pub(super) allow_layer_opacity: bool,
    pub(super) allow_masks: bool,
    pub(super) allow_clipping_runs: bool,
    pub(super) allow_container_isolation: bool,
    pub(super) allow_through_groups: bool,
    pub(super) allow_add_blend: bool,
    pub(super) allow_add_glow_blend: bool,
    pub(super) allow_color_burn_blend: bool,
    pub(super) allow_color_dodge_blend: bool,
    pub(super) allow_extended_blends: bool,
    pub(super) allow_glow_dodge_blend: bool,
    pub(super) allow_hard_mix_blend: bool,
    pub(super) allow_hsl_blends: bool,
    pub(super) allow_simple_blends: bool,
    pub(super) allow_soft_light_blend: bool,
    pub(super) allow_lut_filters: bool,
    pub(super) allow_vivid_light_blend: bool,
    pub(super) allow_w3c_blends: bool,
    pub(super) allow_initial_terminal_container_elision: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ClipBaseState {
    Available,
    Cleared,
    Blocked,
}

pub(super) fn opacity_factor(opacity: LayerOpacity) -> Option<f32> {
    if opacity.0 <= LayerOpacity::MAX.0 {
        Some(f32::from(opacity.0) / f32::from(LayerOpacity::MAX.0))
    } else {
        None
    }
}

pub(super) fn apply_planned_gpu_mask(
    mask: PlannedGpuMaskResource,
    opacity: f32,
) -> (Option<clip_gpu::GpuMaskResourceKey>, f32) {
    match mask {
        PlannedGpuMaskResource::None | PlannedGpuMaskResource::FullyOpaque => (None, opacity),
        PlannedGpuMaskResource::Key(key) => (Some(key), opacity),
        PlannedGpuMaskResource::FullyTransparent => (None, 0.0),
    }
}

pub(super) fn alpha_is_fully_opaque(pixels: &[u8]) -> bool {
    pixels.chunks_exact(4).all(|pixel| pixel[3] == 255)
}

pub(super) fn can_elide_initial_terminal_container(
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

pub(super) fn decoded_rasters_in_draws(
    draws: &[StrictRasterStackDraw],
) -> Vec<&PlannedDecodedRaster> {
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

pub(super) fn decoded_containers_in_draws(
    draws: &[StrictRasterStackDraw],
) -> Vec<&PlannedContainerStack> {
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

pub(super) fn decoded_through_groups_in_draws(
    draws: &[StrictRasterStackDraw],
) -> Vec<&PlannedThroughGroup> {
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

pub(super) fn decoded_lut_filters_in_draws(
    draws: &[StrictRasterStackDraw],
) -> Vec<&PlannedLutFilter> {
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

pub(super) fn gpu_normal_stack_source(
    draw: &StrictRasterStackDraw,
) -> clip_gpu::GpuNormalStackSource {
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
            clipped: run
                .clipped
                .iter()
                .map(gpu_normal_raster_source)
                .map(clip_gpu::GpuClippedStackSource::Raster)
                .collect(),
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
                PlannedLutFilterMode::Hsl {
                    hue_turns,
                    saturation_delta,
                    luminosity_delta,
                } => clip_gpu::GpuLutFilterMode::Hsl(clip_gpu::GpuHslFilterParams {
                    hue_turns,
                    saturation_delta,
                    luminosity_delta,
                }),
            },
        },
    }
}

pub(super) fn gpu_normal_raster_source(
    decoded: &PlannedDecodedRaster,
) -> clip_gpu::GpuNormalRasterSource {
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

pub(super) fn gpu_lut_filter_mode(mode: PlannedLutFilterMode) -> clip_gpu::GpuLutFilterMode {
    match mode {
        PlannedLutFilterMode::ToneCurveRgb => clip_gpu::GpuLutFilterMode::ToneCurveRgb,
        PlannedLutFilterMode::GradientMapLum => clip_gpu::GpuLutFilterMode::GradientMapLum,
        PlannedLutFilterMode::ThresholdLum => clip_gpu::GpuLutFilterMode::ThresholdLum,
        PlannedLutFilterMode::Hsl {
            hue_turns,
            saturation_delta,
            luminosity_delta,
        } => clip_gpu::GpuLutFilterMode::Hsl(clip_gpu::GpuHslFilterParams {
            hue_turns,
            saturation_delta,
            luminosity_delta,
        }),
    }
}

pub(super) fn byte_diff_count(expected: &[u8], actual: &[u8]) -> usize {
    expected
        .iter()
        .zip(actual.iter())
        .filter(|(expected, actual)| expected != actual)
        .count()
        + expected.len().abs_diff(actual.len())
}
