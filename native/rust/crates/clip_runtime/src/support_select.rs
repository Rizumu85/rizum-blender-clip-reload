use clip_graph::RenderNodeKind;

use crate::stack_plan::{ClipBaseState, StrictRasterStackOptions};
use crate::{
    ClipSession, LAYER_COMPOSITE_THROUGH, NormalRasterStackResourceStats, RuntimeError,
    SimpleRasterStackUnsupported, SimpleRasterStackUnsupportedReason,
};

use super::{StrictRasterStackSupportSelection, unsupported_node};

impl ClipSession {
    pub(super) fn select_strict_normal_raster_stack_support(
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

    pub(super) fn collect_strict_support_in_range(
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
                        if options.allow_clipping_runs && !node.clip {
                            let (clipped_count, next_index) = self.collect_strict_clipped_support(
                                sqlite_bytes,
                                subtree_end,
                                node.depth,
                                end,
                                options,
                                resource_stats,
                                unsupported,
                            )?;
                            if next_index > subtree_end {
                                source_count += 1;
                                clip_base_state = ClipBaseState::Cleared;
                                index = next_index;
                                continue;
                            }
                            debug_assert_eq!(clipped_count, 0);
                        }
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
                            sqlite_bytes,
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
                RenderNodeKind::Text => {
                    if self.check_strict_text_node_support(
                        node,
                        options,
                        resource_stats,
                        unsupported,
                    )? {
                        source_count += 1;
                        clip_base_state = ClipBaseState::Available;
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
}
