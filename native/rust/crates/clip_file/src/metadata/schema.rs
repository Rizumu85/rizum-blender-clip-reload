use std::collections::HashSet;

use clip_model::{CanvasSize, LayerId};
use rusqlite::Connection;
use rusqlite::types::ValueRef;

use crate::ClipFileError;

pub(crate) fn connect_sqlite(sqlite_bytes: &[u8]) -> Result<Connection, ClipFileError> {
    let mut conn = Connection::open_in_memory()?;
    conn.deserialize_read_exact("main", sqlite_bytes, sqlite_bytes.len(), true)?;
    Ok(conn)
}

pub(crate) fn table_columns(
    conn: &Connection,
    table: &str,
) -> Result<HashSet<String>, ClipFileError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| {
        let name: String = row.get(1)?;
        Ok(name)
    })?;
    let mut columns = HashSet::new();
    for row in rows {
        columns.insert(row?);
    }
    Ok(columns)
}

pub(crate) fn has_columns(columns: &HashSet<String>, names: &[&str]) -> bool {
    names.iter().all(|name| columns.contains(*name))
}

pub(crate) fn optional_i64_expr(columns: &HashSet<String>, column: &str) -> String {
    if columns.contains(column) {
        column.to_owned()
    } else {
        format!("0 AS {column}")
    }
}

pub(crate) fn optional_text_expr(columns: &HashSet<String>, column: &str) -> String {
    if columns.contains(column) {
        column.to_owned()
    } else {
        format!("'' AS {column}")
    }
}

pub(crate) fn string_from_value(value: ValueRef<'_>) -> rusqlite::Result<String> {
    match value {
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => Ok(std::str::from_utf8(bytes)
            .map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    bytes.len(),
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })?
            .to_owned()),
        other => Err(rusqlite::Error::InvalidColumnType(
            0,
            "BlockData".to_owned(),
            other.data_type(),
        )),
    }
}

pub(crate) fn parse_offscreen_pixel_size(
    attribute: &[u8],
    canvas_size: CanvasSize,
) -> Option<CanvasSize> {
    if attribute.len() < 20 {
        return None;
    }
    let payload_len = read_be_u32(attribute, 12)? as usize;
    let name_len = read_be_u32(attribute, 16)? as usize;
    let name_start = 20usize;
    let payload_start = name_start.checked_add(name_len.checked_mul(2)?)?;
    let payload_end = payload_start.checked_add(payload_len)?;
    if name_len == 0 || payload_end > attribute.len() || payload_len < 8 {
        return None;
    }
    let name = read_utf16_be(&attribute[name_start..payload_start]).ok()?;
    if name != "Parameter" {
        return None;
    }
    let width = read_be_u32(attribute, payload_start)?;
    let height = read_be_u32(attribute, payload_start + 4)?;
    if width == 0
        || height == 0
        || width > canvas_size.width.saturating_mul(4)
        || height > canvas_size.height.saturating_mul(4)
    {
        return None;
    }
    Some(CanvasSize::new(width, height))
}

pub(crate) fn parse_offscreen_init_fill(attribute: &[u8]) -> Option<u8> {
    let marker = "InitColor"
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect::<Vec<_>>();
    let marker_pos = attribute
        .windows(marker.len())
        .position(|window| window == marker.as_slice())?;
    let mut payload_len_offset = marker_pos.checked_add(marker.len())?;
    let payload_len = read_be_u32(attribute, payload_len_offset)? as usize;
    payload_len_offset = payload_len_offset.checked_add(8)?;
    if payload_len < 8 || payload_len_offset.checked_add(4)? > attribute.len() {
        return None;
    }
    Some(attribute[payload_len_offset + 3])
}

fn read_utf16_be(bytes: &[u8]) -> Result<String, std::string::FromUtf16Error> {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
        .collect();
    String::from_utf16(&units)
}

pub(crate) fn read_be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

pub(crate) fn checked_canvas_extent(value: f64, field: &'static str) -> Result<u32, ClipFileError> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > f64::from(u32::MAX) {
        return Err(ClipFileError::InvalidMetadata(field));
    }
    Ok(value as u32)
}

pub(crate) fn checked_i64_to_u32(value: i64, field: &'static str) -> Result<u32, ClipFileError> {
    value
        .try_into()
        .map_err(|_| ClipFileError::InvalidMetadata(field))
}

pub(crate) fn checked_i64_to_usize(
    value: i64,
    field: &'static str,
) -> Result<usize, ClipFileError> {
    value
        .try_into()
        .map_err(|_| ClipFileError::InvalidMetadata(field))
}

pub(crate) fn checked_i64_to_u16(value: i64, field: &'static str) -> Result<u16, ClipFileError> {
    value
        .try_into()
        .map_err(|_| ClipFileError::InvalidMetadata(field))
}

pub(crate) fn checked_i64_to_i32(value: i64, field: &'static str) -> Result<i32, ClipFileError> {
    value
        .try_into()
        .map_err(|_| ClipFileError::InvalidMetadata(field))
}

pub(crate) fn optional_layer_id(
    value: i64,
    field: &'static str,
) -> Result<Option<LayerId>, ClipFileError> {
    Ok(optional_u32(value, field)?.map(LayerId))
}

pub(crate) fn optional_u32(value: i64, field: &'static str) -> Result<Option<u32>, ClipFileError> {
    if value == 0 {
        return Ok(None);
    }
    checked_i64_to_u32(value, field).map(Some)
}
