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

#[cfg(test)]
mod tests {
    use clip_model::{CanvasSize, LayerId};
    use rusqlite::Connection;

    use super::read_raster_layer_source_from_sqlite;

    #[test]
    fn combines_layer_and_render_offscreen_offsets() {
        let conn = Connection::open_in_memory().expect("open metadata database");
        conn.execute_batch(
            "CREATE TABLE Layer (
                MainId INTEGER PRIMARY KEY,
                LayerType INTEGER NOT NULL,
                LayerVisibility INTEGER NOT NULL,
                LayerRenderMipmap INTEGER NOT NULL,
                LayerColorTypeIndex INTEGER,
                LayerOffsetX INTEGER,
                LayerOffsetY INTEGER,
                LayerRenderOffscrOffsetX INTEGER,
                LayerRenderOffscrOffsetY INTEGER
            );
            CREATE TABLE Mipmap (
                MainId INTEGER PRIMARY KEY,
                BaseMipmapInfo INTEGER NOT NULL
            );
            CREATE TABLE MipmapInfo (
                MainId INTEGER PRIMARY KEY,
                Offscreen INTEGER NOT NULL
            );
            CREATE TABLE Offscreen (
                MainId INTEGER PRIMARY KEY,
                BlockData TEXT NOT NULL,
                Attribute BLOB
            );
            CREATE TABLE LayerThumbnail (
                LayerId INTEGER PRIMARY KEY,
                ThumbnailCanvasWidth INTEGER,
                ThumbnailCanvasHeight INTEGER
            );
            INSERT INTO Layer VALUES (
                124, 1, 1, 139, 0, -64, -7, -192, -249
            );
            INSERT INTO Mipmap VALUES (139, 140);
            INSERT INTO MipmapInfo VALUES (140, 141);
            INSERT INTO Offscreen VALUES (141, 'external-raster', NULL);
            INSERT INTO LayerThumbnail VALUES (124, 4608, 4352);",
        )
        .expect("create raster metadata fixture");
        let sqlite_bytes = conn
            .serialize("main")
            .expect("serialize metadata database")
            .to_vec();

        let source = read_raster_layer_source_from_sqlite(
            &sqlite_bytes,
            LayerId(124),
            CanvasSize::new(4096, 4096),
        )
        .expect("read raster metadata");

        assert_eq!(source.offset_x, -256);
        assert_eq!(source.offset_y, -256);
        assert_eq!(source.pixel_size, CanvasSize::new(4608, 4352));
    }
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
            {}, {}, {}, {}, {}, m.BaseMipmapInfo, mi.Offscreen, \
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
        optional_i64_expr(layer_columns, "LayerOffsetX"),
        optional_i64_expr(layer_columns, "LayerOffsetY"),
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
        let render_offset_x: Option<i64> = row.get(5)?;
        let render_offset_y: Option<i64> = row.get(6)?;
        let layer_offset_x: Option<i64> = row.get(7)?;
        let layer_offset_y: Option<i64> = row.get(8)?;
        let offscreen_id: i64 = row.get(10)?;
        let external_id = string_from_value(row.get_ref(11)?)?;
        let attribute = match row.get_ref(12)? {
            ValueRef::Blob(bytes) => Some(bytes.to_vec()),
            ValueRef::Null => None,
            _ => None,
        };
        let thumbnail_width: Option<i64> = row.get(13)?;
        let thumbnail_height: Option<i64> = row.get(14)?;
        Ok((
            id,
            layer_type,
            visibility,
            render_mipmap_id,
            color_type,
            render_offset_x.unwrap_or(0),
            render_offset_y.unwrap_or(0),
            layer_offset_x.unwrap_or(0),
            layer_offset_y.unwrap_or(0),
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
        render_offset_x,
        render_offset_y,
        layer_offset_x,
        layer_offset_y,
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
        offset_x: effective_layer_offset(
            render_offset_x,
            layer_offset_x,
            "Layer.effectiveOffsetX",
        )?,
        offset_y: effective_layer_offset(
            render_offset_y,
            layer_offset_y,
            "Layer.effectiveOffsetY",
        )?,
    })
}

fn effective_layer_offset(
    render_offset: i64,
    layer_offset: i64,
    field: &'static str,
) -> Result<i32, ClipFileError> {
    let offset = render_offset
        .checked_add(layer_offset)
        .ok_or(ClipFileError::InvalidMetadata(field))?;
    checked_i64_to_i32(offset, field)
}
