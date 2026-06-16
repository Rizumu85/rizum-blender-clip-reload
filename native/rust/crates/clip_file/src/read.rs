use std::path::Path;

use clip_model::{CanvasSize, LayerId, Rect};

use crate::ClipFileError;
use crate::container::ClipContainer;
use crate::external::{decode_external_tile_blob, decode_external_tile_blocks};
use crate::metadata::{
    self, read_mask_layer_source_from_sqlite, read_raster_layer_source_from_sqlite,
};
use crate::tile_region::{
    TileBlockRef, decode_gray_tile_blocks_region, decode_mono_tile_blocks_region,
    decode_rgba_tile_blocks_region, tile_indices_for_region,
};
use crate::tiles::{
    AlphaTileImage, GRAY_RGBA_TILE_BYTES, MONO_RGBA_TILE_BYTES, RGBA_TILE_BYTES, RgbaTileImage,
    alpha_tile_blob_len, decode_alpha_tiles, decode_gray_rgba_tiles, decode_gray_rgba_tiles_region,
    decode_mono_rgba_tiles, decode_mono_rgba_tiles_region, decode_rgba_tiles,
    decode_rgba_tiles_region, gray_rgba_tile_blob_len, mono_rgba_tile_blob_len, rgba_tile_blob_len,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipFileSummary {
    pub canvas: CanvasSize,
    pub root_layer_id: LayerId,
    pub layer_count: usize,
    pub external_data_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlacedRgbaTileImage {
    pub image: RgbaTileImage,
    pub offset_x: i32,
    pub offset_y: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RasterLayerSourceInfo {
    pub render_mipmap_id: u32,
    pub pixel_size: CanvasSize,
    pub offset_x: i32,
    pub offset_y: i32,
}

pub fn read_summary(path: impl AsRef<Path>) -> Result<ClipFileSummary, ClipFileError> {
    let container = ClipContainer::open(path)?;
    metadata::read_summary_from_sqlite(container.sqlite_bytes(), container.external_data().len())
}

pub fn read_raster_layer_rgba(
    path: impl AsRef<Path>,
    layer_id: LayerId,
) -> Result<RgbaTileImage, ClipFileError> {
    let container = ClipContainer::open(path)?;
    let summary = metadata::read_summary_from_sqlite(
        container.sqlite_bytes(),
        container.external_data().len(),
    )?;
    let placed =
        read_raster_layer_source_rgba_from_container(&container, summary.canvas, layer_id)?;
    place_rgba_on_canvas(
        placed.image,
        summary.canvas,
        placed.offset_x,
        placed.offset_y,
    )
}

pub fn read_raster_layer_source_rgba(
    path: impl AsRef<Path>,
    layer_id: LayerId,
) -> Result<PlacedRgbaTileImage, ClipFileError> {
    let container = ClipContainer::open(path)?;
    let summary = metadata::read_summary_from_sqlite(
        container.sqlite_bytes(),
        container.external_data().len(),
    )?;
    read_raster_layer_source_rgba_from_container(&container, summary.canvas, layer_id)
}

pub fn read_raster_layer_source_info(
    path: impl AsRef<Path>,
    layer_id: LayerId,
) -> Result<RasterLayerSourceInfo, ClipFileError> {
    let container = ClipContainer::open(path)?;
    let summary = metadata::read_summary_from_sqlite(
        container.sqlite_bytes(),
        container.external_data().len(),
    )?;
    read_raster_layer_source_info_from_container(&container, summary.canvas, layer_id)
}

pub fn read_layer_mask_alpha(
    path: impl AsRef<Path>,
    layer_id: LayerId,
) -> Result<AlphaTileImage, ClipFileError> {
    let container = ClipContainer::open(path)?;
    let summary = metadata::read_summary_from_sqlite(
        container.sqlite_bytes(),
        container.external_data().len(),
    )?;
    read_layer_mask_alpha_from_container(&container, summary.canvas, layer_id)
}

pub fn read_raster_layer_source_info_from_container(
    container: &ClipContainer,
    canvas_size: CanvasSize,
    layer_id: LayerId,
) -> Result<RasterLayerSourceInfo, ClipFileError> {
    let source =
        read_raster_layer_source_from_sqlite(container.sqlite_bytes(), layer_id, canvas_size)?;
    Ok(RasterLayerSourceInfo {
        render_mipmap_id: source.render_mipmap_id,
        pixel_size: source.pixel_size,
        offset_x: source.offset_x,
        offset_y: source.offset_y,
    })
}

pub fn read_raster_layer_source_rgba_from_container(
    container: &ClipContainer,
    canvas_size: CanvasSize,
    layer_id: LayerId,
) -> Result<PlacedRgbaTileImage, ClipFileError> {
    let source =
        read_raster_layer_source_from_sqlite(container.sqlite_bytes(), layer_id, canvas_size)?;
    read_resolved_raster_layer_source_rgba_from_container(container, &source)
}

pub fn read_resolved_raster_layer_source_rgba_from_container(
    container: &ClipContainer,
    source: &metadata::RasterLayerSource,
) -> Result<PlacedRgbaTileImage, ClipFileError> {
    let image = decode_raster_source_rgba(container, source, source.layer.id)?;
    Ok(PlacedRgbaTileImage {
        image,
        offset_x: source.offset_x,
        offset_y: source.offset_y,
    })
}

pub fn read_resolved_raster_layer_source_rgba_region_from_container(
    container: &ClipContainer,
    source: &metadata::RasterLayerSource,
    source_region: Rect,
) -> Result<RgbaTileImage, ClipFileError> {
    decode_raster_source_rgba_region(container, source, source.layer.id, source_region)
}

pub fn read_layer_mask_alpha_from_container(
    container: &ClipContainer,
    canvas_size: CanvasSize,
    layer_id: LayerId,
) -> Result<AlphaTileImage, ClipFileError> {
    let source =
        read_mask_layer_source_from_sqlite(container.sqlite_bytes(), layer_id, canvas_size)?;
    read_resolved_layer_mask_alpha_from_container(container, canvas_size, &source)
}

pub fn read_resolved_layer_mask_alpha_from_container(
    container: &ClipContainer,
    canvas_size: CanvasSize,
    source: &metadata::MaskLayerSource,
) -> Result<AlphaTileImage, ClipFileError> {
    let body = container
        .external_data_body(&source.external_id)
        .ok_or_else(|| ClipFileError::MissingExternalData(source.external_id.clone()))?;
    let expected_len = alpha_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?;
    let blob = decode_external_tile_blob(body, source.empty_fill, Some(expected_len))?;
    let alpha = decode_alpha_tiles(
        &blob.bytes,
        source.pixel_size.width,
        source.pixel_size.height,
    )?;
    place_alpha_on_canvas(
        alpha,
        canvas_size,
        source.offset_x,
        source.offset_y,
        source.empty_fill,
    )
}

fn decode_raster_source_rgba(
    container: &ClipContainer,
    source: &metadata::RasterLayerSource,
    layer_id: LayerId,
) -> Result<RgbaTileImage, ClipFileError> {
    let body = container
        .external_data_body(&source.external_id)
        .ok_or_else(|| ClipFileError::MissingExternalData(source.external_id.clone()))?;
    let color_type = source.color_type.unwrap_or(0);
    let expected_len = match color_type {
        0 => rgba_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?,
        1 => gray_rgba_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?,
        2 => mono_rgba_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?,
        _ => {
            return Err(ClipFileError::UnsupportedLayerColorType {
                layer_id,
                color_type: source.color_type,
            });
        }
    };
    let blob = decode_external_tile_blob(body, 0, Some(expected_len))?;
    match color_type {
        0 => decode_rgba_tiles(
            &blob.bytes,
            source.pixel_size.width,
            source.pixel_size.height,
        ),
        1 => decode_gray_rgba_tiles(
            &blob.bytes,
            source.pixel_size.width,
            source.pixel_size.height,
        ),
        2 => decode_mono_rgba_tiles(
            &blob.bytes,
            source.pixel_size.width,
            source.pixel_size.height,
        ),
        _ => unreachable!("unsupported raster color type should have returned earlier"),
    }
}

fn decode_raster_source_rgba_region(
    container: &ClipContainer,
    source: &metadata::RasterLayerSource,
    layer_id: LayerId,
    source_region: Rect,
) -> Result<RgbaTileImage, ClipFileError> {
    let body = container
        .external_data_body(&source.external_id)
        .ok_or_else(|| ClipFileError::MissingExternalData(source.external_id.clone()))?;
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
            return Err(ClipFileError::UnsupportedLayerColorType {
                layer_id,
                color_type: source.color_type,
            });
        }
    };
    let tile_indices = tile_indices_for_region(
        source.pixel_size.width,
        source.pixel_size.height,
        source_region,
    )?;
    let expected_tile_count = expected_len / per_tile_len;
    if tile_indices.len() == expected_tile_count {
        let blob = decode_external_tile_blob(body, 0, Some(expected_len))?;
        return match color_type {
            0 => decode_rgba_tiles_region(
                &blob.bytes,
                source.pixel_size.width,
                source.pixel_size.height,
                source_region,
            ),
            1 => decode_gray_rgba_tiles_region(
                &blob.bytes,
                source.pixel_size.width,
                source.pixel_size.height,
                source_region,
            ),
            2 => decode_mono_rgba_tiles_region(
                &blob.bytes,
                source.pixel_size.width,
                source.pixel_size.height,
                source_region,
            ),
            _ => unreachable!("unsupported raster color type should have returned earlier"),
        };
    }

    let tile_blocks =
        decode_external_tile_blocks(body, 0, per_tile_len, expected_tile_count, &tile_indices)?;
    let block_refs: Vec<_> = tile_blocks
        .blocks
        .iter()
        .map(|block| TileBlockRef {
            tile_index: block.tile_index,
            bytes: &block.bytes,
        })
        .collect();
    match color_type {
        0 => decode_rgba_tile_blocks_region(
            &block_refs,
            source.pixel_size.width,
            source.pixel_size.height,
            source_region,
        ),
        1 => decode_gray_tile_blocks_region(
            &block_refs,
            source.pixel_size.width,
            source.pixel_size.height,
            source_region,
        ),
        2 => decode_mono_tile_blocks_region(
            &block_refs,
            source.pixel_size.width,
            source.pixel_size.height,
            source_region,
        ),
        _ => unreachable!("unsupported raster color type should have returned earlier"),
    }
}

fn place_alpha_on_canvas(
    alpha: AlphaTileImage,
    canvas_size: CanvasSize,
    offset_x: i32,
    offset_y: i32,
    empty_fill: u8,
) -> Result<AlphaTileImage, ClipFileError> {
    if alpha.width == canvas_size.width
        && alpha.height == canvas_size.height
        && offset_x == 0
        && offset_y == 0
    {
        return Ok(alpha);
    }

    if offset_x == 0
        && offset_y == 0
        && alpha.width >= canvas_size.width
        && alpha.height >= canvas_size.height
    {
        return crop_alpha_top_left(alpha, canvas_size);
    }

    let canvas_width =
        usize::try_from(canvas_size.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let canvas_height =
        usize::try_from(canvas_size.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let alpha_width = usize::try_from(alpha.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let alpha_height =
        usize::try_from(alpha.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let mut pixels = vec![
        empty_fill;
        canvas_width
            .checked_mul(canvas_height)
            .ok_or(ClipFileError::TileSizeOverflow)?
    ];

    let src_x0 =
        usize::try_from((-offset_x).max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let src_y0 =
        usize::try_from((-offset_y).max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let dst_x0 = usize::try_from(offset_x.max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let dst_y0 = usize::try_from(offset_y.max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let paste_w = alpha_width
        .saturating_sub(src_x0)
        .min(canvas_width.saturating_sub(dst_x0));
    let paste_h = alpha_height
        .saturating_sub(src_y0)
        .min(canvas_height.saturating_sub(dst_y0));

    for row in 0..paste_h {
        let src_start = (src_y0 + row) * alpha_width + src_x0;
        let src_end = src_start + paste_w;
        let dst_start = (dst_y0 + row) * canvas_width + dst_x0;
        let dst_end = dst_start + paste_w;
        pixels[dst_start..dst_end].copy_from_slice(&alpha.pixels[src_start..src_end]);
    }

    Ok(AlphaTileImage {
        width: canvas_size.width,
        height: canvas_size.height,
        pixels,
    })
}

fn place_rgba_on_canvas(
    rgba: RgbaTileImage,
    canvas_size: CanvasSize,
    offset_x: i32,
    offset_y: i32,
) -> Result<RgbaTileImage, ClipFileError> {
    if rgba.width == canvas_size.width
        && rgba.height == canvas_size.height
        && offset_x == 0
        && offset_y == 0
    {
        return Ok(rgba);
    }

    if offset_x == 0
        && offset_y == 0
        && rgba.width >= canvas_size.width
        && rgba.height >= canvas_size.height
    {
        return crop_rgba_top_left(rgba, canvas_size);
    }

    let canvas_width =
        usize::try_from(canvas_size.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let canvas_height =
        usize::try_from(canvas_size.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let rgba_width = usize::try_from(rgba.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let rgba_height = usize::try_from(rgba.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let pixel_count = canvas_width
        .checked_mul(canvas_height)
        .ok_or(ClipFileError::TileSizeOverflow)?;
    let mut pixels = vec![
        0u8;
        pixel_count
            .checked_mul(4)
            .ok_or(ClipFileError::TileSizeOverflow)?
    ];
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[0] = 255;
        pixel[1] = 255;
        pixel[2] = 255;
    }

    let src_x0 =
        usize::try_from((-offset_x).max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let src_y0 =
        usize::try_from((-offset_y).max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let dst_x0 = usize::try_from(offset_x.max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let dst_y0 = usize::try_from(offset_y.max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let paste_w = rgba_width
        .saturating_sub(src_x0)
        .min(canvas_width.saturating_sub(dst_x0));
    let paste_h = rgba_height
        .saturating_sub(src_y0)
        .min(canvas_height.saturating_sub(dst_y0));

    for row in 0..paste_h {
        let src_start = ((src_y0 + row) * rgba_width + src_x0) * 4;
        let src_end = src_start + paste_w * 4;
        let dst_start = ((dst_y0 + row) * canvas_width + dst_x0) * 4;
        let dst_end = dst_start + paste_w * 4;
        pixels[dst_start..dst_end].copy_from_slice(&rgba.pixels[src_start..src_end]);
    }

    Ok(RgbaTileImage {
        width: canvas_size.width,
        height: canvas_size.height,
        pixels,
    })
}

fn crop_alpha_top_left(
    alpha: AlphaTileImage,
    canvas_size: CanvasSize,
) -> Result<AlphaTileImage, ClipFileError> {
    let canvas_width =
        usize::try_from(canvas_size.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let canvas_height =
        usize::try_from(canvas_size.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let alpha_width = usize::try_from(alpha.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let mut pixels = vec![
        0u8;
        canvas_width
            .checked_mul(canvas_height)
            .ok_or(ClipFileError::TileSizeOverflow)?
    ];
    for row in 0..canvas_height {
        let src_start = row * alpha_width;
        let src_end = src_start + canvas_width;
        let dst_start = row * canvas_width;
        let dst_end = dst_start + canvas_width;
        pixels[dst_start..dst_end].copy_from_slice(&alpha.pixels[src_start..src_end]);
    }
    Ok(AlphaTileImage {
        width: canvas_size.width,
        height: canvas_size.height,
        pixels,
    })
}

fn crop_rgba_top_left(
    rgba: RgbaTileImage,
    canvas_size: CanvasSize,
) -> Result<RgbaTileImage, ClipFileError> {
    let canvas_width =
        usize::try_from(canvas_size.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let canvas_height =
        usize::try_from(canvas_size.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let rgba_width = usize::try_from(rgba.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let pixel_count = canvas_width
        .checked_mul(canvas_height)
        .ok_or(ClipFileError::TileSizeOverflow)?;
    let mut pixels = vec![
        0u8;
        pixel_count
            .checked_mul(4)
            .ok_or(ClipFileError::TileSizeOverflow)?
    ];
    for row in 0..canvas_height {
        let src_start = row * rgba_width * 4;
        let src_end = src_start + canvas_width * 4;
        let dst_start = row * canvas_width * 4;
        let dst_end = dst_start + canvas_width * 4;
        pixels[dst_start..dst_end].copy_from_slice(&rgba.pixels[src_start..src_end]);
    }
    Ok(RgbaTileImage {
        width: canvas_size.width,
        height: canvas_size.height,
        pixels,
    })
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
