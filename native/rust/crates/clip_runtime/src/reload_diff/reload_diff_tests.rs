use super::{ReloadDiffManifest, ReloadDiffMode, ReloadDiffSource, ReloadPatchRect};
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
    assert_eq!(manifest.abi, 1);
    assert_eq!(manifest.tile_size, 256);
    assert!(
        manifest
            .sources
            .iter()
            .any(|source| !source.tiles.is_empty())
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
        abi: 1,
        tile_size: 256,
        width: 100,
        height: 100,
        root_layer_id: 2,
        nodes: Vec::new(),
        sources: vec![empty_mask_source(0, 1)],
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
        abi: 1,
        tile_size: 256,
        width: 1,
        height: 1,
        root_layer_id: 2,
        nodes: Vec::new(),
        sources: Vec::new(),
    };
    let json = serde_json::to_string(&manifest).expect("serialize manifest");
    let parsed: ReloadDiffManifest = serde_json::from_str(&json).expect("parse manifest");
    assert_eq!(parsed, manifest);
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
