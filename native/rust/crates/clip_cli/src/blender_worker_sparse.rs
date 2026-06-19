use serde_json::json;

pub(crate) fn sparse_atlas_metadata(
    sparse_atlas: &clip_runtime::GpuSparseAtlasReloadPlan,
) -> serde_json::Value {
    json!({
        "cached_tiles": sparse_atlas.cache_stats.cached_tiles,
        "cached_bytes": sparse_atlas.cache_stats.cached_bytes,
        "atlas_count": sparse_atlas.cache_stats.atlas_count,
        "reused_tiles": sparse_atlas.cache_stats.reused_tiles,
        "inserted_tiles": sparse_atlas.cache_stats.inserted_tiles,
        "changed_tiles": sparse_atlas.cache_stats.changed_tiles,
        "evicted_tiles": sparse_atlas.cache_stats.evicted_tiles,
        "upload_bytes": sparse_atlas.cache_stats.upload_bytes,
        "rerunnable_segments": sparse_atlas.rerunnable_segments.iter().map(|segment| {
            json!({
                "ordinal": segment.ordinal,
                "event_ranges": segment.event_ranges.iter().map(|range| {
                    json!({
                        "start": range.start,
                        "end": range.end,
                    })
                }).collect::<Vec<_>>(),
                "resident_slots": segment.resident_slots.iter().map(sparse_atlas_slot_metadata)
                    .collect::<Vec<_>>(),
                "updated_slots": segment.updated_slots.iter().map(sparse_atlas_slot_metadata)
                    .collect::<Vec<_>>(),
            })
        }).collect::<Vec<_>>(),
    })
}

fn sparse_atlas_slot_metadata(slot: &clip_runtime::GpuSparseAtlasSlot) -> serde_json::Value {
    json!({
        "kind": slot.kind,
        "layer_id": slot.layer_id,
        "resource_id": slot.resource_id,
        "tile_x": slot.tile_x,
        "tile_y": slot.tile_y,
        "event_start": slot.event_start,
        "event_end": slot.event_end,
        "canvas_x": slot.canvas_x,
        "canvas_y": slot.canvas_y,
        "source_x": slot.source_x,
        "source_y": slot.source_y,
        "action": slot.action,
        "format": slot.format,
        "atlas_width": slot.atlas_width,
        "atlas_height": slot.atlas_height,
        "atlas_id": slot.atlas_id,
        "atlas_x": slot.atlas_x,
        "atlas_y": slot.atlas_y,
        "width": slot.width,
        "height": slot.height,
    })
}
