use crate::gpu_provider::GpuResourcePlan;
use crate::results::{SimpleRasterStackUnsupported, SimpleRasterStackUnsupportedReason};
use crate::stack_plan::{
    GpuRenderStackSelection, StrictRasterStackOptions, StrictRasterStackSelection,
};
use crate::{ClipSession, RuntimeError};

impl ClipSession {
    pub(super) fn select_strict_normal_raster_stack(
        &self,
        options: StrictRasterStackOptions,
    ) -> Result<StrictRasterStackSelection, RuntimeError> {
        let mut unsupported = Vec::new();
        let draws = if self.render_plan.nodes.first().map(|node| node.layer_id)
            == Some(self.summary.root_layer_id)
        {
            let root_end = self.subtree_end(0);
            self.collect_strict_draws_in_range(1, root_end, 1, options, &mut unsupported)?
        } else {
            self.collect_strict_draws_in_range(
                0,
                self.render_plan.nodes.len(),
                0,
                options,
                &mut unsupported,
            )?
        };

        Ok(StrictRasterStackSelection { draws, unsupported })
    }

    pub(super) fn select_gpu_normal_render_stack(
        &self,
        options: StrictRasterStackOptions,
    ) -> Result<GpuRenderStackSelection, RuntimeError> {
        let mut unsupported = Vec::new();
        let mut resource_plan = GpuResourcePlan::default();
        let sources = if self.render_plan.nodes.first().map(|node| node.layer_id)
            == Some(self.summary.root_layer_id)
        {
            let root_end = self.subtree_end(0);
            self.collect_gpu_sources_in_range(
                1,
                root_end,
                1,
                options,
                &mut unsupported,
                &mut resource_plan,
            )?
        } else {
            self.collect_gpu_sources_in_range(
                0,
                self.render_plan.nodes.len(),
                0,
                options,
                &mut unsupported,
                &mut resource_plan,
            )?
        };

        Ok(GpuRenderStackSelection {
            sources,
            resource_plan,
            unsupported,
        })
    }

    pub(super) fn push_unsupported_subtree(
        &self,
        index: usize,
        subtree_end: usize,
        reason: SimpleRasterStackUnsupportedReason,
        unsupported: &mut Vec<SimpleRasterStackUnsupported>,
    ) {
        let node = &self.render_plan.nodes[index];
        unsupported.push(SimpleRasterStackUnsupported {
            render_node_id: node.id,
            layer_id: node.layer_id,
            kind: node.kind,
            reason,
        });
        for child in &self.render_plan.nodes[index + 1..subtree_end] {
            unsupported.push(SimpleRasterStackUnsupported {
                render_node_id: child.id,
                layer_id: child.layer_id,
                kind: child.kind,
                reason: SimpleRasterStackUnsupportedReason::InsideUnsupportedContainer,
            });
        }
    }

    pub(super) fn subtree_end(&self, index: usize) -> usize {
        let depth = self.render_plan.nodes[index].depth;
        let mut end = index + 1;
        while end < self.render_plan.nodes.len() && self.render_plan.nodes[end].depth > depth {
            end += 1;
        }
        end
    }
}
