use clip_model::CanvasSize;

use crate::stream_bounds::{CanvasRect, union_optional};
use crate::stream_tile_event::{
    PointFilterTileEventPayload, RasterTileEventPayload, TileEventPayload,
};
use crate::stream_tile_silo_plan::{TILE_SIZE, tile_work_lists_for_bounds};
use crate::{
    GpuRenderError, GpuSparseAtlasFormat, GpuSparseAtlasPointFilterEvent,
    GpuSparseAtlasRasterEvent, GpuSparseAtlasRasterEventBatch, GpuSparseAtlasRasterEventBatchKind,
    GpuSparseAtlasTexture, GpuSparseAtlasTextureKey, GpuSparseAtlasTexturePool,
    GpuSparseAtlasTileRef,
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
    )
}

fn prepare_sparse_atlas_raster_events_with_kind<'a>(
    output_size: CanvasSize,
    pool: &'a GpuSparseAtlasTexturePool,
    kind: GpuSparseAtlasRasterEventBatchKind,
    events: &[GpuSparseAtlasRasterEvent],
    filters: &'a [GpuSparseAtlasPointFilterEvent],
) -> Result<PreparedSparseAtlasRasterEvents<'a>, GpuRenderError> {
    let atlas = if events.is_empty() {
        None
    } else {
        let raster_key = common_raster_atlas_key(events)?;
        let atlas = pool
            .texture(raster_key)
            .ok_or(GpuRenderError::MissingSparseAtlasTexture { key: raster_key })?;
        validate_sparse_atlas_format(atlas, GpuSparseAtlasFormat::Rgba8)?;
        Some(atlas)
    };
    let mask_key = common_mask_atlas_key(events, filters)?;
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
    for event in events {
        let atlas = atlas.expect("raster events must have an atlas");
        validate_tile_ref(atlas, event.raster)?;
        if let (Some(mask_atlas), Some(mask)) = (mask_atlas, event.mask) {
            validate_tile_ref(mask_atlas, mask)?;
        }
        let Some(source_bounds) = CanvasRect::from_source(
            event.source_offset_x,
            event.source_offset_y,
            event.raster.size,
            output_size,
        ) else {
            continue;
        };
        pass_bounds = union_optional(pass_bounds, Some(source_bounds));
        bounds.push(source_bounds);
        payloads.push(TileEventPayload::Raster(RasterTileEventPayload {
            atlas_origin: (event.raster.atlas_x, event.raster.atlas_y),
            source_size: event.raster.size,
            source_offset: (event.source_offset_x, event.source_offset_y),
            opacity: event.opacity,
            blend_mode: event.blend_mode,
            mask_atlas_origin: event.mask.map(|mask| (mask.atlas_x, mask.atlas_y)),
        }));
    }
    for filter in filters {
        validate_filter_event(output_size, filter)?;
        if let (Some(mask_atlas), Some(mask)) = (mask_atlas, filter.mask) {
            validate_tile_ref(mask_atlas, mask)?;
        }
        let bounds_rect = CanvasRect {
            x: filter.local_bounds.x,
            y: filter.local_bounds.y,
            width: filter.local_bounds.width,
            height: filter.local_bounds.height,
        };
        pass_bounds = union_optional(pass_bounds, Some(bounds_rect));
        bounds.push(bounds_rect);
        let lut_row =
            u32::try_from(lut_rows.len()).map_err(|_| GpuRenderError::TextureSizeOverflow)?;
        payloads.push(TileEventPayload::PointFilter(PointFilterTileEventPayload {
            lut_row,
            opacity: filter.opacity,
            filter_mode: filter.filter_mode,
            local_bounds: bounds_rect,
            mask_atlas_origin: filter.mask.map(|mask| (mask.atlas_x, mask.atlas_y)),
        }));
        lut_rows.push(filter.lut_rgba.as_slice());
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
) -> Result<Option<GpuSparseAtlasTextureKey>, GpuRenderError> {
    let mut key = None;
    for mask in events
        .iter()
        .filter_map(|event| event.mask)
        .chain(filters.iter().filter_map(|filter| filter.mask))
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

fn validate_filter_event(
    output_size: CanvasSize,
    filter: &GpuSparseAtlasPointFilterEvent,
) -> Result<(), GpuRenderError> {
    if filter.lut_rgba.len() != 256 * 4 {
        return Err(GpuRenderError::InputBufferSizeMismatch {
            expected: 256 * 4,
            actual: filter.lut_rgba.len(),
        });
    }
    let right = filter
        .local_bounds
        .x
        .checked_add(filter.local_bounds.width)
        .ok_or(GpuRenderError::TextureSizeOverflow)?;
    let bottom = filter
        .local_bounds
        .y
        .checked_add(filter.local_bounds.height)
        .ok_or(GpuRenderError::TextureSizeOverflow)?;
    if filter.local_bounds.is_empty() || right > output_size.width || bottom > output_size.height {
        return Err(GpuRenderError::ReadbackRegionOutOfBounds {
            texture_size: output_size,
            origin_x: filter.local_bounds.x,
            origin_y: filter.local_bounds.y,
            read_size: CanvasSize::new(filter.local_bounds.width, filter.local_bounds.height),
        });
    }
    Ok(())
}

fn validate_sparse_atlas_format(
    atlas: &GpuSparseAtlasTexture,
    expected: GpuSparseAtlasFormat,
) -> Result<(), GpuRenderError> {
    if atlas.format() != expected {
        return Err(GpuRenderError::SparseAtlasFormatMismatch {
            expected,
            actual: atlas.format(),
        });
    }
    Ok(())
}

fn validate_tile_ref(
    atlas: &GpuSparseAtlasTexture,
    tile: GpuSparseAtlasTileRef,
) -> Result<(), GpuRenderError> {
    let right = tile
        .atlas_x
        .checked_add(tile.size.width)
        .ok_or(GpuRenderError::TextureSizeOverflow)?;
    let bottom = tile
        .atlas_y
        .checked_add(tile.size.height)
        .ok_or(GpuRenderError::TextureSizeOverflow)?;
    if right > atlas.size().width || bottom > atlas.size().height {
        return Err(GpuRenderError::UploadRegionOutOfBounds {
            texture_size: atlas.size(),
            origin_x: tile.atlas_x,
            origin_y: tile.atlas_y,
            upload_size: tile.size,
        });
    }
    Ok(())
}
