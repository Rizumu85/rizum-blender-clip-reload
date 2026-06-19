use crate::{ClipSession, ReloadDiffMode, RuntimeGpuRenderer};

#[test]
fn sparse_atlas_texture_pool_prepare_warms_then_reuses_textures() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
    let session = ClipSession::open(path).expect("open Test_Clipping.clip");
    let renderer = RuntimeGpuRenderer::new_with_texture_cache().expect("create renderer");

    let full_plan = session.plan_reload_diff(None).expect("full reload plan");
    let manifest = full_plan.manifest.clone();
    let full_stats = renderer
        .prepare_sparse_atlas_textures(&session, &full_plan)
        .expect("prepare full sparse atlas textures");
    assert!(full_stats.created_atlases > 0);
    assert!(full_stats.updated_chunks > 0);
    assert!(full_stats.resident_bytes > 0);

    let no_change_plan = session
        .plan_reload_diff(Some(&manifest))
        .expect("no-change reload plan");
    assert_eq!(no_change_plan.mode, ReloadDiffMode::NoChange);
    let no_change_stats = renderer
        .prepare_sparse_atlas_textures(&session, &no_change_plan)
        .expect("prepare no-change sparse atlas textures");
    assert_eq!(no_change_stats.created_atlases, 0);
    assert_eq!(no_change_stats.updated_chunks, 0);
    assert_eq!(
        no_change_stats.resident_atlases,
        full_stats.resident_atlases
    );
    assert_eq!(no_change_stats.resident_bytes, full_stats.resident_bytes);
}

#[test]
fn sparse_atlas_texture_pool_prepare_updates_changed_patch_slots() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
    let session = ClipSession::open(path).expect("open Test_Clipping.clip");
    let renderer = RuntimeGpuRenderer::new_with_texture_cache().expect("create renderer");

    let full_plan = session.plan_reload_diff(None).expect("full reload plan");
    let mut previous_manifest = full_plan.manifest.clone();
    let changed_tile = previous_manifest
        .sources
        .iter_mut()
        .find_map(|source| source.tiles.first_mut())
        .expect("manifest has compressed tiles");
    changed_tile.compressed_hash ^= 1;

    let patch_plan = session
        .plan_reload_diff(Some(&previous_manifest))
        .expect("patch reload plan");
    assert_eq!(patch_plan.mode, ReloadDiffMode::Patch);
    let patch_stats = renderer
        .prepare_sparse_atlas_textures(&session, &patch_plan)
        .expect("prepare patch sparse atlas textures");
    assert!(patch_stats.created_atlases > 0);
    assert!(patch_stats.updated_chunks > 0);
    assert!(patch_stats.upload_bytes > 0);
}
