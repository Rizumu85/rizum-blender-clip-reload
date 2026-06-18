use std::collections::HashMap;

use clip_model::LayerId;
use rusqlite::types::ValueRef;

use crate::ClipFileError;

use super::records::FilterLayerSource;
use super::schema::{checked_i64_to_u32, connect_sqlite, read_be_u32, table_columns};

pub fn read_filter_layer_source_from_sqlite(
    sqlite_bytes: &[u8],
    layer_id: LayerId,
) -> Result<FilterLayerSource, ClipFileError> {
    let conn = connect_sqlite(sqlite_bytes)?;
    let mut stmt = conn.prepare(filter_layer_source_query())?;
    read_filter_layer_source_with_statement(&mut stmt, layer_id)
}

pub fn read_filter_layer_sources_from_sqlite(
    sqlite_bytes: &[u8],
    layer_ids: &[LayerId],
) -> Result<HashMap<LayerId, FilterLayerSource>, ClipFileError> {
    let conn = connect_sqlite(sqlite_bytes)?;
    let layer_columns = table_columns(&conn, "Layer")?;
    if !layer_columns.contains("FilterLayerInfo") {
        return Ok(HashMap::new());
    }
    let mut stmt = conn.prepare(filter_layer_source_query())?;
    let mut sources = HashMap::with_capacity(layer_ids.len());
    for layer_id in layer_ids {
        let source = read_filter_layer_source_with_statement(&mut stmt, *layer_id)?;
        sources.insert(*layer_id, source);
    }
    Ok(sources)
}

type FilterLayerSourceRow = (i64, Option<Vec<u8>>);

fn filter_layer_source_query() -> &'static str {
    "SELECT MainId, FilterLayerInfo FROM Layer WHERE MainId = ?1"
}

fn read_filter_layer_source_with_statement(
    stmt: &mut rusqlite::Statement<'_>,
    layer_id: LayerId,
) -> Result<FilterLayerSource, ClipFileError> {
    let row: FilterLayerSourceRow = match stmt.query_row([layer_id.0], |row| {
        let id: i64 = row.get(0)?;
        let info = match row.get_ref(1)? {
            ValueRef::Blob(bytes) => Some(bytes.to_vec()),
            ValueRef::Null => None,
            _ => None,
        };
        Ok((id, info))
    }) {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err(ClipFileError::MissingLayer(layer_id));
        }
        Err(err) => return Err(ClipFileError::Sqlite(err)),
    };
    let (id, info) = row;
    let info = info.ok_or(ClipFileError::LayerHasNoFilterInfo { layer_id })?;
    if info.len() < 8 {
        return Err(ClipFileError::MalformedFilterLayerInfo { layer_id });
    }
    let filter_type =
        read_be_u32(&info, 0).ok_or(ClipFileError::MalformedFilterLayerInfo { layer_id })?;
    let payload_len =
        read_be_u32(&info, 4).ok_or(ClipFileError::MalformedFilterLayerInfo { layer_id })? as usize;
    let payload_end = 8usize
        .checked_add(payload_len)
        .ok_or(ClipFileError::MalformedFilterLayerInfo { layer_id })?;
    if payload_end > info.len() {
        return Err(ClipFileError::MalformedFilterLayerInfo { layer_id });
    }
    Ok(FilterLayerSource {
        layer_id: LayerId(checked_i64_to_u32(id, "Layer.MainId")?),
        filter_type,
        payload: info[8..payload_end].to_vec(),
    })
}
