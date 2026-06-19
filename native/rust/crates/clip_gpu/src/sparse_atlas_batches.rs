use crate::{
    GpuRasterBlendMode, GpuSparseAtlasPointFilterEvent, GpuSparseAtlasRasterEvent,
    GpuSparseAtlasTextureKey, GpuSparseAtlasTileRef,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GpuSparseAtlasRasterEventBatchKind {
    RasterRun,
    RasterClippingRun {
        base_event_count: u32,
        resolve_blend_mode: GpuRasterBlendMode,
    },
    PointFilterRun,
    SimpleContainerScope,
    SimpleThroughScope,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GpuSparseAtlasScopeEventKind {
    Container,
    Through,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuSparseAtlasScopeEvent {
    pub kind: GpuSparseAtlasScopeEventKind,
    pub opacity: f32,
    pub blend_mode: GpuRasterBlendMode,
    pub local_bounds: clip_model::Rect,
    pub mask: Option<GpuSparseAtlasTileRef>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpuSparseAtlasRasterEventBatch {
    pub kind: GpuSparseAtlasRasterEventBatchKind,
    pub events: Vec<GpuSparseAtlasRasterEvent>,
    pub filters: Vec<GpuSparseAtlasPointFilterEvent>,
    pub scope: Option<GpuSparseAtlasScopeEvent>,
}

impl GpuSparseAtlasRasterEventBatch {
    pub fn is_empty(&self) -> bool {
        self.events.is_empty() && self.filters.is_empty()
    }

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
            filters: Vec::new(),
            scope: None,
        }
    }

    pub fn point_filter_run(filters: Vec<GpuSparseAtlasPointFilterEvent>) -> Self {
        Self {
            kind: GpuSparseAtlasRasterEventBatchKind::PointFilterRun,
            events: Vec::new(),
            filters,
            scope: None,
        }
    }

    pub fn simple_container_scope(
        events: Vec<GpuSparseAtlasRasterEvent>,
        opacity: f32,
        blend_mode: GpuRasterBlendMode,
        local_bounds: clip_model::Rect,
        mask: Option<GpuSparseAtlasTileRef>,
    ) -> Self {
        Self {
            kind: GpuSparseAtlasRasterEventBatchKind::SimpleContainerScope,
            events,
            filters: Vec::new(),
            scope: Some(GpuSparseAtlasScopeEvent {
                kind: GpuSparseAtlasScopeEventKind::Container,
                opacity,
                blend_mode,
                local_bounds,
                mask,
            }),
        }
    }

    pub fn simple_through_scope(
        events: Vec<GpuSparseAtlasRasterEvent>,
        opacity: f32,
        local_bounds: clip_model::Rect,
        mask: Option<GpuSparseAtlasTileRef>,
    ) -> Self {
        Self {
            kind: GpuSparseAtlasRasterEventBatchKind::SimpleThroughScope,
            events,
            filters: Vec::new(),
            scope: Some(GpuSparseAtlasScopeEvent {
                kind: GpuSparseAtlasScopeEventKind::Through,
                opacity,
                blend_mode: GpuRasterBlendMode::Normal,
                local_bounds,
                mask,
            }),
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
            filters: Vec::new(),
            scope: None,
        }
    }
}
