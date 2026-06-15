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
        for tile_x in 0..cols_usize {
            let tile_index = tile_y * cols_usize + tile_x;
            let tile_start = tile_index * MASK_TILE_BYTES;

            for local_y in 0..TILE_SIZE {
                let y = tile_y * TILE_SIZE + local_y;
                if y >= height_usize {
                    break;
                }
                for local_x in 0..TILE_SIZE {
                    let x = tile_x * TILE_SIZE + local_x;
                    if x >= width_usize {
                        break;
                    }
                    let source_pixel = local_y * TILE_SIZE + local_x;
                    let dest = y * width_usize + x;
                    pixels[dest] = tile_blob[tile_start + source_pixel];
                }
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
        for tile_x in 0..cols_usize {
            let tile_index = tile_y * cols_usize + tile_x;
            let tile_start = tile_index * RGBA_TILE_BYTES;
            let alpha_start = tile_start;
            let bgra_start = tile_start + MASK_TILE_BYTES;

            for local_y in 0..TILE_SIZE {
                let y = tile_y * TILE_SIZE + local_y;
                if y >= height_usize {
                    break;
                }
                for local_x in 0..TILE_SIZE {
                    let x = tile_x * TILE_SIZE + local_x;
                    if x >= width_usize {
                        break;
                    }
                    let source_pixel = local_y * TILE_SIZE + local_x;
                    let bgra = bgra_start + source_pixel * 4;
                    let dest = (y * width_usize + x) * 4;
                    pixels[dest] = tile_blob[bgra + 2];
                    pixels[dest + 1] = tile_blob[bgra + 1];
                    pixels[dest + 2] = tile_blob[bgra];
                    pixels[dest + 3] = tile_blob[alpha_start + source_pixel];
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
        for tile_x in 0..cols_usize {
            let tile_index = tile_y * cols_usize + tile_x;
            let tile_start = tile_index * GRAY_RGBA_TILE_BYTES;
            let alpha_start = tile_start;
            let gray_start = tile_start + MASK_TILE_BYTES;
            for local_y in 0..TILE_SIZE {
                let y = tile_y * TILE_SIZE + local_y;
                if y >= height_usize {
                    break;
                }
                for local_x in 0..TILE_SIZE {
                    let x = tile_x * TILE_SIZE + local_x;
                    if x >= width_usize {
                        break;
                    }
                    let source_pixel = local_y * TILE_SIZE + local_x;
                    let gray = tile_blob[gray_start + source_pixel];
                    let dest = (y * width_usize + x) * 4;
                    pixels[dest] = gray;
                    pixels[dest + 1] = gray;
                    pixels[dest + 2] = gray;
                    pixels[dest + 3] = tile_blob[alpha_start + source_pixel];
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
        for tile_x in 0..cols_usize {
            let tile_index = tile_y * cols_usize + tile_x;
            let tile_start = tile_index * MONO_RGBA_TILE_BYTES;
            let alpha_start = tile_start;
            let white_start = tile_start + half_tile;
            for local_y in 0..TILE_SIZE {
                let y = tile_y * TILE_SIZE + local_y;
                if y >= height_usize {
                    break;
                }
                for local_x in 0..TILE_SIZE {
                    let x = tile_x * TILE_SIZE + local_x;
                    if x >= width_usize {
                        break;
                    }
                    let bit_index = local_y * TILE_SIZE + local_x;
                    let alpha = bit_plane_value(tile_blob, alpha_start, bit_index);
                    let white = bit_plane_value(tile_blob, white_start, bit_index) && alpha;
                    let dest = (y * width_usize + x) * 4;
                    if alpha {
                        pixels[dest + 3] = 255;
                        if white {
                            pixels[dest] = 255;
                            pixels[dest + 1] = 255;
                            pixels[dest + 2] = 255;
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

fn div_ceil_u32(value: u32, divisor: u32) -> Result<u32, ClipFileError> {
    value
        .checked_add(divisor - 1)
        .map(|value| value / divisor)
        .ok_or(ClipFileError::TileSizeOverflow)
}
