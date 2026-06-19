use std::collections::HashMap;

use super::atlas_cache::{
    SparseAtlasCache, SparseAtlasSlot, SparseAtlasTileUpdate, SparseAtlasUpdateAction,
    SparseAtlasUpdatePlan,
};
use crate::reload_diff::{
    ReloadDiffMode, ReloadDiffPlan, ReloadDiffSegmentTileRef, ReloadDirtySegmentEventRange,
};
use crate::{
    GpuSparseAtlasEventRange, GpuSparseAtlasReloadPlan, GpuSparseAtlasRerunSegment,
    GpuSparseAtlasUpdatedSlot,
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
    pub updated_slots: Vec<SparseAtlasRerunSlot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SparseAtlasRerunSlot {
    pub kind: String,
    pub layer_id: u32,
    pub resource_id: u32,
    pub tile_x: u32,
    pub tile_y: u32,
    pub action: SparseAtlasUpdateAction,
    pub format: clip_gpu::GpuSparseAtlasFormat,
    pub atlas_size: clip_model::CanvasSize,
    pub slot: SparseAtlasSlot,
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
    plan.dirty_segments
        .iter()
        .filter_map(|dirty| {
            let segment = plan
                .manifest
                .segments
                .iter()
                .find(|segment| segment.ordinal == dirty.ordinal)?;
            let updated_slots = segment
                .tile_work_list
                .iter()
                .filter(|tile| event_range_intersects_any(tile, &dirty.dirty_event_ranges))
                .filter_map(|tile| {
                    updates
                        .get(&tile_key(tile))
                        .and_then(|update| rerun_slot_for_update(tile, update, cache.atlas_size))
                })
                .collect::<Vec<_>>();
            Some(SparseAtlasRerunSegment {
                ordinal: dirty.ordinal,
                event_ranges: dirty.dirty_event_ranges.clone(),
                updated_slots,
            })
        })
        .collect()
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
) -> Option<SparseAtlasRerunSlot> {
    if update.action == SparseAtlasUpdateAction::Reuse {
        return None;
    }
    Some(SparseAtlasRerunSlot {
        kind: tile.kind.clone(),
        layer_id: tile.layer_id,
        resource_id: tile.resource_id,
        tile_x: tile.tile_x,
        tile_y: tile.tile_y,
        action: update.action,
        format: update.fingerprint.tile.kind.atlas_format(),
        atlas_size,
        slot: update.slot,
    })
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
        kind: match tile.kind.as_str() {
            "raster" => "raster",
            "mask" => "mask",
            _ => "unknown",
        },
        layer_id: tile.layer_id,
        resource_id: tile.resource_id,
        tile_x: tile.tile_x,
        tile_y: tile.tile_y,
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
            updated_slots: value
                .updated_slots
                .into_iter()
                .map(GpuSparseAtlasUpdatedSlot::from)
                .collect(),
        }
    }
}

impl From<SparseAtlasRerunSlot> for GpuSparseAtlasUpdatedSlot {
    fn from(value: SparseAtlasRerunSlot) -> Self {
        Self {
            kind: value.kind,
            layer_id: value.layer_id,
            resource_id: value.resource_id,
            tile_x: value.tile_x,
            tile_y: value.tile_y,
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
