use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::ops::Range;
use std::path::Path;

use crate::ClipFileError;

pub const CLIP_MAGIC: &[u8; 8] = b"CSFCHUNK";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChunkKind {
    Sqlite,
    ExternalData,
    Other([u8; 8]),
}

impl ChunkKind {
    fn from_bytes(bytes: [u8; 8]) -> Self {
        match &bytes {
            b"CHNKSQLi" => Self::Sqlite,
            b"CHNKExta" => Self::ExternalData,
            _ => Self::Other(bytes),
        }
    }
}

impl fmt::Display for ChunkKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite => f.write_str("CHNKSQLi"),
            Self::ExternalData => f.write_str("CHNKExta"),
            Self::Other(raw) => write!(f, "{}", String::from_utf8_lossy(raw)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChunkInfo {
    pub kind: ChunkKind,
    pub body: Range<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalDataChunk {
    pub external_id: String,
    pub body: Range<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipContainer {
    data: Vec<u8>,
    chunks: Vec<ChunkInfo>,
    sqlite_body: Range<usize>,
    external_data: Vec<ExternalDataChunk>,
    external_data_by_id: HashMap<String, Range<usize>>,
}

impl ClipContainer {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ClipFileError> {
        let data = fs::read(path)?;
        Self::from_bytes(data)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Self, ClipFileError> {
        if data.len() < 24 || &data[..8] != CLIP_MAGIC {
            return Err(ClipFileError::InvalidMagic);
        }

        let mut chunks = Vec::new();
        let mut sqlite_body = None;
        let mut external_data = Vec::new();
        let mut external_data_by_id = HashMap::new();
        let mut pos = 24usize;

        while pos < data.len() {
            let header_end = pos
                .checked_add(16)
                .ok_or(ClipFileError::ChunkOffsetOverflow { offset: pos })?;
            if header_end > data.len() {
                return Err(ClipFileError::TruncatedChunkHeader { offset: pos });
            }

            let mut raw_kind = [0u8; 8];
            raw_kind.copy_from_slice(&data[pos..pos + 8]);
            let kind = ChunkKind::from_bytes(raw_kind);
            let body_len = u64::from_be_bytes(data[pos + 8..pos + 16].try_into().unwrap())
                .try_into()
                .map_err(|_| ClipFileError::ChunkTooLarge { offset: pos, kind })?;
            let body_start = header_end;
            let body_end = body_start
                .checked_add(body_len)
                .ok_or(ClipFileError::ChunkOffsetOverflow { offset: pos })?;
            if body_end > data.len() {
                return Err(ClipFileError::TruncatedChunkBody {
                    offset: pos,
                    kind,
                    declared_len: body_len,
                    available: data.len().saturating_sub(body_start),
                });
            }

            let body = body_start..body_end;
            match kind {
                ChunkKind::Sqlite => sqlite_body = Some(body.clone()),
                ChunkKind::ExternalData => {
                    let external_id = read_external_id(&data[body.clone()])?;
                    external_data_by_id
                        .entry(external_id.clone())
                        .or_insert_with(|| body.clone());
                    external_data.push(ExternalDataChunk {
                        external_id,
                        body: body.clone(),
                    });
                }
                ChunkKind::Other(_) => {}
            }

            chunks.push(ChunkInfo { kind, body });
            pos = body_end;
        }

        let sqlite_body = sqlite_body.ok_or(ClipFileError::MissingSqliteChunk)?;
        Ok(Self {
            data,
            chunks,
            sqlite_body,
            external_data,
            external_data_by_id,
        })
    }

    pub fn chunks(&self) -> &[ChunkInfo] {
        &self.chunks
    }

    pub fn external_data(&self) -> &[ExternalDataChunk] {
        &self.external_data
    }

    pub fn external_data_body(&self, external_id: &str) -> Option<&[u8]> {
        let body = self.external_data_by_id.get(external_id)?;
        Some(&self.data[body.clone()])
    }

    pub fn sqlite_bytes(&self) -> &[u8] {
        &self.data[self.sqlite_body.clone()]
    }
}

fn read_external_id(body: &[u8]) -> Result<String, ClipFileError> {
    if body.len() < 8 {
        return Err(ClipFileError::InvalidExternalDataHeader);
    }
    let id_len: usize = u64::from_be_bytes(body[..8].try_into().unwrap())
        .try_into()
        .map_err(|_| ClipFileError::InvalidExternalDataHeader)?;
    let id_end = 8usize
        .checked_add(id_len)
        .ok_or(ClipFileError::InvalidExternalDataHeader)?;
    if id_end > body.len() {
        return Err(ClipFileError::InvalidExternalDataHeader);
    }
    let id = std::str::from_utf8(&body[8..id_end])
        .map_err(|_| ClipFileError::InvalidExternalDataHeader)?;
    Ok(id.to_owned())
}
