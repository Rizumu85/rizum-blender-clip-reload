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
pub enum GpuSparseAtlasTileEvent {
    Raster(GpuSparseAtlasRasterEvent),
    PointFilter(GpuSparseAtlasPointFilterEvent),
    BeginScope(GpuSparseAtlasScopeEvent),
    EndScope(GpuSparseAtlasScopeEvent),
    BeginClipBase(GpuSparseAtlasScopeEvent),
    ClipBaseRaster(GpuSparseAtlasRasterEvent),
    ClippedRaster(GpuSparseAtlasRasterEvent),
    ResolveClipBase(GpuSparseAtlasScopeEvent),
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpuSparseAtlasRasterEventBatch {
    pub kind: GpuSparseAtlasRasterEventBatchKind,
    pub events: Vec<GpuSparseAtlasRasterEvent>,
    pub filters: Vec<GpuSparseAtlasPointFilterEvent>,
    pub scope: Option<GpuSparseAtlasScopeEvent>,
    pub tile_events: Vec<GpuSparseAtlasTileEvent>,
}

impl GpuSparseAtlasRasterEventBatch {
    pub fn is_empty(&self) -> bool {
        self.events.is_empty() && self.filters.is_empty() && self.tile_events.is_empty()
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
            tile_events: Vec::new(),
        }
    }

    pub fn point_filter_run(filters: Vec<GpuSparseAtlasPointFilterEvent>) -> Self {
        Self {
            kind: GpuSparseAtlasRasterEventBatchKind::PointFilterRun,
            events: Vec::new(),
            filters,
            scope: None,
            tile_events: Vec::new(),
        }
    }

    pub fn simple_container_scope(
        events: Vec<GpuSparseAtlasRasterEvent>,
        opacity: f32,
        blend_mode: GpuRasterBlendMode,
        local_bounds: clip_model::Rect,
        mask: Option<GpuSparseAtlasTileRef>,
    ) -> Self {
        let tile_events = events
            .iter()
            .copied()
            .map(GpuSparseAtlasTileEvent::Raster)
            .collect();
        Self::simple_container_scope_tile_events(
            tile_events,
            opacity,
            blend_mode,
            local_bounds,
            mask,
        )
    }

    pub fn simple_container_scope_tile_events(
        tile_events: Vec<GpuSparseAtlasTileEvent>,
        opacity: f32,
        blend_mode: GpuRasterBlendMode,
        local_bounds: clip_model::Rect,
        mask: Option<GpuSparseAtlasTileRef>,
    ) -> Self {
        let (events, filters) = split_tile_events(&tile_events);
        Self {
            kind: GpuSparseAtlasRasterEventBatchKind::SimpleContainerScope,
            events,
            filters,
            scope: Some(GpuSparseAtlasScopeEvent {
                kind: GpuSparseAtlasScopeEventKind::Container,
                opacity,
                blend_mode,
                local_bounds,
                mask,
            }),
            tile_events,
        }
    }

    pub fn simple_through_scope(
        events: Vec<GpuSparseAtlasRasterEvent>,
        opacity: f32,
        local_bounds: clip_model::Rect,
        mask: Option<GpuSparseAtlasTileRef>,
    ) -> Self {
        let tile_events = events
            .iter()
            .copied()
            .map(GpuSparseAtlasTileEvent::Raster)
            .collect();
        Self::simple_through_scope_tile_events(tile_events, opacity, local_bounds, mask)
    }

    pub fn simple_through_scope_tile_events(
        tile_events: Vec<GpuSparseAtlasTileEvent>,
        opacity: f32,
        local_bounds: clip_model::Rect,
        mask: Option<GpuSparseAtlasTileRef>,
    ) -> Self {
        let (events, filters) = split_tile_events(&tile_events);
        Self {
            kind: GpuSparseAtlasRasterEventBatchKind::SimpleThroughScope,
            events,
            filters,
            scope: Some(GpuSparseAtlasScopeEvent {
                kind: GpuSparseAtlasScopeEventKind::Through,
                opacity,
                blend_mode: GpuRasterBlendMode::Normal,
                local_bounds,
                mask,
            }),
            tile_events,
        }
    }
}

fn split_tile_events(
    tile_events: &[GpuSparseAtlasTileEvent],
) -> (
    Vec<GpuSparseAtlasRasterEvent>,
    Vec<GpuSparseAtlasPointFilterEvent>,
) {
    let mut events = Vec::new();
    let mut filters = Vec::new();
    for event in tile_events {
        match event {
            GpuSparseAtlasTileEvent::Raster(event) => events.push(*event),
            GpuSparseAtlasTileEvent::ClipBaseRaster(event) => events.push(*event),
            GpuSparseAtlasTileEvent::ClippedRaster(event) => events.push(*event),
            GpuSparseAtlasTileEvent::PointFilter(filter) => filters.push(filter.clone()),
            GpuSparseAtlasTileEvent::BeginScope(_)
            | GpuSparseAtlasTileEvent::EndScope(_)
            | GpuSparseAtlasTileEvent::BeginClipBase(_)
            | GpuSparseAtlasTileEvent::ResolveClipBase(_) => {}
        }
    }
    (events, filters)
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
            tile_events: Vec::new(),
        }
    }
}
