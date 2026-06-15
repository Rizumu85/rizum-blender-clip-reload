use std::io::Read;

use clip_model::Rect;
use flate2::read::ZlibDecoder;

use crate::ClipFileError;

pub const TILE_SIZE: usize = 256;
pub const MASK_TILE_BYTES: usize = TILE_SIZE * TILE_SIZE;
pub const RGBA_TILE_BYTES: usize = MASK_TILE_BYTES * 5;
pub const GRAY_RGBA_TILE_BYTES: usize = MASK_TILE_BYTES * 2;
pub const MONO_RGBA_TILE_BYTES: usize = MASK_TILE_BYTES / 4;
const BLOCK_NAME_MARKER: u32 = 0x0042_006c;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TileAddress {
    pub layer_rect: Rect,
    pub mip_level: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalTileBlob {
    pub external_id: String,
    pub bytes: Vec<u8>,
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

pub fn decode_external_tile_blob(
    body: &[u8],
    empty_fill: u8,
    expected_len: Option<usize>,
) -> Result<ExternalTileBlob, ClipFileError> {
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

    let mut output = match expected_len {
        Some(expected_len) => vec![empty_fill; expected_len],
        None => Vec::new(),
    };
    let mut output_len = 0usize;

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

        match name.as_str() {
            "BlockDataBeginChunk" => {
                skip(body, &mut pos, 4)?;
                let uncompressed_len = read_be_u32(body, &mut pos)? as usize;
                skip(body, &mut pos, 8)?;
                let exists = read_be_u32(body, &mut pos)?;

                if exists > 0 {
                    let inner_len = read_be_u32(body, &mut pos)? as usize;
                    let compressed_len = read_le_u32(body, &mut pos)? as usize;
                    let compressed_end = pos.checked_add(compressed_len).ok_or(
                        ClipFileError::InvalidExternalDataBlock("compressed block overflow"),
                    )?;
                    let compressed = body.get(pos..compressed_end).ok_or(
                        ClipFileError::InvalidExternalDataBlock("truncated compressed block"),
                    )?;
                    let decoded = inflate_zlib(compressed)?;
                    if decoded.len() != uncompressed_len {
                        return Err(ClipFileError::UnexpectedDecompressedSize {
                            expected: uncompressed_len,
                            actual: decoded.len(),
                        });
                    }
                    append_decoded_bytes(&mut output, &mut output_len, &decoded, expected_len)?;
                    block_end = inner_start
                        .checked_add(24)
                        .and_then(|offset| offset.checked_add(inner_len))
                        .ok_or(ClipFileError::InvalidExternalDataBlock(
                            "data block end overflow",
                        ))?;
                } else {
                    append_empty_bytes(
                        &mut output,
                        &mut output_len,
                        empty_fill,
                        uncompressed_len,
                        expected_len,
                    )?;
                    block_end = inner_start.checked_add(20).ok_or(
                        ClipFileError::InvalidExternalDataBlock("empty block end overflow"),
                    )?;
                }
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

        if let Some(expected_len) = expected_len
            && output_len >= expected_len
        {
            break;
        }
    }

    if expected_len.is_some() {
        output.truncate(output_len);
    }

    Ok(ExternalTileBlob {
        external_id,
        bytes: output,
    })
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

fn append_decoded_bytes(
    output: &mut Vec<u8>,
    output_len: &mut usize,
    decoded: &[u8],
    expected_len: Option<usize>,
) -> Result<(), ClipFileError> {
    if let Some(expected_len) = expected_len {
        let remaining = expected_len.saturating_sub(*output_len);
        let write_len = decoded.len().min(remaining);
        output[*output_len..*output_len + write_len].copy_from_slice(&decoded[..write_len]);
        *output_len += write_len;
    } else {
        output.extend_from_slice(decoded);
        *output_len = output.len();
    }
    Ok(())
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

fn inflate_zlib(compressed: &[u8]) -> Result<Vec<u8>, ClipFileError> {
    let mut decoder = ZlibDecoder::new(compressed);
    let mut decoded = Vec::new();
    decoder
        .read_to_end(&mut decoded)
        .map_err(ClipFileError::Zlib)?;
    Ok(decoded)
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

fn div_ceil_u32(value: u32, divisor: u32) -> Result<u32, ClipFileError> {
    value
        .checked_add(divisor - 1)
        .map(|value| value / divisor)
        .ok_or(ClipFileError::TileSizeOverflow)
}
