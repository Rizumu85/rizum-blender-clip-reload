use std::collections::{BTreeMap, BTreeSet};

use crate::reload_diff::{
    FULL_DIRTY_AREA_RATIO, MANIFEST_ABI, MAX_PATCH_RECTS, ReloadDiffManifest, ReloadDiffMode,
    ReloadDiffNode, ReloadDiffPlan, ReloadDiffSegment, ReloadDiffSource, ReloadDiffTile,
    ReloadDirtySegment, ReloadDirtySegmentEventRange, ReloadPatchRect,
};

pub(crate) fn plan_reload_diff(
    previous: &ReloadDiffManifest,
    manifest: ReloadDiffManifest,
) -> ReloadDiffPlan {
    if previous.abi != MANIFEST_ABI || manifest.abi != MANIFEST_ABI {
        return full_plan(manifest, "reload manifest ABI changed");
    }
    if previous.tile_size != manifest.tile_size {
        return full_plan(manifest, "reload tile size changed");
    }
    if previous.tile_event_abi_version != manifest.tile_event_abi_version {
        return full_plan(manifest, "tile event ABI changed");
    }
    if previous.width != manifest.width
        || previous.height != manifest.height
        || previous.root_layer_id != manifest.root_layer_id
    {
        return full_plan(manifest, "canvas or root layer changed");
    }
    if !same_node_sequence(&previous.nodes, &manifest.nodes) {
        return full_plan(manifest, "visible render graph order changed");
    }
    if !same_segment_sequence(&previous.segments, &manifest.segments) {
        return full_plan(manifest, "render segment plan changed");
    }

    let mut dirty = Vec::new();
    let mut dirty_layers = BTreeSet::new();
    let mut dirty_resources = BTreeSet::new();
    let mut dirty_tiles = BTreeSet::new();
    for (old_node, new_node) in previous.nodes.iter().zip(&manifest.nodes) {
        if old_node.signature == new_node.signature {
            continue;
        }
        match new_node.kind.as_str() {
            "Raster" => {
                dirty_layers.insert(old_node.layer_id);
                dirty_layers.insert(new_node.layer_id);
                dirty.extend(source_bounds_for_layer(previous, old_node.layer_id));
                dirty.extend(source_bounds_for_layer(&manifest, new_node.layer_id));
            }
            _ => return full_plan(manifest, "non-raster render node semantics changed"),
        }
    }

    let previous_sources = source_map(previous);
    let current_sources = source_map(&manifest);
    let source_keys: BTreeSet<_> = previous_sources
        .keys()
        .chain(current_sources.keys())
        .cloned()
        .collect();
    for key in source_keys {
        match (previous_sources.get(&key), current_sources.get(&key)) {
            (Some(old_source), Some(new_source)) => {
                if old_source.signature != new_source.signature {
                    dirty_resources.insert(source_key(old_source));
                    dirty_resources.insert(source_key(new_source));
                    dirty.extend(source_bounds(previous, old_source));
                    dirty.extend(source_bounds(&manifest, new_source));
                    continue;
                }
                for change in changed_tile_refs(old_source, new_source) {
                    dirty.push(change.rect);
                    dirty_tiles.insert(change.tile);
                }
            }
            (Some(old_source), None) => {
                dirty_resources.insert(source_key(old_source));
                dirty.extend(source_bounds(previous, old_source));
            }
            (None, Some(new_source)) => {
                dirty_resources.insert(source_key(new_source));
                dirty.extend(source_bounds(&manifest, new_source));
            }
            (None, None) => {}
        }
    }

    let dirty = coalesce_dirty_rects(dirty);
    let dirty_segments =
        dirty_segments_for_changes(&manifest, &dirty_layers, &dirty_resources, &dirty_tiles);
    if dirty.is_empty() {
        return ReloadDiffPlan {
            manifest,
            mode: ReloadDiffMode::NoChange,
            reason: "reload manifest is unchanged".to_string(),
            dirty_rects: Vec::new(),
            dirty_segments: Vec::new(),
        };
    }
    if should_promote_to_full(&manifest, &dirty) {
        return full_plan(manifest, "dirty area is large enough for full upload");
    }
    ReloadDiffPlan {
        manifest,
        mode: ReloadDiffMode::Patch,
        reason: "source tile payload or metadata changed".to_string(),
        dirty_rects: dirty,
        dirty_segments,
    }
}

fn same_node_sequence(left: &[ReloadDiffNode], right: &[ReloadDiffNode]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.layer_id == right.layer_id && left.kind == right.kind && left.depth == right.depth
        })
}

fn same_segment_sequence(left: &[ReloadDiffSegment], right: &[ReloadDiffSegment]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.ordinal == right.ordinal
                && left.depth == right.depth
                && left.source_start == right.source_start
                && left.source_end == right.source_end
                && left.kind == right.kind
                && left.barrier_reason == right.barrier_reason
                && left.signature == right.signature
        })
}

fn source_map(manifest: &ReloadDiffManifest) -> BTreeMap<String, &ReloadDiffSource> {
    manifest
        .sources
        .iter()
        .map(|source| (source_identity(source), source))
        .collect()
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SourceKey {
    kind: String,
    layer_id: u32,
    resource_id: u32,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct TileKey {
    source: SourceKey,
    tile_x: u32,
    tile_y: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ChangedTileRef {
    tile: TileKey,
    rect: ReloadPatchRect,
}

fn source_identity(source: &ReloadDiffSource) -> String {
    format!("{}:{}", source.kind, source.layer_id)
}

fn changed_tile_refs(
    previous: &ReloadDiffSource,
    current: &ReloadDiffSource,
) -> Vec<ChangedTileRef> {
    let old_tiles = tile_map(previous);
    let new_tiles = tile_map(current);
    old_tiles
        .keys()
        .chain(new_tiles.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .flat_map(|key| match (old_tiles.get(&key), new_tiles.get(&key)) {
            (Some(old_tile), Some(new_tile)) => {
                if old_tile.compressed_hash == new_tile.compressed_hash
                    && old_tile.compressed_bytes == new_tile.compressed_bytes
                {
                    Vec::new()
                } else {
                    vec![
                        changed_tile_ref(previous, **old_tile),
                        changed_tile_ref(current, **new_tile),
                    ]
                }
            }
            (Some(old_tile), None) => vec![changed_tile_ref(previous, **old_tile)],
            (None, Some(new_tile)) => vec![changed_tile_ref(current, **new_tile)],
            (None, None) => Vec::new(),
        })
        .collect()
}

fn changed_tile_ref(source: &ReloadDiffSource, tile: ReloadDiffTile) -> ChangedTileRef {
    ChangedTileRef {
        tile: TileKey {
            source: source_key(source),
            tile_x: tile.tile_x,
            tile_y: tile.tile_y,
        },
        rect: tile_rect(tile),
    }
}

fn source_key(source: &ReloadDiffSource) -> SourceKey {
    SourceKey {
        kind: source.kind.clone(),
        layer_id: source.layer_id,
        resource_id: source.resource_id,
    }
}

fn tile_map(source: &ReloadDiffSource) -> BTreeMap<(u32, u32), &ReloadDiffTile> {
    source
        .tiles
        .iter()
        .map(|tile| ((tile.tile_x, tile.tile_y), tile))
        .collect()
}

fn source_bounds_for_layer(manifest: &ReloadDiffManifest, layer_id: u32) -> Vec<ReloadPatchRect> {
    manifest
        .sources
        .iter()
        .filter(|source| source.layer_id == layer_id)
        .flat_map(|source| source_bounds(manifest, source))
        .collect()
}

fn source_bounds(
    manifest: &ReloadDiffManifest,
    source: &ReloadDiffSource,
) -> Option<ReloadPatchRect> {
    let x0 = i64::from(source.offset_x).max(0);
    let y0 = i64::from(source.offset_y).max(0);
    let x1 = (i64::from(source.offset_x) + i64::from(source.width)).min(i64::from(manifest.width));
    let y1 =
        (i64::from(source.offset_y) + i64::from(source.height)).min(i64::from(manifest.height));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(ReloadPatchRect {
        x: u32::try_from(x0).ok()?,
        y: u32::try_from(y0).ok()?,
        width: u32::try_from(x1 - x0).ok()?,
        height: u32::try_from(y1 - y0).ok()?,
    })
}

fn dirty_segments_for_changes(
    manifest: &ReloadDiffManifest,
    dirty_layers: &BTreeSet<u32>,
    dirty_resources: &BTreeSet<SourceKey>,
    dirty_tiles: &BTreeSet<TileKey>,
) -> Vec<ReloadDirtySegment> {
    manifest
        .segments
        .iter()
        .filter_map(|segment| {
            let mut dirty_event_ranges = Vec::new();
            let dirty_resource_count = segment
                .resources
                .iter()
                .filter(|resource| {
                    dirty_layers.contains(&resource.layer_id)
                        || dirty_resources.contains(&SourceKey {
                            kind: resource.kind.clone(),
                            layer_id: resource.layer_id,
                            resource_id: resource.resource_id,
                        })
                })
                .count();
            if dirty_resource_count > 0 {
                dirty_event_ranges.push(segment_full_event_range(segment));
            }
            let dirty_tile_count = segment
                .tile_work_list
                .iter()
                .filter(|tile| {
                    let is_dirty = dirty_tiles.contains(&TileKey {
                        source: SourceKey {
                            kind: tile.kind.clone(),
                            layer_id: tile.layer_id,
                            resource_id: tile.resource_id,
                        },
                        tile_x: tile.tile_x,
                        tile_y: tile.tile_y,
                    });
                    if is_dirty {
                        dirty_event_ranges.push(tile_event_range(tile));
                    }
                    is_dirty
                })
                .count();
            if dirty_resource_count == 0 && dirty_tile_count == 0 {
                return None;
            }
            Some(ReloadDirtySegment {
                ordinal: segment.ordinal,
                dirty_tile_count: usize_to_u32(dirty_tile_count),
                dirty_resource_count: usize_to_u32(dirty_resource_count),
                dirty_event_ranges: coalesce_event_ranges(dirty_event_ranges),
            })
        })
        .collect()
}

fn segment_full_event_range(segment: &ReloadDiffSegment) -> ReloadDirtySegmentEventRange {
    ReloadDirtySegmentEventRange {
        start: 0,
        end: segment.tile_events,
    }
}

fn tile_event_range(
    tile: &crate::reload_diff::ReloadDiffSegmentTileRef,
) -> ReloadDirtySegmentEventRange {
    ReloadDirtySegmentEventRange {
        start: tile.event_start,
        end: tile.event_end,
    }
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

fn tile_rect(tile: ReloadDiffTile) -> ReloadPatchRect {
    ReloadPatchRect {
        x: tile.x,
        y: tile.y,
        width: tile.width,
        height: tile.height,
    }
}

pub(crate) fn coalesce_dirty_rects(mut rects: Vec<ReloadPatchRect>) -> Vec<ReloadPatchRect> {
    rects.retain(|rect| rect.width > 0 && rect.height > 0);
    rects.sort_by_key(|rect| (rect.y, rect.x, rect.width, rect.height));
    rects.dedup();
    let mut changed = true;
    while changed {
        changed = false;
        let mut merged = Vec::with_capacity(rects.len());
        for rect in rects {
            if let Some(existing) = merged
                .iter_mut()
                .find(|existing| can_merge(**existing, rect))
            {
                *existing = merge_rects(*existing, rect);
                changed = true;
            } else {
                merged.push(rect);
            }
        }
        rects = merged;
    }
    rects.sort_by_key(|rect| (rect.y, rect.x, rect.width, rect.height));
    rects
}

fn can_merge(left: ReloadPatchRect, right: ReloadPatchRect) -> bool {
    let left_right = left.x + left.width;
    let right_right = right.x + right.width;
    let left_bottom = left.y + left.height;
    let right_bottom = right.y + right.height;
    left.x <= right_right
        && right.x <= left_right
        && left.y <= right_bottom
        && right.y <= left_bottom
}

fn merge_rects(left: ReloadPatchRect, right: ReloadPatchRect) -> ReloadPatchRect {
    let x0 = left.x.min(right.x);
    let y0 = left.y.min(right.y);
    let x1 = (left.x + left.width).max(right.x + right.width);
    let y1 = (left.y + left.height).max(right.y + right.height);
    ReloadPatchRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

fn should_promote_to_full(manifest: &ReloadDiffManifest, rects: &[ReloadPatchRect]) -> bool {
    if rects.len() > MAX_PATCH_RECTS {
        return true;
    }
    let dirty_area: u64 = rects
        .iter()
        .map(|rect| u64::from(rect.width) * u64::from(rect.height))
        .sum();
    let canvas_area = u64::from(manifest.width) * u64::from(manifest.height);
    canvas_area > 0 && dirty_area as f64 / canvas_area as f64 >= FULL_DIRTY_AREA_RATIO
}

pub(crate) fn full_plan(manifest: ReloadDiffManifest, reason: &str) -> ReloadDiffPlan {
    let rect = ReloadPatchRect {
        x: 0,
        y: 0,
        width: manifest.width,
        height: manifest.height,
    };
    ReloadDiffPlan {
        manifest,
        mode: ReloadDiffMode::Full,
        reason: reason.to_string(),
        dirty_rects: vec![rect],
        dirty_segments: Vec::new(),
    }
}

fn usize_to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
