use crate::{GpuRasterBlendMode, GpuSparseAtlasRasterEvent, GpuSparseAtlasTextureKey};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GpuSparseAtlasRasterEventBatchKind {
    RasterRun,
    RasterClippingRun {
        base_event_count: u32,
        resolve_blend_mode: GpuRasterBlendMode,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpuSparseAtlasRasterEventBatch {
    pub kind: GpuSparseAtlasRasterEventBatchKind,
    pub events: Vec<GpuSparseAtlasRasterEvent>,
}

impl GpuSparseAtlasRasterEventBatch {
    pub fn raster_clipping_run(
        events: Vec<GpuSparseAtlasRasterEvent>,
        base_event_count: u32,
        resolve_blend_mode: GpuRasterBlendMode,
    ) -> Self {
        Self {
            kind: GpuSparseAtlasRasterEventBatchKind::RasterClippingRun {
                base_event_count,
                resolve_blend_mode,
            },
            events,
        }
    }
}

pub fn split_sparse_atlas_raster_event_batches(
    events: &[GpuSparseAtlasRasterEvent],
) -> Vec<GpuSparseAtlasRasterEventBatch> {
    let mut batches = Vec::new();
    let mut current = CurrentSparseAtlasBatch::default();
    for event in events {
        if !current.can_accept(*event) {
            batches.push(current.finish());
        }
        current.push(*event);
    }
    if !current.events.is_empty() {
        batches.push(current.finish());
    }
    batches
}

#[derive(Default)]
struct CurrentSparseAtlasBatch {
    raster_key: Option<GpuSparseAtlasTextureKey>,
    mask_key: Option<GpuSparseAtlasTextureKey>,
    events: Vec<GpuSparseAtlasRasterEvent>,
}

impl CurrentSparseAtlasBatch {
    fn can_accept(&self, event: GpuSparseAtlasRasterEvent) -> bool {
        let Some(raster_key) = self.raster_key else {
            return true;
        };
        if event.raster.key != raster_key {
            return false;
        }
        match (self.mask_key, event.mask.map(|mask| mask.key)) {
            (Some(current), Some(next)) => current == next,
            _ => true,
        }
    }

    fn push(&mut self, event: GpuSparseAtlasRasterEvent) {
        self.raster_key.get_or_insert(event.raster.key);
        if self.mask_key.is_none() {
            self.mask_key = event.mask.map(|mask| mask.key);
        }
        self.events.push(event);
    }

    fn finish(&mut self) -> GpuSparseAtlasRasterEventBatch {
        let events = std::mem::take(&mut self.events);
        self.raster_key = None;
        self.mask_key = None;
        GpuSparseAtlasRasterEventBatch {
            kind: GpuSparseAtlasRasterEventBatchKind::RasterRun,
            events,
        }
    }
}
