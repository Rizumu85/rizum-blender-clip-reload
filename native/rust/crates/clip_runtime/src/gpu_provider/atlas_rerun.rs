use std::collections::HashMap;

use super::atlas_cache::{
    SparseAtlasCache, SparseAtlasSlot, SparseAtlasTileUpdate, SparseAtlasUpdateAction,
    SparseAtlasUpdatePlan,
};
use crate::reload_diff::{
    ReloadDiffManifest, ReloadDiffMode, ReloadDiffPlan, ReloadDiffSegment,
    ReloadDiffSegmentTileRef, ReloadDiffTile, ReloadDirtySegmentEventRange, ReloadPatchRect,
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

pub(crate) fn affected_rerun_segments(
    plan: &ReloadDiffPlan,
    cache: &SparseAtlasUpdatePlan,
) -> Vec<SparseAtlasRerunSegment> {
    if plan.mode != ReloadDiffMode::Patch || plan.dirty_rects.is_empty() {
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
        .filter(|segment| segment_affects_dirty_rects(segment, &source_tiles, &plan.dirty_rects))
        .map(|segment| {
            let unknown_work_tile = segment_has_unknown_work_tile(segment, &source_tiles);
            let event_ranges = if unknown_work_tile {
                full_segment_event_range(segment.tile_events)
            } else {
                affected_tile_event_ranges(segment, &source_tiles, &plan.dirty_rects)
            };
            let resident_slots = if unknown_work_tile {
                Vec::new()
            } else {
                let affecting_tiles =
                    tiles_affecting_dirty_rects(segment, &source_tiles, &plan.dirty_rects);
                resident_slots_for_tiles(affecting_tiles, &updates, &source_tiles, cache.atlas_size)
            };
            let updated_slots = updated_slots_from_resident(&resident_slots);
            SparseAtlasRerunSegment {
                ordinal: segment.ordinal,
                event_ranges,
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

fn segment_affects_dirty_rects(
    segment: &ReloadDiffSegment,
    source_tiles: &HashMap<AtlasTileKey, ReloadDiffTile>,
    dirty_rects: &[ReloadPatchRect],
) -> bool {
    if segment.tile_work_list.is_empty() {
        return segment.tile_events > 0 || segment.legacy_sources > 0;
    }
    segment
        .tile_work_list
        .iter()
        .any(|tile| tile_affects_dirty_rects(tile, source_tiles, dirty_rects))
}

fn segment_has_unknown_work_tile(
    segment: &ReloadDiffSegment,
    source_tiles: &HashMap<AtlasTileKey, ReloadDiffTile>,
) -> bool {
    segment
        .tile_work_list
        .iter()
        .any(|tile| !source_tiles.contains_key(&tile_key(tile)))
}

fn tiles_affecting_dirty_rects<'a>(
    segment: &'a ReloadDiffSegment,
    source_tiles: &HashMap<AtlasTileKey, ReloadDiffTile>,
    dirty_rects: &[ReloadPatchRect],
) -> impl Iterator<Item = &'a ReloadDiffSegmentTileRef> {
    segment
        .tile_work_list
        .iter()
        .filter(move |tile| tile_affects_dirty_rects(tile, source_tiles, dirty_rects))
}

fn affected_tile_event_ranges(
    segment: &ReloadDiffSegment,
    source_tiles: &HashMap<AtlasTileKey, ReloadDiffTile>,
    dirty_rects: &[ReloadPatchRect],
) -> Vec<ReloadDirtySegmentEventRange> {
    if segment.tile_work_list.is_empty() {
        return full_segment_event_range(segment.tile_events);
    }
    let ranges = segment
        .tile_work_list
        .iter()
        .filter(|tile| tile_affects_dirty_rects(tile, source_tiles, dirty_rects))
        .map(|tile| ReloadDirtySegmentEventRange {
            start: tile.event_start,
            end: tile.event_end,
        })
        .collect::<Vec<_>>();
    coalesce_event_ranges(ranges)
}

fn tile_affects_dirty_rects(
    tile: &ReloadDiffSegmentTileRef,
    source_tiles: &HashMap<AtlasTileKey, ReloadDiffTile>,
    dirty_rects: &[ReloadPatchRect],
) -> bool {
    source_tiles
        .get(&tile_key(tile))
        .map(|source_tile| {
            dirty_rects
                .iter()
                .any(|rect| rects_intersect(*source_tile, *rect))
        })
        .unwrap_or(true)
}

fn rects_intersect(tile: ReloadDiffTile, rect: ReloadPatchRect) -> bool {
    let left_x1 = u64::from(tile.x) + u64::from(tile.width);
    let left_y1 = u64::from(tile.y) + u64::from(tile.height);
    let right_x1 = u64::from(rect.x) + u64::from(rect.width);
    let right_y1 = u64::from(rect.y) + u64::from(rect.height);
    u64::from(tile.x) < right_x1
        && u64::from(rect.x) < left_x1
        && u64::from(tile.y) < right_y1
        && u64::from(rect.y) < left_y1
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

fn coalesce_event_ranges(
    mut ranges: Vec<ReloadDirtySegmentEventRange>,
) -> Vec<ReloadDirtySegmentEventRange> {
    ranges.retain(|range| range.end > range.start);
    ranges.sort_by_key(|range| (range.start, range.end));
    let mut output: Vec<ReloadDirtySegmentEventRange> = Vec::with_capacity(ranges.len());
    for range in ranges {
        if let Some(last) = output.last_mut()
            && range.start <= last.end
        {
            last.end = last.end.max(range.end);
            continue;
        }
        output.push(range);
    }
    output
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
