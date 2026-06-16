use clip_model::Rect;

use crate::ClipFileError;
use crate::tiles::{
    AlphaTileImage, GRAY_RGBA_TILE_BYTES, MASK_TILE_BYTES, MONO_RGBA_TILE_BYTES, RGBA_TILE_BYTES,
    RgbaTileImage, TILE_SIZE,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct TileBlockRef<'a> {
    pub(crate) tile_index: usize,
    pub(crate) bytes: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TileBlockSelection {
    cols: usize,
    tile_x_start: usize,
    tile_x_end: usize,
    tile_y_start: usize,
    tile_y_end: usize,
    count: usize,
}

impl TileBlockSelection {
    pub(crate) fn count(self) -> usize {
        self.count
    }

    pub(crate) fn covers_all_tiles(self, expected_tile_count: usize) -> bool {
        self.count == expected_tile_count
    }

    pub(crate) fn contains(self, tile_index: usize) -> bool {
        let tile_x = tile_index % self.cols;
        let tile_y = tile_index / self.cols;
        tile_x >= self.tile_x_start
            && tile_x < self.tile_x_end
            && tile_y >= self.tile_y_start
            && tile_y < self.tile_y_end
    }

    pub(crate) fn last_tile_index(self) -> Option<usize> {
        if self.count == 0 {
            return None;
        }
        Some((self.tile_y_end - 1) * self.cols + self.tile_x_end - 1)
    }
}

pub(crate) struct TileRegionWriter {
    region: TileRegion,
    cols: usize,
    tile_count: usize,
    pixels: Vec<u8>,
}

impl TileRegionWriter {
    pub(crate) fn new(width: u32, height: u32, region: Rect) -> Result<Self, ClipFileError> {
        let region = validate_tile_region(width, height, region)?;
        let cols = tile_cols(width)?;
        let tile_count = checked_tile_count(width, height)?;
        let pixels = rgba_output_buffer(region.width, region.height)?;
        Ok(Self {
            region,
            cols,
            tile_count,
            pixels,
        })
    }

    pub(crate) fn write_rgba_block(
        &mut self,
        block: TileBlockRef<'_>,
    ) -> Result<(), ClipFileError> {
        validate_block(&block, self.tile_count, RGBA_TILE_BYTES)?;
        let tile_x = block.tile_index % self.cols;
        let tile_y = block.tile_index / self.cols;
        let Some(copy) = tile_copy_rect(self.region, tile_x, tile_y) else {
            return Ok(());
        };
        let alpha_start = 0usize;
        let bgra_start = MASK_TILE_BYTES;

        for row in 0..copy.height {
            let source_pixel = (copy.source_y + row) * TILE_SIZE + copy.source_x;
            let alpha_row_start = alpha_start + source_pixel;
            let bgra_row_start = bgra_start + source_pixel * 4;
            let dest_start = ((copy.dest_y + row) * self.region.width + copy.dest_x) * 4;
            let alpha_row = &block.bytes[alpha_row_start..alpha_row_start + copy.width];
            let bgra_row = &block.bytes[bgra_row_start..bgra_row_start + copy.width * 4];
            let dest_row = &mut self.pixels[dest_start..dest_start + copy.width * 4];

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
        Ok(())
    }

    pub(crate) fn write_gray_block(
        &mut self,
        block: TileBlockRef<'_>,
    ) -> Result<(), ClipFileError> {
        validate_block(&block, self.tile_count, GRAY_RGBA_TILE_BYTES)?;
        let tile_x = block.tile_index % self.cols;
        let tile_y = block.tile_index / self.cols;
        let Some(copy) = tile_copy_rect(self.region, tile_x, tile_y) else {
            return Ok(());
        };
        let alpha_start = 0usize;
        let gray_start = MASK_TILE_BYTES;

        for row in 0..copy.height {
            let source_pixel = (copy.source_y + row) * TILE_SIZE + copy.source_x;
            let alpha_row_start = alpha_start + source_pixel;
            let gray_row_start = gray_start + source_pixel;
            let dest_start = ((copy.dest_y + row) * self.region.width + copy.dest_x) * 4;
            let alpha_row = &block.bytes[alpha_row_start..alpha_row_start + copy.width];
            let gray_row = &block.bytes[gray_row_start..gray_row_start + copy.width];
            let dest_row = &mut self.pixels[dest_start..dest_start + copy.width * 4];

            for ((dst, gray), alpha) in dest_row.chunks_exact_mut(4).zip(gray_row).zip(alpha_row) {
                dst[0] = *gray;
                dst[1] = *gray;
                dst[2] = *gray;
                dst[3] = *alpha;
            }
        }
        Ok(())
    }

    pub(crate) fn write_mono_block(
        &mut self,
        block: TileBlockRef<'_>,
    ) -> Result<(), ClipFileError> {
        validate_block(&block, self.tile_count, MONO_RGBA_TILE_BYTES)?;
        let tile_x = block.tile_index % self.cols;
        let tile_y = block.tile_index / self.cols;
        let Some(copy) = tile_copy_rect(self.region, tile_x, tile_y) else {
            return Ok(());
        };
        let white_start = MONO_RGBA_TILE_BYTES / 2;

        for row in 0..copy.height {
            let dest_start = ((copy.dest_y + row) * self.region.width + copy.dest_x) * 4;
            let dest_row = &mut self.pixels[dest_start..dest_start + copy.width * 4];

            for (local_x, dst) in dest_row.chunks_exact_mut(4).enumerate() {
                let bit_index = (copy.source_y + row) * TILE_SIZE + copy.source_x + local_x;
                let alpha = bit_plane_value(block.bytes, 0, bit_index);
                let white = bit_plane_value(block.bytes, white_start, bit_index) && alpha;
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
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<RgbaTileImage, ClipFileError> {
        region_image(self.region, self.pixels)
    }
}

pub(crate) struct AlphaTileRegionWriter {
    region: TileRegion,
    cols: usize,
    tile_count: usize,
    pixels: Vec<u8>,
}

impl AlphaTileRegionWriter {
    pub(crate) fn new(width: u32, height: u32, region: Rect) -> Result<Self, ClipFileError> {
        let region = validate_tile_region(width, height, region)?;
        let cols = tile_cols(width)?;
        let tile_count = checked_tile_count(width, height)?;
        let pixels = alpha_output_buffer(region.width, region.height)?;
        Ok(Self {
            region,
            cols,
            tile_count,
            pixels,
        })
    }

    pub(crate) fn write_alpha_block(
        &mut self,
        block: TileBlockRef<'_>,
    ) -> Result<(), ClipFileError> {
        validate_block(&block, self.tile_count, MASK_TILE_BYTES)?;
        let tile_x = block.tile_index % self.cols;
        let tile_y = block.tile_index / self.cols;
        let Some(copy) = tile_copy_rect(self.region, tile_x, tile_y) else {
            return Ok(());
        };

        for row in 0..copy.height {
            let source_pixel = (copy.source_y + row) * TILE_SIZE + copy.source_x;
            let source_start = source_pixel;
            let dest_start = (copy.dest_y + row) * self.region.width + copy.dest_x;
            self.pixels[dest_start..dest_start + copy.width]
                .copy_from_slice(&block.bytes[source_start..source_start + copy.width]);
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<AlphaTileImage, ClipFileError> {
        alpha_region_image(self.region, self.pixels)
    }
}

pub(crate) fn tile_block_selection_for_region(
    width: u32,
    height: u32,
    region: Rect,
) -> Result<TileBlockSelection, ClipFileError> {
    let region = validate_tile_region(width, height, region)?;
    let cols = tile_cols(width)?;
    let tile_x_start = region.x / TILE_SIZE;
    let tile_x_end = region.right.div_ceil(TILE_SIZE);
    let tile_y_start = region.y / TILE_SIZE;
    let tile_y_end = region.bottom.div_ceil(TILE_SIZE);
    let width = tile_x_end
        .checked_sub(tile_x_start)
        .ok_or(ClipFileError::TileSizeOverflow)?;
    let height = tile_y_end
        .checked_sub(tile_y_start)
        .ok_or(ClipFileError::TileSizeOverflow)?;
    let count = width
        .checked_mul(height)
        .ok_or(ClipFileError::TileSizeOverflow)?;
    Ok(TileBlockSelection {
        cols,
        tile_x_start,
        tile_x_end,
        tile_y_start,
        tile_y_end,
        count,
    })
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

fn validate_block(
    block: &TileBlockRef<'_>,
    tile_count: usize,
    per_tile: usize,
) -> Result<(), ClipFileError> {
    if block.tile_index >= tile_count {
        return Err(ClipFileError::UnexpectedTileCount {
            expected: tile_count,
            actual: block.tile_index + 1,
        });
    }
    if block.bytes.len() != per_tile {
        return Err(ClipFileError::InvalidTileBlobSize {
            actual: block.bytes.len(),
            per_tile,
        });
    }
    Ok(())
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

fn checked_tile_count(width: u32, height: u32) -> Result<usize, ClipFileError> {
    let cols = tile_cols(width)?;
    let rows = usize::try_from(div_ceil_u32(height, TILE_SIZE as u32)?)
        .map_err(|_| ClipFileError::TileSizeOverflow)?;
    cols.checked_mul(rows)
        .ok_or(ClipFileError::TileSizeOverflow)
}

fn tile_cols(width: u32) -> Result<usize, ClipFileError> {
    usize::try_from(div_ceil_u32(width, TILE_SIZE as u32)?)
        .map_err(|_| ClipFileError::TileSizeOverflow)
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

fn alpha_output_buffer(width: usize, height: usize) -> Result<Vec<u8>, ClipFileError> {
    let pixel_count = width
        .checked_mul(height)
        .ok_or(ClipFileError::TileSizeOverflow)?;
    Ok(vec![0u8; pixel_count])
}

fn region_image(region: TileRegion, pixels: Vec<u8>) -> Result<RgbaTileImage, ClipFileError> {
    Ok(RgbaTileImage {
        width: u32::try_from(region.width).map_err(|_| ClipFileError::TileSizeOverflow)?,
        height: u32::try_from(region.height).map_err(|_| ClipFileError::TileSizeOverflow)?,
        pixels,
    })
}

fn alpha_region_image(
    region: TileRegion,
    pixels: Vec<u8>,
) -> Result<AlphaTileImage, ClipFileError> {
    Ok(AlphaTileImage {
        width: u32::try_from(region.width).map_err(|_| ClipFileError::TileSizeOverflow)?,
        height: u32::try_from(region.height).map_err(|_| ClipFileError::TileSizeOverflow)?,
        pixels,
    })
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

#[cfg(test)]
mod tests {
    use clip_model::Rect;

    use super::{AlphaTileRegionWriter, TileBlockRef};
    use crate::tiles::{MASK_TILE_BYTES, TILE_SIZE};

    #[test]
    fn alpha_region_writer_copies_only_requested_tile_rect() {
        let mut left = vec![0u8; MASK_TILE_BYTES];
        let mut right = vec![0u8; MASK_TILE_BYTES];
        left[255] = 10;
        left[TILE_SIZE + 255] = 11;
        right[0] = 20;
        right[1] = 21;
        right[TILE_SIZE] = 22;
        right[TILE_SIZE + 1] = 23;

        let mut writer = AlphaTileRegionWriter::new(512, 256, Rect::new(255, 0, 3, 2)).unwrap();
        writer
            .write_alpha_block(TileBlockRef {
                tile_index: 0,
                bytes: &left,
            })
            .unwrap();
        writer
            .write_alpha_block(TileBlockRef {
                tile_index: 1,
                bytes: &right,
            })
            .unwrap();
        let image = writer.finish().unwrap();

        assert_eq!(image.width, 3);
        assert_eq!(image.height, 2);
        assert_eq!(image.pixels, vec![10, 20, 21, 11, 22, 23]);
    }
}
