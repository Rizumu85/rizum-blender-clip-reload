use std::collections::BTreeMap;

use clip_model::{LayerId, Rect};

use super::atlas_cache::{SparseAtlasResourceKind, SparseAtlasUpdateAction, SparseAtlasUpdatePlan};
use crate::{ClipSession, RuntimeError};

type PoolUpdateKey = (clip_gpu::GpuSparseAtlasFormat, u32, u32, u32);

pub(crate) fn sparse_atlas_texture_pool_updates(
    session: &ClipSession,
    plan: &SparseAtlasUpdatePlan,
) -> Result<Vec<clip_gpu::GpuSparseAtlasTexturePoolUpdate>, RuntimeError> {
    let mut grouped: BTreeMap<PoolUpdateKey, Vec<clip_gpu::GpuSparseAtlasUpdateChunk>> =
        BTreeMap::new();
    for update in plan
        .updates
        .iter()
        .filter(|update| update.action != SparseAtlasUpdateAction::Reuse)
    {
        let format = update.fingerprint.tile.kind.atlas_format();
        let pixels = decode_update_pixels(session, update)?;
        grouped
            .entry((
                format,
                update.slot.atlas_id,
                plan.atlas_size.width,
                plan.atlas_size.height,
            ))
            .or_default()
            .push(clip_gpu::GpuSparseAtlasUpdateChunk {
                atlas_x: update.slot.x,
                atlas_y: update.slot.y,
                size: clip_model::CanvasSize::new(
                    update.fingerprint.width,
                    update.fingerprint.height,
                ),
                pixels,
            });
    }
    Ok(grouped
        .into_iter()
        .map(|((format, atlas_id, width, height), chunks)| {
            clip_gpu::GpuSparseAtlasTexturePoolUpdate {
                key: clip_gpu::GpuSparseAtlasTextureKey { format, atlas_id },
                atlas_size: clip_model::CanvasSize::new(width, height),
                chunks,
            }
        })
        .collect())
}

fn decode_update_pixels(
    session: &ClipSession,
    update: &super::atlas_cache::SparseAtlasTileUpdate,
) -> Result<Vec<u8>, RuntimeError> {
    let rect = Rect::new(
        update.fingerprint.source_x,
        update.fingerprint.source_y,
        update.fingerprint.width,
        update.fingerprint.height,
    );
    match update.fingerprint.tile.kind {
        SparseAtlasResourceKind::Raster => {
            let source = session
                .raster_sources
                .values()
                .find(|source| {
                    source.layer.id.0 == update.fingerprint.tile.layer_id
                        && source.render_mipmap_id == update.fingerprint.tile.resource_id
                        && source.external_id == update.fingerprint.tile.external_id
                })
                .ok_or(RuntimeError::MissingRasterRenderMipmap {
                    layer_id: LayerId(update.fingerprint.tile.layer_id),
                })?;
            Ok(
                clip_file::read_resolved_raster_layer_source_rgba_region_from_container(
                    &session.container,
                    source,
                    rect,
                )?
                .pixels,
            )
        }
        SparseAtlasResourceKind::Mask => {
            let source = session
                .mask_sources
                .values()
                .find(|source| {
                    source.layer_id.0 == update.fingerprint.tile.layer_id
                        && source.mask_mipmap_id == update.fingerprint.tile.resource_id
                        && source.external_id == update.fingerprint.tile.external_id
                })
                .ok_or(clip_file::ClipFileError::LayerHasNoMask {
                    layer_id: LayerId(update.fingerprint.tile.layer_id),
                })?;
            Ok(
                clip_file::read_resolved_layer_mask_alpha_region_from_container(
                    &session.container,
                    source,
                    rect,
                )?
                .pixels,
            )
        }
    }
}
