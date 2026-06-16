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
}

pub fn inspect_external_tile_blocks(
    body: &[u8],
    per_tile_len: usize,
    expected_tile_count: usize,
) -> Result<ExternalTileBlockStats, ClipFileError> {
    let mut stats = ExternalTileBlockStats {
        external_id: String::new(),
        block_count: 0,
        compressed_block_count: 0,
        empty_block_count: 0,
        compressed_bytes: 0,
        uncompressed_bytes: 0,
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
    Ok(stats)
}
