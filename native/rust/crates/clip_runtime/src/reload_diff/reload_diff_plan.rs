use std::collections::{BTreeMap, BTreeSet};

use crate::reload_diff::{
    FULL_DIRTY_AREA_RATIO, MANIFEST_ABI, MAX_PATCH_RECTS, ReloadDiffManifest, ReloadDiffMode,
    ReloadDiffNode, ReloadDiffPlan, ReloadDiffSource, ReloadDiffTile, ReloadPatchRect,
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
    if previous.width != manifest.width
        || previous.height != manifest.height
        || previous.root_layer_id != manifest.root_layer_id
    {
        return full_plan(manifest, "canvas or root layer changed");
    }
    if !same_node_sequence(&previous.nodes, &manifest.nodes) {
        return full_plan(manifest, "visible render graph order changed");
    }

    let mut dirty = Vec::new();
    for (old_node, new_node) in previous.nodes.iter().zip(&manifest.nodes) {
        if old_node.signature == new_node.signature {
            continue;
        }
        match new_node.kind.as_str() {
            "Raster" => {
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
                    dirty.extend(source_bounds(previous, old_source));
                    dirty.extend(source_bounds(&manifest, new_source));
                    continue;
                }
                dirty.extend(changed_tile_rects(old_source, new_source));
            }
            (Some(old_source), None) => dirty.extend(source_bounds(previous, old_source)),
            (None, Some(new_source)) => dirty.extend(source_bounds(&manifest, new_source)),
            (None, None) => {}
        }
    }

    let dirty = coalesce_dirty_rects(dirty);
    if dirty.is_empty() {
        return ReloadDiffPlan {
            manifest,
            mode: ReloadDiffMode::NoChange,
            reason: "reload manifest is unchanged".to_string(),
            dirty_rects: Vec::new(),
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
    }
}

fn same_node_sequence(left: &[ReloadDiffNode], right: &[ReloadDiffNode]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.layer_id == right.layer_id && left.kind == right.kind && left.depth == right.depth
        })
}

fn source_map(manifest: &ReloadDiffManifest) -> BTreeMap<String, &ReloadDiffSource> {
    manifest
        .sources
        .iter()
        .map(|source| (source_identity(source), source))
        .collect()
}

fn source_identity(source: &ReloadDiffSource) -> String {
    format!("{}:{}", source.kind, source.layer_id)
}

fn changed_tile_rects(
    previous: &ReloadDiffSource,
    current: &ReloadDiffSource,
) -> Vec<ReloadPatchRect> {
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
                    vec![tile_rect(**old_tile), tile_rect(**new_tile)]
                }
            }
            (Some(old_tile), None) => vec![tile_rect(**old_tile)],
            (None, Some(new_tile)) => vec![tile_rect(**new_tile)],
            (None, None) => Vec::new(),
        })
        .collect()
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
    }
}
