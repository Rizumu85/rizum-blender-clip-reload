use std::collections::HashMap;

use clip_file::tiles::{
    GRAY_RGBA_TILE_BYTES, MASK_TILE_BYTES, MONO_RGBA_TILE_BYTES, RGBA_TILE_BYTES, TILE_SIZE,
    alpha_tile_blob_len, gray_rgba_tile_blob_len, mono_rgba_tile_blob_len, rgba_tile_blob_len,
};
use clip_model::{CanvasSize, Rect};

use crate::{RuntimeError, source_crop};

use super::GpuResourcePlan;

pub(super) fn planned_sparse_raster_regions(
    container: &clip_file::container::ClipContainer,
    canvas: CanvasSize,
    plan: &GpuResourcePlan,
) -> Result<
    HashMap<clip_gpu::GpuRasterResourceKey, Option<source_crop::RasterSourceDecodeRegion>>,
    RuntimeError,
> {
    let mut regions = HashMap::with_capacity(plan.rasters.len());
    for (key, meta) in &plan.rasters {
        regions.insert(
            *key,
            sparse_raster_source_decode_region(container, canvas, &meta.source)?,
        );
    }
    for (key, meta) in &plan.text_rasters {
        regions.insert(
            *key,
            source_crop::visible_raster_source_decode_region(
                meta.layout.size,
                meta.layout.offset_x,
                meta.layout.offset_y,
                canvas,
            )?,
        );
    }
    Ok(regions)
}

fn sparse_raster_source_decode_region(
    container: &clip_file::container::ClipContainer,
    canvas: CanvasSize,
    source: &clip_file::metadata::RasterLayerSource,
) -> Result<Option<source_crop::RasterSourceDecodeRegion>, RuntimeError> {
    let Some(visible) = source_crop::visible_raster_source_decode_region(
        source.pixel_size,
        source.offset_x,
        source.offset_y,
        canvas,
    )?
    else {
        return Ok(None);
    };
    let Some(compressed_rect) = compressed_raster_source_rect(container, source)? else {
        return Ok(None);
    };
    clipped_decode_region(source, visible, compressed_rect)
}

pub(super) fn sparse_mask_source_decode_region(
    container: &clip_file::container::ClipContainer,
    canvas: CanvasSize,
    source: &clip_file::metadata::MaskLayerSource,
) -> Result<Option<source_crop::RasterSourceDecodeRegion>, RuntimeError> {
    let Some(visible) = source_crop::visible_raster_source_decode_region(
        source.pixel_size,
        source.offset_x,
        source.offset_y,
        canvas,
    )?
    else {
        return Ok(None);
    };
    let Some(compressed_rect) = compressed_mask_source_rect(container, source)? else {
        return Ok(None);
    };
    clipped_decode_region(source, visible, compressed_rect)
}

trait SparseSourcePlacement {
    fn offset_x(&self) -> i32;
    fn offset_y(&self) -> i32;
}

impl SparseSourcePlacement for clip_file::metadata::RasterLayerSource {
    fn offset_x(&self) -> i32 {
        self.offset_x
    }

    fn offset_y(&self) -> i32 {
        self.offset_y
    }
}

impl SparseSourcePlacement for clip_file::metadata::MaskLayerSource {
    fn offset_x(&self) -> i32 {
        self.offset_x
    }

    fn offset_y(&self) -> i32 {
        self.offset_y
    }
}

fn clipped_decode_region(
    source: &impl SparseSourcePlacement,
    visible: source_crop::RasterSourceDecodeRegion,
    compressed_rect: Rect,
) -> Result<Option<source_crop::RasterSourceDecodeRegion>, RuntimeError> {
    let Some(source_rect) = intersect_rect(visible.source_rect, compressed_rect) else {
        return Ok(None);
    };
    let offset_x = i64::from(source.offset_x()) + i64::from(source_rect.x);
    let offset_y = i64::from(source.offset_y()) + i64::from(source_rect.y);
    Ok(Some(source_crop::RasterSourceDecodeRegion {
        source_rect,
        offset_x: i32::try_from(offset_x)
            .map_err(|_| clip_file::ClipFileError::TileSizeOverflow)?,
        offset_y: i32::try_from(offset_y)
            .map_err(|_| clip_file::ClipFileError::TileSizeOverflow)?,
    }))
}

fn compressed_raster_source_rect(
    container: &clip_file::container::ClipContainer,
    source: &clip_file::metadata::RasterLayerSource,
) -> Result<Option<Rect>, RuntimeError> {
    let color_type = source.color_type.unwrap_or(0);
    let (expected_len, per_tile_len) = match color_type {
        0 => (
            rgba_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?,
            RGBA_TILE_BYTES,
        ),
        1 => (
            gray_rgba_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?,
            GRAY_RGBA_TILE_BYTES,
        ),
        2 => (
            mono_rgba_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?,
            MONO_RGBA_TILE_BYTES,
        ),
        _ => {
            return Err(clip_file::ClipFileError::UnsupportedLayerColorType {
                layer_id: source.layer.id,
                color_type: source.color_type,
            }
            .into());
        }
    };
    compressed_source_rect(
        container,
        &source.external_id,
        source.pixel_size,
        per_tile_len,
        expected_len,
    )
}

fn compressed_mask_source_rect(
    container: &clip_file::container::ClipContainer,
    source: &clip_file::metadata::MaskLayerSource,
) -> Result<Option<Rect>, RuntimeError> {
    compressed_source_rect(
        container,
        &source.external_id,
        source.pixel_size,
        MASK_TILE_BYTES,
        alpha_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?,
    )
}

fn compressed_source_rect(
    container: &clip_file::container::ClipContainer,
    external_id: &str,
    source_size: CanvasSize,
    per_tile_len: usize,
    expected_len: usize,
) -> Result<Option<Rect>, RuntimeError> {
    let body = container
        .external_data_body(external_id)
        .ok_or_else(|| clip_file::ClipFileError::MissingExternalData(external_id.to_owned()))?;
    let tile_cols = tile_cols(source_size.width)?;
    let stats = clip_file::external::inspect_external_tile_blocks(
        body,
        per_tile_len,
        expected_len / per_tile_len,
        tile_cols,
    )?;
    stats
        .compressed_tile_bounds
        .map(|bounds| tile_bounds_to_rect(bounds, source_size))
        .transpose()
}

fn tile_bounds_to_rect(
    bounds: clip_file::external::ExternalTileBlockBounds,
    source_size: CanvasSize,
) -> Result<Rect, RuntimeError> {
    let x = checked_tile_coord(bounds.tile_x)?;
    let y = checked_tile_coord(bounds.tile_y)?;
    let right_tile = bounds
        .tile_x
        .checked_add(bounds.tile_width)
        .ok_or(clip_file::ClipFileError::TileSizeOverflow)?;
    let bottom_tile = bounds
        .tile_y
        .checked_add(bounds.tile_height)
        .ok_or(clip_file::ClipFileError::TileSizeOverflow)?;
    let right = checked_tile_coord(right_tile)?.min(source_size.width);
    let bottom = checked_tile_coord(bottom_tile)?.min(source_size.height);
    Ok(Rect::new(x, y, right - x, bottom - y))
}

fn intersect_rect(left: Rect, right: Rect) -> Option<Rect> {
    let x0 = left.x.max(right.x);
    let y0 = left.y.max(right.y);
    let x1 = left
        .x
        .checked_add(left.width)?
        .min(right.x.checked_add(right.width)?);
    let y1 = left
        .y
        .checked_add(left.height)?
        .min(right.y.checked_add(right.height)?);
    (x1 > x0 && y1 > y0).then_some(Rect::new(x0, y0, x1 - x0, y1 - y0))
}

fn checked_tile_coord(tile: usize) -> Result<u32, RuntimeError> {
    let coord = tile
        .checked_mul(TILE_SIZE)
        .ok_or(clip_file::ClipFileError::TileSizeOverflow)?;
    Ok(u32::try_from(coord).map_err(|_| clip_file::ClipFileError::TileSizeOverflow)?)
}

fn tile_cols(width: u32) -> Result<usize, RuntimeError> {
    let cols = width
        .checked_add(TILE_SIZE as u32 - 1)
        .map(|width| width / TILE_SIZE as u32)
        .ok_or(clip_file::ClipFileError::TileSizeOverflow)?;
    Ok(usize::try_from(cols).map_err(|_| clip_file::ClipFileError::TileSizeOverflow)?)
}
