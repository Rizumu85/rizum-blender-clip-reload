use clip_graph::RenderNodeKind;
use clip_model::LayerId;

use crate::blend::strict_raster_blend_mode;
use crate::filter_lut::lut_filter_rgba;
use crate::stack_plan::{StrictRasterStackOptions, opacity_factor};
use crate::{
    ClipSession, LAYER_COMPOSITE_THROUGH, NormalRasterStackResourceStats, RuntimeError,
    SimpleRasterStackUnsupported, SimpleRasterStackUnsupportedReason,
};

use super::unsupported_node;

impl ClipSession {
    pub(super) fn check_strict_container_support(
        &self,
        sqlite_bytes: &[u8],
        index: usize,
        subtree_end: usize,
        options: StrictRasterStackOptions,
        resource_stats: &mut NormalRasterStackResourceStats,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
    ) -> Result<bool, RuntimeError> {
        let node = &self.render_plan.nodes[index];
        if !options.allow_container_isolation {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::ContainerSemantics,
                unsupported,
            );
            return Ok(false);
        }
        if node.clip {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::ContainerSemantics,
                unsupported,
            );
            return Ok(false);
        }
        if strict_raster_blend_mode(node, options, false).is_none() {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::Composite(node.composite),
                unsupported,
            );
            return Ok(false);
        }
        if !options.allow_layer_opacity && node.opacity != clip_model::LayerOpacity::MAX {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::Opacity(node.opacity.0),
                unsupported,
            );
            return Ok(false);
        }
        if opacity_factor(node.opacity).is_none() {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::OpacityOutOfRange(node.opacity.0),
                unsupported,
            );
            return Ok(false);
        }
        if node.mask_mipmap_id.is_some() && !options.allow_masks {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::Mask,
                unsupported,
            );
            return Ok(false);
        }
        if node.mask_mipmap_id.is_some() {
            let mask = self.check_mask_metadata(node.layer_id)?;
            resource_stats.add_mask_source(mask);
        }

        let child_count = self.collect_strict_support_in_range(
            sqlite_bytes,
            index + 1,
            subtree_end,
            node.depth + 1,
            options,
            resource_stats,
            unsupported,
        )?;
        Ok(child_count > 0)
    }

    pub(super) fn check_strict_through_group_support(
        &self,
        sqlite_bytes: &[u8],
        index: usize,
        subtree_end: usize,
        options: StrictRasterStackOptions,
        resource_stats: &mut NormalRasterStackResourceStats,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
    ) -> Result<bool, RuntimeError> {
        let node = &self.render_plan.nodes[index];
        if !options.allow_through_groups {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::ContainerSemantics,
                unsupported,
            );
            return Ok(false);
        }
        if node.clip || node.composite != LAYER_COMPOSITE_THROUGH {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::ContainerSemantics,
                unsupported,
            );
            return Ok(false);
        }
        if !options.allow_layer_opacity && node.opacity != clip_model::LayerOpacity::MAX {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::Opacity(node.opacity.0),
                unsupported,
            );
            return Ok(false);
        }
        if opacity_factor(node.opacity).is_none() {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::OpacityOutOfRange(node.opacity.0),
                unsupported,
            );
            return Ok(false);
        }
        if node.mask_mipmap_id.is_some() && !options.allow_masks {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::Mask,
                unsupported,
            );
            return Ok(false);
        }
        if node.mask_mipmap_id.is_some() {
            let mask = self.check_mask_metadata(node.layer_id)?;
            resource_stats.add_mask_source(mask);
        }

        let child_count = self.collect_strict_support_in_range(
            sqlite_bytes,
            index + 1,
            subtree_end,
            node.depth + 1,
            options,
            resource_stats,
            unsupported,
        )?;
        Ok(child_count > 0)
    }

    pub(super) fn check_strict_raster_node_support(
        &self,
        node: &clip_graph::RenderNode,
        options: StrictRasterStackOptions,
        allow_clip_flag: bool,
        resource_stats: &mut NormalRasterStackResourceStats,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
    ) -> Result<bool, RuntimeError> {
        if node.clip && !allow_clip_flag {
            unsupported.push(unsupported_node(
                node.id,
                node.layer_id,
                node.kind,
                SimpleRasterStackUnsupportedReason::Clipping,
            ));
            return Ok(false);
        }
        if strict_raster_blend_mode(node, options, allow_clip_flag).is_none() {
            unsupported.push(unsupported_node(
                node.id,
                node.layer_id,
                node.kind,
                SimpleRasterStackUnsupportedReason::Composite(node.composite),
            ));
            return Ok(false);
        }
        if !options.allow_layer_opacity && node.opacity != clip_model::LayerOpacity::MAX {
            unsupported.push(unsupported_node(
                node.id,
                node.layer_id,
                node.kind,
                SimpleRasterStackUnsupportedReason::Opacity(node.opacity.0),
            ));
            return Ok(false);
        }
        if opacity_factor(node.opacity).is_none() {
            unsupported.push(unsupported_node(
                node.id,
                node.layer_id,
                node.kind,
                SimpleRasterStackUnsupportedReason::OpacityOutOfRange(node.opacity.0),
            ));
            return Ok(false);
        }
        if node.mask_mipmap_id.is_some() && !options.allow_masks {
            unsupported.push(unsupported_node(
                node.id,
                node.layer_id,
                node.kind,
                SimpleRasterStackUnsupportedReason::Mask,
            ));
            return Ok(false);
        }

        node.render_mipmap_id
            .ok_or(RuntimeError::MissingRasterRenderMipmap {
                layer_id: node.layer_id,
            })?;
        let source = self
            .raster_sources
            .get(&node.layer_id)
            .ok_or(clip_file::ClipFileError::MissingLayer(node.layer_id))?;
        if !matches!(source.color_type.unwrap_or(0), 0 | 1 | 2) {
            unsupported.push(unsupported_node(
                node.id,
                node.layer_id,
                node.kind,
                SimpleRasterStackUnsupportedReason::RasterColorType(source.color_type),
            ));
            return Ok(false);
        }
        resource_stats.add_raster_source(source);
        if node.mask_mipmap_id.is_some() {
            let mask = self.check_mask_metadata(node.layer_id)?;
            resource_stats.add_mask_source(mask);
        }

        Ok(true)
    }

    pub(super) fn check_strict_text_node_support(
        &self,
        node: &clip_graph::RenderNode,
        options: StrictRasterStackOptions,
        resource_stats: &mut NormalRasterStackResourceStats,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
    ) -> Result<bool, RuntimeError> {
        if node.clip {
            unsupported.push(unsupported_node(
                node.id,
                node.layer_id,
                node.kind,
                SimpleRasterStackUnsupportedReason::Clipping,
            ));
            return Ok(false);
        }
        if strict_raster_blend_mode(node, options, false).is_none() {
            unsupported.push(unsupported_node(
                node.id,
                node.layer_id,
                node.kind,
                SimpleRasterStackUnsupportedReason::Composite(node.composite),
            ));
            return Ok(false);
        }
        if !options.allow_layer_opacity && node.opacity != clip_model::LayerOpacity::MAX {
            unsupported.push(unsupported_node(
                node.id,
                node.layer_id,
                node.kind,
                SimpleRasterStackUnsupportedReason::Opacity(node.opacity.0),
            ));
            return Ok(false);
        }
        if opacity_factor(node.opacity).is_none() {
            unsupported.push(unsupported_node(
                node.id,
                node.layer_id,
                node.kind,
                SimpleRasterStackUnsupportedReason::OpacityOutOfRange(node.opacity.0),
            ));
            return Ok(false);
        }
        if node.mask_mipmap_id.is_some() {
            unsupported.push(unsupported_node(
                node.id,
                node.layer_id,
                node.kind,
                SimpleRasterStackUnsupportedReason::Mask,
            ));
            return Ok(false);
        }

        let source = self
            .text_sources
            .get(&node.layer_id)
            .ok_or(clip_file::ClipFileError::MissingLayer(node.layer_id))?;
        let layout = crate::text_render::measure_text_source(source, self.summary.canvas)?;
        if layout.size.width == 0 || layout.size.height == 0 {
            return Ok(false);
        }
        resource_stats.add_text_source(node.layer_id, layout.size);
        Ok(true)
    }

    pub(super) fn check_strict_lut_filter_support(
        &self,
        node: &clip_graph::RenderNode,
        options: StrictRasterStackOptions,
        resource_stats: &mut NormalRasterStackResourceStats,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
    ) -> Result<bool, RuntimeError> {
        if !options.allow_lut_filters {
            unsupported.push(unsupported_node(
                node.id,
                node.layer_id,
                node.kind,
                SimpleRasterStackUnsupportedReason::Filter,
            ));
            return Ok(false);
        }
        if node.clip {
            unsupported.push(unsupported_node(
                node.id,
                node.layer_id,
                node.kind,
                SimpleRasterStackUnsupportedReason::Clipping,
            ));
            return Ok(false);
        }
        if node.composite != 0 {
            unsupported.push(unsupported_node(
                node.id,
                node.layer_id,
                node.kind,
                SimpleRasterStackUnsupportedReason::Composite(node.composite),
            ));
            return Ok(false);
        }
        if opacity_factor(node.opacity).is_none() {
            unsupported.push(unsupported_node(
                node.id,
                node.layer_id,
                node.kind,
                SimpleRasterStackUnsupportedReason::OpacityOutOfRange(node.opacity.0),
            ));
            return Ok(false);
        }
        if node.mask_mipmap_id.is_some() && !options.allow_masks {
            unsupported.push(unsupported_node(
                node.id,
                node.layer_id,
                node.kind,
                SimpleRasterStackUnsupportedReason::Mask,
            ));
            return Ok(false);
        }

        let filter = self.filter_sources.get(&node.layer_id).ok_or(
            clip_file::ClipFileError::LayerHasNoFilterInfo {
                layer_id: node.layer_id,
            },
        )?;
        if lut_filter_rgba(filter.filter_type, &filter.payload).is_none() {
            unsupported.push(unsupported_node(
                node.id,
                node.layer_id,
                node.kind,
                SimpleRasterStackUnsupportedReason::Filter,
            ));
            return Ok(false);
        }
        if node.mask_mipmap_id.is_some() {
            let mask = self.check_mask_metadata(node.layer_id)?;
            resource_stats.add_mask_source(mask);
        }

        Ok(true)
    }

    pub(super) fn collect_strict_clipped_support(
        &self,
        sqlite_bytes: &[u8],
        mut index: usize,
        base_depth: u16,
        end: usize,
        options: StrictRasterStackOptions,
        resource_stats: &mut NormalRasterStackResourceStats,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
    ) -> Result<(usize, usize), RuntimeError> {
        let mut clipped_count = 0;
        while index < end {
            let node = &self.render_plan.nodes[index];
            if node.depth != base_depth || !node.clip {
                break;
            }
            let subtree_end = self.subtree_end(index).min(end);
            match node.kind {
                RenderNodeKind::Raster => {
                    if self.check_strict_raster_node_support(
                        node,
                        options,
                        true,
                        resource_stats,
                        unsupported,
                    )? {
                        clipped_count += 1;
                    }
                    index += 1;
                }
                RenderNodeKind::Container => {
                    if self.check_strict_clipped_container_support(
                        sqlite_bytes,
                        index,
                        subtree_end,
                        options,
                        resource_stats,
                        unsupported,
                    )? {
                        clipped_count += 1;
                    }
                    index = subtree_end;
                }
                RenderNodeKind::Paper => {
                    unsupported.push(unsupported_node(
                        node.id,
                        node.layer_id,
                        node.kind,
                        SimpleRasterStackUnsupportedReason::PaperSemantics,
                    ));
                    index += 1;
                }
                RenderNodeKind::Filter => {
                    unsupported.push(unsupported_node(
                        node.id,
                        node.layer_id,
                        node.kind,
                        SimpleRasterStackUnsupportedReason::Filter,
                    ));
                    index += 1;
                }
                RenderNodeKind::Text => {
                    unsupported.push(unsupported_node(
                        node.id,
                        node.layer_id,
                        node.kind,
                        SimpleRasterStackUnsupportedReason::Clipping,
                    ));
                    index += 1;
                }
                RenderNodeKind::Unsupported(raw_kind) => {
                    unsupported.push(unsupported_node(
                        node.id,
                        node.layer_id,
                        node.kind,
                        SimpleRasterStackUnsupportedReason::UnsupportedLayerKind(raw_kind),
                    ));
                    index += 1;
                }
            }
        }
        Ok((clipped_count, index))
    }

    pub(super) fn check_strict_clipped_container_support(
        &self,
        sqlite_bytes: &[u8],
        index: usize,
        subtree_end: usize,
        options: StrictRasterStackOptions,
        resource_stats: &mut NormalRasterStackResourceStats,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
    ) -> Result<bool, RuntimeError> {
        let node = &self.render_plan.nodes[index];
        if !options.allow_container_isolation {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::ContainerSemantics,
                unsupported,
            );
            return Ok(false);
        }
        if node.composite == LAYER_COMPOSITE_THROUGH {
            if !options.allow_through_groups {
                self.push_unsupported_subtree(
                    index,
                    subtree_end,
                    SimpleRasterStackUnsupportedReason::ContainerSemantics,
                    unsupported,
                );
                return Ok(false);
            }
        } else if strict_raster_blend_mode(node, options, true).is_none() {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::Composite(node.composite),
                unsupported,
            );
            return Ok(false);
        }
        if !options.allow_layer_opacity && node.opacity != clip_model::LayerOpacity::MAX {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::Opacity(node.opacity.0),
                unsupported,
            );
            return Ok(false);
        }
        if opacity_factor(node.opacity).is_none() {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::OpacityOutOfRange(node.opacity.0),
                unsupported,
            );
            return Ok(false);
        }
        if node.mask_mipmap_id.is_some() && !options.allow_masks {
            self.push_unsupported_subtree(
                index,
                subtree_end,
                SimpleRasterStackUnsupportedReason::Mask,
                unsupported,
            );
            return Ok(false);
        }
        if node.mask_mipmap_id.is_some() {
            let mask = self.check_mask_metadata(node.layer_id)?;
            resource_stats.add_mask_source(mask);
        }

        let child_count = self.collect_strict_support_in_range(
            sqlite_bytes,
            index + 1,
            subtree_end,
            node.depth + 1,
            options,
            resource_stats,
            unsupported,
        )?;
        Ok(child_count > 0)
    }

    pub(super) fn check_mask_metadata(
        &self,
        layer_id: LayerId,
    ) -> Result<&clip_file::metadata::MaskLayerSource, RuntimeError> {
        let source = self
            .mask_sources
            .get(&layer_id)
            .ok_or(clip_file::ClipFileError::LayerHasNoMask { layer_id })?;
        Ok(source)
    }
}
