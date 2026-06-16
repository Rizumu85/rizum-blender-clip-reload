use crate::ClipFileError;

use super::{ExternalTileBlock, ExternalTileBlocks, visit_external_tile_blocks_matching};

pub fn decode_external_tile_blocks(
    body: &[u8],
    empty_fill: u8,
    per_tile_len: usize,
    expected_tile_count: usize,
    wanted_tile_indices: &[usize],
) -> Result<ExternalTileBlocks, ClipFileError> {
    let mut wanted = wanted_tile_indices.to_vec();
    wanted.sort_unstable();
    wanted.dedup();

    let mut next_wanted = 0usize;
    let mut blocks = Vec::with_capacity(wanted.len());
    let external_id = visit_external_tile_blocks_matching(
        body,
        empty_fill,
        per_tile_len,
        expected_tile_count,
        wanted.len(),
        wanted.last().copied(),
        true,
        |tile_index| {
            if wanted.get(next_wanted).copied() != Some(tile_index) {
                return false;
            }
            next_wanted += 1;
            true
        },
        |tile_index, bytes| {
            blocks.push(ExternalTileBlock {
                tile_index,
                bytes: bytes.to_vec(),
            });
            Ok(())
        },
    )?;

    if blocks.len() != wanted.len() {
        return Err(ClipFileError::UnexpectedTileCount {
            expected: wanted.len(),
            actual: blocks.len(),
        });
    }

    Ok(ExternalTileBlocks {
        external_id,
        blocks,
    })
}
