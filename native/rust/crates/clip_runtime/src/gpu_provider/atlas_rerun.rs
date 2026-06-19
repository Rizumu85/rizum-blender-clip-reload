use std::collections::HashMap;

use super::atlas_cache::{
    SparseAtlasCache, SparseAtlasSlot, SparseAtlasTileUpdate, SparseAtlasUpdateAction,
    SparseAtlasUpdatePlan,
};
use crate::reload_diff::{
    ReloadDiffManifest, ReloadDiffMode, ReloadDiffPlan, ReloadDiffSegmentTileRef, ReloadDiffTile,
    ReloadDirtySegmentEventRange,
};
use crate::{
    GpuSparseAtlasEventRange, GpuSparseAtlasReloadPlan, GpuSparseAtlasRerunSegment,
    GpuSparseAtlasSlot,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SparseAtlasReloadPlan {
    pub cache: SparseAtlasUpdatePlan,
    pub rerunnable_segments: Vec<SparseAtlasRerunSegment>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SparseAtlasRerunSegment {
    pub ordinal: u32,
    pub event_ranges: Vec<ReloadDirtySegmentEventRange>,
    pub resident_slots: Vec<SparseAtlasRerunSlot>,
    pub updated_slots: Vec<SparseAtlasRerunSlot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SparseAtlasRerunSlot {
    pub kind: String,
    pub layer_id: u32,
    pub resource_id: u32,
    pub tile_x: u32,
    pub tile_y: u32,
    pub event_start: u32,
    pub event_end: u32,
    pub canvas_x: u32,
    pub canvas_y: u32,
    pub source_x: u32,
    pub source_y: u32,
    pub action: SparseAtlasUpdateAction,
    pub format: clip_gpu::GpuSparseAtlasFormat,
    pub atlas_size: clip_model::CanvasSize,
    pub slot: SparseAtlasSlot,
}

pub(crate) fn suffix_rerun_segments(
    plan: &ReloadDiffPlan,
    cache: &SparseAtlasUpdatePlan,
) -> Vec<SparseAtlasRerunSegment> {
    if plan.mode != ReloadDiffMode::Patch {
        return Vec::new();
    }
    let Some(first_dirty_ordinal) = plan
        .dirty_segments
        .iter()
        .map(|segment| segment.ordinal)
        .min()
    else {
        return Vec::new();
    };

    let updates = update_lookup(cache);
    let source_tiles = source_tile_lookup(&plan.manifest);
    plan.manifest
        .segments
        .iter()
        .filter(|segment| segment.ordinal >= first_dirty_ordinal)
        .map(|segment| {
            let resident_slots = resident_slots_for_tiles(
                &segment.tile_work_list,
                &updates,
                &source_tiles,
                cache.atlas_size,
            );
            let updated_slots = updated_slots_from_resident(&resident_slots);
            SparseAtlasRerunSegment {
                ordinal: segment.ordinal,
                event_ranges: full_segment_event_range(segment.tile_events),
                resident_slots,
                updated_slots,
            }
        })
        .collect()
}

impl SparseAtlasCache {
    pub(crate) fn plan_reload_diff(&mut self, plan: &ReloadDiffPlan) -> SparseAtlasReloadPlan {
        let cache = self.plan_reload_manifest(&plan.manifest);
        let rerunnable_segments = if plan.mode == ReloadDiffMode::Patch {
            rerunnable_segments(plan, &cache)
        } else {
            Vec::new()
        };
        SparseAtlasReloadPlan {
            cache,
            rerunnable_segments,
        }
    }
}

fn rerunnable_segments(
    plan: &ReloadDiffPlan,
    cache: &SparseAtlasUpdatePlan,
) -> Vec<SparseAtlasRerunSegment> {
    let updates = update_lookup(cache);
    let source_tiles = source_tile_lookup(&plan.manifest);
    plan.dirty_segments
        .iter()
        .filter_map(|dirty| {
            let segment = plan
                .manifest
                .segments
                .iter()
                .find(|segment| segment.ordinal == dirty.ordinal)?;
            let tiles = segment
                .tile_work_list
                .iter()
                .filter(|tile| event_range_intersects_any(tile, &dirty.dirty_event_ranges));
            let resident_slots =
                resident_slots_for_tiles(tiles, &updates, &source_tiles, cache.atlas_size);
            let updated_slots = updated_slots_from_resident(&resident_slots);
            Some(SparseAtlasRerunSegment {
                ordinal: dirty.ordinal,
                event_ranges: dirty.dirty_event_ranges.clone(),
                resident_slots,
                updated_slots,
            })
        })
        .collect()
}

fn resident_slots_for_tiles<'a>(
    tiles: impl IntoIterator<Item = &'a ReloadDiffSegmentTileRef>,
    updates: &HashMap<AtlasTileKey, &SparseAtlasTileUpdate>,
    source_tiles: &HashMap<AtlasTileKey, ReloadDiffTile>,
    atlas_size: clip_model::CanvasSize,
) -> Vec<SparseAtlasRerunSlot> {
    tiles
        .into_iter()
        .filter_map(|tile| {
            let key = tile_key(tile);
            let canvas_tile = source_tiles.get(&key)?;
            updates
                .get(&key)
                .map(|update| rerun_slot_for_update(tile, update, atlas_size, canvas_tile))
        })
        .collect()
}

fn updated_slots_from_resident(
    resident_slots: &[SparseAtlasRerunSlot],
) -> Vec<SparseAtlasRerunSlot> {
    resident_slots
        .iter()
        .filter(|slot| slot.action != SparseAtlasUpdateAction::Reuse)
        .cloned()
        .collect()
}

fn full_segment_event_range(tile_events: u32) -> Vec<ReloadDirtySegmentEventRange> {
    (tile_events > 0)
        .then_some(ReloadDirtySegmentEventRange {
            start: 0,
            end: tile_events,
        })
        .into_iter()
        .collect()
}

fn source_tile_lookup(manifest: &ReloadDiffManifest) -> HashMap<AtlasTileKey, ReloadDiffTile> {
    let mut tiles = HashMap::new();
    for source in &manifest.sources {
        for tile in &source.tiles {
            tiles.insert(
                AtlasTileKey {
                    kind: reload_kind(&source.kind),
                    layer_id: source.layer_id,
                    resource_id: source.resource_id,
                    tile_x: tile.tile_x,
                    tile_y: tile.tile_y,
                },
                *tile,
            );
        }
    }
    tiles
}

fn update_lookup(cache: &SparseAtlasUpdatePlan) -> HashMap<AtlasTileKey, &SparseAtlasTileUpdate> {
    cache
        .updates
        .iter()
        .map(|update| {
            (
                AtlasTileKey {
                    kind: update.fingerprint.tile.kind.as_reload_kind(),
                    layer_id: update.fingerprint.tile.layer_id,
                    resource_id: update.fingerprint.tile.resource_id,
                    tile_x: update.fingerprint.tile.tile_x,
                    tile_y: update.fingerprint.tile.tile_y,
                },
                update,
            )
        })
        .collect()
}

fn rerun_slot_for_update(
    tile: &ReloadDiffSegmentTileRef,
    update: &SparseAtlasTileUpdate,
    atlas_size: clip_model::CanvasSize,
    canvas_tile: &ReloadDiffTile,
) -> SparseAtlasRerunSlot {
    SparseAtlasRerunSlot {
        kind: tile.kind.clone(),
        layer_id: tile.layer_id,
        resource_id: tile.resource_id,
        tile_x: tile.tile_x,
        tile_y: tile.tile_y,
        event_start: tile.event_start,
        event_end: tile.event_end,
        canvas_x: canvas_tile.x,
        canvas_y: canvas_tile.y,
        source_x: update.fingerprint.source_x,
        source_y: update.fingerprint.source_y,
        action: update.action,
        format: update.fingerprint.tile.kind.atlas_format(),
        atlas_size,
        slot: update.slot,
    }
}

fn event_range_intersects_any(
    tile: &ReloadDiffSegmentTileRef,
    ranges: &[ReloadDirtySegmentEventRange],
) -> bool {
    ranges
        .iter()
        .any(|range| tile.event_start < range.end && range.start < tile.event_end)
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct AtlasTileKey {
    kind: &'static str,
    layer_id: u32,
    resource_id: u32,
    tile_x: u32,
    tile_y: u32,
}

fn tile_key(tile: &ReloadDiffSegmentTileRef) -> AtlasTileKey {
    AtlasTileKey {
        kind: reload_kind(&tile.kind),
        layer_id: tile.layer_id,
        resource_id: tile.resource_id,
        tile_x: tile.tile_x,
        tile_y: tile.tile_y,
    }
}

fn reload_kind(kind: &str) -> &'static str {
    match kind {
        "raster" => "raster",
        "mask" => "mask",
        _ => "unknown",
    }
}

impl From<SparseAtlasReloadPlan> for GpuSparseAtlasReloadPlan {
    fn from(value: SparseAtlasReloadPlan) -> Self {
        Self {
            cache_stats: value.cache.stats.into(),
            rerunnable_segments: value
                .rerunnable_segments
                .into_iter()
                .map(GpuSparseAtlasRerunSegment::from)
                .collect(),
        }
    }
}

impl From<SparseAtlasRerunSegment> for GpuSparseAtlasRerunSegment {
    fn from(value: SparseAtlasRerunSegment) -> Self {
        Self {
            ordinal: value.ordinal,
            event_ranges: value
                .event_ranges
                .into_iter()
                .map(|range| GpuSparseAtlasEventRange {
                    start: range.start,
                    end: range.end,
                })
                .collect(),
            resident_slots: value
                .resident_slots
                .into_iter()
                .map(GpuSparseAtlasSlot::from)
                .collect(),
            updated_slots: value
                .updated_slots
                .into_iter()
                .map(GpuSparseAtlasSlot::from)
                .collect(),
        }
    }
}

impl From<SparseAtlasRerunSlot> for GpuSparseAtlasSlot {
    fn from(value: SparseAtlasRerunSlot) -> Self {
        Self {
            kind: value.kind,
            layer_id: value.layer_id,
            resource_id: value.resource_id,
            tile_x: value.tile_x,
            tile_y: value.tile_y,
            event_start: value.event_start,
            event_end: value.event_end,
            canvas_x: value.canvas_x,
            canvas_y: value.canvas_y,
            source_x: value.source_x,
            source_y: value.source_y,
            action: value.action.as_str().to_string(),
            format: sparse_atlas_format_name(value.format).to_string(),
            atlas_width: value.atlas_size.width,
            atlas_height: value.atlas_size.height,
            atlas_id: value.slot.atlas_id,
            atlas_x: value.slot.x,
            atlas_y: value.slot.y,
            width: value.slot.width,
            height: value.slot.height,
        }
    }
}

fn sparse_atlas_format_name(format: clip_gpu::GpuSparseAtlasFormat) -> &'static str {
    match format {
        clip_gpu::GpuSparseAtlasFormat::Rgba8 => "rgba8",
        clip_gpu::GpuSparseAtlasFormat::R8 => "r8",
    }
}
