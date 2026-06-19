#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SuffixCheckpointCandidate {
    pub(crate) source_start: u32,
    pub(crate) priority: u32,
}

pub(crate) fn suffix_starts_at_initial_accumulator(plan: &crate::ReloadDiffPlan) -> bool {
    suffix_checkpoint_candidate(plan).is_some_and(|candidate| candidate.source_start == 0)
}

pub(crate) fn suffix_checkpoint_candidate(
    plan: &crate::ReloadDiffPlan,
) -> Option<SuffixCheckpointCandidate> {
    suffix_checkpoint_segment(plan).map(|segment| SuffixCheckpointCandidate {
        source_start: segment.source_start,
        priority: segment.checkpoint_priority,
    })
}

pub(crate) fn suffix_manifest_is_raster_only(plan: &crate::ReloadDiffPlan) -> bool {
    let Some(first_dirty_ordinal) = first_dirty_ordinal(plan) else {
        return false;
    };
    plan.manifest
        .segments
        .iter()
        .filter(|segment| segment.ordinal >= first_dirty_ordinal)
        .all(|segment| segment.kind == "RasterRun")
}

fn suffix_checkpoint_segment(plan: &crate::ReloadDiffPlan) -> Option<&crate::ReloadDiffSegment> {
    let first_dirty_ordinal = first_dirty_ordinal(plan)?;
    plan.manifest.segments.iter().find(|segment| {
        segment.ordinal == first_dirty_ordinal && segment.depth == 0 && segment.checkpoint_before
    })
}

fn first_dirty_ordinal(plan: &crate::ReloadDiffPlan) -> Option<u32> {
    plan.dirty_segments
        .iter()
        .map(|segment| segment.ordinal)
        .min()
}

#[cfg(test)]
mod tests {
    use super::{
        suffix_checkpoint_candidate, suffix_manifest_is_raster_only,
        suffix_starts_at_initial_accumulator,
    };

    #[test]
    fn suffix_initial_base_requires_first_dirty_segment_at_source_zero() {
        let mut plan = patch_plan_with_segment_start(0);
        assert!(suffix_starts_at_initial_accumulator(&plan));

        plan.manifest.segments[0].source_start = 1;
        assert!(!suffix_starts_at_initial_accumulator(&plan));
        assert_eq!(
            suffix_checkpoint_candidate(&plan).map(|candidate| candidate.source_start),
            Some(1)
        );
    }

    #[test]
    fn suffix_checkpoint_requires_explicit_depth_zero_candidate() {
        let mut plan = patch_plan_with_segment_start(1);
        plan.manifest.segments[0].checkpoint_before = false;
        assert_eq!(suffix_checkpoint_candidate(&plan), None);

        plan.manifest.segments[0].checkpoint_before = true;
        plan.manifest.segments[0].depth = 1;
        assert_eq!(suffix_checkpoint_candidate(&plan), None);
    }

    #[test]
    fn suffix_checkpoint_carries_candidate_priority() {
        let mut plan = patch_plan_with_segment_start(1);
        plan.manifest.segments[0].checkpoint_priority = 45;

        assert_eq!(
            suffix_checkpoint_candidate(&plan).map(|candidate| candidate.priority),
            Some(45)
        );
    }

    #[test]
    fn suffix_initial_base_requires_raster_only_suffix() {
        let mut plan = patch_plan_with_segment_start(0);
        assert!(suffix_manifest_is_raster_only(&plan));

        plan.manifest.segments.push(crate::ReloadDiffSegment {
            ordinal: 8,
            depth: 0,
            source_start: 1,
            source_end: 2,
            checkpoint_before: false,
            checkpoint_priority: 0,
            kind: "Barrier".to_string(),
            barrier_reason: Some("SolidColorNotLowered".to_string()),
            expected_passes: 1,
            tile_events: 0,
            legacy_sources: 1,
            resources: Vec::new(),
            tile_work_list_source_count: 0,
            tile_work_list_tile_count: 0,
            tile_work_list_signature: 0,
            tile_work_list: Vec::new(),
            signature: 0,
        });
        assert!(!suffix_manifest_is_raster_only(&plan));
    }

    fn patch_plan_with_segment_start(source_start: u32) -> crate::ReloadDiffPlan {
        crate::ReloadDiffPlan {
            manifest: crate::ReloadDiffManifest {
                abi: 4,
                tile_size: 256,
                tile_event_abi_version: clip_gpu::TILE_EVENT_ABI_VERSION,
                width: 2,
                height: 1,
                root_layer_id: 1,
                nodes: Vec::new(),
                sources: Vec::new(),
                segments: vec![crate::ReloadDiffSegment {
                    ordinal: 7,
                    depth: 0,
                    source_start,
                    source_end: source_start + 1,
                    checkpoint_before: true,
                    checkpoint_priority: 1,
                    kind: "RasterRun".to_string(),
                    barrier_reason: None,
                    expected_passes: 1,
                    tile_events: 1,
                    legacy_sources: 0,
                    resources: Vec::new(),
                    tile_work_list_source_count: 0,
                    tile_work_list_tile_count: 0,
                    tile_work_list_signature: 0,
                    tile_work_list: Vec::new(),
                    signature: 0,
                }],
            },
            mode: crate::ReloadDiffMode::Patch,
            reason: "test".to_string(),
            dirty_rects: vec![crate::ReloadPatchRect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            }],
            dirty_segments: vec![crate::ReloadDirtySegment {
                ordinal: 7,
                dirty_tile_count: 1,
                dirty_resource_count: 0,
                dirty_event_ranges: vec![crate::ReloadDirtySegmentEventRange { start: 0, end: 1 }],
            }],
        }
    }
}
