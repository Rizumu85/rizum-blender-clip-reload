use super::atlas_cache::SparseAtlasCache;
use crate::reload_diff::{
    ReloadDiffManifest, ReloadDiffMode, ReloadDiffPlan, ReloadDiffSegment,
    ReloadDiffSegmentResource, ReloadDiffSegmentTileRef, ReloadDiffSource, ReloadDiffTile,
    ReloadDirtySegment, ReloadDirtySegmentEventRange, ReloadPatchRect,
};

#[test]
fn changed_tile_update_maps_to_rerunnable_segment_slot() {
    let mut cache = SparseAtlasCache::new(256, 2);
    let previous = manifest_with_tile(100, 100);
    cache.plan_reload_manifest(&previous);

    let plan = ReloadDiffPlan {
        manifest: manifest_with_tile(100, 200),
        mode: ReloadDiffMode::Patch,
        reason: "test patch".to_string(),
        dirty_rects: vec![rect()],
        dirty_segments: vec![ReloadDirtySegment {
            ordinal: 7,
            dirty_tile_count: 1,
            dirty_resource_count: 0,
            dirty_event_ranges: vec![ReloadDirtySegmentEventRange { start: 2, end: 3 }],
        }],
    };

    let reload = cache.plan_reload_diff(&plan);

    assert_eq!(reload.cache.stats.changed_tiles, 1);
    assert_eq!(reload.rerunnable_segments.len(), 1);
    let segment = &reload.rerunnable_segments[0];
    assert_eq!(segment.ordinal, 7);
    assert_eq!(
        segment.event_ranges,
        vec![ReloadDirtySegmentEventRange { start: 2, end: 3 }]
    );
    assert_eq!(segment.resident_slots.len(), 1);
    assert_eq!(segment.resident_slots[0].tile_x, 0);
    assert_eq!(segment.resident_slots[0].event_start, 2);
    assert_eq!(segment.resident_slots[0].event_end, 3);
    assert_eq!(segment.resident_slots[0].canvas_x, 0);
    assert_eq!(segment.resident_slots[0].canvas_y, 0);
    assert_eq!(segment.resident_slots[0].action.as_str(), "upload_changed");
    assert_eq!(segment.updated_slots.len(), 1);
    assert_eq!(segment.updated_slots[0].tile_x, 0);
    assert_eq!(segment.updated_slots[0].action.as_str(), "upload_changed");
    assert_eq!(
        segment.updated_slots[0].format,
        clip_gpu::GpuSparseAtlasFormat::Rgba8
    );
    assert_eq!(
        segment.updated_slots[0].atlas_size,
        clip_model::CanvasSize::new(512, 512)
    );
}

#[test]
fn resource_dirty_segment_can_rerun_without_atlas_upload() {
    let mut cache = SparseAtlasCache::new(256, 2);
    let previous = manifest_with_tile(100, 100);
    cache.plan_reload_manifest(&previous);

    let plan = ReloadDiffPlan {
        manifest: manifest_with_tile(100, 100),
        mode: ReloadDiffMode::Patch,
        reason: "resource metadata changed".to_string(),
        dirty_rects: vec![rect()],
        dirty_segments: vec![ReloadDirtySegment {
            ordinal: 7,
            dirty_tile_count: 0,
            dirty_resource_count: 1,
            dirty_event_ranges: vec![ReloadDirtySegmentEventRange { start: 0, end: 8 }],
        }],
    };

    let reload = cache.plan_reload_diff(&plan);

    assert_eq!(reload.cache.stats.reused_tiles, 1);
    assert_eq!(reload.rerunnable_segments.len(), 1);
    assert_eq!(reload.rerunnable_segments[0].resident_slots.len(), 1);
    assert_eq!(
        reload.rerunnable_segments[0].resident_slots[0]
            .action
            .as_str(),
        "reuse"
    );
    assert_eq!(reload.rerunnable_segments[0].updated_slots.len(), 0);
    assert_eq!(
        reload.rerunnable_segments[0].event_ranges,
        vec![ReloadDirtySegmentEventRange { start: 0, end: 8 }]
    );
}

#[test]
fn resident_slot_keeps_canvas_rect_separate_from_source_rect() {
    let mut cache = SparseAtlasCache::new(256, 2);
    let previous = manifest_with_offset_tile(100, 100);
    cache.plan_reload_manifest(&previous);

    let plan = ReloadDiffPlan {
        manifest: manifest_with_offset_tile(100, 200),
        mode: ReloadDiffMode::Patch,
        reason: "offset source tile changed".to_string(),
        dirty_rects: vec![rect()],
        dirty_segments: vec![ReloadDirtySegment {
            ordinal: 7,
            dirty_tile_count: 1,
            dirty_resource_count: 0,
            dirty_event_ranges: vec![ReloadDirtySegmentEventRange { start: 2, end: 3 }],
        }],
    };

    let reload = cache.plan_reload_diff(&plan);

    let slot = &reload.rerunnable_segments[0].resident_slots[0];
    assert_eq!(slot.canvas_x, 0);
    assert_eq!(slot.canvas_y, 20);
    assert_eq!(slot.source_x, 10);
    assert_eq!(slot.source_y, 15);
    assert_eq!(slot.event_start, 2);
    assert_eq!(slot.event_end, 3);
}

#[test]
fn no_change_reload_keeps_cache_warm_without_rerunnable_segments() {
    let mut cache = SparseAtlasCache::new(256, 2);
    let manifest = manifest_with_tile(100, 100);
    cache.plan_reload_manifest(&manifest);

    let plan = ReloadDiffPlan {
        manifest,
        mode: ReloadDiffMode::NoChange,
        reason: "unchanged".to_string(),
        dirty_rects: Vec::new(),
        dirty_segments: Vec::new(),
    };

    let reload = cache.plan_reload_diff(&plan);

    assert_eq!(reload.cache.stats.reused_tiles, 1);
    assert!(reload.rerunnable_segments.is_empty());
}

fn manifest_with_tile(source_signature: u64, compressed_hash: u64) -> ReloadDiffManifest {
    ReloadDiffManifest {
        abi: 4,
        tile_size: 256,
        tile_event_abi_version: 8,
        width: 512,
        height: 512,
        root_layer_id: 1,
        nodes: Vec::new(),
        sources: vec![source(source_signature, compressed_hash)],
        segments: vec![segment()],
    }
}

fn manifest_with_offset_tile(source_signature: u64, compressed_hash: u64) -> ReloadDiffManifest {
    let mut manifest = manifest_with_tile(source_signature, compressed_hash);
    manifest.sources[0].offset_x = -10;
    manifest.sources[0].offset_y = 5;
    manifest.sources[0].tiles[0].x = 0;
    manifest.sources[0].tiles[0].y = 20;
    manifest
}

fn source(signature: u64, compressed_hash: u64) -> ReloadDiffSource {
    ReloadDiffSource {
        kind: "raster".to_string(),
        layer_id: 10,
        resource_id: 1,
        external_id: "ext-10-1".to_string(),
        offset_x: 0,
        offset_y: 0,
        width: 512,
        height: 512,
        color_type: Some(0),
        empty_fill: None,
        signature,
        tiles: vec![ReloadDiffTile {
            tile_x: 0,
            tile_y: 0,
            x: 0,
            y: 0,
            width: 256,
            height: 256,
            compressed_bytes: 1234,
            compressed_hash,
        }],
    }
}

fn segment() -> ReloadDiffSegment {
    ReloadDiffSegment {
        ordinal: 7,
        depth: 0,
        source_start: 0,
        source_end: 1,
        kind: "RasterRun".to_string(),
        barrier_reason: None,
        expected_passes: 1,
        tile_events: 8,
        legacy_sources: 0,
        resources: vec![ReloadDiffSegmentResource {
            kind: "raster".to_string(),
            layer_id: 10,
            resource_id: 1,
        }],
        tile_work_list_source_count: 1,
        tile_work_list_tile_count: 1,
        tile_work_list_signature: 0,
        tile_work_list: vec![ReloadDiffSegmentTileRef {
            kind: "raster".to_string(),
            layer_id: 10,
            resource_id: 1,
            tile_x: 0,
            tile_y: 0,
            event_start: 2,
            event_end: 3,
        }],
        signature: 0,
    }
}

fn rect() -> ReloadPatchRect {
    ReloadPatchRect {
        x: 0,
        y: 0,
        width: 256,
        height: 256,
    }
}
