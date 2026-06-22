use clip_graph::RenderNodeKind;
use clip_model::LayerOpacity;

use crate::blend::{gpu_raster_blend_mode, strict_raster_blend_mode};
use crate::gpu_provider::{GpuResourcePlan, plan_gpu_mask_resource};
use crate::results::{SimpleRasterStackUnsupported, SimpleRasterStackUnsupportedReason};
use crate::stack_plan::{
    ClipBaseState, StrictRasterStackDraw, StrictRasterStackOptions, apply_planned_gpu_mask,
    can_elide_initial_terminal_container, opacity_factor,
};
use crate::{ClipSession, LAYER_COMPOSITE_THROUGH, RuntimeError};

impl ClipSession {
    pub(super) fn collect_gpu_sources_in_range(
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
                        if options.allow_clipping_runs && !node.clip {
                            let (clipped, next_index) = self.collect_gpu_clipped_siblings(
                                subtree_end,
                                node.depth,
                                end,
                                options,
                                unsupported,
                                resource_plan,
                            )?;
                            if !clipped.is_empty() {
                                if let clip_gpu::GpuNormalStackSource::Container {
                                    children,
                                    opacity,
                                    mask_key,
                                    blend_mode,
                                } = container
                                {
                                    has_drawn_output = true;
                                    sources.push(
                                        clip_gpu::GpuNormalStackSource::ContainerClippingRun {
                                            children,
                                            opacity,
                                            mask_key,
                                            blend_mode,
                                            clipped,
                                        },
                                    );
                                    clip_base_state = ClipBaseState::Cleared;
                                    index = next_index;
                                    continue;
                                }
                            }
                            if next_index > subtree_end {
                                has_drawn_output = true;
                                sources.push(container);
                                clip_base_state = ClipBaseState::Cleared;
                                index = next_index;
                                continue;
                            }
                        }
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
                RenderNodeKind::Text => {
                    let Some(raster) =
                        self.plan_gpu_text_source(node, options, unsupported, resource_plan)?
                    else {
                        clip_base_state = ClipBaseState::Blocked;
                        index += 1;
                        continue;
                    };

                    if !options.allow_alpha_compositing && has_drawn_output {
                        unsupported.push(SimpleRasterStackUnsupported {
                            render_node_id: node.id,
                            layer_id: node.layer_id,
                            kind: node.kind,
                            reason: SimpleRasterStackUnsupportedReason::RequiresAlphaCompositing,
                        });
                        index += 1;
                        continue;
                    }

                    has_drawn_output = true;
                    clip_base_state = ClipBaseState::Available;
                    sources.push(clip_gpu::GpuNormalStackSource::Raster(raster));
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
}
