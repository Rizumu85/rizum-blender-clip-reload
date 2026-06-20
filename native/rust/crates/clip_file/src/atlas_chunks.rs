use clip_model::{CanvasSize, Rect};
use std::time::Instant;

use crate::ClipFileError;
use crate::decode_profile;
use crate::tile_region::{TileBlockRef, TileBlockSelection, tile_block_selection_for_region};
use crate::tiles::{
    GRAY_RGBA_TILE_BYTES, MASK_TILE_BYTES, MONO_RGBA_TILE_BYTES, RGBA_TILE_BYTES, TILE_SIZE,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RgbaAtlasTileChunk {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RgbaAtlasChunkTarget {
    pub(crate) size: CanvasSize,
    pub(crate) origin_x: u32,
    pub(crate) origin_y: u32,
}

pub(crate) struct RgbaAtlasChunkCollector {
    region: TileRegion,
    cols: usize,
    tile_count: usize,
    target: RgbaAtlasChunkTarget,
    chunks: Vec<RgbaAtlasTileChunk>,
}

impl RgbaAtlasChunkCollector {
    pub(crate) fn new(
        width: u32,
        height: u32,
        region: Rect,
        target: RgbaAtlasChunkTarget,
    ) -> Result<Self, ClipFileError> {
        let region = validate_tile_region(width, height, region)?;
        validate_target(target, region)?;
        Ok(Self {
            region,
            cols: tile_cols(width)?,
            tile_count: checked_tile_count(width, height)?,
            target,
            chunks: Vec::new(),
        })
    }

    pub(crate) fn selection(&self) -> Result<TileBlockSelection, ClipFileError> {
        tile_block_selection_for_region(
            self.region.image_width,
            self.region.image_height,
            Rect::new(
                self.region.x as u32,
                self.region.y as u32,
                self.region.width as u32,
                self.region.height as u32,
            ),
        )
    }

    pub(crate) fn write_rgba_block(
        &mut self,
        block: TileBlockRef<'_>,
    ) -> Result<(), ClipFileError> {
        let start = Instant::now();
        validate_block(&block, self.tile_count, RGBA_TILE_BYTES)?;
        let tile_x = block.tile_index % self.cols;
        let tile_y = block.tile_index / self.cols;
        let Some(copy) = tile_copy_rect(self.region, tile_x, tile_y) else {
            return Ok(());
        };
        let mut pixels = rgba_chunk_buffer(copy.width, copy.height)?;
        let alpha_start = 0usize;
        let bgra_start = MASK_TILE_BYTES;

        for row in 0..copy.height {
            let source_pixel = (copy.source_y + row) * TILE_SIZE + copy.source_x;
            let alpha_row_start = alpha_start + source_pixel;
            let bgra_row_start = bgra_start + source_pixel * 4;
            let dest_start = row * copy.width * 4;
            let alpha_row = &block.bytes[alpha_row_start..alpha_row_start + copy.width];
            let bgra_row = &block.bytes[bgra_row_start..bgra_row_start + copy.width * 4];
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
        let result = self.push_chunk(copy, pixels);
        decode_profile::record_tile_swizzle_rgba(start.elapsed());
        decode_profile::record_atlas_chunk_build(start.elapsed());
        result
    }

    pub(crate) fn write_gray_block(
        &mut self,
        block: TileBlockRef<'_>,
    ) -> Result<(), ClipFileError> {
        let start = Instant::now();
        validate_block(&block, self.tile_count, GRAY_RGBA_TILE_BYTES)?;
        let tile_x = block.tile_index % self.cols;
        let tile_y = block.tile_index / self.cols;
        let Some(copy) = tile_copy_rect(self.region, tile_x, tile_y) else {
            return Ok(());
        };
        let mut pixels = rgba_chunk_buffer(copy.width, copy.height)?;
        let alpha_start = 0usize;
        let gray_start = MASK_TILE_BYTES;

        for row in 0..copy.height {
            let source_pixel = (copy.source_y + row) * TILE_SIZE + copy.source_x;
            let alpha_row_start = alpha_start + source_pixel;
            let gray_row_start = gray_start + source_pixel;
            let dest_start = row * copy.width * 4;
            let alpha_row = &block.bytes[alpha_row_start..alpha_row_start + copy.width];
            let gray_row = &block.bytes[gray_row_start..gray_row_start + copy.width];
            let dest_row = &mut pixels[dest_start..dest_start + copy.width * 4];

            for ((dst, gray), alpha) in dest_row.chunks_exact_mut(4).zip(gray_row).zip(alpha_row) {
                dst[0] = *gray;
                dst[1] = *gray;
                dst[2] = *gray;
                dst[3] = *alpha;
            }
        }
        let result = self.push_chunk(copy, pixels);
        decode_profile::record_tile_swizzle_rgba(start.elapsed());
        decode_profile::record_atlas_chunk_build(start.elapsed());
        result
    }

    pub(crate) fn write_mono_block(
        &mut self,
        block: TileBlockRef<'_>,
    ) -> Result<(), ClipFileError> {
        let start = Instant::now();
        validate_block(&block, self.tile_count, MONO_RGBA_TILE_BYTES)?;
        let tile_x = block.tile_index % self.cols;
        let tile_y = block.tile_index / self.cols;
        let Some(copy) = tile_copy_rect(self.region, tile_x, tile_y) else {
            return Ok(());
        };
        let mut pixels = rgba_chunk_buffer(copy.width, copy.height)?;
        let white_start = MONO_RGBA_TILE_BYTES / 2;

        for row in 0..copy.height {
            let dest_start = row * copy.width * 4;
            let dest_row = &mut pixels[dest_start..dest_start + copy.width * 4];

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
        let result = self.push_chunk(copy, pixels);
        decode_profile::record_tile_swizzle_rgba(start.elapsed());
        decode_profile::record_atlas_chunk_build(start.elapsed());
        result
    }

    pub(crate) fn finish(self) -> Vec<RgbaAtlasTileChunk> {
        self.chunks
    }

    fn push_chunk(&mut self, copy: TileCopyRect, pixels: Vec<u8>) -> Result<(), ClipFileError> {
        let x = self
            .target
            .origin_x
            .checked_add(u32::try_from(copy.dest_x).map_err(|_| ClipFileError::TileSizeOverflow)?)
            .ok_or(ClipFileError::TileSizeOverflow)?;
        let y = self
            .target
            .origin_y
            .checked_add(u32::try_from(copy.dest_y).map_err(|_| ClipFileError::TileSizeOverflow)?)
            .ok_or(ClipFileError::TileSizeOverflow)?;
        self.chunks.push(RgbaAtlasTileChunk {
            x,
            y,
            width: u32::try_from(copy.width).map_err(|_| ClipFileError::TileSizeOverflow)?,
            height: u32::try_from(copy.height).map_err(|_| ClipFileError::TileSizeOverflow)?,
            pixels,
        });
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
struct TileRegion {
    image_width: u32,
    image_height: u32,
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
        image_width: width,
        image_height: height,
        x: usize::try_from(region.x).map_err(|_| ClipFileError::TileSizeOverflow)?,
        y: usize::try_from(region.y).map_err(|_| ClipFileError::TileSizeOverflow)?,
        width: usize::try_from(region.width).map_err(|_| ClipFileError::TileSizeOverflow)?,
        height: usize::try_from(region.height).map_err(|_| ClipFileError::TileSizeOverflow)?,
        right: usize::try_from(right).map_err(|_| ClipFileError::TileSizeOverflow)?,
        bottom: usize::try_from(bottom).map_err(|_| ClipFileError::TileSizeOverflow)?,
    })
}

fn validate_target(target: RgbaAtlasChunkTarget, region: TileRegion) -> Result<(), ClipFileError> {
    let right = target
        .origin_x
        .checked_add(u32::try_from(region.width).map_err(|_| ClipFileError::TileSizeOverflow)?)
        .ok_or(ClipFileError::TileSizeOverflow)?;
    let bottom = target
        .origin_y
        .checked_add(u32::try_from(region.height).map_err(|_| ClipFileError::TileSizeOverflow)?)
        .ok_or(ClipFileError::TileSizeOverflow)?;
    if right > target.size.width || bottom > target.size.height {
        return Err(ClipFileError::InvalidTileRegion {
            image_size: Rect::new(0, 0, target.size.width, target.size.height),
            region: Rect::new(
                target.origin_x,
                target.origin_y,
                right - target.origin_x,
                bottom - target.origin_y,
            ),
        });
    }
    Ok(())
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

fn rgba_chunk_buffer(width: usize, height: usize) -> Result<Vec<u8>, ClipFileError> {
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
