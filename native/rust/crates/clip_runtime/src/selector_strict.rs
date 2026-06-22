use clip_graph::RenderNodeKind;
use clip_model::LayerOpacity;

use crate::blend::strict_raster_blend_mode;
use crate::results::{SimpleRasterStackUnsupported, SimpleRasterStackUnsupportedReason};
use crate::stack_plan::{
    ClipBaseState, PlannedClippingRun, PlannedContainerStack, PlannedDecodedMask,
    PlannedThroughGroup, StrictRasterStackDraw, StrictRasterStackOptions, alpha_is_fully_opaque,
    can_elide_initial_terminal_container, opacity_factor,
};
use crate::{ClipSession, LAYER_COMPOSITE_THROUGH, RuntimeError};

impl ClipSession {
    pub(super) fn collect_strict_draws_in_range(
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
                RenderNodeKind::Text => {
                    unsupported.push(SimpleRasterStackUnsupported {
                        render_node_id: node.id,
                        layer_id: node.layer_id,
                        kind: node.kind,
                        reason: SimpleRasterStackUnsupportedReason::Text,
                    });
                    clip_base_state = ClipBaseState::Blocked;
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

    pub(super) fn collect_strict_paper_draw(
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
}
