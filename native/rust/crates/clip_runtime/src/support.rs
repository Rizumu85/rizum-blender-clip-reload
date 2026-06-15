use clip_graph::{RenderNodeId, RenderNodeKind};
use clip_model::LayerId;

use super::{
    ClipBaseState, ClipSession, LAYER_COMPOSITE_THROUGH, NormalRasterStackResourceStats,
    NormalRasterStackSupportResult, RuntimeError, SimpleRasterStackUnsupported,
    SimpleRasterStackUnsupportedReason, StrictRasterStackOptions, lut_filter_rgba, opacity_factor,
    strict_raster_blend_mode,
};

#[derive(Debug)]
struct StrictRasterStackSupportSelection {
    source_count: usize,
    resource_stats: NormalRasterStackResourceStats,
    unsupported: Vec<SimpleRasterStackUnsupported>,
}

impl NormalRasterStackResourceStats {
    fn add_raster_source(&mut self, source: &clip_file::metadata::RasterLayerSource) {
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

    fn add_mask_source(&mut self, source: &clip_file::metadata::MaskLayerSource) {
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

    fn select_strict_normal_raster_stack_support(
        &self,
        sqlite_bytes: &[u8],
        options: StrictRasterStackOptions,
    ) -> Result<StrictRasterStackSupportSelection, RuntimeError> {
        let mut unsupported = Vec::new();
        let mut resource_stats = NormalRasterStackResourceStats::default();
        let source_count = if self.render_plan.nodes.first().map(|node| node.layer_id)
            == Some(self.summary.root_layer_id)
        {
            let root_end = self.subtree_end(0);
            self.collect_strict_support_in_range(
                sqlite_bytes,
                1,
                root_end,
                1,
                options,
                &mut resource_stats,
                &mut unsupported,
            )?
        } else {
            self.collect_strict_support_in_range(
                sqlite_bytes,
                0,
                self.render_plan.nodes.len(),
                0,
                options,
                &mut resource_stats,
                &mut unsupported,
            )?
        };

        Ok(StrictRasterStackSupportSelection {
            source_count,
            resource_stats,
            unsupported,
        })
    }

    fn collect_strict_support_in_range(
        &self,
        sqlite_bytes: &[u8],
        start: usize,
        end: usize,
        depth: u16,
        options: StrictRasterStackOptions,
        resource_stats: &mut NormalRasterStackResourceStats,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
    ) -> Result<usize, RuntimeError> {
        let mut source_count = 0;
        let mut clip_base_state = ClipBaseState::Cleared;
        let mut index = start;

        while index < end {
            let node = &self.render_plan.nodes[index];
            if node.depth < depth {
                break;
            }
            if node.depth > depth {
                unsupported.push(unsupported_node(
                    node.id,
                    node.layer_id,
                    node.kind,
                    SimpleRasterStackUnsupportedReason::InsideUnsupportedContainer,
                ));
                clip_base_state = ClipBaseState::Blocked;
                index += 1;
                continue;
            }

            match node.kind {
                RenderNodeKind::Container => {
                    let subtree_end = self.subtree_end(index);
                    let has_supported_children = if node.composite == LAYER_COMPOSITE_THROUGH {
                        self.check_strict_through_group_support(
                            sqlite_bytes,
                            index,
                            subtree_end,
                            options,
                            resource_stats,
                            unsupported,
                        )?
                    } else {
                        self.check_strict_container_support(
                            sqlite_bytes,
                            index,
                            subtree_end,
                            options,
                            resource_stats,
                            unsupported,
                        )?
                    };
                    if has_supported_children {
                        source_count += 1;
                        clip_base_state = if node.composite == LAYER_COMPOSITE_THROUGH {
                            ClipBaseState::Cleared
                        } else {
                            ClipBaseState::Available
                        };
                    } else {
                        clip_base_state = ClipBaseState::Blocked;
                    }
                    index = subtree_end;
                }
                RenderNodeKind::Paper => {
                    if self
                        .collect_strict_paper_draw(node, options, unsupported)
                        .is_some()
                    {
                        source_count += 1;
                        clip_base_state = ClipBaseState::Available;
                    } else {
                        clip_base_state = ClipBaseState::Blocked;
                    }
                    index += 1;
                }
                RenderNodeKind::Raster => {
                    let orphan_clipped = node.clip && clip_base_state == ClipBaseState::Cleared;
                    let supported = self.check_strict_raster_node_support(
                        node,
                        options,
                        orphan_clipped,
                        resource_stats,
                        unsupported,
                    )?;
                    if !supported {
                        clip_base_state = ClipBaseState::Blocked;
                        index += 1;
                        continue;
                    }

                    if options.allow_clipping_runs && !node.clip {
                        let (_clipped_count, next_index) = self.collect_strict_clipped_support(
                            index + 1,
                            node.depth,
                            end,
                            options,
                            resource_stats,
                            unsupported,
                        )?;
                        source_count += 1;
                        clip_base_state = ClipBaseState::Cleared;
                        index = next_index;
                    } else {
                        source_count += 1;
                        clip_base_state = if node.clip {
                            ClipBaseState::Cleared
                        } else {
                            ClipBaseState::Available
                        };
                        index += 1;
                    }
                }
                RenderNodeKind::Filter => {
                    if self.check_strict_lut_filter_support(
                        node,
                        options,
                        resource_stats,
                        unsupported,
                    )? {
                        source_count += 1;
                        clip_base_state = ClipBaseState::Cleared;
                    } else {
                        clip_base_state = ClipBaseState::Blocked;
                    }
                    index += 1;
                }
                RenderNodeKind::Unsupported(raw_kind) => {
                    unsupported.push(unsupported_node(
                        node.id,
                        node.layer_id,
                        node.kind,
                        SimpleRasterStackUnsupportedReason::UnsupportedLayerKind(raw_kind),
                    ));
                    clip_base_state = ClipBaseState::Blocked;
                    index += 1;
                }
            }
        }

        Ok(source_count)
    }

    fn check_strict_container_support(
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
        if super::strict_raster_blend_mode(node, options, false).is_none() {
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

    fn check_strict_through_group_support(
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

    fn check_strict_raster_node_support(
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

    fn check_strict_lut_filter_support(
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

    fn collect_strict_clipped_support(
        &self,
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
                    self.push_unsupported_subtree(
                        index,
                        subtree_end,
                        SimpleRasterStackUnsupportedReason::ContainerSemantics,
                        unsupported,
                    );
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

    fn check_mask_metadata(
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
