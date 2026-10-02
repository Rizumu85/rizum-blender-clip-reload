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
