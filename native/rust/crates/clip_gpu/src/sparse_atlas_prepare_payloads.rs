use clip_model::CanvasSize;

use crate::stream_bounds::{CanvasRect, union_optional};
use crate::stream_tile_event::{
    PointFilterTileEventPayload, RasterTileEventPayload, ScopeTileEventPayload,
    SolidColorTileEventPayload, TileEventPayload,
};
use crate::{
    GpuRenderError, GpuSparseAtlasFormat, GpuSparseAtlasPointFilterEvent,
    GpuSparseAtlasRasterEvent, GpuSparseAtlasScopeEvent, GpuSparseAtlasScopeEventKind,
    GpuSparseAtlasSolidColorEvent, GpuSparseAtlasTexture, GpuSparseAtlasTileRef,
};

pub(crate) fn append_raster_payload(
    output_size: CanvasSize,
    atlas: Option<&GpuSparseAtlasTexture>,
    mask_atlas: Option<&GpuSparseAtlasTexture>,
    event: GpuSparseAtlasRasterEvent,
    payloads: &mut Vec<TileEventPayload>,
    bounds: &mut Vec<CanvasRect>,
    pass_bounds: &mut Option<CanvasRect>,
) -> Result<(), GpuRenderError> {
    append_raster_payload_kind(
        output_size,
        atlas,
        mask_atlas,
        event,
        TileEventPayload::Raster,
        payloads,
        bounds,
        pass_bounds,
    )
}

pub(crate) fn append_clip_base_raster_payload(
    output_size: CanvasSize,
    atlas: Option<&GpuSparseAtlasTexture>,
    mask_atlas: Option<&GpuSparseAtlasTexture>,
    event: GpuSparseAtlasRasterEvent,
    payloads: &mut Vec<TileEventPayload>,
    bounds: &mut Vec<CanvasRect>,
    pass_bounds: &mut Option<CanvasRect>,
) -> Result<(), GpuRenderError> {
    append_raster_payload_kind(
        output_size,
        atlas,
        mask_atlas,
        event,
        TileEventPayload::ClipBaseRaster,
        payloads,
        bounds,
        pass_bounds,
    )
}

pub(crate) fn append_clipped_raster_payload(
    output_size: CanvasSize,
    atlas: Option<&GpuSparseAtlasTexture>,
    mask_atlas: Option<&GpuSparseAtlasTexture>,
    event: GpuSparseAtlasRasterEvent,
    payloads: &mut Vec<TileEventPayload>,
    bounds: &mut Vec<CanvasRect>,
    pass_bounds: &mut Option<CanvasRect>,
) -> Result<(), GpuRenderError> {
    append_raster_payload_kind(
        output_size,
        atlas,
        mask_atlas,
        event,
        TileEventPayload::ClippedRaster,
        payloads,
        bounds,
        pass_bounds,
    )
}

fn append_raster_payload_kind(
    output_size: CanvasSize,
    atlas: Option<&GpuSparseAtlasTexture>,
    mask_atlas: Option<&GpuSparseAtlasTexture>,
    event: GpuSparseAtlasRasterEvent,
    payload_kind: fn(RasterTileEventPayload) -> TileEventPayload,
    payloads: &mut Vec<TileEventPayload>,
    bounds: &mut Vec<CanvasRect>,
    pass_bounds: &mut Option<CanvasRect>,
) -> Result<(), GpuRenderError> {
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
        return Ok(());
    };
    *pass_bounds = union_optional(*pass_bounds, Some(source_bounds));
    bounds.push(source_bounds);
    payloads.push(payload_kind(RasterTileEventPayload {
        atlas_origin: (event.raster.atlas_x, event.raster.atlas_y),
        source_size: event.raster.size,
        source_offset: (event.source_offset_x, event.source_offset_y),
        opacity: event.opacity,
        blend_mode: event.blend_mode,
        mask_atlas_origin: event.mask.map(|mask| (mask.atlas_x, mask.atlas_y)),
    }));
    Ok(())
}

pub(crate) fn append_filter_payload<'a>(
    output_size: CanvasSize,
    mask_atlas: Option<&GpuSparseAtlasTexture>,
    filter: &'a GpuSparseAtlasPointFilterEvent,
    payloads: &mut Vec<TileEventPayload>,
    lut_rows: &mut Vec<&'a [u8]>,
    bounds: &mut Vec<CanvasRect>,
    pass_bounds: &mut Option<CanvasRect>,
) -> Result<(), GpuRenderError> {
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
    *pass_bounds = union_optional(*pass_bounds, Some(bounds_rect));
    bounds.push(bounds_rect);
    let lut_row = u32::try_from(lut_rows.len()).map_err(|_| GpuRenderError::TextureSizeOverflow)?;
    payloads.push(TileEventPayload::PointFilter(PointFilterTileEventPayload {
        lut_row,
        opacity: filter.opacity,
        filter_mode: filter.filter_mode,
        local_bounds: bounds_rect,
        mask_atlas_origin: filter.mask.map(|mask| (mask.atlas_x, mask.atlas_y)),
    }));
    lut_rows.push(filter.lut_rgba.as_slice());
    Ok(())
}

pub(crate) fn append_solid_color_payload(
    output_size: CanvasSize,
    event: GpuSparseAtlasSolidColorEvent,
    payloads: &mut Vec<TileEventPayload>,
    bounds: &mut Vec<CanvasRect>,
    pass_bounds: &mut Option<CanvasRect>,
) -> Result<(), GpuRenderError> {
    validate_solid_color_event(output_size, &event)?;
    let bounds_rect = CanvasRect {
        x: event.local_bounds.x,
        y: event.local_bounds.y,
        width: event.local_bounds.width,
        height: event.local_bounds.height,
    };
    *pass_bounds = union_optional(*pass_bounds, Some(bounds_rect));
    bounds.push(bounds_rect);
    payloads.push(TileEventPayload::SolidColor(SolidColorTileEventPayload {
        color: event.color,
        opacity: event.opacity,
        local_bounds: bounds_rect,
    }));
    Ok(())
}

pub(crate) fn append_clip_scope_marker(
    output_size: CanvasSize,
    scope: GpuSparseAtlasScopeEvent,
    begin: bool,
    payloads: &mut Vec<TileEventPayload>,
    bounds: &mut Vec<CanvasRect>,
) -> Result<(), GpuRenderError> {
    validate_scope_event(output_size, &scope)?;
    let (_, _, bounds_rect) = scope_payloads(scope);
    let payload = ScopeTileEventPayload {
        opacity: scope.opacity,
        blend_mode: scope.blend_mode,
        local_bounds: bounds_rect,
        mask_atlas_origin: scope.mask.map(|mask| (mask.atlas_x, mask.atlas_y)),
    };
    bounds.push(bounds_rect);
    payloads.push(if begin {
        TileEventPayload::BeginClipBase(payload)
    } else {
        TileEventPayload::ResolveClipBase(payload)
    });
    Ok(())
}

pub(crate) fn append_scope_marker(
    output_size: CanvasSize,
    scope: GpuSparseAtlasScopeEvent,
    begin: bool,
    payloads: &mut Vec<TileEventPayload>,
    bounds: &mut Vec<CanvasRect>,
) -> Result<(), GpuRenderError> {
    validate_scope_event(output_size, &scope)?;
    let (begin_payload, end_payload, bounds_rect) = scope_payloads(scope);
    bounds.push(bounds_rect);
    payloads.push(if begin { begin_payload } else { end_payload });
    Ok(())
}

pub(crate) fn append_clipped_scope_marker(
    output_size: CanvasSize,
    scope: GpuSparseAtlasScopeEvent,
    begin: bool,
    payloads: &mut Vec<TileEventPayload>,
    bounds: &mut Vec<CanvasRect>,
) -> Result<(), GpuRenderError> {
    validate_scope_event(output_size, &scope)?;
    let (_, _, bounds_rect) = scope_payloads(scope);
    let payload = ScopeTileEventPayload {
        opacity: scope.opacity,
        blend_mode: scope.blend_mode,
        local_bounds: bounds_rect,
        mask_atlas_origin: scope.mask.map(|mask| (mask.atlas_x, mask.atlas_y)),
    };
    bounds.push(bounds_rect);
    payloads.push(if begin {
        TileEventPayload::BeginClippedContainer(payload)
    } else {
        TileEventPayload::EndClippedContainer(payload)
    });
    Ok(())
}

pub(crate) fn scope_payloads(
    scope: GpuSparseAtlasScopeEvent,
) -> (TileEventPayload, TileEventPayload, CanvasRect) {
    let bounds = CanvasRect {
        x: scope.local_bounds.x,
        y: scope.local_bounds.y,
        width: scope.local_bounds.width,
        height: scope.local_bounds.height,
    };
    let payload = ScopeTileEventPayload {
        opacity: scope.opacity,
        blend_mode: scope.blend_mode,
        local_bounds: bounds,
        mask_atlas_origin: scope.mask.map(|mask| (mask.atlas_x, mask.atlas_y)),
    };
    let events = match scope.kind {
        GpuSparseAtlasScopeEventKind::Container => (
            TileEventPayload::BeginContainer(payload),
            TileEventPayload::EndContainer(payload),
        ),
        GpuSparseAtlasScopeEventKind::Through => (
            TileEventPayload::BeginThrough(payload),
            TileEventPayload::EndThrough(payload),
        ),
    };
    (events.0, events.1, bounds)
}

pub(crate) fn validate_scope_event(
    output_size: CanvasSize,
    scope: &GpuSparseAtlasScopeEvent,
) -> Result<(), GpuRenderError> {
    let right = scope
        .local_bounds
        .x
        .checked_add(scope.local_bounds.width)
        .ok_or(GpuRenderError::TextureSizeOverflow)?;
    let bottom = scope
        .local_bounds
        .y
        .checked_add(scope.local_bounds.height)
        .ok_or(GpuRenderError::TextureSizeOverflow)?;
    if scope.local_bounds.is_empty() || right > output_size.width || bottom > output_size.height {
        return Err(GpuRenderError::ReadbackRegionOutOfBounds {
            texture_size: output_size,
            origin_x: scope.local_bounds.x,
            origin_y: scope.local_bounds.y,
            read_size: CanvasSize::new(scope.local_bounds.width, scope.local_bounds.height),
        });
    }
    Ok(())
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

fn validate_solid_color_event(
    output_size: CanvasSize,
    event: &GpuSparseAtlasSolidColorEvent,
) -> Result<(), GpuRenderError> {
    let right = event
        .local_bounds
        .x
        .checked_add(event.local_bounds.width)
        .ok_or(GpuRenderError::TextureSizeOverflow)?;
    let bottom = event
        .local_bounds
        .y
        .checked_add(event.local_bounds.height)
        .ok_or(GpuRenderError::TextureSizeOverflow)?;
    if event.local_bounds.is_empty() || right > output_size.width || bottom > output_size.height {
        return Err(GpuRenderError::ReadbackRegionOutOfBounds {
            texture_size: output_size,
            origin_x: event.local_bounds.x,
            origin_y: event.local_bounds.y,
            read_size: CanvasSize::new(event.local_bounds.width, event.local_bounds.height),
        });
    }
    Ok(())
}

pub(crate) fn validate_sparse_atlas_format(
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

pub(crate) fn validate_tile_ref(
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
