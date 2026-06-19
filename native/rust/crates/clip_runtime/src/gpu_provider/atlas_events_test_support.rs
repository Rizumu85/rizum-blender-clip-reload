use clip_model::{CanvasSize, LayerId};

use super::atlas_cache::{
    SparseAtlasFingerprint, SparseAtlasResourceKind, SparseAtlasSlot, SparseAtlasTileId,
    SparseAtlasTileUpdate, SparseAtlasUpdateAction, SparseAtlasUpdatePlan,
};
use super::atlas_rerun::{SparseAtlasReloadPlan, SparseAtlasRerunSegment, SparseAtlasRerunSlot};
use crate::reload_diff::{
    ReloadDiffManifest, ReloadDiffMode, ReloadDiffPlan, ReloadDiffSegment,
    ReloadDirtySegmentEventRange,
};

pub(super) fn diff_with_segment(segment: ReloadDiffSegment) -> ReloadDiffPlan {
    ReloadDiffPlan {
        manifest: ReloadDiffManifest {
            abi: 4,
            tile_size: 256,
            tile_event_abi_version: clip_gpu::TILE_EVENT_ABI_VERSION,
            width: 128,
            height: 128,
            root_layer_id: 1,
            nodes: Vec::new(),
            sources: Vec::new(),
            segments: vec![segment],
        },
        mode: ReloadDiffMode::Patch,
        reason: "test".to_string(),
        dirty_rects: Vec::new(),
        dirty_segments: Vec::new(),
    }
}

pub(super) fn diff_with_segments(
    segments: Vec<ReloadDiffSegment>,
    sources: Vec<crate::ReloadDiffSource>,
    dirty_segments: Vec<crate::ReloadDirtySegment>,
) -> ReloadDiffPlan {
    ReloadDiffPlan {
        manifest: ReloadDiffManifest {
            abi: 4,
            tile_size: 256,
            tile_event_abi_version: clip_gpu::TILE_EVENT_ABI_VERSION,
            width: 128,
            height: 128,
            root_layer_id: 1,
            nodes: Vec::new(),
            sources,
            segments,
        },
        mode: ReloadDiffMode::Patch,
        reason: "test".to_string(),
        dirty_rects: Vec::new(),
        dirty_segments,
    }
}

pub(super) fn diff_with_segments_and_rects(
    segments: Vec<ReloadDiffSegment>,
    sources: Vec<crate::ReloadDiffSource>,
    dirty_segments: Vec<crate::ReloadDirtySegment>,
    dirty_rects: Vec<crate::ReloadPatchRect>,
) -> ReloadDiffPlan {
    ReloadDiffPlan {
        dirty_rects,
        ..diff_with_segments(segments, sources, dirty_segments)
    }
}

pub(super) fn segment(kind: &str) -> ReloadDiffSegment {
    ReloadDiffSegment {
        ordinal: 7,
        depth: 0,
        source_start: 0,
        source_end: 1,
        checkpoint_before: false,
        checkpoint_priority: 0,
        kind: kind.to_string(),
        barrier_reason: None,
        expected_passes: 1,
        tile_events: 4,
        legacy_sources: 0,
        resources: Vec::new(),
        tile_work_list_source_count: 0,
        tile_work_list_tile_count: 0,
        tile_work_list_signature: 0,
        tile_work_list: Vec::new(),
        signature: 0,
    }
}

pub(super) fn manifest_segment(
    ordinal: u32,
    source_start: u32,
    source_end: u32,
    kind: &str,
    layer_id: u32,
    resource_id: u32,
) -> ReloadDiffSegment {
    ReloadDiffSegment {
        ordinal,
        source_start,
        source_end,
        kind: kind.to_string(),
        resources: vec![crate::ReloadDiffSegmentResource {
            kind: "raster".to_string(),
            layer_id,
            resource_id,
        }],
        tile_work_list_source_count: 1,
        tile_work_list_tile_count: 1,
        tile_work_list_signature: 1,
        tile_work_list: vec![tile_ref_at_event(layer_id, resource_id, 0, 0, 1)],
        ..segment(kind)
    }
}

pub(super) fn manifest_segment_at_rect(
    ordinal: u32,
    source_start: u32,
    source_end: u32,
    kind: &str,
    layer_id: u32,
    resource_id: u32,
    _x: u32,
    _y: u32,
) -> ReloadDiffSegment {
    manifest_segment(
        ordinal,
        source_start,
        source_end,
        kind,
        layer_id,
        resource_id,
    )
}

pub(super) fn manifest_segment_with_tiles(
    ordinal: u32,
    source_start: u32,
    source_end: u32,
    kind: &str,
    layer_id: u32,
    resource_id: u32,
    tile_work_list: Vec<crate::ReloadDiffSegmentTileRef>,
) -> ReloadDiffSegment {
    ReloadDiffSegment {
        ordinal,
        source_start,
        source_end,
        kind: kind.to_string(),
        resources: vec![crate::ReloadDiffSegmentResource {
            kind: "raster".to_string(),
            layer_id,
            resource_id,
        }],
        tile_work_list_source_count: 1,
        tile_work_list_tile_count: u32::try_from(tile_work_list.len()).unwrap_or(u32::MAX),
        tile_work_list_signature: 1,
        tile_work_list,
        ..segment(kind)
    }
}

pub(super) fn tile_ref_at_event(
    layer_id: u32,
    resource_id: u32,
    tile_x: u32,
    event_start: u32,
    event_end: u32,
) -> crate::ReloadDiffSegmentTileRef {
    crate::ReloadDiffSegmentTileRef {
        kind: "raster".to_string(),
        layer_id,
        resource_id,
        tile_x,
        tile_y: 0,
        event_start,
        event_end,
    }
}

pub(super) fn source_manifest_at_rect(
    layer_id: u32,
    resource_id: u32,
    x: u32,
    y: u32,
) -> crate::ReloadDiffSource {
    source_manifest_with_tiles(layer_id, resource_id, vec![source_tile_at(0, x, y)])
}

pub(super) fn source_manifest_with_tiles(
    layer_id: u32,
    resource_id: u32,
    tiles: Vec<crate::ReloadDiffTile>,
) -> crate::ReloadDiffSource {
    crate::ReloadDiffSource {
        kind: "raster".to_string(),
        layer_id,
        resource_id,
        external_id: format!("ext-{layer_id}-{resource_id}"),
        offset_x: 0,
        offset_y: 0,
        width: 160,
        height: 128,
        color_type: Some(0),
        empty_fill: None,
        signature: u64::from(layer_id) << 32 | u64::from(resource_id),
        tiles,
    }
}

pub(super) fn source_tile_at(tile_x: u32, x: u32, y: u32) -> crate::ReloadDiffTile {
    crate::ReloadDiffTile {
        tile_x,
        tile_y: 0,
        x,
        y,
        width: 64,
        height: 32,
        compressed_bytes: 10,
        compressed_hash: u64::from(tile_x) + 1,
    }
}

pub(super) fn dirty_segment(ordinal: u32) -> crate::ReloadDirtySegment {
    crate::ReloadDirtySegment {
        ordinal,
        dirty_tile_count: 1,
        dirty_resource_count: 0,
        dirty_event_ranges: vec![ReloadDirtySegmentEventRange { start: 0, end: 1 }],
    }
}

pub(super) fn reload_with_slots(slots: Vec<SparseAtlasRerunSlot>) -> SparseAtlasReloadPlan {
    SparseAtlasReloadPlan {
        cache: SparseAtlasUpdatePlan::default(),
        rerunnable_segments: vec![SparseAtlasRerunSegment {
            ordinal: 7,
            event_ranges: vec![ReloadDirtySegmentEventRange { start: 2, end: 3 }],
            resident_slots: slots.clone(),
            updated_slots: slots,
        }],
    }
}

pub(super) fn reload_with_cache_updates(
    updates: Vec<SparseAtlasTileUpdate>,
) -> SparseAtlasReloadPlan {
    SparseAtlasReloadPlan {
        cache: SparseAtlasUpdatePlan {
            generation: 1,
            atlas_size: CanvasSize::new(256, 256),
            updates,
            stats: Default::default(),
        },
        rerunnable_segments: Vec::new(),
    }
}

pub(super) fn cache_update(
    layer_id: u32,
    resource_id: u32,
    slot_index: u32,
) -> SparseAtlasTileUpdate {
    cache_update_at_tile(layer_id, resource_id, 0, slot_index)
}

pub(super) fn cache_update_at_tile(
    layer_id: u32,
    resource_id: u32,
    tile_x: u32,
    slot_index: u32,
) -> SparseAtlasTileUpdate {
    SparseAtlasTileUpdate {
        fingerprint: SparseAtlasFingerprint {
            tile: SparseAtlasTileId {
                kind: SparseAtlasResourceKind::Raster,
                layer_id,
                resource_id,
                external_id: format!("ext-{layer_id}-{resource_id}"),
                source_signature: u64::from(layer_id) << 32 | u64::from(resource_id),
                tile_x,
                tile_y: 0,
            },
            source_x: tile_x * 64,
            source_y: 0,
            width: 64,
            height: 32,
            compressed_bytes: 10,
            compressed_hash: (u64::from(layer_id) << 32) | u64::from(resource_id + tile_x),
        },
        slot: SparseAtlasSlot {
            index: slot_index,
            atlas_id: 3,
            x: 16 + slot_index * 64,
            y: 32,
            width: 64,
            height: 32,
        },
        action: SparseAtlasUpdateAction::Reuse,
    }
}

pub(super) fn slot(
    kind: &str,
    layer_id: u32,
    resource_id: u32,
    slot_index: u32,
    canvas_x: u32,
    canvas_y: u32,
) -> SparseAtlasRerunSlot {
    let format = if kind == "mask" {
        clip_gpu::GpuSparseAtlasFormat::R8
    } else {
        clip_gpu::GpuSparseAtlasFormat::Rgba8
    };
    SparseAtlasRerunSlot {
        kind: kind.to_string(),
        layer_id,
        resource_id,
        tile_x: slot_index,
        tile_y: 0,
        event_start: 2,
        event_end: 3,
        canvas_x,
        canvas_y,
        source_x: canvas_x,
        source_y: canvas_y,
        action: SparseAtlasUpdateAction::Reuse,
        format,
        atlas_size: CanvasSize::new(256, 256),
        slot: SparseAtlasSlot {
            index: slot_index,
            atlas_id: 3,
            x: 16 + slot_index * 64,
            y: 32,
            width: 64,
            height: 32,
        },
    }
}

pub(super) fn raster_source(
    layer_id: u32,
    render_mipmap_id: u32,
    opacity: f32,
    blend_mode: clip_gpu::GpuRasterBlendMode,
    mask_mipmap_id: Option<u32>,
) -> clip_gpu::GpuNormalRasterSource {
    clip_gpu::GpuNormalRasterSource {
        key: clip_gpu::GpuRasterResourceKey {
            layer_id: LayerId(layer_id),
            render_mipmap_id,
        },
        opacity,
        mask_key: mask_mipmap_id.map(|mask_mipmap_id| clip_gpu::GpuMaskResourceKey {
            layer_id: LayerId(layer_id),
            mask_mipmap_id,
        }),
        offset_x: 0,
        offset_y: 0,
        blend_mode,
    }
}
