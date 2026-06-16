use clip_file::tiles::{
    GRAY_RGBA_TILE_BYTES, MASK_TILE_BYTES, MONO_RGBA_TILE_BYTES, RGBA_TILE_BYTES, TILE_SIZE,
    alpha_tile_blob_len, gray_rgba_tile_blob_len, mono_rgba_tile_blob_len, rgba_tile_blob_len,
};
use clip_model::CanvasSize;

use crate::{ClipSession, RuntimeError};

pub(super) fn inspect_raster_blocks(
    session: &ClipSession,
    source: &clip_file::metadata::RasterLayerSource,
) -> Result<clip_file::external::ExternalTileBlockInspection, RuntimeError> {
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
    inspect_source_blocks_with_compressed_tiles(
        session,
        &source.external_id,
        source.pixel_size.width,
        per_tile_len,
        expected_len,
    )
}

pub(super) fn inspect_mask_block_stats(
    session: &ClipSession,
    source: &clip_file::metadata::MaskLayerSource,
) -> Result<clip_file::external::ExternalTileBlockStats, RuntimeError> {
    let expected_len = alpha_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?;
    inspect_source_blocks(
        session,
        &source.external_id,
        source.pixel_size.width,
        MASK_TILE_BYTES,
        expected_len,
    )
}

pub(super) fn add_compressed_raster_tile_events(
    events_by_tile: &mut [u32],
    canvas: CanvasSize,
    canvas_tiles_x: u32,
    tile_size: u32,
    source: &clip_file::metadata::RasterLayerSource,
    compressed_tiles: &[clip_file::external::ExternalCompressedTile],
) -> Result<(), RuntimeError> {
    for tile in compressed_tiles {
        let source_x0 = source_tile_origin(tile.tile_x)?;
        let source_y0 = source_tile_origin(tile.tile_y)?;
        if source_x0 >= source.pixel_size.width || source_y0 >= source.pixel_size.height {
            continue;
        }

        let source_x1 = source_x0
            .checked_add(TILE_SIZE as u32)
            .ok_or(clip_file::ClipFileError::TileSizeOverflow)?
            .min(source.pixel_size.width);
        let source_y1 = source_y0
            .checked_add(TILE_SIZE as u32)
            .ok_or(clip_file::ClipFileError::TileSizeOverflow)?
            .min(source.pixel_size.height);
        let canvas_x0 = i64::from(source.offset_x) + i64::from(source_x0);
        let canvas_y0 = i64::from(source.offset_y) + i64::from(source_y0);
        let canvas_x1 = i64::from(source.offset_x) + i64::from(source_x1);
        let canvas_y1 = i64::from(source.offset_y) + i64::from(source_y1);
        let clipped_x0 = canvas_x0.max(0);
        let clipped_y0 = canvas_y0.max(0);
        let clipped_x1 = canvas_x1.min(i64::from(canvas.width));
        let clipped_y1 = canvas_y1.min(i64::from(canvas.height));
        if clipped_x1 <= clipped_x0 || clipped_y1 <= clipped_y0 {
            continue;
        }

        let tile_x0 = u32::try_from(clipped_x0)
            .map_err(|_| clip_file::ClipFileError::TileSizeOverflow)?
            / tile_size;
        let tile_y0 = u32::try_from(clipped_y0)
            .map_err(|_| clip_file::ClipFileError::TileSizeOverflow)?
            / tile_size;
        let tile_x1 = (u32::try_from(clipped_x1)
            .map_err(|_| clip_file::ClipFileError::TileSizeOverflow)?
            - 1)
            / tile_size;
        let tile_y1 = (u32::try_from(clipped_y1)
            .map_err(|_| clip_file::ClipFileError::TileSizeOverflow)?
            - 1)
            / tile_size;

        for y in tile_y0..=tile_y1 {
            for x in tile_x0..=tile_x1 {
                let index =
                    usize::try_from(u64::from(y) * u64::from(canvas_tiles_x) + u64::from(x))
                        .map_err(|_| clip_file::ClipFileError::TileSizeOverflow)?;
                let events = events_by_tile
                    .get_mut(index)
                    .ok_or(clip_file::ClipFileError::TileSizeOverflow)?;
                *events = events
                    .checked_add(1)
                    .ok_or(clip_file::ClipFileError::TileSizeOverflow)?;
            }
        }
    }
    Ok(())
}

fn inspect_source_blocks(
    session: &ClipSession,
    external_id: &str,
    source_width: u32,
    per_tile_len: usize,
    expected_len: usize,
) -> Result<clip_file::external::ExternalTileBlockStats, RuntimeError> {
    let body = session
        .container
        .external_data_body(external_id)
        .ok_or_else(|| clip_file::ClipFileError::MissingExternalData(external_id.to_owned()))?;
    let expected_tile_count = expected_len / per_tile_len;
    Ok(clip_file::external::inspect_external_tile_blocks(
        body,
        per_tile_len,
        expected_tile_count,
        tile_cols(source_width)?,
    )?)
}

fn inspect_source_blocks_with_compressed_tiles(
    session: &ClipSession,
    external_id: &str,
    source_width: u32,
    per_tile_len: usize,
    expected_len: usize,
) -> Result<clip_file::external::ExternalTileBlockInspection, RuntimeError> {
    let body = session
        .container
        .external_data_body(external_id)
        .ok_or_else(|| clip_file::ClipFileError::MissingExternalData(external_id.to_owned()))?;
    let expected_tile_count = expected_len / per_tile_len;
    Ok(
        clip_file::external::inspect_external_tile_blocks_with_compressed_tiles(
            body,
            per_tile_len,
            expected_tile_count,
            tile_cols(source_width)?,
        )?,
    )
}

fn tile_cols(width: u32) -> Result<usize, RuntimeError> {
    let cols = width
        .checked_add(TILE_SIZE as u32 - 1)
        .map(|width| width / TILE_SIZE as u32)
        .ok_or(clip_file::ClipFileError::TileSizeOverflow)?;
    Ok(usize::try_from(cols).map_err(|_| clip_file::ClipFileError::TileSizeOverflow)?)
}

fn source_tile_origin(tile: usize) -> Result<u32, RuntimeError> {
    let coord = tile
        .checked_mul(TILE_SIZE)
        .ok_or(clip_file::ClipFileError::TileSizeOverflow)?;
    Ok(u32::try_from(coord).map_err(|_| clip_file::ClipFileError::TileSizeOverflow)?)
}
