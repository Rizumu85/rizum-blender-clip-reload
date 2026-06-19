use super::atlas_cache::{SparseAtlasCache, SparseAtlasFingerprint, SparseAtlasUpdateAction};
use crate::reload_diff::{
    ReloadDiffManifest, ReloadDiffSegment, ReloadDiffSegmentTileRef, ReloadDiffSource,
    ReloadDiffTile,
};

#[test]
fn unchanged_tile_reuses_slot_across_generations() {
    let mut cache = SparseAtlasCache::new(256, 2);
    let first = cache.plan_generation([fingerprint("raster", 10, 1, 0, 0, 100)]);
    assert_eq!(first.stats.inserted_tiles, 1);
    assert_eq!(
        first.updates[0].action,
        SparseAtlasUpdateAction::UploadInserted
    );

    let second = cache.plan_generation([fingerprint("raster", 10, 1, 0, 0, 100)]);
    assert_eq!(second.stats.reused_tiles, 1);
    assert_eq!(second.stats.upload_bytes, 0);
    assert_eq!(second.updates[0].action, SparseAtlasUpdateAction::Reuse);
    assert_eq!(first.updates[0].slot, second.updates[0].slot);
    assert_eq!(cache.stats().cached_tiles, 1);
}

#[test]
fn changed_payload_updates_existing_slot() {
    let mut cache = SparseAtlasCache::new(256, 2);
    let first = cache.plan_generation([fingerprint("raster", 10, 1, 0, 0, 100)]);
    let second = cache.plan_generation([fingerprint("raster", 10, 1, 0, 0, 200)]);

    assert_eq!(second.stats.changed_tiles, 1);
    assert_eq!(
        second.updates[0].action,
        SparseAtlasUpdateAction::UploadChanged
    );
    assert_eq!(first.updates[0].slot.index, second.updates[0].slot.index);
}

#[test]
fn unused_tiles_are_reclaimed_and_slots_are_reused() {
    let mut cache = SparseAtlasCache::new(256, 2);
    let first = cache.plan_generation([
        fingerprint("raster", 10, 1, 0, 0, 100),
        fingerprint("raster", 10, 1, 1, 0, 101),
    ]);
    let stale_slot = first.updates[1].slot.index;

    let second = cache.plan_generation([fingerprint("raster", 10, 1, 0, 0, 100)]);
    assert_eq!(second.stats.evicted_tiles, 1);
    assert_eq!(second.stats.cached_tiles, 1);

    let third = cache.plan_generation([
        fingerprint("raster", 10, 1, 0, 0, 100),
        fingerprint("raster", 10, 1, 2, 0, 102),
    ]);
    let inserted = third
        .updates
        .iter()
        .find(|update| update.action == SparseAtlasUpdateAction::UploadInserted)
        .expect("inserted tile update");
    assert_eq!(inserted.slot.index, stale_slot);
}

#[test]
fn raster_and_mask_tiles_do_not_collide() {
    let mut cache = SparseAtlasCache::new(256, 2);
    let plan = cache.plan_generation([
        fingerprint("raster", 10, 1, 0, 0, 100),
        fingerprint("mask", 10, 1, 0, 0, 100),
    ]);

    assert_eq!(plan.stats.inserted_tiles, 2);
    assert_ne!(plan.updates[0].slot.index, plan.updates[1].slot.index);
}

#[test]
fn reload_manifest_sources_convert_to_cache_fingerprints() {
    let mut cache = SparseAtlasCache::new(256, 2);
    let manifest = ReloadDiffManifest {
        abi: 4,
        tile_size: 256,
        tile_event_abi_version: 8,
        width: 512,
        height: 512,
        root_layer_id: 1,
        nodes: Vec::new(),
        sources: vec![source("raster", 10, 1, 100), source("mask", 20, 2, 200)],
        segments: Vec::new(),
    };

    let plan = cache.plan_reload_manifest(&manifest);

    assert_eq!(plan.stats.inserted_tiles, 2);
    assert_eq!(plan.stats.upload_bytes, 256 * 256 * 4 + 256 * 256);
}

#[test]
fn reload_manifest_prefers_segment_tile_work_list() {
    let mut cache = SparseAtlasCache::new(256, 2);
    let mut source = source("raster", 10, 1, 100);
    source.tiles.push(ReloadDiffTile {
        tile_x: 1,
        tile_y: 0,
        x: 256,
        y: 0,
        width: 256,
        height: 256,
        compressed_bytes: 1234,
        compressed_hash: 101,
    });
    let manifest = ReloadDiffManifest {
        abi: 4,
        tile_size: 256,
        tile_event_abi_version: 8,
        width: 512,
        height: 512,
        root_layer_id: 1,
        nodes: Vec::new(),
        sources: vec![source],
        segments: vec![segment_tile_ref("raster", 10, 1, 1, 0)],
    };

    let plan = cache.plan_reload_manifest(&manifest);

    assert_eq!(plan.stats.inserted_tiles, 1);
    assert_eq!(plan.updates[0].fingerprint.tile.tile_x, 1);
}

#[test]
fn source_rect_comes_from_canvas_rect_and_source_offset() {
    let mut source = source("raster", 10, 1, 100);
    source.offset_x = -32;
    source.offset_y = -16;
    let tile = ReloadDiffTile {
        tile_x: 0,
        tile_y: 0,
        x: 0,
        y: 0,
        width: 224,
        height: 240,
        compressed_bytes: 1234,
        compressed_hash: 100,
    };

    let fingerprint = SparseAtlasFingerprint::from_reload_source_tile(&source, &tile)
        .expect("supported cache source kind");

    assert_eq!(fingerprint.source_x, 32);
    assert_eq!(fingerprint.source_y, 16);
    assert_eq!(fingerprint.width, 224);
    assert_eq!(fingerprint.height, 240);
}

fn fingerprint(
    kind: &str,
    layer_id: u32,
    resource_id: u32,
    tile_x: u32,
    tile_y: u32,
    compressed_hash: u64,
) -> SparseAtlasFingerprint {
    let source = source(kind, layer_id, resource_id, 999);
    let tile = ReloadDiffTile {
        tile_x,
        tile_y,
        x: tile_x * 256,
        y: tile_y * 256,
        width: 256,
        height: 256,
        compressed_bytes: 1234,
        compressed_hash,
    };
    SparseAtlasFingerprint::from_reload_source_tile(&source, &tile)
        .expect("supported cache source kind")
}

fn source(kind: &str, layer_id: u32, resource_id: u32, signature: u64) -> ReloadDiffSource {
    ReloadDiffSource {
        kind: kind.to_string(),
        layer_id,
        resource_id,
        external_id: format!("ext-{layer_id}-{resource_id}"),
        offset_x: 0,
        offset_y: 0,
        width: 512,
        height: 512,
        color_type: if kind == "raster" { Some(0) } else { None },
        empty_fill: if kind == "mask" { Some(255) } else { None },
        signature,
        tiles: vec![ReloadDiffTile {
            tile_x: 0,
            tile_y: 0,
            x: 0,
            y: 0,
            width: 256,
            height: 256,
            compressed_bytes: 1234,
            compressed_hash: signature,
        }],
    }
}

fn segment_tile_ref(
    kind: &str,
    layer_id: u32,
    resource_id: u32,
    tile_x: u32,
    tile_y: u32,
) -> ReloadDiffSegment {
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
        resources: Vec::new(),
        tile_work_list_source_count: 1,
        tile_work_list_tile_count: 1,
        tile_work_list_signature: 0,
        tile_work_list: vec![ReloadDiffSegmentTileRef {
            kind: kind.to_string(),
            layer_id,
            resource_id,
            tile_x,
            tile_y,
            event_start: 0,
            event_end: 1,
        }],
        signature: 0,
    }
}
