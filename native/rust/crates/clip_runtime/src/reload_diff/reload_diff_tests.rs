use super::{
    ReloadDiffManifest, ReloadDiffMode, ReloadDiffSegment, ReloadDiffSource, ReloadPatchRect,
};
use crate::reload_diff::reload_diff_plan::{coalesce_dirty_rects, plan_reload_diff};

#[test]
fn unchanged_manifest_plans_no_change() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
    let session = crate::ClipSession::open(path).expect("open Test_Clipping.clip");
    let manifest = session
        .reload_diff_manifest()
        .expect("build reload manifest");
    let plan = session
        .plan_reload_diff(Some(&manifest))
        .expect("plan reload diff");
    assert_eq!(plan.mode, ReloadDiffMode::NoChange);
    assert!(plan.dirty_rects.is_empty());
}

#[test]
fn manifest_contains_compressed_tile_fingerprints() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
    let session = crate::ClipSession::open(path).expect("open Test_Clipping.clip");
    let manifest = session
        .reload_diff_manifest()
        .expect("build reload manifest");
    assert_eq!(manifest.abi, super::MANIFEST_ABI);
    assert_eq!(manifest.tile_size, 256);
    assert_eq!(
        manifest.tile_event_abi_version,
        clip_gpu::TILE_EVENT_ABI_VERSION
    );
    assert!(
        manifest
            .sources
            .iter()
            .any(|source| !source.tiles.is_empty())
    );
    assert!(!manifest.segments.is_empty());
    assert!(
        manifest
            .segments
            .iter()
            .any(|segment| segment.signature != 0)
    );
}

#[test]
fn coalesces_touching_dirty_rects() {
    assert_eq!(
        coalesce_dirty_rects(vec![
            ReloadPatchRect {
                x: 0,
                y: 0,
                width: 4,
                height: 4,
            },
            ReloadPatchRect {
                x: 4,
                y: 0,
                width: 2,
                height: 4,
            },
        ]),
        vec![ReloadPatchRect {
            x: 0,
            y: 0,
            width: 6,
            height: 4,
        }]
    );
}

#[test]
fn source_metadata_change_without_tiles_dirties_source_extent() {
    let previous = ReloadDiffManifest {
        abi: super::MANIFEST_ABI,
        tile_size: 256,
        tile_event_abi_version: clip_gpu::TILE_EVENT_ABI_VERSION,
        width: 100,
        height: 100,
        root_layer_id: 2,
        nodes: Vec::new(),
        sources: vec![empty_mask_source(0, 1)],
        segments: Vec::new(),
    };
    let mut current = previous.clone();
    current.sources[0].empty_fill = Some(255);
    current.sources[0].signature = 2;

    let plan = plan_reload_diff(&previous, current);

    assert_eq!(plan.mode, ReloadDiffMode::Patch);
    assert_eq!(
        plan.dirty_rects,
        vec![ReloadPatchRect {
            x: 10,
            y: 20,
            width: 30,
            height: 40,
        }]
    );
}

#[test]
fn manifest_is_json_roundtrippable() {
    let manifest = ReloadDiffManifest {
        abi: super::MANIFEST_ABI,
        tile_size: 256,
        tile_event_abi_version: clip_gpu::TILE_EVENT_ABI_VERSION,
        width: 1,
        height: 1,
        root_layer_id: 2,
        nodes: Vec::new(),
        sources: Vec::new(),
        segments: vec![test_segment(1)],
    };
    let json = serde_json::to_string(&manifest).expect("serialize manifest");
    let parsed: ReloadDiffManifest = serde_json::from_str(&json).expect("parse manifest");
    assert_eq!(parsed, manifest);
}

#[test]
fn segment_plan_change_promotes_to_full() {
    let previous = ReloadDiffManifest {
        abi: super::MANIFEST_ABI,
        tile_size: 256,
        tile_event_abi_version: clip_gpu::TILE_EVENT_ABI_VERSION,
        width: 100,
        height: 100,
        root_layer_id: 2,
        nodes: Vec::new(),
        sources: Vec::new(),
        segments: vec![test_segment(1)],
    };
    let mut current = previous.clone();
    current.segments[0].signature = 2;

    let plan = plan_reload_diff(&previous, current);

    assert_eq!(plan.mode, ReloadDiffMode::Full);
    assert_eq!(plan.reason, "render segment plan changed");
}

#[test]
fn tile_event_abi_change_promotes_to_full() {
    let previous = ReloadDiffManifest {
        abi: super::MANIFEST_ABI,
        tile_size: 256,
        tile_event_abi_version: clip_gpu::TILE_EVENT_ABI_VERSION,
        width: 100,
        height: 100,
        root_layer_id: 2,
        nodes: Vec::new(),
        sources: Vec::new(),
        segments: Vec::new(),
    };
    let mut current = previous.clone();
    current.tile_event_abi_version += 1;

    let plan = plan_reload_diff(&previous, current);

    assert_eq!(plan.mode, ReloadDiffMode::Full);
    assert_eq!(plan.reason, "tile event ABI changed");
}

fn empty_mask_source(empty_fill: u8, signature: u64) -> ReloadDiffSource {
    ReloadDiffSource {
        kind: "mask".to_string(),
        layer_id: 7,
        resource_id: 8,
        external_id: "mask-ext".to_string(),
        offset_x: 10,
        offset_y: 20,
        width: 30,
        height: 40,
        color_type: None,
        empty_fill: Some(empty_fill),
        signature,
        tiles: Vec::new(),
    }
}

fn test_segment(signature: u64) -> ReloadDiffSegment {
    ReloadDiffSegment {
        ordinal: 0,
        depth: 0,
        source_start: 0,
        source_end: 1,
        kind: "RasterRun".to_string(),
        barrier_reason: None,
        expected_passes: 1,
        tile_events: 1,
        legacy_sources: 0,
        signature,
    }
}
