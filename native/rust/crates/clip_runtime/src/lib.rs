#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use clip_file::ClipFileSummary;
use clip_graph::{LayerGraphInput, RenderNodeKind, RenderPlan};
use clip_model::LayerId;

mod blend;
mod error;
mod filter_lut;
mod gpu_api;
mod gpu_provider;
mod performance_plan;
mod region;
mod reload_diff;
mod results;
mod selector_gpu;
mod selector_gpu_resources;
mod selector_strict;
mod selector_strict_decode;
mod selector_tree;
mod source_crop;
mod stack_plan;
mod support;
mod tile_silo_estimate;
mod tile_silo_occupancy;
mod tile_silo_options;

pub use error::RuntimeError;
pub use gpu_api::RuntimeGpuRenderer;
pub use reload_diff::{
    ReloadDiffManifest, ReloadDiffMode, ReloadDiffNode, ReloadDiffPlan, ReloadDiffSegment,
    ReloadDiffSegmentResource, ReloadDiffSegmentTileRef, ReloadDiffSource, ReloadDiffTile,
    ReloadDirtySegment, ReloadDirtySegmentEventRange, ReloadPatchRect,
};
pub use results::{
    DrawRasterLayerGpuResult, GpuSparseAtlasCacheStats, GpuSparseAtlasEventRange,
    GpuSparseAtlasReloadPlan, GpuSparseAtlasRerunSegment, GpuSparseAtlasUpdatedSlot,
    GpuTextureCacheStats, NativePerformancePlanResult, NativeTileSiloEstimateResult,
    NormalRasterStackGpuPatchResult, NormalRasterStackGpuResult, NormalRasterStackPixelTraceInput,
    NormalRasterStackPixelTraceResult, NormalRasterStackPixelTraceSample,
    NormalRasterStackResourceStats, NormalRasterStackSupportResult, SimpleRasterStackGpuResult,
    SimpleRasterStackUnsupported, SimpleRasterStackUnsupportedReason,
};

#[cfg(test)]
use blend::{
    LAYER_COMPOSITE_ADD, LAYER_COMPOSITE_ADD_GLOW, LAYER_COMPOSITE_BRIGHTNESS,
    LAYER_COMPOSITE_COLOR, LAYER_COMPOSITE_COLOR_BURN, LAYER_COMPOSITE_COLOR_DODGE,
    LAYER_COMPOSITE_DARKEN, LAYER_COMPOSITE_DARKER_COLOR, LAYER_COMPOSITE_DIFFERENCE,
    LAYER_COMPOSITE_DIVIDE, LAYER_COMPOSITE_EXCLUSION, LAYER_COMPOSITE_GLOW_DODGE,
    LAYER_COMPOSITE_HARD_LIGHT, LAYER_COMPOSITE_HARD_MIX, LAYER_COMPOSITE_HUE,
    LAYER_COMPOSITE_LIGHTEN, LAYER_COMPOSITE_LIGHTER_COLOR, LAYER_COMPOSITE_LINEAR_BURN,
    LAYER_COMPOSITE_LINEAR_LIGHT, LAYER_COMPOSITE_MULTIPLY, LAYER_COMPOSITE_OVERLAY,
    LAYER_COMPOSITE_PIN_LIGHT, LAYER_COMPOSITE_SATURATION, LAYER_COMPOSITE_SCREEN,
    LAYER_COMPOSITE_SOFT_LIGHT, LAYER_COMPOSITE_SUBTRACT, LAYER_COMPOSITE_VIVID_LIGHT,
    StrictRasterBlendMode, strict_raster_blend_mode,
};

#[cfg(test)]
use stack_plan::{
    StrictRasterStackDraw, StrictRasterStackOptions, alpha_is_fully_opaque, byte_diff_count,
};

const LAYER_COMPOSITE_THROUGH: u32 = 30;

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
        Self::from_container(path, container)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Self, RuntimeError> {
        let container = clip_file::container::ClipContainer::from_bytes(data)?;
        Self::from_container(PathBuf::new(), container)
    }

    fn from_container(
        path: PathBuf,
        container: clip_file::container::ClipContainer,
    ) -> Result<Self, RuntimeError> {
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

    pub fn layer_name(&self, layer_id: LayerId) -> Option<&str> {
        self.render_plan
            .nodes
            .iter()
            .find(|node| node.layer_id == layer_id)
            .map(|node| node.layer_name.as_str())
    }
}

fn layer_graph_input_from_file(record: &clip_file::metadata::LayerGraphRecord) -> LayerGraphInput {
    LayerGraphInput {
        id: record.id,
        name: record.name.clone(),
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
mod session_tests;
