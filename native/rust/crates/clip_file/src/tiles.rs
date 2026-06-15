use clip_model::Rect;

use crate::ClipFileError;

pub const TILE_SIZE: usize = 256;
pub const MASK_TILE_BYTES: usize = TILE_SIZE * TILE_SIZE;
pub const RGBA_TILE_BYTES: usize = MASK_TILE_BYTES * 5;
pub const GRAY_RGBA_TILE_BYTES: usize = MASK_TILE_BYTES * 2;
pub const MONO_RGBA_TILE_BYTES: usize = MASK_TILE_BYTES / 4;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TileAddress {
    pub layer_rect: Rect,
    pub mip_level: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RgbaTileImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AlphaTileImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub fn alpha_tile_blob_len(width: u32, height: u32) -> Result<usize, ClipFileError> {
    let cols = div_ceil_u32(width, TILE_SIZE as u32)?;
    let rows = div_ceil_u32(height, TILE_SIZE as u32)?;
    let tile_count = usize::try_from(u64::from(cols) * u64::from(rows))
        .map_err(|_| ClipFileError::TileSizeOverflow)?;
    tile_count
        .checked_mul(MASK_TILE_BYTES)
        .ok_or(ClipFileError::TileSizeOverflow)
}

pub fn rgba_tile_blob_len(width: u32, height: u32) -> Result<usize, ClipFileError> {
    tile_blob_len(width, height, RGBA_TILE_BYTES)
}

pub fn gray_rgba_tile_blob_len(width: u32, height: u32) -> Result<usize, ClipFileError> {
    tile_blob_len(width, height, GRAY_RGBA_TILE_BYTES)
}

pub fn mono_rgba_tile_blob_len(width: u32, height: u32) -> Result<usize, ClipFileError> {
    tile_blob_len(width, height, MONO_RGBA_TILE_BYTES)
}

fn tile_blob_len(width: u32, height: u32, per_tile: usize) -> Result<usize, ClipFileError> {
    let cols = div_ceil_u32(width, TILE_SIZE as u32)?;
    let rows = div_ceil_u32(height, TILE_SIZE as u32)?;
    let tile_count = usize::try_from(u64::from(cols) * u64::from(rows))
        .map_err(|_| ClipFileError::TileSizeOverflow)?;
    tile_count
        .checked_mul(per_tile)
        .ok_or(ClipFileError::TileSizeOverflow)
}

pub fn decode_alpha_tiles(
    tile_blob: &[u8],
    width: u32,
    height: u32,
) -> Result<AlphaTileImage, ClipFileError> {
    if !tile_blob.len().is_multiple_of(MASK_TILE_BYTES) {
        return Err(ClipFileError::InvalidTileBlobSize {
            actual: tile_blob.len(),
            per_tile: MASK_TILE_BYTES,
        });
    }

    let cols = div_ceil_u32(width, TILE_SIZE as u32)?;
    let rows = div_ceil_u32(height, TILE_SIZE as u32)?;
    let expected_tiles = usize::try_from(u64::from(cols) * u64::from(rows))
        .map_err(|_| ClipFileError::TileSizeOverflow)?;
    let actual_tiles = tile_blob.len() / MASK_TILE_BYTES;
    if actual_tiles != expected_tiles {
        return Err(ClipFileError::UnexpectedTileCount {
            expected: expected_tiles,
            actual: actual_tiles,
        });
    }

    let width_usize = usize::try_from(width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let height_usize = usize::try_from(height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let pixel_count = width_usize
        .checked_mul(height_usize)
        .ok_or(ClipFileError::TileSizeOverflow)?;
    let mut pixels = vec![0u8; pixel_count];

    let cols_usize = usize::try_from(cols).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let rows_usize = usize::try_from(rows).map_err(|_| ClipFileError::TileSizeOverflow)?;
    for tile_y in 0..rows_usize {
        let (dst_y0, tile_height) = active_tile_span(tile_y, height_usize);
        for tile_x in 0..cols_usize {
            let tile_index = tile_y * cols_usize + tile_x;
            let tile_start = tile_index * MASK_TILE_BYTES;
            let (dst_x0, tile_width) = active_tile_span(tile_x, width_usize);

            for local_y in 0..tile_height {
                let source_start = tile_start + local_y * TILE_SIZE;
                let source_end = source_start + tile_width;
                let dest_start = (dst_y0 + local_y) * width_usize + dst_x0;
                let dest_end = dest_start + tile_width;
                pixels[dest_start..dest_end].copy_from_slice(&tile_blob[source_start..source_end]);
            }
        }
    }

    Ok(AlphaTileImage {
        width,
        height,
        pixels,
    })
}

pub fn decode_rgba_tiles(
    tile_blob: &[u8],
    width: u32,
    height: u32,
) -> Result<RgbaTileImage, ClipFileError> {
    if !tile_blob.len().is_multiple_of(RGBA_TILE_BYTES) {
        return Err(ClipFileError::InvalidTileBlobSize {
            actual: tile_blob.len(),
            per_tile: RGBA_TILE_BYTES,
        });
    }

    let cols = div_ceil_u32(width, TILE_SIZE as u32)?;
    let rows = div_ceil_u32(height, TILE_SIZE as u32)?;
    let expected_tiles = usize::try_from(u64::from(cols) * u64::from(rows))
        .map_err(|_| ClipFileError::TileSizeOverflow)?;
    let actual_tiles = tile_blob.len() / RGBA_TILE_BYTES;
    if actual_tiles != expected_tiles {
        return Err(ClipFileError::UnexpectedTileCount {
            expected: expected_tiles,
            actual: actual_tiles,
        });
    }

    let width_usize = usize::try_from(width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let height_usize = usize::try_from(height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let pixel_count = width_usize
        .checked_mul(height_usize)
        .ok_or(ClipFileError::TileSizeOverflow)?;
    let mut pixels = vec![
        0u8;
        pixel_count
            .checked_mul(4)
            .ok_or(ClipFileError::TileSizeOverflow)?
    ];

    let cols_usize = usize::try_from(cols).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let rows_usize = usize::try_from(rows).map_err(|_| ClipFileError::TileSizeOverflow)?;
    for tile_y in 0..rows_usize {
        let (dst_y0, tile_height) = active_tile_span(tile_y, height_usize);
        for tile_x in 0..cols_usize {
            let tile_index = tile_y * cols_usize + tile_x;
            let tile_start = tile_index * RGBA_TILE_BYTES;
            let alpha_start = tile_start;
            let bgra_start = tile_start + MASK_TILE_BYTES;
            let (dst_x0, tile_width) = active_tile_span(tile_x, width_usize);

            for local_y in 0..tile_height {
                let source_pixel = local_y * TILE_SIZE;
                let alpha_row_start = alpha_start + source_pixel;
                let bgra_row_start = bgra_start + source_pixel * 4;
                let dest_start = ((dst_y0 + local_y) * width_usize + dst_x0) * 4;
                let alpha_row = &tile_blob[alpha_row_start..alpha_row_start + tile_width];
                let bgra_row = &tile_blob[bgra_row_start..bgra_row_start + tile_width * 4];
                let dest_row = &mut pixels[dest_start..dest_start + tile_width * 4];

                for ((dst, bgra), alpha) in dest_row
                    .chunks_exact_mut(4)
                    .zip(bgra_row.chunks_exact(4))
                    .zip(alpha_row)
                {
                    dst[0] = bgra[2];
                    dst[1] = bgra[1];
                    dst[2] = bgra[0];
                    dst[3] = *alpha;
                }
            }
        }
    }

    Ok(RgbaTileImage {
        width,
        height,
        pixels,
    })
}

pub fn decode_gray_rgba_tiles(
    tile_blob: &[u8],
    width: u32,
    height: u32,
) -> Result<RgbaTileImage, ClipFileError> {
    validate_tile_blob(tile_blob, width, height, GRAY_RGBA_TILE_BYTES)?;

    let width_usize = usize::try_from(width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let height_usize = usize::try_from(height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let mut pixels = rgba_output_buffer(width_usize, height_usize)?;
    let cols_usize = usize::try_from(div_ceil_u32(width, TILE_SIZE as u32)?)
        .map_err(|_| ClipFileError::TileSizeOverflow)?;
    let rows_usize = usize::try_from(div_ceil_u32(height, TILE_SIZE as u32)?)
        .map_err(|_| ClipFileError::TileSizeOverflow)?;

    for tile_y in 0..rows_usize {
        let (dst_y0, tile_height) = active_tile_span(tile_y, height_usize);
        for tile_x in 0..cols_usize {
            let tile_index = tile_y * cols_usize + tile_x;
            let tile_start = tile_index * GRAY_RGBA_TILE_BYTES;
            let alpha_start = tile_start;
            let gray_start = tile_start + MASK_TILE_BYTES;
            let (dst_x0, tile_width) = active_tile_span(tile_x, width_usize);

            for local_y in 0..tile_height {
                let source_pixel = local_y * TILE_SIZE;
                let alpha_row_start = alpha_start + source_pixel;
                let gray_row_start = gray_start + source_pixel;
                let dest_start = ((dst_y0 + local_y) * width_usize + dst_x0) * 4;
                let alpha_row = &tile_blob[alpha_row_start..alpha_row_start + tile_width];
                let gray_row = &tile_blob[gray_row_start..gray_row_start + tile_width];
                let dest_row = &mut pixels[dest_start..dest_start + tile_width * 4];

                for ((dst, gray), alpha) in
                    dest_row.chunks_exact_mut(4).zip(gray_row).zip(alpha_row)
                {
                    dst[0] = *gray;
                    dst[1] = *gray;
                    dst[2] = *gray;
                    dst[3] = *alpha;
                }
            }
        }
    }

    Ok(RgbaTileImage {
        width,
        height,
        pixels,
    })
}

pub fn decode_mono_rgba_tiles(
    tile_blob: &[u8],
    width: u32,
    height: u32,
) -> Result<RgbaTileImage, ClipFileError> {
    validate_tile_blob(tile_blob, width, height, MONO_RGBA_TILE_BYTES)?;

    let width_usize = usize::try_from(width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let height_usize = usize::try_from(height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let mut pixels = rgba_output_buffer(width_usize, height_usize)?;
    let cols_usize = usize::try_from(div_ceil_u32(width, TILE_SIZE as u32)?)
        .map_err(|_| ClipFileError::TileSizeOverflow)?;
    let rows_usize = usize::try_from(div_ceil_u32(height, TILE_SIZE as u32)?)
        .map_err(|_| ClipFileError::TileSizeOverflow)?;
    let half_tile = MONO_RGBA_TILE_BYTES / 2;

    for tile_y in 0..rows_usize {
        let (dst_y0, tile_height) = active_tile_span(tile_y, height_usize);
        for tile_x in 0..cols_usize {
            let tile_index = tile_y * cols_usize + tile_x;
            let tile_start = tile_index * MONO_RGBA_TILE_BYTES;
            let alpha_start = tile_start;
            let white_start = tile_start + half_tile;
            let (dst_x0, tile_width) = active_tile_span(tile_x, width_usize);

            for local_y in 0..tile_height {
                let dest_start = ((dst_y0 + local_y) * width_usize + dst_x0) * 4;
                let dest_row = &mut pixels[dest_start..dest_start + tile_width * 4];

                for (local_x, dst) in dest_row.chunks_exact_mut(4).enumerate() {
                    let bit_index = local_y * TILE_SIZE + local_x;
                    let alpha = bit_plane_value(tile_blob, alpha_start, bit_index);
                    let white = bit_plane_value(tile_blob, white_start, bit_index) && alpha;
                    if alpha {
                        dst[3] = 255;
                        if white {
                            dst[0] = 255;
                            dst[1] = 255;
                            dst[2] = 255;
                        }
                    }
                }
            }
        }
    }

    Ok(RgbaTileImage {
        width,
        height,
        pixels,
    })
}

fn validate_tile_blob(
    tile_blob: &[u8],
    width: u32,
    height: u32,
    per_tile: usize,
) -> Result<(), ClipFileError> {
    if !tile_blob.len().is_multiple_of(per_tile) {
        return Err(ClipFileError::InvalidTileBlobSize {
            actual: tile_blob.len(),
            per_tile,
        });
    }
    let cols = div_ceil_u32(width, TILE_SIZE as u32)?;
    let rows = div_ceil_u32(height, TILE_SIZE as u32)?;
    let expected_tiles = usize::try_from(u64::from(cols) * u64::from(rows))
        .map_err(|_| ClipFileError::TileSizeOverflow)?;
    let actual_tiles = tile_blob.len() / per_tile;
    if actual_tiles != expected_tiles {
        return Err(ClipFileError::UnexpectedTileCount {
            expected: expected_tiles,
            actual: actual_tiles,
        });
    }
    Ok(())
}

fn rgba_output_buffer(width: usize, height: usize) -> Result<Vec<u8>, ClipFileError> {
    let pixel_count = width
        .checked_mul(height)
        .ok_or(ClipFileError::TileSizeOverflow)?;
    Ok(vec![
        0u8;
        pixel_count
            .checked_mul(4)
            .ok_or(ClipFileError::TileSizeOverflow)?
    ])
}

fn bit_plane_value(bytes: &[u8], plane_start: usize, bit_index: usize) -> bool {
    let byte = bytes[plane_start + bit_index / 8];
    let mask = 0x80 >> (bit_index % 8);
    byte & mask != 0
}

fn active_tile_span(tile_index: usize, image_len: usize) -> (usize, usize) {
    let start = tile_index * TILE_SIZE;
    let len = image_len.saturating_sub(start).min(TILE_SIZE);
    (start, len)
}

fn div_ceil_u32(value: u32, divisor: u32) -> Result<u32, ClipFileError> {
    value
        .checked_add(divisor - 1)
        .map(|value| value / divisor)
        .ok_or(ClipFileError::TileSizeOverflow)
}
