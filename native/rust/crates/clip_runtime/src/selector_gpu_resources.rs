use clip_graph::RenderNodeKind;
use clip_model::LayerOpacity;

use crate::blend::{gpu_raster_blend_mode, strict_raster_blend_mode};
use crate::filter_lut::lut_filter_rgba;
use crate::gpu_provider::{GpuResourcePlan, plan_gpu_mask_resource};
use crate::results::{SimpleRasterStackUnsupported, SimpleRasterStackUnsupportedReason};
use crate::stack_plan::{
    StrictRasterStackOptions, apply_planned_gpu_mask, gpu_lut_filter_mode, opacity_factor,
};
use crate::{ClipSession, LAYER_COMPOSITE_THROUGH, RuntimeError};

impl ClipSession {
    pub(super) fn plan_gpu_raster_source(
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

    pub(super) fn plan_gpu_text_source(
        &self,
        node: &clip_graph::RenderNode,
        options: StrictRasterStackOptions,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
        resource_plan: &mut GpuResourcePlan,
    ) -> Result<Option<clip_gpu::GpuNormalRasterSource>, RuntimeError> {
        if node.clip {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Clipping,
            });
            return Ok(None);
        }
        let Some(blend_mode) = strict_raster_blend_mode(node, options, false) else {
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
        if node.mask_mipmap_id.is_some() {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: node.id,
                layer_id: node.layer_id,
                kind: node.kind,
                reason: SimpleRasterStackUnsupportedReason::Mask,
            });
            return Ok(None);
        }

        let render_mipmap_id = node.render_mipmap_id.unwrap_or(node.layer_id.0);
        let source = self
            .text_sources
            .get(&node.layer_id)
            .cloned()
            .ok_or(clip_file::ClipFileError::MissingLayer(node.layer_id))?;
        let layout = crate::text_render::measure_text_source(&source, self.summary.canvas)?;
        if layout.size.width == 0 || layout.size.height == 0 {
            return Ok(None);
        }
        let key = clip_gpu::GpuRasterResourceKey {
            layer_id: node.layer_id,
            render_mipmap_id,
        };
        resource_plan.insert_text(
            key,
            node.id,
            node.layer_id,
            render_mipmap_id,
            source,
            layout,
        );
        let (mask_key, opacity) = apply_planned_gpu_mask(
            plan_gpu_mask_resource(&self.mask_sources, node, self.summary.canvas, resource_plan)?,
            opacity,
        );
        Ok(Some(clip_gpu::GpuNormalRasterSource {
            key,
            opacity,
            mask_key,
            offset_x: layout.offset_x,
            offset_y: layout.offset_y,
            blend_mode: gpu_raster_blend_mode(blend_mode),
        }))
    }

    pub(super) fn plan_gpu_lut_filter_source(
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

    pub(super) fn collect_gpu_clipped_siblings(
        &self,
        mut index: usize,
        base_depth: u16,
        end: usize,
        options: StrictRasterStackOptions,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
        resource_plan: &mut GpuResourcePlan,
    ) -> Result<(Vec<clip_gpu::GpuClippedStackSource>, usize), RuntimeError> {
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
                        clipped.push(clip_gpu::GpuClippedStackSource::Raster(raster));
                    }
                    index += 1;
                }
                RenderNodeKind::Container => {
                    if let Some(container) = self.collect_gpu_clipped_container_source(
                        index,
                        subtree_end,
                        options,
                        unsupported,
                        resource_plan,
                    )? {
                        clipped.push(container);
                    }
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
                RenderNodeKind::Text => {
                    unsupported.push(SimpleRasterStackUnsupported {
                        render_node_id: node.id,
                        layer_id: node.layer_id,
                        kind: node.kind,
                        reason: SimpleRasterStackUnsupportedReason::Clipping,
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

    pub(super) fn collect_gpu_clipped_container_source(
        &self,
        index: usize,
        subtree_end: usize,
        options: StrictRasterStackOptions,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
        resource_plan: &mut GpuResourcePlan,
    ) -> Result<Option<clip_gpu::GpuClippedStackSource>, RuntimeError> {
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
        let blend_mode = if node.composite == LAYER_COMPOSITE_THROUGH {
            if !options.allow_through_groups {
                self.push_unsupported_subtree(
                    index,
                    subtree_end,
                    SimpleRasterStackUnsupportedReason::ContainerSemantics,
                    unsupported,
                );
                return Ok(None);
            }
            clip_gpu::GpuRasterBlendMode::Normal
        } else {
            let Some(blend_mode) = strict_raster_blend_mode(node, options, true) else {
                self.push_unsupported_subtree(
                    index,
                    subtree_end,
                    SimpleRasterStackUnsupportedReason::Composite(node.composite),
                    unsupported,
                );
                return Ok(None);
            };
            gpu_raster_blend_mode(blend_mode)
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
        Ok(Some(clip_gpu::GpuClippedStackSource::Container {
            layer_id: node.layer_id,
            children,
            opacity,
            mask_key,
            blend_mode,
        }))
    }
}
