use std::collections::{HashMap, HashSet};

use clip_model::{CanvasSize, LayerId, LayerKind, LayerVisibility};
use rusqlite::types::ValueRef;

use crate::ClipFileError;

use super::records::{LayerRecord, RasterLayerSource, layer_kind};
use super::schema::{
    checked_i64_to_i32, checked_i64_to_u32, connect_sqlite, optional_i64_expr,
    parse_offscreen_pixel_size, string_from_value, table_columns,
};

pub fn read_raster_layer_source_from_sqlite(
    sqlite_bytes: &[u8],
    layer_id: LayerId,
    canvas_size: CanvasSize,
) -> Result<RasterLayerSource, ClipFileError> {
    let conn = connect_sqlite(sqlite_bytes)?;
    let layer_columns = table_columns(&conn, "Layer")?;
    let query = raster_layer_source_query(&layer_columns);
    let mut stmt = conn.prepare(&query)?;
    read_layer_render_source_with_statement(&mut stmt, layer_id, canvas_size, true)
}

pub fn read_layer_render_source_from_sqlite(
    sqlite_bytes: &[u8],
    layer_id: LayerId,
    canvas_size: CanvasSize,
) -> Result<RasterLayerSource, ClipFileError> {
    let conn = connect_sqlite(sqlite_bytes)?;
    let layer_columns = table_columns(&conn, "Layer")?;
    let query = raster_layer_source_query(&layer_columns);
    let mut stmt = conn.prepare(&query)?;
    read_layer_render_source_with_statement(&mut stmt, layer_id, canvas_size, false)
}

pub fn read_raster_layer_sources_from_sqlite(
    sqlite_bytes: &[u8],
    layer_ids: &[LayerId],
    canvas_size: CanvasSize,
) -> Result<HashMap<LayerId, RasterLayerSource>, ClipFileError> {
    let conn = connect_sqlite(sqlite_bytes)?;
    let layer_columns = table_columns(&conn, "Layer")?;
    let query = raster_layer_source_query(&layer_columns);
    let mut stmt = conn.prepare(&query)?;
    let mut sources = HashMap::with_capacity(layer_ids.len());
    for layer_id in layer_ids {
        let source =
            read_layer_render_source_with_statement(&mut stmt, *layer_id, canvas_size, true)?;
        sources.insert(*layer_id, source);
    }
    Ok(sources)
}

type RasterLayerSourceRow = (
    i64,
    i64,
    i64,
    i64,
    Option<i64>,
    i64,
    i64,
    i64,
    String,
    Option<Vec<u8>>,
    Option<i64>,
    Option<i64>,
);

fn raster_layer_source_query(layer_columns: &HashSet<String>) -> String {
    format!(
        "SELECT \
            l.MainId, l.LayerType, l.LayerVisibility, l.LayerRenderMipmap, \
            {}, {}, {}, m.BaseMipmapInfo, mi.Offscreen, \
            o.BlockData, o.Attribute, lt.ThumbnailCanvasWidth, lt.ThumbnailCanvasHeight \
         FROM Layer l \
         JOIN Mipmap m ON m.MainId = l.LayerRenderMipmap \
         JOIN MipmapInfo mi ON mi.MainId = m.BaseMipmapInfo \
         JOIN Offscreen o ON o.MainId = mi.Offscreen \
         LEFT JOIN LayerThumbnail lt ON lt.LayerId = l.MainId \
         WHERE l.MainId = ?1",
        optional_i64_expr(layer_columns, "LayerColorTypeIndex"),
        optional_i64_expr(layer_columns, "LayerRenderOffscrOffsetX"),
        optional_i64_expr(layer_columns, "LayerRenderOffscrOffsetY"),
    )
}

fn read_layer_render_source_with_statement(
    stmt: &mut rusqlite::Statement<'_>,
    layer_id: LayerId,
    canvas_size: CanvasSize,
    require_raster: bool,
) -> Result<RasterLayerSource, ClipFileError> {
    let row: RasterLayerSourceRow = match stmt.query_row([layer_id.0], |row| {
        let id: i64 = row.get(0)?;
        let layer_type: i64 = row.get(1)?;
        let visibility: i64 = row.get(2)?;
        let render_mipmap_id: i64 = row.get(3)?;
        let color_type: Option<i64> = row.get(4)?;
        let offset_x: Option<i64> = row.get(5)?;
        let offset_y: Option<i64> = row.get(6)?;
        let offscreen_id: i64 = row.get(8)?;
        let external_id = string_from_value(row.get_ref(9)?)?;
        let attribute = match row.get_ref(10)? {
            ValueRef::Blob(bytes) => Some(bytes.to_vec()),
            ValueRef::Null => None,
            _ => None,
        };
        let thumbnail_width: Option<i64> = row.get(11)?;
        let thumbnail_height: Option<i64> = row.get(12)?;
        Ok((
            id,
            layer_type,
            visibility,
            render_mipmap_id,
            color_type,
            offset_x.unwrap_or(0),
            offset_y.unwrap_or(0),
            offscreen_id,
            external_id,
            attribute,
            thumbnail_width,
            thumbnail_height,
        ))
    }) {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err(ClipFileError::MissingLayer(layer_id));
        }
        Err(err) => return Err(ClipFileError::Sqlite(err)),
    };

    let (
        id,
        layer_type,
        visibility,
        render_mipmap_id,
        color_type,
        offset_x,
        offset_y,
        offscreen_id,
        external_id,
        attribute,
        thumbnail_width,
        thumbnail_height,
    ) = row;

    let layer_type = checked_i64_to_u32(layer_type, "Layer.LayerType")?;
    let kind = layer_kind(layer_type);
    if require_raster && !matches!(kind, LayerKind::Raster | LayerKind::MaskedRaster) {
        return Err(ClipFileError::LayerIsNotRaster {
            layer_id,
            layer_type,
        });
    }

    let pixel_size = attribute
        .as_deref()
        .and_then(|attribute| parse_offscreen_pixel_size(attribute, canvas_size))
        .or_else(|| match (thumbnail_width, thumbnail_height) {
            (Some(width), Some(height)) => Some(CanvasSize::new(
                checked_i64_to_u32(width, "LayerThumbnail.ThumbnailCanvasWidth").ok()?,
                checked_i64_to_u32(height, "LayerThumbnail.ThumbnailCanvasHeight").ok()?,
            )),
            _ => None,
        })
        .unwrap_or(canvas_size);

    Ok(RasterLayerSource {
        layer: LayerRecord {
            id: LayerId(checked_i64_to_u32(id, "Layer.MainId")?),
            kind,
            visibility: LayerVisibility(checked_i64_to_u32(visibility, "Layer.LayerVisibility")?),
        },
        render_mipmap_id: checked_i64_to_u32(render_mipmap_id, "Layer.LayerRenderMipmap")?,
        offscreen_id: checked_i64_to_u32(offscreen_id, "MipmapInfo.Offscreen")?,
        external_id,
        pixel_size,
        color_type: color_type
            .map(|value| checked_i64_to_u32(value, "Layer.LayerColorTypeIndex"))
            .transpose()?,
        offset_x: checked_i64_to_i32(offset_x, "Layer.LayerRenderOffscrOffsetX")?,
        offset_y: checked_i64_to_i32(offset_y, "Layer.LayerRenderOffscrOffsetY")?,
    })
}
