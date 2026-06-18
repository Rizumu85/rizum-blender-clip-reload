use std::collections::{HashMap, HashSet};

use clip_model::{CanvasSize, LayerId};
use rusqlite::types::ValueRef;

use crate::ClipFileError;

use super::records::MaskLayerSource;
use super::schema::{
    checked_i64_to_i32, checked_i64_to_u32, connect_sqlite, optional_i64_expr,
    parse_offscreen_init_fill, parse_offscreen_pixel_size, string_from_value, table_columns,
};

pub fn read_mask_layer_source_from_sqlite(
    sqlite_bytes: &[u8],
    layer_id: LayerId,
    canvas_size: CanvasSize,
) -> Result<MaskLayerSource, ClipFileError> {
    let conn = connect_sqlite(sqlite_bytes)?;
    let layer_columns = table_columns(&conn, "Layer")?;
    let query = mask_layer_source_query(&layer_columns);
    let mut stmt = conn.prepare(&query)?;
    read_mask_layer_source_with_statement(&mut stmt, layer_id, canvas_size)
}

pub fn read_mask_layer_sources_from_sqlite(
    sqlite_bytes: &[u8],
    layer_ids: &[LayerId],
    canvas_size: CanvasSize,
) -> Result<HashMap<LayerId, MaskLayerSource>, ClipFileError> {
    let conn = connect_sqlite(sqlite_bytes)?;
    let layer_columns = table_columns(&conn, "Layer")?;
    let query = mask_layer_source_query(&layer_columns);
    let mut stmt = conn.prepare(&query)?;
    let mut sources = HashMap::with_capacity(layer_ids.len());
    for layer_id in layer_ids {
        let source = read_mask_layer_source_with_statement(&mut stmt, *layer_id, canvas_size)?;
        sources.insert(*layer_id, source);
    }
    Ok(sources)
}

type MaskLayerSourceRow = (i64, i64, i64, i64, i64, String, Option<Vec<u8>>);

fn mask_layer_source_query(layer_columns: &HashSet<String>) -> String {
    format!(
        "SELECT \
            l.MainId, l.LayerLayerMaskMipmap, {}, {}, \
            m.BaseMipmapInfo, mi.Offscreen, o.BlockData, o.Attribute \
         FROM Layer l \
         JOIN Mipmap m ON m.MainId = l.LayerLayerMaskMipmap \
         JOIN MipmapInfo mi ON mi.MainId = m.BaseMipmapInfo \
         JOIN Offscreen o ON o.MainId = mi.Offscreen \
         WHERE l.MainId = ?1 AND l.LayerLayerMaskMipmap != 0",
        optional_i64_expr(layer_columns, "LayerMaskOffscrOffsetX"),
        optional_i64_expr(layer_columns, "LayerMaskOffscrOffsetY"),
    )
}

fn read_mask_layer_source_with_statement(
    stmt: &mut rusqlite::Statement<'_>,
    layer_id: LayerId,
    canvas_size: CanvasSize,
) -> Result<MaskLayerSource, ClipFileError> {
    let row: MaskLayerSourceRow = match stmt.query_row([layer_id.0], |row| {
        let id: i64 = row.get(0)?;
        let mask_mipmap_id: i64 = row.get(1)?;
        let offset_x: Option<i64> = row.get(2)?;
        let offset_y: Option<i64> = row.get(3)?;
        let offscreen_id: i64 = row.get(5)?;
        let external_id = string_from_value(row.get_ref(6)?)?;
        let attribute = match row.get_ref(7)? {
            ValueRef::Blob(bytes) => Some(bytes.to_vec()),
            ValueRef::Null => None,
            _ => None,
        };
        Ok((
            id,
            mask_mipmap_id,
            offset_x.unwrap_or(0),
            offset_y.unwrap_or(0),
            offscreen_id,
            external_id,
            attribute,
        ))
    }) {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err(ClipFileError::LayerHasNoMask { layer_id });
        }
        Err(err) => return Err(ClipFileError::Sqlite(err)),
    };

    let (id, mask_mipmap_id, offset_x, offset_y, offscreen_id, external_id, attribute) = row;
    let pixel_size = attribute
        .as_deref()
        .and_then(|attribute| parse_offscreen_pixel_size(attribute, canvas_size))
        .unwrap_or(canvas_size);
    let empty_fill = attribute
        .as_deref()
        .and_then(parse_offscreen_init_fill)
        .unwrap_or(0);

    Ok(MaskLayerSource {
        layer_id: LayerId(checked_i64_to_u32(id, "Layer.MainId")?),
        mask_mipmap_id: checked_i64_to_u32(mask_mipmap_id, "Layer.LayerLayerMaskMipmap")?,
        offscreen_id: checked_i64_to_u32(offscreen_id, "MipmapInfo.Offscreen")?,
        external_id,
        pixel_size,
        empty_fill,
        offset_x: checked_i64_to_i32(offset_x, "Layer.LayerMaskOffscrOffsetX")?,
        offset_y: checked_i64_to_i32(offset_y, "Layer.LayerMaskOffscrOffsetY")?,
    })
}
