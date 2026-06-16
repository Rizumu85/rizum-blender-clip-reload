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
    decode_rgba_tiles_region(tile_blob, width, height, Rect::new(0, 0, width, height))
}

pub fn decode_rgba_tiles_region(
    tile_blob: &[u8],
    width: u32,
    height: u32,
    region: Rect,
) -> Result<RgbaTileImage, ClipFileError> {
    validate_tile_blob(tile_blob, width, height, RGBA_TILE_BYTES)?;

    let region = validate_tile_region(width, height, region)?;
    let mut pixels = rgba_output_buffer(region.width, region.height)?;
    let cols_usize = usize::try_from(div_ceil_u32(width, TILE_SIZE as u32)?)
        .map_err(|_| ClipFileError::TileSizeOverflow)?;

    for tile_y in tile_span(region.y, region.bottom) {
        for tile_x in tile_span(region.x, region.right) {
            let Some(copy) = tile_copy_rect(region, tile_x, tile_y) else {
                continue;
            };
            let tile_index = tile_y * cols_usize + tile_x;
            let tile_start = tile_index * RGBA_TILE_BYTES;
            let alpha_start = tile_start;
            let bgra_start = tile_start + MASK_TILE_BYTES;

            for row in 0..copy.height {
                let source_pixel = (copy.source_y + row) * TILE_SIZE + copy.source_x;
                let alpha_row_start = alpha_start + source_pixel;
                let bgra_row_start = bgra_start + source_pixel * 4;
                let dest_start = ((copy.dest_y + row) * region.width + copy.dest_x) * 4;
                let alpha_row = &tile_blob[alpha_row_start..alpha_row_start + copy.width];
                let bgra_row = &tile_blob[bgra_row_start..bgra_row_start + copy.width * 4];
                let dest_row = &mut pixels[dest_start..dest_start + copy.width * 4];

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
        width: u32::try_from(region.width).map_err(|_| ClipFileError::TileSizeOverflow)?,
        height: u32::try_from(region.height).map_err(|_| ClipFileError::TileSizeOverflow)?,
        pixels,
    })
}

pub fn decode_gray_rgba_tiles(
    tile_blob: &[u8],
    width: u32,
    height: u32,
) -> Result<RgbaTileImage, ClipFileError> {
    decode_gray_rgba_tiles_region(tile_blob, width, height, Rect::new(0, 0, width, height))
}

pub fn decode_gray_rgba_tiles_region(
    tile_blob: &[u8],
    width: u32,
    height: u32,
    region: Rect,
) -> Result<RgbaTileImage, ClipFileError> {
    validate_tile_blob(tile_blob, width, height, GRAY_RGBA_TILE_BYTES)?;

    let region = validate_tile_region(width, height, region)?;
    let mut pixels = rgba_output_buffer(region.width, region.height)?;
    let cols_usize = usize::try_from(div_ceil_u32(width, TILE_SIZE as u32)?)
        .map_err(|_| ClipFileError::TileSizeOverflow)?;

    for tile_y in tile_span(region.y, region.bottom) {
        for tile_x in tile_span(region.x, region.right) {
            let Some(copy) = tile_copy_rect(region, tile_x, tile_y) else {
                continue;
            };
            let tile_index = tile_y * cols_usize + tile_x;
            let tile_start = tile_index * GRAY_RGBA_TILE_BYTES;
            let alpha_start = tile_start;
            let gray_start = tile_start + MASK_TILE_BYTES;

            for row in 0..copy.height {
                let source_pixel = (copy.source_y + row) * TILE_SIZE + copy.source_x;
                let alpha_row_start = alpha_start + source_pixel;
                let gray_row_start = gray_start + source_pixel;
                let dest_start = ((copy.dest_y + row) * region.width + copy.dest_x) * 4;
                let alpha_row = &tile_blob[alpha_row_start..alpha_row_start + copy.width];
                let gray_row = &tile_blob[gray_row_start..gray_row_start + copy.width];
                let dest_row = &mut pixels[dest_start..dest_start + copy.width * 4];

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
        width: u32::try_from(region.width).map_err(|_| ClipFileError::TileSizeOverflow)?,
        height: u32::try_from(region.height).map_err(|_| ClipFileError::TileSizeOverflow)?,
        pixels,
    })
}

pub fn decode_mono_rgba_tiles(
    tile_blob: &[u8],
    width: u32,
    height: u32,
) -> Result<RgbaTileImage, ClipFileError> {
    decode_mono_rgba_tiles_region(tile_blob, width, height, Rect::new(0, 0, width, height))
}

pub fn decode_mono_rgba_tiles_region(
    tile_blob: &[u8],
    width: u32,
    height: u32,
    region: Rect,
) -> Result<RgbaTileImage, ClipFileError> {
    validate_tile_blob(tile_blob, width, height, MONO_RGBA_TILE_BYTES)?;

    let region = validate_tile_region(width, height, region)?;
    let mut pixels = rgba_output_buffer(region.width, region.height)?;
    let cols_usize = usize::try_from(div_ceil_u32(width, TILE_SIZE as u32)?)
        .map_err(|_| ClipFileError::TileSizeOverflow)?;
    let half_tile = MONO_RGBA_TILE_BYTES / 2;

    for tile_y in tile_span(region.y, region.bottom) {
        for tile_x in tile_span(region.x, region.right) {
            let Some(copy) = tile_copy_rect(region, tile_x, tile_y) else {
                continue;
            };
            let tile_index = tile_y * cols_usize + tile_x;
            let tile_start = tile_index * MONO_RGBA_TILE_BYTES;
            let alpha_start = tile_start;
            let white_start = tile_start + half_tile;

            for row in 0..copy.height {
                let dest_start = ((copy.dest_y + row) * region.width + copy.dest_x) * 4;
                let dest_row = &mut pixels[dest_start..dest_start + copy.width * 4];

                for (local_x, dst) in dest_row.chunks_exact_mut(4).enumerate() {
                    let bit_index = (copy.source_y + row) * TILE_SIZE + copy.source_x + local_x;
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
        width: u32::try_from(region.width).map_err(|_| ClipFileError::TileSizeOverflow)?,
        height: u32::try_from(region.height).map_err(|_| ClipFileError::TileSizeOverflow)?,
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

#[derive(Clone, Copy, Debug)]
struct TileRegion {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    right: usize,
    bottom: usize,
}

#[derive(Clone, Copy, Debug)]
struct TileCopyRect {
    source_x: usize,
    source_y: usize,
    dest_x: usize,
    dest_y: usize,
    width: usize,
    height: usize,
}

fn validate_tile_region(
    width: u32,
    height: u32,
    region: Rect,
) -> Result<TileRegion, ClipFileError> {
    let right = region
        .x
        .checked_add(region.width)
        .ok_or(ClipFileError::TileSizeOverflow)?;
    let bottom = region
        .y
        .checked_add(region.height)
        .ok_or(ClipFileError::TileSizeOverflow)?;
    if region.is_empty() || right > width || bottom > height {
        return Err(ClipFileError::InvalidTileRegion {
            image_size: Rect::new(0, 0, width, height),
            region,
        });
    }

    Ok(TileRegion {
        x: usize::try_from(region.x).map_err(|_| ClipFileError::TileSizeOverflow)?,
        y: usize::try_from(region.y).map_err(|_| ClipFileError::TileSizeOverflow)?,
        width: usize::try_from(region.width).map_err(|_| ClipFileError::TileSizeOverflow)?,
        height: usize::try_from(region.height).map_err(|_| ClipFileError::TileSizeOverflow)?,
        right: usize::try_from(right).map_err(|_| ClipFileError::TileSizeOverflow)?,
        bottom: usize::try_from(bottom).map_err(|_| ClipFileError::TileSizeOverflow)?,
    })
}

fn tile_span(start: usize, end: usize) -> std::ops::Range<usize> {
    start / TILE_SIZE..end.div_ceil(TILE_SIZE)
}

fn tile_copy_rect(region: TileRegion, tile_x: usize, tile_y: usize) -> Option<TileCopyRect> {
    let tile_origin_x = tile_x * TILE_SIZE;
    let tile_origin_y = tile_y * TILE_SIZE;
    let copy_x0 = region.x.max(tile_origin_x);
    let copy_y0 = region.y.max(tile_origin_y);
    let copy_x1 = region.right.min(tile_origin_x + TILE_SIZE);
    let copy_y1 = region.bottom.min(tile_origin_y + TILE_SIZE);
    if copy_x1 <= copy_x0 || copy_y1 <= copy_y0 {
        return None;
    }

    Some(TileCopyRect {
        source_x: copy_x0 - tile_origin_x,
        source_y: copy_y0 - tile_origin_y,
        dest_x: copy_x0 - region.x,
        dest_y: copy_y0 - region.y,
        width: copy_x1 - copy_x0,
        height: copy_y1 - copy_y0,
    })
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

#[cfg(test)]
mod tests {
    use clip_model::Rect;

    use super::{
        GRAY_RGBA_TILE_BYTES, MASK_TILE_BYTES, MONO_RGBA_TILE_BYTES, RGBA_TILE_BYTES, TILE_SIZE,
        decode_gray_rgba_tiles_region, decode_mono_rgba_tiles_region, decode_rgba_tiles_region,
        gray_rgba_tile_blob_len, mono_rgba_tile_blob_len, rgba_tile_blob_len,
    };

    #[test]
    fn rgba_region_decodes_across_tile_boundary() {
        let width = 258;
        let height = 2;
        let mut blob = vec![0u8; rgba_tile_blob_len(width, height).unwrap()];
        set_rgba_pixel(&mut blob, width, 255, 0, [1, 2, 3, 4]);
        set_rgba_pixel(&mut blob, width, 256, 0, [5, 6, 7, 8]);
        set_rgba_pixel(&mut blob, width, 257, 1, [9, 10, 11, 12]);

        let image =
            decode_rgba_tiles_region(&blob, width, height, Rect::new(255, 0, 3, 2)).unwrap();

        assert_eq!(image.width, 3);
        assert_eq!(image.height, 2);
        assert_eq!(pixel_at(&image.pixels, 3, 0, 0), [1, 2, 3, 4]);
        assert_eq!(pixel_at(&image.pixels, 3, 1, 0), [5, 6, 7, 8]);
        assert_eq!(pixel_at(&image.pixels, 3, 2, 1), [9, 10, 11, 12]);
        assert_eq!(pixel_at(&image.pixels, 3, 0, 1), [0, 0, 0, 0]);
    }

    #[test]
    fn gray_region_decodes_across_tile_boundary() {
        let width = 258;
        let height = 1;
        let mut blob = vec![0u8; gray_rgba_tile_blob_len(width, height).unwrap()];
        set_gray_pixel(&mut blob, width, 255, 0, 10, 20);
        set_gray_pixel(&mut blob, width, 256, 0, 30, 40);

        let image =
            decode_gray_rgba_tiles_region(&blob, width, height, Rect::new(255, 0, 3, 1)).unwrap();

        assert_eq!(image.width, 3);
        assert_eq!(image.height, 1);
        assert_eq!(pixel_at(&image.pixels, 3, 0, 0), [10, 10, 10, 20]);
        assert_eq!(pixel_at(&image.pixels, 3, 1, 0), [30, 30, 30, 40]);
        assert_eq!(pixel_at(&image.pixels, 3, 2, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn mono_region_decodes_across_tile_boundary() {
        let width = 258;
        let height = 1;
        let mut blob = vec![0u8; mono_rgba_tile_blob_len(width, height).unwrap()];
        set_mono_pixel(&mut blob, width, 255, 0, true, true);
        set_mono_pixel(&mut blob, width, 256, 0, false, true);
        set_mono_pixel(&mut blob, width, 257, 0, true, false);

        let image =
            decode_mono_rgba_tiles_region(&blob, width, height, Rect::new(255, 0, 3, 1)).unwrap();

        assert_eq!(image.width, 3);
        assert_eq!(image.height, 1);
        assert_eq!(pixel_at(&image.pixels, 3, 0, 0), [255, 255, 255, 255]);
        assert_eq!(pixel_at(&image.pixels, 3, 1, 0), [0, 0, 0, 255]);
        assert_eq!(pixel_at(&image.pixels, 3, 2, 0), [0, 0, 0, 0]);
    }

    fn set_rgba_pixel(blob: &mut [u8], image_width: u32, x: usize, y: usize, rgba: [u8; 4]) {
        let (tile_start, bit_index) = tile_pixel_address(image_width, x, y, RGBA_TILE_BYTES);
        blob[tile_start + bit_index] = rgba[3];
        let bgra_start = tile_start + MASK_TILE_BYTES + bit_index * 4;
        blob[bgra_start] = rgba[2];
        blob[bgra_start + 1] = rgba[1];
        blob[bgra_start + 2] = rgba[0];
    }

    fn set_gray_pixel(blob: &mut [u8], image_width: u32, x: usize, y: usize, gray: u8, alpha: u8) {
        let (tile_start, bit_index) = tile_pixel_address(image_width, x, y, GRAY_RGBA_TILE_BYTES);
        blob[tile_start + bit_index] = alpha;
        blob[tile_start + MASK_TILE_BYTES + bit_index] = gray;
    }

    fn set_mono_pixel(
        blob: &mut [u8],
        image_width: u32,
        x: usize,
        y: usize,
        white: bool,
        alpha: bool,
    ) {
        let (tile_start, bit_index) = tile_pixel_address(image_width, x, y, MONO_RGBA_TILE_BYTES);
        if alpha {
            set_bit(blob, tile_start, bit_index);
        }
        if white {
            set_bit(blob, tile_start + MONO_RGBA_TILE_BYTES / 2, bit_index);
        }
    }

    fn tile_pixel_address(
        image_width: u32,
        x: usize,
        y: usize,
        tile_bytes: usize,
    ) -> (usize, usize) {
        let cols = (image_width as usize).div_ceil(TILE_SIZE);
        let tile_x = x / TILE_SIZE;
        let tile_y = y / TILE_SIZE;
        let local_x = x % TILE_SIZE;
        let local_y = y % TILE_SIZE;
        let tile_start = (tile_y * cols + tile_x) * tile_bytes;
        (tile_start, local_y * TILE_SIZE + local_x)
    }

    fn set_bit(blob: &mut [u8], plane_start: usize, bit_index: usize) {
        blob[plane_start + bit_index / 8] |= 0x80 >> (bit_index % 8);
    }

    fn pixel_at(pixels: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
        let start = (y * width + x) * 4;
        pixels[start..start + 4].try_into().unwrap()
    }
}
