use serde_json::json;

#[derive(Default)]
pub(crate) struct RenderTaskGraphBuilder {
    tasks: Vec<serde_json::Value>,
    next_id: u32,
}

impl RenderTaskGraphBuilder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push(
        &mut self,
        kind: &'static str,
        dependencies: &[u32],
        reason: impl Into<String>,
        estimated_cost: u64,
        outcome: TaskOutcome,
    ) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.tasks.push(json!({
            "id": id,
            "kind": kind,
            "dependencies": dependencies,
            "reason": reason.into(),
            "estimated_cost": estimated_cost,
            "executed": outcome.executed,
            "skip_fallback_reason": outcome.skip_fallback_reason,
            "actual_ms": outcome.actual_ms,
        }));
        id
    }

    pub(crate) fn into_json(self) -> serde_json::Value {
        json!({
            "tasks": self.tasks,
        })
    }
}

pub(crate) struct TaskOutcome {
    executed: bool,
    skip_fallback_reason: Option<String>,
    actual_ms: Option<u64>,
}

impl TaskOutcome {
    pub(crate) fn executed(actual_ms: u64) -> Self {
        Self {
            executed: true,
            skip_fallback_reason: None,
            actual_ms: Some(actual_ms),
        }
    }

    pub(crate) fn executed_unmeasured() -> Self {
        Self {
            executed: true,
            skip_fallback_reason: None,
            actual_ms: None,
        }
    }

    pub(crate) fn skipped(reason: impl Into<String>) -> Self {
        Self {
            executed: false,
            skip_fallback_reason: Some(reason.into()),
            actual_ms: None,
        }
    }
}

pub(crate) fn render_task_graph_no_change() -> Option<serde_json::Value> {
    clip_runtime::render_profile::enabled().then(|| {
        let mut graph = RenderTaskGraphBuilder::new();
        graph.push(
            "DecodeTile",
            &[],
            "reload diff mode is no_change",
            0,
            TaskOutcome::skipped("no dirty source tiles"),
        );
        graph.push(
            "UploadAtlasSlot",
            &[],
            "reload diff mode is no_change",
            0,
            TaskOutcome::skipped("no dirty atlas slots"),
        );
        graph.push(
            "RunSegment",
            &[],
            "reload diff mode is no_change",
            0,
            TaskOutcome::skipped("no dirty segments"),
        );
        graph.push(
            "ReadbackPatch",
            &[],
            "reload diff mode is no_change",
            0,
            TaskOutcome::skipped("no dirty output rects"),
        );
        graph.into_json()
    })
}

pub(crate) fn render_task_graph_for_patch(
    plan: &clip_runtime::ReloadDiffPlan,
    patch_renderer: &str,
    fallback_reason: Option<&str>,
    sparse_initial_ms: u64,
    sparse_reconstructed_ms: Option<u64>,
    region_fallback_ms: Option<u64>,
) -> Option<serde_json::Value> {
    clip_runtime::render_profile::enabled().then(|| {
        let mut graph = RenderTaskGraphBuilder::new();
        let dirty_source_tiles = plan
            .dirty_segments
            .iter()
            .map(|segment| u64::from(segment.dirty_tile_count))
            .sum::<u64>();
        let dirty_output_tiles = dirty_output_tile_count(plan);
        let sparse_selected = patch_renderer.starts_with("sparse_atlas");
        let decode_id = graph.push(
            "DecodeTile",
            &[],
            "prepare dirty sparse atlas tile payloads",
            dirty_source_tiles,
            if sparse_selected {
                TaskOutcome::executed_unmeasured()
            } else {
                TaskOutcome::skipped("sparse atlas segment execution was not selected")
            },
        );
        let upload_id = graph.push(
            "UploadAtlasSlot",
            &[decode_id],
            "upload inserted or changed sparse atlas slots",
            dirty_source_tiles,
            if sparse_selected {
                TaskOutcome::executed_unmeasured()
            } else {
                TaskOutcome::skipped("sparse atlas segment execution was not selected")
            },
        );
        let checkpoint_id = graph.push(
            "BuildCheckpoint",
            &[],
            "reconstruct segment-before checkpoint for sparse patch",
            dirty_output_tiles,
            if patch_renderer == "sparse_atlas_reconstructed_segments" {
                TaskOutcome::executed_unmeasured()
            } else {
                TaskOutcome::skipped("checkpoint path was not selected")
            },
        );
        let sparse_deps = if patch_renderer == "sparse_atlas_reconstructed_segments" {
            vec![upload_id, checkpoint_id]
        } else {
            vec![upload_id]
        };
        let run_id = graph.push(
            "RunSegment",
            &sparse_deps,
            "execute sparse atlas affected segment window",
            dirty_output_tiles,
            match patch_renderer {
                "sparse_atlas_initial_segments" => TaskOutcome::executed(sparse_initial_ms),
                "sparse_atlas_reconstructed_segments" => {
                    TaskOutcome::executed(sparse_reconstructed_ms.unwrap_or(0))
                }
                _ => TaskOutcome::skipped(
                    fallback_reason.unwrap_or("sparse atlas patch path was not executable"),
                ),
            },
        );
        let region_id = graph.push(
            "RegionFallback",
            &[],
            "render dirty rects with normal region renderer",
            dirty_output_tiles,
            if patch_renderer == "region" {
                TaskOutcome::executed(region_fallback_ms.unwrap_or(0))
            } else {
                TaskOutcome::skipped("sparse atlas patch renderer handled the patch")
            },
        );
        graph.push(
            "ReadbackPatch",
            &[if patch_renderer == "region" {
                region_id
            } else {
                run_id
            }],
            "read back dirty patch payload",
            dirty_output_tiles,
            TaskOutcome::executed_unmeasured(),
        );
        graph.into_json()
    })
}

pub(crate) fn render_task_graph_for_full_render(
    plan: &clip_runtime::ReloadDiffPlan,
    render_ms: u64,
    patch_renderer: Option<&str>,
) -> Option<serde_json::Value> {
    clip_runtime::render_profile::enabled().then(|| {
        let mut graph = RenderTaskGraphBuilder::new();
        let run_id = graph.push(
            "RunSegment",
            &[],
            "full native render path",
            u64::from(plan.manifest.width).saturating_mul(u64::from(plan.manifest.height)),
            TaskOutcome::executed(render_ms),
        );
        graph.push(
            "ReadbackPatch",
            &[run_id],
            "full image readback; patch extraction is CPU-side when needed",
            dirty_output_tile_count(plan),
            if patch_renderer == Some("full_render_patch_extract") {
                TaskOutcome::executed_unmeasured()
            } else {
                TaskOutcome::skipped("full render returned the complete image payload")
            },
        );
        graph.push(
            "RegionFallback",
            &[],
            "persistent RuntimeGpuRenderer was unavailable or full render was requested",
            0,
            if patch_renderer == Some("full_render_patch_extract") {
                TaskOutcome::executed_unmeasured()
            } else {
                TaskOutcome::skipped("not a patch reload fallback")
            },
        );
        graph.into_json()
    })
}

fn dirty_output_tile_count(plan: &clip_runtime::ReloadDiffPlan) -> u64 {
    const TILE_SIZE: u32 = clip_file::tiles::TILE_SIZE as u32;
    plan.dirty_rects
        .iter()
        .map(|rect| {
            let x0 = rect.x / TILE_SIZE;
            let y0 = rect.y / TILE_SIZE;
            let x1 = rect.x.saturating_add(rect.width).saturating_sub(1) / TILE_SIZE;
            let y1 = rect.y.saturating_add(rect.height).saturating_sub(1) / TILE_SIZE;
            u64::from(x1.saturating_sub(x0).saturating_add(1))
                .saturating_mul(u64::from(y1.saturating_sub(y0).saturating_add(1)))
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use clip_runtime::{
        ReloadDiffManifest, ReloadDiffMode, ReloadDiffPlan, ReloadDiffSegment, ReloadPatchRect,
    };

    use super::dirty_output_tile_count;

    #[test]
    fn dirty_output_tile_count_counts_intersected_tiles() {
        let plan = ReloadDiffPlan {
            manifest: ReloadDiffManifest {
                abi: 4,
                tile_size: clip_file::tiles::TILE_SIZE as u32,
                tile_event_abi_version: 0,
                width: 512,
                height: 512,
                root_layer_id: 1,
                nodes: Vec::new(),
                sources: Vec::new(),
                segments: Vec::<ReloadDiffSegment>::new(),
            },
            mode: ReloadDiffMode::Patch,
            reason: "test".to_string(),
            dirty_rects: vec![ReloadPatchRect {
                x: 250,
                y: 250,
                width: 20,
                height: 20,
            }],
            dirty_segments: Vec::new(),
        };

        assert_eq!(dirty_output_tile_count(&plan), 4);
    }
}
