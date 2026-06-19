use clip_model::CanvasSize;

use crate::sparse_atlas_prepare_payloads::{
    append_clip_base_raster_payload, append_clip_scope_marker, append_clipped_raster_payload,
    append_filter_payload, append_raster_payload, append_scope_marker, scope_payloads,
    validate_scope_event, validate_sparse_atlas_format, validate_tile_ref,
};
use crate::stream_bounds::{CanvasRect, union_optional};
use crate::stream_tile_event::TileEventPayload;
use crate::stream_tile_silo_plan::{TILE_SIZE, tile_work_lists_for_bounds};
use crate::{
    GpuRenderError, GpuSparseAtlasFormat, GpuSparseAtlasPointFilterEvent,
    GpuSparseAtlasRasterEvent, GpuSparseAtlasRasterEventBatch, GpuSparseAtlasRasterEventBatchKind,
    GpuSparseAtlasScopeEvent, GpuSparseAtlasTexture, GpuSparseAtlasTextureKey,
    GpuSparseAtlasTexturePool, GpuSparseAtlasTileEvent,
};

pub(crate) struct PreparedSparseAtlasRasterEvents<'a> {
    pub(crate) kind: GpuSparseAtlasRasterEventBatchKind,
    pub(crate) atlas: Option<&'a GpuSparseAtlasTexture>,
    pub(crate) mask_atlas: Option<&'a GpuSparseAtlasTexture>,
    pub(crate) payloads: Vec<TileEventPayload>,
    pub(crate) lut_rows: Vec<&'a [u8]>,
    pub(crate) work_indices: Vec<u32>,
    pub(crate) tile_spans: Vec<u32>,
    pub(crate) tile_cols: u32,
    pub(crate) pass_bounds: CanvasRect,
}

pub(crate) fn prepare_sparse_atlas_raster_events<'a>(
    output_size: CanvasSize,
    pool: &'a GpuSparseAtlasTexturePool,
    events: &[GpuSparseAtlasRasterEvent],
) -> Result<PreparedSparseAtlasRasterEvents<'a>, GpuRenderError> {
    prepare_sparse_atlas_raster_events_with_kind(
        output_size,
        pool,
        GpuSparseAtlasRasterEventBatchKind::RasterRun,
        events,
        &[],
        None,
        &[],
    )
}

pub(crate) fn prepare_sparse_atlas_raster_event_batch<'a>(
    output_size: CanvasSize,
    pool: &'a GpuSparseAtlasTexturePool,
    batch: &'a GpuSparseAtlasRasterEventBatch,
) -> Result<PreparedSparseAtlasRasterEvents<'a>, GpuRenderError> {
    prepare_sparse_atlas_raster_events_with_kind(
        output_size,
        pool,
        batch.kind,
        &batch.events,
        &batch.filters,
        batch.scope,
        &batch.tile_events,
    )
}

fn prepare_sparse_atlas_raster_events_with_kind<'a>(
    output_size: CanvasSize,
    pool: &'a GpuSparseAtlasTexturePool,
    kind: GpuSparseAtlasRasterEventBatchKind,
    events: &[GpuSparseAtlasRasterEvent],
    filters: &'a [GpuSparseAtlasPointFilterEvent],
    scope: Option<GpuSparseAtlasScopeEvent>,
    tile_events: &'a [GpuSparseAtlasTileEvent],
) -> Result<PreparedSparseAtlasRasterEvents<'a>, GpuRenderError> {
    let raster_events = raster_events_for_prepare(events, tile_events);
    let atlas = if raster_events.is_empty() {
        None
    } else {
        let raster_key = common_raster_atlas_key(&raster_events)?;
        let atlas = pool
            .texture(raster_key)
            .ok_or(GpuRenderError::MissingSparseAtlasTexture { key: raster_key })?;
        validate_sparse_atlas_format(atlas, GpuSparseAtlasFormat::Rgba8)?;
        Some(atlas)
    };
    let mask_key = common_mask_atlas_key(events, filters, scope, tile_events)?;
    let mask_atlas = match mask_key {
        Some(key) => {
            let atlas = pool
                .texture(key)
                .ok_or(GpuRenderError::MissingSparseAtlasTexture { key })?;
            validate_sparse_atlas_format(atlas, GpuSparseAtlasFormat::R8)?;
            Some(atlas)
        }
        None => None,
    };

    let mut payloads = Vec::new();
    let mut lut_rows = Vec::new();
    let mut bounds = Vec::new();
    let mut pass_bounds = None;
    if let Some(scope) = scope {
        validate_scope_event(output_size, &scope)?;
        if let (Some(mask_atlas), Some(mask)) = (mask_atlas, scope.mask) {
            validate_tile_ref(mask_atlas, mask)?;
        }
        let (begin, end, bounds_rect) = scope_payloads(scope);
        pass_bounds = union_optional(pass_bounds, Some(bounds_rect));
        bounds.push(bounds_rect);
        payloads.push(begin);
        if tile_events.is_empty() {
            for event in events {
                append_raster_payload(
                    output_size,
                    atlas,
                    mask_atlas,
                    *event,
                    &mut payloads,
                    &mut bounds,
                    &mut pass_bounds,
                )?;
            }
        } else {
            for event in tile_events {
                match event {
                    GpuSparseAtlasTileEvent::Raster(event) => append_raster_payload(
                        output_size,
                        atlas,
                        mask_atlas,
                        *event,
                        &mut payloads,
                        &mut bounds,
                        &mut pass_bounds,
                    )?,
                    GpuSparseAtlasTileEvent::ClipBaseRaster(event) => {
                        append_clip_base_raster_payload(
                            output_size,
                            atlas,
                            mask_atlas,
                            *event,
                            &mut payloads,
                            &mut bounds,
                            &mut pass_bounds,
                        )?
                    }
                    GpuSparseAtlasTileEvent::ClippedRaster(event) => append_clipped_raster_payload(
                        output_size,
                        atlas,
                        mask_atlas,
                        *event,
                        &mut payloads,
                        &mut bounds,
                        &mut pass_bounds,
                    )?,
                    GpuSparseAtlasTileEvent::BeginScope(scope) => {
                        append_scope_marker(output_size, *scope, true, &mut payloads, &mut bounds)?
                    }
                    GpuSparseAtlasTileEvent::EndScope(scope) => {
                        append_scope_marker(output_size, *scope, false, &mut payloads, &mut bounds)?
                    }
                    GpuSparseAtlasTileEvent::BeginClipBase(scope) => append_clip_scope_marker(
                        output_size,
                        *scope,
                        true,
                        &mut payloads,
                        &mut bounds,
                    )?,
                    GpuSparseAtlasTileEvent::ResolveClipBase(scope) => append_clip_scope_marker(
                        output_size,
                        *scope,
                        false,
                        &mut payloads,
                        &mut bounds,
                    )?,
                    GpuSparseAtlasTileEvent::PointFilter(filter) => append_filter_payload(
                        output_size,
                        mask_atlas,
                        filter,
                        &mut payloads,
                        &mut lut_rows,
                        &mut bounds,
                        &mut pass_bounds,
                    )?,
                }
            }
        }
        bounds.push(bounds_rect);
        payloads.push(end);
    } else {
        for event in events {
            append_raster_payload(
                output_size,
                atlas,
                mask_atlas,
                *event,
                &mut payloads,
                &mut bounds,
                &mut pass_bounds,
            )?;
        }
        for filter in filters {
            append_filter_payload(
                output_size,
                mask_atlas,
                filter,
                &mut payloads,
                &mut lut_rows,
                &mut bounds,
                &mut pass_bounds,
            )?;
        }
    }
    let pass_bounds = match pass_bounds {
        Some(bounds) => bounds,
        None => CanvasRect::full(output_size).ok_or(GpuRenderError::InvalidImageSize)?,
    };
    let tile_cols = output_size.width.div_ceil(TILE_SIZE);
    let tile_count =
        usize::try_from(u64::from(tile_cols) * u64::from(output_size.height.div_ceil(TILE_SIZE)))
            .map_err(|_| GpuRenderError::TextureSizeOverflow)?;
    let (work_indices, tile_spans) = tile_work_lists_for_bounds(tile_count, tile_cols, &bounds)?;
    Ok(PreparedSparseAtlasRasterEvents {
        kind,
        atlas,
        mask_atlas,
        payloads,
        lut_rows,
        work_indices,
        tile_spans,
        tile_cols,
        pass_bounds,
    })
}

fn raster_events_for_prepare(
    events: &[GpuSparseAtlasRasterEvent],
    tile_events: &[GpuSparseAtlasTileEvent],
) -> Vec<GpuSparseAtlasRasterEvent> {
    if tile_events.is_empty() {
        return events.to_vec();
    }
    tile_events
        .iter()
        .filter_map(|event| match event {
            GpuSparseAtlasTileEvent::Raster(event) => Some(*event),
            GpuSparseAtlasTileEvent::ClipBaseRaster(event) => Some(*event),
            GpuSparseAtlasTileEvent::ClippedRaster(event) => Some(*event),
            GpuSparseAtlasTileEvent::PointFilter(_) => None,
            GpuSparseAtlasTileEvent::BeginScope(_)
            | GpuSparseAtlasTileEvent::EndScope(_)
            | GpuSparseAtlasTileEvent::BeginClipBase(_)
            | GpuSparseAtlasTileEvent::ResolveClipBase(_) => None,
        })
        .collect()
}

fn common_raster_atlas_key(
    events: &[GpuSparseAtlasRasterEvent],
) -> Result<GpuSparseAtlasTextureKey, GpuRenderError> {
    let key = events[0].raster.key;
    if key.format != GpuSparseAtlasFormat::Rgba8 {
        return Err(GpuRenderError::SparseAtlasFormatMismatch {
            expected: GpuSparseAtlasFormat::Rgba8,
            actual: key.format,
        });
    }
    if events.iter().any(|event| event.raster.key != key) {
        return Err(GpuRenderError::SparseAtlasMixedTextureKeys);
    }
    Ok(key)
}

fn common_mask_atlas_key(
    events: &[GpuSparseAtlasRasterEvent],
    filters: &[GpuSparseAtlasPointFilterEvent],
    scope: Option<GpuSparseAtlasScopeEvent>,
    tile_events: &[GpuSparseAtlasTileEvent],
) -> Result<Option<GpuSparseAtlasTextureKey>, GpuRenderError> {
    let mut key = None;
    let event_masks = if tile_events.is_empty() {
        events
            .iter()
            .filter_map(|event| event.mask)
            .collect::<Vec<_>>()
    } else {
        tile_events
            .iter()
            .filter_map(|event| match event {
                GpuSparseAtlasTileEvent::Raster(event) => event.mask,
                GpuSparseAtlasTileEvent::ClipBaseRaster(event) => event.mask,
                GpuSparseAtlasTileEvent::ClippedRaster(event) => event.mask,
                GpuSparseAtlasTileEvent::PointFilter(filter) => filter.mask,
                GpuSparseAtlasTileEvent::BeginScope(scope)
                | GpuSparseAtlasTileEvent::EndScope(scope)
                | GpuSparseAtlasTileEvent::BeginClipBase(scope)
                | GpuSparseAtlasTileEvent::ResolveClipBase(scope) => scope.mask,
            })
            .collect()
    };
    for mask in event_masks
        .into_iter()
        .chain(filters.iter().filter_map(|filter| filter.mask))
        .chain(scope.into_iter().filter_map(|scope| scope.mask))
    {
        if mask.key.format != GpuSparseAtlasFormat::R8 {
            return Err(GpuRenderError::SparseAtlasFormatMismatch {
                expected: GpuSparseAtlasFormat::R8,
                actual: mask.key.format,
            });
        }
        if let Some(existing) = key {
            if existing != mask.key {
                return Err(GpuRenderError::SparseAtlasMixedTextureKeys);
            }
        } else {
            key = Some(mask.key);
        }
    }
    Ok(key)
}
