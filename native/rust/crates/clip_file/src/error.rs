use std::error::Error;
use std::fmt;

use clip_model::{LayerId, Rect};

use crate::container::ChunkKind;

#[derive(Debug)]
pub enum ClipFileError {
    Io(std::io::Error),
    InvalidMagic,
    TruncatedChunkHeader {
        offset: usize,
    },
    TruncatedChunkBody {
        offset: usize,
        kind: ChunkKind,
        declared_len: usize,
        available: usize,
    },
    ChunkOffsetOverflow {
        offset: usize,
    },
    ChunkTooLarge {
        offset: usize,
        kind: ChunkKind,
    },
    MissingSqliteChunk,
    InvalidExternalDataHeader,
    InvalidExternalDataBlock(&'static str),
    UnsupportedExternalDataBlock(String),
    Sqlite(rusqlite::Error),
    Zlib(std::io::Error),
    InvalidMetadata(&'static str),
    MissingLayer(LayerId),
    LayerIsNotRaster {
        layer_id: LayerId,
        layer_type: u32,
    },
    LayerHasNoMask {
        layer_id: LayerId,
    },
    LayerHasNoFilterInfo {
        layer_id: LayerId,
    },
    MalformedFilterLayerInfo {
        layer_id: LayerId,
    },
    UnsupportedLayerColorType {
        layer_id: LayerId,
        color_type: Option<u32>,
    },
    MissingExternalData(String),
    UnexpectedDecompressedSize {
        expected: usize,
        actual: usize,
    },
    InvalidTileBlobSize {
        actual: usize,
        per_tile: usize,
    },
    UnexpectedTileCount {
        expected: usize,
        actual: usize,
    },
    InvalidTileRegion {
        image_size: Rect,
        region: Rect,
    },
    TileSizeOverflow,
}

impl fmt::Display for ClipFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::InvalidMagic => f.write_str("not a CLIP file: missing CSFCHUNK magic"),
            Self::TruncatedChunkHeader { offset } => {
                write!(f, "truncated chunk header at byte offset {offset}")
            }
            Self::TruncatedChunkBody {
                offset,
                kind,
                declared_len,
                available,
            } => write!(
                f,
                "truncated {kind} chunk body at byte offset {offset}: declared {declared_len} bytes, only {available} available",
            ),
            Self::ChunkOffsetOverflow { offset } => {
                write!(f, "chunk offset overflow near byte offset {offset}")
            }
            Self::ChunkTooLarge { offset, kind } => {
                write!(
                    f,
                    "{kind} chunk at byte offset {offset} is too large for this platform"
                )
            }
            Self::MissingSqliteChunk => f.write_str("CLIP file has no CHNKSQLi chunk"),
            Self::InvalidExternalDataHeader => f.write_str("invalid CHNKExta external id header"),
            Self::InvalidExternalDataBlock(reason) => {
                write!(f, "invalid CHNKExta block: {reason}")
            }
            Self::UnsupportedExternalDataBlock(name) => {
                write!(f, "unsupported CHNKExta block {name}")
            }
            Self::Sqlite(err) => write!(f, "SQLite metadata error: {err}"),
            Self::Zlib(err) => write!(f, "zlib decode error: {err}"),
            Self::InvalidMetadata(field) => write!(f, "invalid metadata field {field}"),
            Self::MissingLayer(layer_id) => write!(f, "missing layer {}", layer_id.0),
            Self::LayerIsNotRaster {
                layer_id,
                layer_type,
            } => write!(
                f,
                "layer {} is not a raster layer, LayerType={layer_type}",
                layer_id.0,
            ),
            Self::LayerHasNoMask { layer_id } => {
                write!(f, "layer {} has no layer mask mipmap", layer_id.0)
            }
            Self::LayerHasNoFilterInfo { layer_id } => {
                write!(f, "layer {} has no filter layer info", layer_id.0)
            }
            Self::MalformedFilterLayerInfo { layer_id } => {
                write!(f, "layer {} has malformed filter layer info", layer_id.0)
            }
            Self::UnsupportedLayerColorType {
                layer_id,
                color_type,
            } => write!(
                f,
                "layer {} has unsupported color type {:?}",
                layer_id.0, color_type,
            ),
            Self::MissingExternalData(external_id) => {
                write!(f, "missing external data body {external_id}")
            }
            Self::UnexpectedDecompressedSize { expected, actual } => write!(
                f,
                "unexpected decompressed size: expected {expected} bytes, got {actual}",
            ),
            Self::InvalidTileBlobSize { actual, per_tile } => write!(
                f,
                "invalid tile blob size: {actual} bytes is not a multiple of {per_tile}",
            ),
            Self::UnexpectedTileCount { expected, actual } => {
                write!(
                    f,
                    "unexpected tile count: expected {expected}, got {actual}"
                )
            }
            Self::InvalidTileRegion { image_size, region } => write!(
                f,
                "invalid tile region {region:?} for image size {}x{}",
                image_size.width, image_size.height
            ),
            Self::TileSizeOverflow => f.write_str("tile size calculation overflow"),
        }
    }
}

impl Error for ClipFileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Sqlite(err) => Some(err),
            Self::Zlib(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ClipFileError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<rusqlite::Error> for ClipFileError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

impl From<std::string::FromUtf16Error> for ClipFileError {
    fn from(_value: std::string::FromUtf16Error) -> Self {
        Self::InvalidExternalDataBlock("invalid utf16")
    }
}
