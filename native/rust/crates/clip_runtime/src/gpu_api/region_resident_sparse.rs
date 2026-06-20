use std::collections::HashMap;
use std::time::Instant;

use super::RuntimeGpuRenderer;
use crate::gpu_provider::{
    RuntimeGpuResourceProvider, RuntimeResidentAtlasSlot, RuntimeResidentSourceKey,
    RuntimeResidentSourceKind,
    atlas_cache::{SparseAtlasResourceKind, SparseAtlasUpdatePlan},
    atlas_upload::sparse_atlas_texture_pool_updates,
};
use crate::{
    ClipSession, NormalRasterStackGpuPatchResult, ReloadDiffManifest, ReloadDiffPlan,
    ReloadDiffSource, ReloadDiffTile, RuntimeError, stack_plan::GpuRenderStackSelection,
};

const REGION_RASTER_RESIDENT_ATLAS_ENV: &str = "RIZUM_CLIP_REGION_RASTER_RESIDENT_ATLAS";

impl RuntimeGpuRenderer {
    pub fn draw_normal_raster_stack_patches_with_region_resident_sparse_atlas(
        &self,
        session: &ClipSession,
        plan: &ReloadDiffPlan,
    ) -> Result<
        Option<(
            NormalRasterStackGpuPatchResult,
            crate::GpuSparseAtlasReloadPlan,
        )>,
        RuntimeError,
    > {
        if std::env::var_os(REGION_RASTER_RESIDENT_ATLAS_ENV).is_none()
            || plan.mode != crate::ReloadDiffMode::Patch
            || plan.dirty_rects.is_empty()
        {
            return Ok(None);
        }

        let selection_start = Instant::now();
        let selection =
            session.select_gpu_normal_render_stack(crate::tile_silo_options::tile_silo_options())?;
        clip_gpu::render_profile::record_source_selection(selection_start.elapsed());
        let GpuRenderStackSelection {
            sources,
            resource_plan,
            unsupported,
        } = selection;
        if sources.is_empty() || !unsupported.is_empty() {
            return Ok(None);
        }
        let source_count = sources.len();
        let resource_stats = resource_plan.resource_stats();
        let reload = self.sparse_atlas_cache.borrow_mut().plan_reload_diff(plan);
        let sparse_atlas = reload.clone().into();
        let updates = sparse_atlas_texture_pool_updates(session, &reload.cache)?;
        let update_start = Instant::now();
        self.renderer
            .update_sparse_atlas_texture_pool(
                &mut self.sparse_atlas_textures.borrow_mut(),
                &updates,
            )
            .map_err(RuntimeError::from)?;
        clip_file::decode_profile::record_sparse_atlas_pool_update(update_start.elapsed());
        clip_gpu::render_profile::record_sparse_atlas_update(update_start.elapsed());

        let slots_by_source = resident_slots_by_source(&plan.manifest, &reload.cache);
        if slots_by_source.is_empty() {
            return Ok(None);
        }

        let mut texture_cache = self.texture_cache.borrow_mut();
        let pool = self.sparse_atlas_textures.borrow();
        if !resident_pool_has_all_slots(&pool, &slots_by_source) {
            return Ok(None);
        }
        let mut provider = match texture_cache.as_mut() {
            Some(cache) => {
                cache.begin_frame();
                RuntimeGpuResourceProvider::with_texture_cache(
                    &session.container,
                    session.summary.canvas,
                    resource_plan,
                    cache,
                )?
            }
            None => RuntimeGpuResourceProvider::new(
                &session.container,
                session.summary.canvas,
                resource_plan,
            )?,
        };
        provider.set_region_resident_sparse_atlas(&pool, slots_by_source);

        let mut payload = Vec::new();
        let mut drawn_resources = Vec::new();
        let render_start = Instant::now();
        for rect in &plan.dirty_rects {
            let output = self
                .renderer
                .draw_normal_stack_region_with_provider_to_rgba8(
                    session.summary.canvas,
                    clip_model::Rect::new(rect.x, rect.y, rect.width, rect.height),
                    &sources,
                    &mut provider,
                )?;
            payload.extend_from_slice(&output.pixels);
            drawn_resources.extend(output.drawn_resources);
        }
        clip_file::decode_profile::record_region_patch_render(render_start.elapsed());
        let mask_resources = std::mem::take(&mut provider.mask_resources);
        drop(provider);
        drop(pool);
        let texture_cache_stats = texture_cache
            .as_ref()
            .map(crate::gpu_provider::cache::PersistentGpuTextureCache::frame_stats)
            .unwrap_or_default();

        Ok(Some((
            NormalRasterStackGpuPatchResult {
                payload,
                source_count,
                resource_stats,
                texture_cache_stats,
                drawn_resources,
                mask_resources,
                unsupported,
            },
            sparse_atlas,
        )))
    }
}

fn resident_slots_by_source(
    manifest: &ReloadDiffManifest,
    cache: &SparseAtlasUpdatePlan,
) -> HashMap<RuntimeResidentSourceKey, Vec<RuntimeResidentAtlasSlot>> {
    let tiles = manifest_tile_lookup(&manifest.sources);
    let mut slots_by_source: HashMap<_, Vec<_>> = HashMap::new();
    for update in &cache.updates {
        let Some(kind) = resident_kind(update.fingerprint.tile.kind) else {
            continue;
        };
        let tile_key = ManifestTileKey {
            kind,
            layer_id: update.fingerprint.tile.layer_id,
            resource_id: update.fingerprint.tile.resource_id,
            tile_x: update.fingerprint.tile.tile_x,
            tile_y: update.fingerprint.tile.tile_y,
        };
        let Some(tile) = tiles.get(&tile_key) else {
            continue;
        };
        let source_key = RuntimeResidentSourceKey {
            kind,
            layer_id: update.fingerprint.tile.layer_id,
            resource_id: update.fingerprint.tile.resource_id,
        };
        slots_by_source
            .entry(source_key)
            .or_default()
            .push(RuntimeResidentAtlasSlot {
                canvas_x: tile.x,
                canvas_y: tile.y,
                width: tile.width,
                height: tile.height,
                format: resident_format(kind),
                atlas_id: update.slot.atlas_id,
                atlas_x: update.slot.x,
                atlas_y: update.slot.y,
            });
    }
    for slots in slots_by_source.values_mut() {
        slots.sort_by_key(|slot| {
            (
                slot.canvas_y,
                slot.canvas_x,
                slot.atlas_id,
                slot.atlas_y,
                slot.atlas_x,
            )
        });
    }
    slots_by_source
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct ManifestTileKey {
    kind: RuntimeResidentSourceKind,
    layer_id: u32,
    resource_id: u32,
    tile_x: u32,
    tile_y: u32,
}

fn manifest_tile_lookup(sources: &[ReloadDiffSource]) -> HashMap<ManifestTileKey, ReloadDiffTile> {
    let mut tiles = HashMap::new();
    for source in sources {
        let Some(kind) = source_kind(&source.kind) else {
            continue;
        };
        for tile in &source.tiles {
            tiles.insert(
                ManifestTileKey {
                    kind,
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

fn source_kind(kind: &str) -> Option<RuntimeResidentSourceKind> {
    match kind {
        "raster" => Some(RuntimeResidentSourceKind::Raster),
        "mask" => Some(RuntimeResidentSourceKind::Mask),
        _ => None,
    }
}

fn resident_kind(kind: SparseAtlasResourceKind) -> Option<RuntimeResidentSourceKind> {
    match kind {
        SparseAtlasResourceKind::Raster => Some(RuntimeResidentSourceKind::Raster),
        SparseAtlasResourceKind::Mask => Some(RuntimeResidentSourceKind::Mask),
    }
}

fn resident_format(kind: RuntimeResidentSourceKind) -> clip_gpu::GpuSparseAtlasFormat {
    match kind {
        RuntimeResidentSourceKind::Raster => clip_gpu::GpuSparseAtlasFormat::Rgba8,
        RuntimeResidentSourceKind::Mask => clip_gpu::GpuSparseAtlasFormat::R8,
    }
}

fn resident_pool_has_all_slots(
    pool: &clip_gpu::GpuSparseAtlasTexturePool,
    slots_by_source: &HashMap<RuntimeResidentSourceKey, Vec<RuntimeResidentAtlasSlot>>,
) -> bool {
    slots_by_source.values().flatten().all(|slot| {
        pool.texture(clip_gpu::GpuSparseAtlasTextureKey {
            format: slot.format,
            atlas_id: slot.atlas_id,
        })
        .is_some()
    })
}

#[cfg(test)]
fn resident_raster_run_segment_is_eligible(
    manifest_segment: &crate::ReloadDiffSegment,
    event_segment: &crate::gpu_provider::atlas_events::SparseAtlasRasterEventSegment,
) -> bool {
    if manifest_segment.kind != "RasterRun"
        || manifest_segment.tile_work_list.is_empty()
        || event_segment.event_ranges.is_empty()
        || event_segment.batches.len() != 1
    {
        return false;
    }
    let batch = &event_segment.batches[0];
    if batch.kind != clip_gpu::GpuSparseAtlasRasterEventBatchKind::RasterRun
        || batch.events.is_empty()
        || !batch.filters.is_empty()
        || batch.scope.is_some()
        || !batch.tile_events.is_empty()
    {
        return false;
    }
    let expected_events = event_segment
        .event_ranges
        .iter()
        .map(|range| range.end.saturating_sub(range.start))
        .sum::<u32>();
    expected_events > 0 && expected_events as usize == batch.events.len()
}

#[cfg(test)]
mod tests {
    use clip_gpu::GpuSparseAtlasRasterEventBatch;

    use super::{resident_pool_has_all_slots, resident_raster_run_segment_is_eligible};

    #[test]
    fn resident_raster_run_requires_exact_single_batch_event_coverage() {
        let manifest = segment("RasterRun", 0, 2, 2);
        let event = crate::gpu_provider::atlas_events::SparseAtlasRasterEventSegment {
            ordinal: 7,
            event_ranges: vec![crate::ReloadDirtySegmentEventRange { start: 0, end: 2 }],
            batches: vec![GpuSparseAtlasRasterEventBatch {
                kind: clip_gpu::GpuSparseAtlasRasterEventBatchKind::RasterRun,
                events: vec![raster_event(0), raster_event(1)],
                filters: Vec::new(),
                scope: None,
                tile_events: Vec::new(),
            }],
        };

        assert!(resident_raster_run_segment_is_eligible(&manifest, &event));
    }

    #[test]
    fn resident_raster_run_rejects_conservative_or_split_shapes() {
        let manifest = segment("RasterRun", 0, 2, 2);
        let wrong_count = crate::gpu_provider::atlas_events::SparseAtlasRasterEventSegment {
            ordinal: 7,
            event_ranges: vec![crate::ReloadDirtySegmentEventRange { start: 0, end: 2 }],
            batches: vec![GpuSparseAtlasRasterEventBatch {
                kind: clip_gpu::GpuSparseAtlasRasterEventBatchKind::RasterRun,
                events: vec![raster_event(0)],
                filters: Vec::new(),
                scope: None,
                tile_events: Vec::new(),
            }],
        };
        let split_batch = crate::gpu_provider::atlas_events::SparseAtlasRasterEventSegment {
            ordinal: 7,
            event_ranges: vec![crate::ReloadDirtySegmentEventRange { start: 0, end: 2 }],
            batches: vec![
                GpuSparseAtlasRasterEventBatch {
                    kind: clip_gpu::GpuSparseAtlasRasterEventBatchKind::RasterRun,
                    events: vec![raster_event(0)],
                    filters: Vec::new(),
                    scope: None,
                    tile_events: Vec::new(),
                },
                GpuSparseAtlasRasterEventBatch {
                    kind: clip_gpu::GpuSparseAtlasRasterEventBatchKind::RasterRun,
                    events: vec![raster_event(1)],
                    filters: Vec::new(),
                    scope: None,
                    tile_events: Vec::new(),
                },
            ],
        };

        assert!(!resident_raster_run_segment_is_eligible(
            &manifest,
            &wrong_count
        ));
        assert!(!resident_raster_run_segment_is_eligible(
            &manifest,
            &split_batch
        ));
    }

    #[test]
    fn resident_pool_must_contain_all_referenced_atlases() {
        let pool = clip_gpu::GpuSparseAtlasTexturePool::default();
        let mut slots = std::collections::HashMap::new();
        slots.insert(
            crate::gpu_provider::RuntimeResidentSourceKey::raster(10, 1),
            vec![crate::gpu_provider::RuntimeResidentAtlasSlot {
                canvas_x: 0,
                canvas_y: 0,
                width: 16,
                height: 16,
                format: clip_gpu::GpuSparseAtlasFormat::Rgba8,
                atlas_id: 0,
                atlas_x: 0,
                atlas_y: 0,
            }],
        );

        assert!(!resident_pool_has_all_slots(&pool, &slots));
    }

    fn segment(
        kind: &str,
        source_start: u32,
        source_end: u32,
        tile_events: u32,
    ) -> crate::ReloadDiffSegment {
        crate::ReloadDiffSegment {
            ordinal: 7,
            depth: 0,
            source_start,
            source_end,
            checkpoint_before: false,
            checkpoint_priority: 0,
            kind: kind.to_string(),
            barrier_reason: None,
            expected_passes: 1,
            tile_events,
            legacy_sources: 0,
            resources: Vec::new(),
            tile_work_list_source_count: 1,
            tile_work_list_tile_count: tile_events,
            tile_work_list_signature: 1,
            tile_work_list: (0..tile_events)
                .map(|index| crate::ReloadDiffSegmentTileRef {
                    kind: "raster".to_string(),
                    layer_id: 1,
                    resource_id: 1,
                    tile_x: index,
                    tile_y: 0,
                    event_start: index,
                    event_end: index + 1,
                })
                .collect(),
            signature: 1,
        }
    }

    fn raster_event(index: u32) -> clip_gpu::GpuSparseAtlasRasterEvent {
        clip_gpu::GpuSparseAtlasRasterEvent {
            raster: clip_gpu::GpuSparseAtlasTileRef {
                key: clip_gpu::GpuSparseAtlasTextureKey {
                    format: clip_gpu::GpuSparseAtlasFormat::Rgba8,
                    atlas_id: 0,
                },
                atlas_x: index * 256,
                atlas_y: 0,
                size: clip_model::CanvasSize::new(256, 256),
            },
            source_offset_x: (index * 256) as i32,
            source_offset_y: 0,
            opacity: 1.0,
            blend_mode: clip_gpu::GpuRasterBlendMode::Normal,
            mask: None,
        }
    }
}
