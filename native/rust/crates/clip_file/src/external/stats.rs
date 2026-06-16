use crate::ClipFileError;

use super::{ExternalBlockPayload, visit_external_data_blocks};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalTileBlockStats {
    pub external_id: String,
    pub block_count: usize,
    pub compressed_block_count: usize,
    pub empty_block_count: usize,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub compressed_tile_bounds: Option<ExternalTileBlockBounds>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalTileBlockInspection {
    pub stats: ExternalTileBlockStats,
    pub compressed_tiles: Vec<ExternalCompressedTile>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExternalTileBlockBounds {
    pub tile_x: usize,
    pub tile_y: usize,
    pub tile_width: usize,
    pub tile_height: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExternalCompressedTile {
    pub tile_index: usize,
    pub tile_x: usize,
    pub tile_y: usize,
    pub compressed_bytes: usize,
}

pub fn inspect_external_tile_blocks(
    body: &[u8],
    per_tile_len: usize,
    expected_tile_count: usize,
    tile_cols: usize,
) -> Result<ExternalTileBlockStats, ClipFileError> {
    Ok(inspect_external_tile_blocks_inner(
        body,
        per_tile_len,
        expected_tile_count,
        tile_cols,
        false,
    )?
    .stats)
}

pub fn inspect_external_tile_blocks_with_compressed_tiles(
    body: &[u8],
    per_tile_len: usize,
    expected_tile_count: usize,
    tile_cols: usize,
) -> Result<ExternalTileBlockInspection, ClipFileError> {
    inspect_external_tile_blocks_inner(body, per_tile_len, expected_tile_count, tile_cols, true)
}

fn inspect_external_tile_blocks_inner(
    body: &[u8],
    per_tile_len: usize,
    expected_tile_count: usize,
    tile_cols: usize,
    collect_compressed_tiles: bool,
) -> Result<ExternalTileBlockInspection, ClipFileError> {
    if tile_cols == 0 {
        return Err(ClipFileError::TileSizeOverflow);
    }

    let mut bounds = CompressedTileBoundsBuilder::default();
    let mut compressed_tiles = Vec::new();
    let mut stats = ExternalTileBlockStats {
        external_id: String::new(),
        block_count: 0,
        compressed_block_count: 0,
        empty_block_count: 0,
        compressed_bytes: 0,
        uncompressed_bytes: 0,
        compressed_tile_bounds: None,
    };
    let visited = visit_external_data_blocks(body, |block| {
        if block.index >= expected_tile_count {
            return Ok(false);
        }
        if block.uncompressed_len != per_tile_len {
            return Err(ClipFileError::InvalidExternalDataBlock(
                "unexpected tile block size",
            ));
        }

        stats.block_count = stats
            .block_count
            .checked_add(1)
            .ok_or(ClipFileError::TileSizeOverflow)?;
        stats.uncompressed_bytes = stats
            .uncompressed_bytes
            .checked_add(block.uncompressed_len as u64)
            .ok_or(ClipFileError::TileSizeOverflow)?;
        match block.payload {
            ExternalBlockPayload::Compressed(compressed) => {
                stats.compressed_block_count = stats
                    .compressed_block_count
                    .checked_add(1)
                    .ok_or(ClipFileError::TileSizeOverflow)?;
                stats.compressed_bytes = stats
                    .compressed_bytes
                    .checked_add(compressed.len() as u64)
                    .ok_or(ClipFileError::TileSizeOverflow)?;
                bounds.include(block.index, tile_cols);
                if collect_compressed_tiles {
                    compressed_tiles.push(ExternalCompressedTile {
                        tile_index: block.index,
                        tile_x: block.index % tile_cols,
                        tile_y: block.index / tile_cols,
                        compressed_bytes: compressed.len(),
                    });
                }
            }
            ExternalBlockPayload::Empty => {
                stats.empty_block_count = stats
                    .empty_block_count
                    .checked_add(1)
                    .ok_or(ClipFileError::TileSizeOverflow)?;
            }
        }

        Ok(stats.block_count < expected_tile_count)
    })?;

    if stats.block_count != expected_tile_count {
        return Err(ClipFileError::UnexpectedTileCount {
            expected: expected_tile_count,
            actual: stats.block_count,
        });
    }

    stats.external_id = visited.external_id;
    stats.compressed_tile_bounds = bounds.finish();
    Ok(ExternalTileBlockInspection {
        stats,
        compressed_tiles,
    })
}

#[derive(Default)]
struct CompressedTileBoundsBuilder {
    min_x: Option<usize>,
    min_y: Option<usize>,
    max_x: usize,
    max_y: usize,
}

impl CompressedTileBoundsBuilder {
    fn include(&mut self, tile_index: usize, tile_cols: usize) {
        let x = tile_index % tile_cols;
        let y = tile_index / tile_cols;
        self.min_x = Some(self.min_x.map_or(x, |min_x| min_x.min(x)));
        self.min_y = Some(self.min_y.map_or(y, |min_y| min_y.min(y)));
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }

    fn finish(self) -> Option<ExternalTileBlockBounds> {
        Some(ExternalTileBlockBounds {
            tile_x: self.min_x?,
            tile_y: self.min_y?,
            tile_width: self.max_x - self.min_x? + 1,
            tile_height: self.max_y - self.min_y? + 1,
        })
    }
}
