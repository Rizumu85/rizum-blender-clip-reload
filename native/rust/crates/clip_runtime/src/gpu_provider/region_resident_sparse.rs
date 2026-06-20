use std::collections::HashMap;

use clip_model::CanvasSize;

const MAX_REGION_RESIDENT_RASTER_RUN_SOURCES: usize = 32;
const MAX_REGION_RESIDENT_RASTER_RUN_EVENTS: usize = 64;

#[derive(Debug)]
pub(crate) struct RuntimeRegionResidentSparseAtlas<'a> {
    pub(crate) pool: &'a clip_gpu::GpuSparseAtlasTexturePool,
    slots_by_source: HashMap<RuntimeResidentSourceKey, Vec<RuntimeResidentAtlasSlot>>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum RuntimeResidentSourceKind {
    Raster,
    Mask,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct RuntimeResidentSourceKey {
    pub(crate) kind: RuntimeResidentSourceKind,
    pub(crate) layer_id: u32,
    pub(crate) resource_id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeResidentAtlasSlot {
    pub(crate) canvas_x: u32,
    pub(crate) canvas_y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) format: clip_gpu::GpuSparseAtlasFormat,
    pub(crate) atlas_id: u32,
    pub(crate) atlas_x: u32,
    pub(crate) atlas_y: u32,
}

impl<'a> RuntimeRegionResidentSparseAtlas<'a> {
    pub(crate) fn new(
        pool: &'a clip_gpu::GpuSparseAtlasTexturePool,
        slots_by_source: HashMap<RuntimeResidentSourceKey, Vec<RuntimeResidentAtlasSlot>>,
    ) -> Self {
        Self {
            pool,
            slots_by_source,
        }
    }

    pub(crate) fn raster_run_batches(
        &self,
        canvas: CanvasSize,
        target_origin: (i32, i32),
        target_size: CanvasSize,
        sources: &[clip_gpu::GpuNormalStackSource],
    ) -> Option<Vec<clip_gpu::GpuSparseAtlasRasterEventBatch>> {
        if sources.len() > MAX_REGION_RESIDENT_RASTER_RUN_SOURCES {
            return None;
        }
        let target = target_rect(target_origin, target_size, canvas)?;
        let mut events = Vec::new();
        for source in sources {
            let clip_gpu::GpuNormalStackSource::Raster(raster) = source else {
                return None;
            };
            let source_bounds = self.raster_source_bounds(*raster, canvas)?;
            if !rects_intersect(source_bounds, target) {
                continue;
            }
            let mut raster_slots = self
                .slots_for(RuntimeResidentSourceKey::raster(
                    raster.key.layer_id.0,
                    raster.key.render_mipmap_id,
                ))?
                .iter()
                .filter(|slot| rects_intersect(slot.rect(), target))
                .copied()
                .collect::<Vec<_>>();
            if raster_slots.is_empty() {
                return None;
            }
            raster_slots.sort_by_key(|slot| {
                (
                    slot.canvas_y,
                    slot.canvas_x,
                    slot.atlas_id,
                    slot.atlas_y,
                    slot.atlas_x,
                )
            });
            for slot in raster_slots {
                if events.len() >= MAX_REGION_RESIDENT_RASTER_RUN_EVENTS {
                    return None;
                }
                let mask = match raster.mask_key {
                    Some(mask_key) => Some(self.mask_slot_for_raster(mask_key, slot)?),
                    None => None,
                };
                events.push(clip_gpu::GpuSparseAtlasRasterEvent {
                    raster: slot.tile_ref(),
                    source_offset_x: i32::try_from(slot.canvas_x).ok()?,
                    source_offset_y: i32::try_from(slot.canvas_y).ok()?,
                    opacity: raster.opacity,
                    blend_mode: raster.blend_mode,
                    mask,
                });
            }
        }
        if events.is_empty() {
            return None;
        }
        Some(clip_gpu::split_sparse_atlas_raster_event_batches(&events))
    }

    fn raster_source_bounds(
        &self,
        raster: clip_gpu::GpuNormalRasterSource,
        canvas: CanvasSize,
    ) -> Option<ResidentRect> {
        let slots = self.slots_for(RuntimeResidentSourceKey::raster(
            raster.key.layer_id.0,
            raster.key.render_mipmap_id,
        ))?;
        let min_x = slots.iter().map(|slot| slot.canvas_x).min()?;
        let min_y = slots.iter().map(|slot| slot.canvas_y).min()?;
        let max_right = slots
            .iter()
            .filter_map(|slot| slot.canvas_x.checked_add(slot.width))
            .max()?;
        let max_bottom = slots
            .iter()
            .filter_map(|slot| slot.canvas_y.checked_add(slot.height))
            .max()?;
        let x0 = min_x.min(canvas.width);
        let y0 = min_y.min(canvas.height);
        let x1 = max_right.min(canvas.width);
        let y1 = max_bottom.min(canvas.height);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some(ResidentRect {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        })
    }

    fn slots_for(&self, key: RuntimeResidentSourceKey) -> Option<&[RuntimeResidentAtlasSlot]> {
        self.slots_by_source.get(&key).map(Vec::as_slice)
    }

    fn mask_slot_for_raster(
        &self,
        mask_key: clip_gpu::GpuMaskResourceKey,
        raster_slot: RuntimeResidentAtlasSlot,
    ) -> Option<clip_gpu::GpuSparseAtlasTileRef> {
        self.slots_for(RuntimeResidentSourceKey::mask(
            mask_key.layer_id.0,
            mask_key.mask_mipmap_id,
        ))?
        .iter()
        .find(|slot| {
            slot.canvas_x == raster_slot.canvas_x
                && slot.canvas_y == raster_slot.canvas_y
                && slot.width == raster_slot.width
                && slot.height == raster_slot.height
        })
        .map(|slot| slot.tile_ref())
    }
}

impl RuntimeResidentSourceKey {
    pub(crate) fn raster(layer_id: u32, resource_id: u32) -> Self {
        Self {
            kind: RuntimeResidentSourceKind::Raster,
            layer_id,
            resource_id,
        }
    }

    pub(crate) fn mask(layer_id: u32, resource_id: u32) -> Self {
        Self {
            kind: RuntimeResidentSourceKind::Mask,
            layer_id,
            resource_id,
        }
    }
}

impl RuntimeResidentAtlasSlot {
    fn rect(self) -> ResidentRect {
        ResidentRect {
            x: self.canvas_x,
            y: self.canvas_y,
            width: self.width,
            height: self.height,
        }
    }

    fn tile_ref(self) -> clip_gpu::GpuSparseAtlasTileRef {
        clip_gpu::GpuSparseAtlasTileRef {
            key: clip_gpu::GpuSparseAtlasTextureKey {
                format: self.format,
                atlas_id: self.atlas_id,
            },
            atlas_x: self.atlas_x,
            atlas_y: self.atlas_y,
            size: CanvasSize::new(self.width, self.height),
        }
    }
}

#[derive(Clone, Copy)]
struct ResidentRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

fn target_rect(
    target_origin: (i32, i32),
    target_size: CanvasSize,
    canvas: CanvasSize,
) -> Option<ResidentRect> {
    let x = u32::try_from(target_origin.0).ok()?;
    let y = u32::try_from(target_origin.1).ok()?;
    let rect = ResidentRect {
        x,
        y,
        width: target_size.width,
        height: target_size.height,
    };
    if rect.width == 0
        || rect.height == 0
        || rect.right()? > canvas.width
        || rect.bottom()? > canvas.height
    {
        return None;
    }
    Some(rect)
}

fn rects_intersect(left: ResidentRect, right: ResidentRect) -> bool {
    let (Some(left_right), Some(left_bottom), Some(right_right), Some(right_bottom)) =
        (left.right(), left.bottom(), right.right(), right.bottom())
    else {
        return false;
    };
    left.x < right_right && right.x < left_right && left.y < right_bottom && right.y < left_bottom
}

impl ResidentRect {
    fn right(self) -> Option<u32> {
        self.x.checked_add(self.width)
    }

    fn bottom(self) -> Option<u32> {
        self.y.checked_add(self.height)
    }
}

#[cfg(test)]
mod tests {
    use clip_model::LayerId;

    use super::*;

    #[test]
    fn resident_raster_run_builds_canvas_tile_events_for_small_dirty_region() {
        let pool = clip_gpu::GpuSparseAtlasTexturePool::default();
        let resident = resident_atlas(&pool, vec![slot(10, 1, 12, 34)]);

        let batches = resident
            .raster_run_batches(
                CanvasSize::new(512, 512),
                (0, 0),
                CanvasSize::new(128, 128),
                &[clip_gpu::GpuNormalStackSource::Raster(raster_source(10, 1))],
            )
            .expect("resident raster batch");

        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].events.len(), 1);
        assert_eq!(batches[0].events[0].source_offset_x, 12);
        assert_eq!(batches[0].events[0].source_offset_y, 34);
        assert_eq!(batches[0].events[0].raster.atlas_x, 100);
        assert_eq!(batches[0].events[0].raster.atlas_y, 200);
    }

    #[test]
    fn resident_raster_run_fails_closed_for_large_source_count() {
        let pool = clip_gpu::GpuSparseAtlasTexturePool::default();
        let slots = (0..=MAX_REGION_RESIDENT_RASTER_RUN_SOURCES)
            .map(|index| slot(index as u32 + 1, 1, index as u32, 0))
            .collect();
        let resident = resident_atlas(&pool, slots);
        let sources = (0..=MAX_REGION_RESIDENT_RASTER_RUN_SOURCES)
            .map(|index| clip_gpu::GpuNormalStackSource::Raster(raster_source(index as u32 + 1, 1)))
            .collect::<Vec<_>>();

        assert!(
            resident
                .raster_run_batches(
                    CanvasSize::new(512, 512),
                    (0, 0),
                    CanvasSize::new(128, 128),
                    &sources,
                )
                .is_none()
        );
    }

    #[test]
    fn resident_raster_run_fails_closed_for_large_event_count() {
        let pool = clip_gpu::GpuSparseAtlasTexturePool::default();
        let slots = (0..=MAX_REGION_RESIDENT_RASTER_RUN_EVENTS)
            .map(|index| RuntimeResidentAtlasSlot {
                canvas_x: index as u32,
                canvas_y: 0,
                width: 1,
                height: 1,
                format: clip_gpu::GpuSparseAtlasFormat::Rgba8,
                atlas_id: 3,
                atlas_x: index as u32,
                atlas_y: 0,
            })
            .collect();
        let resident =
            resident_atlas_for_key(&pool, RuntimeResidentSourceKey::raster(10, 1), slots);

        assert!(
            resident
                .raster_run_batches(
                    CanvasSize::new(512, 512),
                    (0, 0),
                    CanvasSize::new(128, 128),
                    &[clip_gpu::GpuNormalStackSource::Raster(raster_source(10, 1))],
                )
                .is_none()
        );
    }

    fn resident_atlas<'a>(
        pool: &'a clip_gpu::GpuSparseAtlasTexturePool,
        slots: Vec<RuntimeResidentAtlasSlot>,
    ) -> RuntimeRegionResidentSparseAtlas<'a> {
        resident_atlas_for_key(pool, RuntimeResidentSourceKey::raster(10, 1), slots)
    }

    fn resident_atlas_for_key<'a>(
        pool: &'a clip_gpu::GpuSparseAtlasTexturePool,
        key: RuntimeResidentSourceKey,
        slots: Vec<RuntimeResidentAtlasSlot>,
    ) -> RuntimeRegionResidentSparseAtlas<'a> {
        let mut slots_by_source = HashMap::new();
        slots_by_source.insert(key, slots);
        RuntimeRegionResidentSparseAtlas::new(pool, slots_by_source)
    }

    fn slot(
        layer_id: u32,
        resource_id: u32,
        canvas_x: u32,
        canvas_y: u32,
    ) -> RuntimeResidentAtlasSlot {
        let _ = (layer_id, resource_id);
        RuntimeResidentAtlasSlot {
            canvas_x,
            canvas_y,
            width: 16,
            height: 16,
            format: clip_gpu::GpuSparseAtlasFormat::Rgba8,
            atlas_id: 3,
            atlas_x: 100,
            atlas_y: 200,
        }
    }

    fn raster_source(layer_id: u32, render_mipmap_id: u32) -> clip_gpu::GpuNormalRasterSource {
        clip_gpu::GpuNormalRasterSource {
            key: clip_gpu::GpuRasterResourceKey {
                layer_id: LayerId(layer_id),
                render_mipmap_id,
            },
            opacity: 1.0,
            mask_key: None,
            offset_x: 0,
            offset_y: 0,
            blend_mode: clip_gpu::GpuRasterBlendMode::Normal,
        }
    }
}
