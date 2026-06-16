#[path = "support_checks.rs"]
mod support_checks;
#[path = "support_select.rs"]
mod support_select;

use clip_graph::{RenderNodeId, RenderNodeKind};
use clip_model::LayerId;

use crate::stack_plan::StrictRasterStackOptions;

use super::{
    ClipSession, NormalRasterStackResourceStats, NormalRasterStackSupportResult, RuntimeError,
    SimpleRasterStackUnsupported, SimpleRasterStackUnsupportedReason,
};

#[derive(Debug)]
pub(super) struct StrictRasterStackSupportSelection {
    pub(super) source_count: usize,
    pub(super) resource_stats: NormalRasterStackResourceStats,
    pub(super) unsupported: Vec<SimpleRasterStackUnsupported>,
}

impl NormalRasterStackResourceStats {
    pub(super) fn add_raster_source(&mut self, source: &clip_file::metadata::RasterLayerSource) {
        let bytes = u64::from(source.pixel_size.width) * u64::from(source.pixel_size.height) * 4;
        self.raster_count += 1;
        self.raster_bytes += bytes;
        if bytes > self.max_raster_bytes {
            self.max_raster_bytes = bytes;
            self.max_raster_layer_id = Some(source.layer.id);
            self.max_raster_width = source.pixel_size.width;
            self.max_raster_height = source.pixel_size.height;
        }
    }

    pub(super) fn add_mask_source(&mut self, source: &clip_file::metadata::MaskLayerSource) {
        let bytes = u64::from(source.pixel_size.width) * u64::from(source.pixel_size.height);
        self.mask_count += 1;
        self.mask_bytes += bytes;
        if bytes > self.max_mask_bytes {
            self.max_mask_bytes = bytes;
            self.max_mask_layer_id = Some(source.layer_id);
            self.max_mask_width = source.pixel_size.width;
            self.max_mask_height = source.pixel_size.height;
        }
    }
}

impl ClipSession {
    pub fn check_normal_raster_stack_support(
        &self,
    ) -> Result<NormalRasterStackSupportResult, RuntimeError> {
        let selection = self.select_strict_normal_raster_stack_support(
            self.container.sqlite_bytes(),
            support_options(),
        )?;
        Ok(NormalRasterStackSupportResult {
            source_count: selection.source_count,
            resource_stats: selection.resource_stats,
            unsupported: selection.unsupported,
        })
    }
}

fn support_options() -> StrictRasterStackOptions {
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

fn unsupported_node(
    render_node_id: RenderNodeId,
    layer_id: LayerId,
    kind: RenderNodeKind,
    reason: SimpleRasterStackUnsupportedReason,
) -> SimpleRasterStackUnsupported {
    SimpleRasterStackUnsupported {
        render_node_id,
        layer_id,
        kind,
        reason,
    }
}
