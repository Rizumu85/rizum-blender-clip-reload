use clip_graph::RenderNodeKind;
use clip_model::LayerOpacity;

use crate::blend::strict_raster_blend_mode;
use crate::filter_lut::lut_filter_rgba;
use crate::results::{SimpleRasterStackUnsupported, SimpleRasterStackUnsupportedReason};
use crate::stack_plan::{
    PlannedDecodedMask, PlannedDecodedRaster, PlannedLutFilter, StrictRasterStackOptions,
    opacity_factor,
};
use crate::{ClipSession, RuntimeError};

impl ClipSession {
    pub(super) fn decode_strict_normal_raster_node(
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

    pub(super) fn decode_strict_lut_filter_node(
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

    pub(super) fn collect_strict_clipped_siblings(
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
                RenderNodeKind::Text => {
                    unsupported.push(SimpleRasterStackUnsupported {
                        render_node_id: node.id,
                        layer_id: node.layer_id,
                        kind: node.kind,
                        reason: SimpleRasterStackUnsupportedReason::Text,
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
}
