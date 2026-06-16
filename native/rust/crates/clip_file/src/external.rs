use std::io::Read;

use flate2::read::ZlibDecoder;

use crate::ClipFileError;

const BLOCK_NAME_MARKER: u32 = 0x0042_006c;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalTileBlob {
    pub external_id: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalTileBlock {
    pub tile_index: usize,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalTileBlocks {
    pub external_id: String,
    pub blocks: Vec<ExternalTileBlock>,
}

pub fn decode_external_tile_blob(
    body: &[u8],
    empty_fill: u8,
    expected_len: Option<usize>,
) -> Result<ExternalTileBlob, ClipFileError> {
    let mut output = match expected_len {
        Some(expected_len) => vec![empty_fill; expected_len],
        None => Vec::new(),
    };
    let mut output_len = 0usize;

    let visited = visit_external_data_blocks(body, |block| {
        match block.payload {
            ExternalBlockPayload::Compressed(compressed) => append_inflated_bytes(
                &mut output,
                &mut output_len,
                compressed,
                block.uncompressed_len,
                expected_len,
            )?,
            ExternalBlockPayload::Empty => append_empty_bytes(
                &mut output,
                &mut output_len,
                empty_fill,
                block.uncompressed_len,
                expected_len,
            )?,
        }
        Ok(expected_len
            .map(|expected_len| output_len < expected_len)
            .unwrap_or(true))
    })?;

    if expected_len.is_some() {
        output.truncate(output_len);
    }

    Ok(ExternalTileBlob {
        external_id: visited.external_id,
        bytes: output,
    })
}

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
    if wanted
        .last()
        .is_some_and(|tile_index| *tile_index >= expected_tile_count)
    {
        return Err(ClipFileError::UnexpectedTileCount {
            expected: expected_tile_count,
            actual: wanted.last().copied().unwrap_or(0) + 1,
        });
    }

    let mut next_wanted = 0usize;
    let mut blocks = Vec::with_capacity(wanted.len());
    let visited = visit_external_data_blocks(body, |block| {
        if block.index >= expected_tile_count {
            return Ok(false);
        }
        if wanted.get(next_wanted).copied() != Some(block.index) {
            return Ok(true);
        }
        next_wanted += 1;
        if block.uncompressed_len != per_tile_len {
            return Err(ClipFileError::InvalidExternalDataBlock(
                "unexpected tile block size",
            ));
        }

        let bytes = match block.payload {
            ExternalBlockPayload::Compressed(compressed) => {
                inflate_block_to_vec(compressed, block.uncompressed_len)?
            }
            ExternalBlockPayload::Empty => vec![empty_fill; block.uncompressed_len],
        };
        blocks.push(ExternalTileBlock {
            tile_index: block.index,
            bytes,
        });
        Ok(next_wanted < wanted.len())
    })?;

    if visited.data_block_count < expected_tile_count {
        return Err(ClipFileError::UnexpectedTileCount {
            expected: expected_tile_count,
            actual: visited.data_block_count,
        });
    }
    if blocks.len() != wanted.len() {
        return Err(ClipFileError::UnexpectedTileCount {
            expected: wanted.len(),
            actual: blocks.len(),
        });
    }

    Ok(ExternalTileBlocks {
        external_id: visited.external_id,
        blocks,
    })
}

struct VisitedExternalBlocks {
    external_id: String,
    data_block_count: usize,
}

struct ExternalBlockData<'a> {
    index: usize,
    uncompressed_len: usize,
    payload: ExternalBlockPayload<'a>,
}

enum ExternalBlockPayload<'a> {
    Compressed(&'a [u8]),
    Empty,
}

fn visit_external_data_blocks<F>(
    body: &[u8],
    mut visit: F,
) -> Result<VisitedExternalBlocks, ClipFileError>
where
    F: FnMut(ExternalBlockData<'_>) -> Result<bool, ClipFileError>,
{
    let mut pos = 0usize;
    let id_len = read_be_u64(body, &mut pos)? as usize;
    let id_end = pos
        .checked_add(id_len)
        .ok_or(ClipFileError::InvalidExternalDataHeader)?;
    let external_id = std::str::from_utf8(
        body.get(pos..id_end)
            .ok_or(ClipFileError::InvalidExternalDataHeader)?,
    )
    .map_err(|_| ClipFileError::InvalidExternalDataHeader)?
    .to_owned();
    pos = id_end;
    skip(body, &mut pos, 8)?;

    let mut data_block_count = 0usize;
    while pos < body.len() {
        if body.len().saturating_sub(pos) < 8 {
            break;
        }
        let block_start = pos;
        let s1 = read_be_u32(body, &mut pos)?;
        let s2 = read_be_u32(body, &mut pos)?;

        let (name_len, outer_payload) = if s2 == BLOCK_NAME_MARKER {
            pos = block_start + 4;
            (s1, 0)
        } else {
            (s2, s1)
        };

        if name_len == 0 || name_len >= 256 {
            return Err(ClipFileError::InvalidExternalDataBlock(
                "bad block name length",
            ));
        }

        let name_byte_len = usize::try_from(name_len)
            .ok()
            .and_then(|len| len.checked_mul(2))
            .ok_or(ClipFileError::InvalidExternalDataBlock(
                "block name too large",
            ))?;
        let name_end =
            pos.checked_add(name_byte_len)
                .ok_or(ClipFileError::InvalidExternalDataBlock(
                    "block name offset overflow",
                ))?;
        let name = read_utf16_be(body.get(pos..name_end).ok_or(
            ClipFileError::InvalidExternalDataBlock("truncated block name"),
        )?)?;
        pos = name_end;

        let inner_start = pos;
        let mut block_end =
            inner_start
                .checked_add(usize::try_from(outer_payload).map_err(|_| {
                    ClipFileError::InvalidExternalDataBlock("block payload too large")
                })?)
                .ok_or(ClipFileError::InvalidExternalDataBlock(
                    "block end overflow",
                ))?;
        let mut should_continue = true;

        match name.as_str() {
            "BlockDataBeginChunk" => {
                skip(body, &mut pos, 4)?;
                let uncompressed_len = read_be_u32(body, &mut pos)? as usize;
                skip(body, &mut pos, 8)?;
                let exists = read_be_u32(body, &mut pos)?;

                let payload = if exists > 0 {
                    let inner_len = read_be_u32(body, &mut pos)? as usize;
                    let compressed_len = read_le_u32(body, &mut pos)? as usize;
                    let compressed_end = pos.checked_add(compressed_len).ok_or(
                        ClipFileError::InvalidExternalDataBlock("compressed block overflow"),
                    )?;
                    let compressed = body.get(pos..compressed_end).ok_or(
                        ClipFileError::InvalidExternalDataBlock("truncated compressed block"),
                    )?;
                    block_end = inner_start
                        .checked_add(24)
                        .and_then(|offset| offset.checked_add(inner_len))
                        .ok_or(ClipFileError::InvalidExternalDataBlock(
                            "data block end overflow",
                        ))?;
                    ExternalBlockPayload::Compressed(compressed)
                } else {
                    block_end = inner_start.checked_add(20).ok_or(
                        ClipFileError::InvalidExternalDataBlock("empty block end overflow"),
                    )?;
                    ExternalBlockPayload::Empty
                };

                should_continue = visit(ExternalBlockData {
                    index: data_block_count,
                    uncompressed_len,
                    payload,
                })?;
                data_block_count = data_block_count
                    .checked_add(1)
                    .ok_or(ClipFileError::TileSizeOverflow)?;
            }
            "BlockStatus" | "BlockCheckSum" => {
                block_end =
                    inner_start
                        .checked_add(24)
                        .ok_or(ClipFileError::InvalidExternalDataBlock(
                            "status block end overflow",
                        ))?;
                if inner_start + 12 <= body.len() {
                    let mut header_pos = inner_start;
                    let _header_len = read_be_u32(body, &mut header_pos)?;
                    let item_count = read_be_u32(body, &mut header_pos)?;
                    let item_size = read_be_u32(body, &mut header_pos)?;
                    let variable_end = usize::try_from(item_count)
                        .ok()
                        .and_then(|count| {
                            usize::try_from(item_size)
                                .ok()
                                .and_then(|size| count.checked_mul(size))
                        })
                        .and_then(|bytes| inner_start.checked_add(12 + bytes));
                    if item_size <= 64
                        && let Some(variable_end) = variable_end
                        && variable_end <= body.len()
                    {
                        block_end = variable_end;
                    }
                }
            }
            "BlockDataEndChunk" => {}
            other => {
                return Err(ClipFileError::UnsupportedExternalDataBlock(
                    other.to_owned(),
                ));
            }
        }

        if block_end > body.len() {
            return Err(ClipFileError::InvalidExternalDataBlock(
                "block end exceeds external data body",
            ));
        }
        pos = block_end;

        if !should_continue {
            break;
        }
    }

    Ok(VisitedExternalBlocks {
        external_id,
        data_block_count,
    })
}

fn inflate_block_to_vec(
    compressed: &[u8],
    uncompressed_len: usize,
) -> Result<Vec<u8>, ClipFileError> {
    let mut output = Vec::with_capacity(uncompressed_len);
    let mut decoder = ZlibDecoder::new(compressed);
    decoder
        .read_to_end(&mut output)
        .map_err(ClipFileError::Zlib)?;
    if output.len() != uncompressed_len {
        return Err(ClipFileError::UnexpectedDecompressedSize {
            expected: uncompressed_len,
            actual: output.len(),
        });
    }
    Ok(output)
}

fn append_inflated_bytes(
    output: &mut Vec<u8>,
    output_len: &mut usize,
    compressed: &[u8],
    uncompressed_len: usize,
    expected_len: Option<usize>,
) -> Result<(), ClipFileError> {
    let mut decoder = ZlibDecoder::new(compressed);
    if let Some(expected_len) = expected_len {
        let remaining = expected_len.saturating_sub(*output_len);
        let write_len = uncompressed_len.min(remaining);
        let output_end = output_len
            .checked_add(write_len)
            .ok_or(ClipFileError::TileSizeOverflow)?;
        let written = read_into_slice(&mut decoder, &mut output[*output_len..output_end])?;
        let discarded = drain_reader(&mut decoder)?;
        let actual = written
            .checked_add(discarded)
            .ok_or(ClipFileError::TileSizeOverflow)?;
        if actual != uncompressed_len {
            return Err(ClipFileError::UnexpectedDecompressedSize {
                expected: uncompressed_len,
                actual,
            });
        }
        *output_len = output_end;
    } else {
        let start_len = output.len();
        decoder.read_to_end(output).map_err(ClipFileError::Zlib)?;
        let actual = output
            .len()
            .checked_sub(start_len)
            .ok_or(ClipFileError::TileSizeOverflow)?;
        if actual != uncompressed_len {
            return Err(ClipFileError::UnexpectedDecompressedSize {
                expected: uncompressed_len,
                actual,
            });
        }
        *output_len = output.len();
    }
    Ok(())
}

fn read_into_slice<R: Read>(reader: &mut R, output: &mut [u8]) -> Result<usize, ClipFileError> {
    let mut total = 0usize;
    while total < output.len() {
        let read = reader
            .read(&mut output[total..])
            .map_err(ClipFileError::Zlib)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read)
            .ok_or(ClipFileError::TileSizeOverflow)?;
    }
    Ok(total)
}

fn drain_reader<R: Read>(reader: &mut R) -> Result<usize, ClipFileError> {
    let mut scratch = [0u8; 8192];
    let mut total = 0usize;
    loop {
        let read = reader.read(&mut scratch).map_err(ClipFileError::Zlib)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read)
            .ok_or(ClipFileError::TileSizeOverflow)?;
    }
    Ok(total)
}

fn append_empty_bytes(
    output: &mut Vec<u8>,
    output_len: &mut usize,
    empty_fill: u8,
    len: usize,
    expected_len: Option<usize>,
) -> Result<(), ClipFileError> {
    if let Some(expected_len) = expected_len {
        let remaining = expected_len.saturating_sub(*output_len);
        let write_len = len.min(remaining);
        output[*output_len..*output_len + write_len].fill(empty_fill);
        *output_len += write_len;
    } else {
        output.resize(output.len() + len, empty_fill);
        *output_len = output.len();
    }
    Ok(())
}

fn read_utf16_be(bytes: &[u8]) -> Result<String, ClipFileError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(ClipFileError::InvalidExternalDataBlock(
            "odd-length utf16 block name",
        ));
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
        .collect();
    String::from_utf16(&units).map_err(|_| ClipFileError::InvalidExternalDataBlock("bad utf16"))
}

fn skip(bytes: &[u8], pos: &mut usize, len: usize) -> Result<(), ClipFileError> {
    let end = pos
        .checked_add(len)
        .ok_or(ClipFileError::InvalidExternalDataBlock("offset overflow"))?;
    if end > bytes.len() {
        return Err(ClipFileError::InvalidExternalDataBlock("truncated field"));
    }
    *pos = end;
    Ok(())
}

fn read_be_u64(bytes: &[u8], pos: &mut usize) -> Result<u64, ClipFileError> {
    let end = pos
        .checked_add(8)
        .ok_or(ClipFileError::InvalidExternalDataBlock("offset overflow"))?;
    let value = u64::from_be_bytes(
        bytes
            .get(*pos..end)
            .ok_or(ClipFileError::InvalidExternalDataBlock("truncated u64"))?
            .try_into()
            .unwrap(),
    );
    *pos = end;
    Ok(value)
}

fn read_be_u32(bytes: &[u8], pos: &mut usize) -> Result<u32, ClipFileError> {
    let end = pos
        .checked_add(4)
        .ok_or(ClipFileError::InvalidExternalDataBlock("offset overflow"))?;
    let value = u32::from_be_bytes(
        bytes
            .get(*pos..end)
            .ok_or(ClipFileError::InvalidExternalDataBlock("truncated u32"))?
            .try_into()
            .unwrap(),
    );
    *pos = end;
    Ok(value)
}

fn read_le_u32(bytes: &[u8], pos: &mut usize) -> Result<u32, ClipFileError> {
    let end = pos
        .checked_add(4)
        .ok_or(ClipFileError::InvalidExternalDataBlock("offset overflow"))?;
    let value = u32::from_le_bytes(
        bytes
            .get(*pos..end)
            .ok_or(ClipFileError::InvalidExternalDataBlock("truncated u32"))?
            .try_into()
            .unwrap(),
    );
    *pos = end;
    Ok(value)
}

#[cfg(test)]
#[path = "external_tests.rs"]
mod tests;
