use std::collections::{HashMap, HashSet};

use clip_model::{CanvasSize, LayerId, LayerKind, LayerOpacity, LayerVisibility, Rgba8};
use rusqlite::Connection;
use rusqlite::types::ValueRef;

use crate::{ClipFileError, ClipFileSummary};

const LAYER_TYPE_RASTER: u32 = 1;
const LAYER_TYPE_RASTER_MASKED: u32 = 3;
const LAYER_TYPE_LAYER_FOLDER: u32 = 0;
const LAYER_TYPE_GROUP: u32 = 2;
const LAYER_TYPE_FOLDER: u32 = 256;
const LAYER_TYPE_PAPER: u32 = 1584;
const LAYER_TYPE_FILTER: u32 = 4098;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayerRecord {
    pub id: LayerId,
    pub kind: LayerKind,
    pub visibility: LayerVisibility,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanvasRecord {
    pub id: u32,
    pub size: CanvasSize,
    pub root_layer_id: LayerId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RasterLayerSource {
    pub layer: LayerRecord,
    pub render_mipmap_id: u32,
    pub offscreen_id: u32,
    pub external_id: String,
    pub pixel_size: CanvasSize,
    pub color_type: Option<u32>,
    pub offset_x: i32,
    pub offset_y: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaskLayerSource {
    pub layer_id: LayerId,
    pub mask_mipmap_id: u32,
    pub offscreen_id: u32,
    pub external_id: String,
    pub pixel_size: CanvasSize,
    pub empty_fill: u8,
    pub offset_x: i32,
    pub offset_y: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterLayerSource {
    pub layer_id: LayerId,
    pub filter_type: u32,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayerGraphRecord {
    pub id: LayerId,
    pub kind: LayerKind,
    pub visibility: LayerVisibility,
    pub clip: bool,
    pub opacity: LayerOpacity,
    pub composite: u32,
    pub next_layer_id: Option<LayerId>,
    pub first_child_layer_id: Option<LayerId>,
    pub render_mipmap_id: Option<u32>,
    pub mask_mipmap_id: Option<u32>,
    pub paper_color: Option<Rgba8>,
}

pub fn read_filter_layer_source_from_sqlite(
    sqlite_bytes: &[u8],
    layer_id: LayerId,
) -> Result<FilterLayerSource, ClipFileError> {
    let conn = connect_sqlite(sqlite_bytes)?;
    let row = conn.query_row(
        "SELECT MainId, FilterLayerInfo FROM Layer WHERE MainId = ?1",
        [layer_id.0],
        |row| {
            let id: i64 = row.get(0)?;
            let info = match row.get_ref(1)? {
                ValueRef::Blob(bytes) => Some(bytes.to_vec()),
                ValueRef::Null => None,
                _ => None,
            };
            Ok((id, info))
        },
    );

    let (id, info) = match row {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err(ClipFileError::MissingLayer(layer_id));
        }
        Err(err) => return Err(ClipFileError::Sqlite(err)),
    };
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

pub fn read_summary_from_sqlite(
    sqlite_bytes: &[u8],
    external_data_count: usize,
) -> Result<ClipFileSummary, ClipFileError> {
    let conn = connect_sqlite(sqlite_bytes)?;

    let (id, width, height, root_layer_id): (i64, f64, f64, i64) = conn.query_row(
        "SELECT MainId, CanvasWidth, CanvasHeight, CanvasRootFolder FROM Canvas LIMIT 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    let canvas = CanvasRecord {
        id: checked_i64_to_u32(id, "Canvas.MainId")?,
        size: CanvasSize::new(
            checked_canvas_extent(width, "Canvas.CanvasWidth")?,
            checked_canvas_extent(height, "Canvas.CanvasHeight")?,
        ),
        root_layer_id: LayerId(checked_i64_to_u32(
            root_layer_id,
            "Canvas.CanvasRootFolder",
        )?),
    };

    let layer_count: i64 = conn.query_row("SELECT COUNT(*) FROM Layer", [], |row| row.get(0))?;
    Ok(ClipFileSummary {
        canvas: canvas.size,
        root_layer_id: canvas.root_layer_id,
        layer_count: checked_i64_to_usize(layer_count, "Layer count")?,
        external_data_count,
    })
}

pub fn read_raster_layer_source_from_sqlite(
    sqlite_bytes: &[u8],
    layer_id: LayerId,
    canvas_size: CanvasSize,
) -> Result<RasterLayerSource, ClipFileError> {
    let conn = connect_sqlite(sqlite_bytes)?;
    let layer_columns = table_columns(&conn, "Layer")?;
    let query = raster_layer_source_query(&layer_columns);
    let mut stmt = conn.prepare(&query)?;
    read_raster_layer_source_with_statement(&mut stmt, layer_id, canvas_size)
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
        let source = read_raster_layer_source_with_statement(&mut stmt, *layer_id, canvas_size)?;
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

fn read_raster_layer_source_with_statement(
    stmt: &mut rusqlite::Statement<'_>,
    layer_id: LayerId,
    canvas_size: CanvasSize,
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
    if !matches!(kind, LayerKind::Raster | LayerKind::MaskedRaster) {
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

pub fn read_layer_graph_records_from_sqlite(
    sqlite_bytes: &[u8],
) -> Result<Vec<LayerGraphRecord>, ClipFileError> {
    let conn = connect_sqlite(sqlite_bytes)?;
    let layer_columns = table_columns(&conn, "Layer")?;
    let thumbnail_columns = table_columns(&conn, "LayerThumbnail")?;
    let has_draw_color = has_columns(
        &layer_columns,
        &[
            "DrawColorEnable",
            "DrawColorMainRed",
            "DrawColorMainGreen",
            "DrawColorMainBlue",
        ],
    );
    let query = format!(
        "SELECT \
            MainId, LayerType, LayerVisibility, LayerClip, LayerOpacity, \
            LayerComposite, LayerNextIndex, LayerFirstChildIndex, \
            LayerRenderMipmap, LayerLayerMaskMipmap, \
            {}, {}, {}, {}, {}, {}, {} \
         FROM Layer \
         ORDER BY MainId",
        optional_i64_expr(&layer_columns, "DrawColorEnable"),
        optional_i64_expr(&layer_columns, "DrawColorMainRed"),
        optional_i64_expr(&layer_columns, "DrawColorMainGreen"),
        optional_i64_expr(&layer_columns, "DrawColorMainBlue"),
        optional_i64_expr(&layer_columns, "LayerPaletteRed"),
        optional_i64_expr(&layer_columns, "LayerPaletteGreen"),
        optional_i64_expr(&layer_columns, "LayerPaletteBlue"),
    );
    let mut stmt = conn.prepare(&query)?;

    let rows = stmt.query_map([], |row| {
        let id: i64 = row.get(0)?;
        let layer_type: i64 = row.get(1)?;
        let visibility: i64 = row.get(2)?;
        let clip: i64 = row.get(3)?;
        let opacity: i64 = row.get(4)?;
        let composite: i64 = row.get(5)?;
        let next_layer_id: i64 = row.get(6)?;
        let first_child_layer_id: i64 = row.get(7)?;
        let render_mipmap_id: i64 = row.get(8)?;
        let mask_mipmap_id: i64 = row.get(9)?;
        let draw_color_enable: i64 = row.get::<_, Option<i64>>(10)?.unwrap_or(0);
        let draw_color_red: i64 = row.get::<_, Option<i64>>(11)?.unwrap_or(0);
        let draw_color_green: i64 = row.get::<_, Option<i64>>(12)?.unwrap_or(0);
        let draw_color_blue: i64 = row.get::<_, Option<i64>>(13)?.unwrap_or(0);
        let palette_red: i64 = row.get::<_, Option<i64>>(14)?.unwrap_or(0);
        let palette_green: i64 = row.get::<_, Option<i64>>(15)?.unwrap_or(0);
        let palette_blue: i64 = row.get::<_, Option<i64>>(16)?.unwrap_or(0);
        Ok((
            id,
            layer_type,
            visibility,
            clip,
            opacity,
            composite,
            next_layer_id,
            first_child_layer_id,
            render_mipmap_id,
            mask_mipmap_id,
            draw_color_enable,
            draw_color_red,
            draw_color_green,
            draw_color_blue,
            palette_red,
            palette_green,
            palette_blue,
        ))
    })?;

    let mut records = Vec::new();
    for row in rows {
        let (
            id,
            layer_type,
            visibility,
            clip,
            opacity,
            composite,
            next_layer_id,
            first_child_layer_id,
            render_mipmap_id,
            mask_mipmap_id,
            draw_color_enable,
            draw_color_red,
            draw_color_green,
            draw_color_blue,
            palette_red,
            palette_green,
            palette_blue,
        ) = row?;
        let layer_type = checked_i64_to_u32(layer_type, "Layer.LayerType")?;
        let id = LayerId(checked_i64_to_u32(id, "Layer.MainId")?);
        let kind = layer_kind(layer_type);
        let paper_color = if kind == LayerKind::Paper {
            read_paper_color(
                &conn,
                &thumbnail_columns,
                id,
                has_draw_color,
                draw_color_enable,
                [draw_color_red, draw_color_green, draw_color_blue],
                [palette_red, palette_green, palette_blue],
            )?
        } else {
            None
        };
        records.push(LayerGraphRecord {
            id,
            kind,
            visibility: LayerVisibility(checked_i64_to_u32(visibility, "Layer.LayerVisibility")?),
            clip: clip != 0,
            opacity: LayerOpacity(checked_i64_to_u16(opacity, "Layer.LayerOpacity")?),
            composite: checked_i64_to_u32(composite, "Layer.LayerComposite")?,
            next_layer_id: optional_layer_id(next_layer_id, "Layer.LayerNextIndex")?,
            first_child_layer_id: optional_layer_id(
                first_child_layer_id,
                "Layer.LayerFirstChildIndex",
            )?,
            render_mipmap_id: optional_u32(render_mipmap_id, "Layer.LayerRenderMipmap")?,
            mask_mipmap_id: optional_u32(mask_mipmap_id, "Layer.LayerLayerMaskMipmap")?,
            paper_color,
        });
    }
    Ok(records)
}

fn connect_sqlite(sqlite_bytes: &[u8]) -> Result<Connection, ClipFileError> {
    let mut conn = Connection::open_in_memory()?;
    conn.deserialize_read_exact("main", sqlite_bytes, sqlite_bytes.len(), true)?;
    Ok(conn)
}

fn table_columns(conn: &Connection, table: &str) -> Result<HashSet<String>, ClipFileError> {
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

fn has_columns(columns: &HashSet<String>, names: &[&str]) -> bool {
    names.iter().all(|name| columns.contains(*name))
}

fn optional_i64_expr(columns: &HashSet<String>, column: &str) -> String {
    if columns.contains(column) {
        column.to_owned()
    } else {
        format!("0 AS {column}")
    }
}

fn read_paper_color(
    conn: &Connection,
    thumbnail_columns: &HashSet<String>,
    layer_id: LayerId,
    has_draw_color: bool,
    draw_color_enable: i64,
    draw_rgb: [i64; 3],
    palette_rgb: [i64; 3],
) -> Result<Option<Rgba8>, ClipFileError> {
    let draw_rgb = color_rgb(draw_rgb);
    if has_draw_color && (draw_rgb != [0, 0, 0] || draw_color_enable != 0) {
        return Ok(Some(opaque_rgba(draw_rgb)));
    }

    if has_columns(
        thumbnail_columns,
        &[
            "LayerId",
            "ThumbnailMainColorRed",
            "ThumbnailMainColorGreen",
            "ThumbnailMainColorBlue",
        ],
    ) {
        let thumbnail = conn.query_row(
            "SELECT ThumbnailMainColorRed, ThumbnailMainColorGreen, ThumbnailMainColorBlue \
             FROM LayerThumbnail \
             WHERE LayerId = ?1",
            [layer_id.0],
            |row| {
                Ok([
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ])
            },
        );
        match thumbnail {
            Ok(rgb) => {
                let rgb = color_rgb(rgb);
                if rgb != [0, 0, 0] {
                    return Ok(Some(opaque_rgba(rgb)));
                }
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(err) => return Err(ClipFileError::Sqlite(err)),
        }
    }

    Ok(Some(opaque_rgba(color_rgb(palette_rgb))))
}

fn color_rgb(values: [i64; 3]) -> [u8; 3] {
    values.map(clip_color_component)
}

fn clip_color_component(value: i64) -> u8 {
    let value = if value < 0 {
        value as i32 as u32
    } else {
        u32::try_from(value).unwrap_or(u32::MAX)
    };
    let value = if value > 255 {
        (value >> 24) & 0xff
    } else {
        value
    };
    value as u8
}

fn opaque_rgba(rgb: [u8; 3]) -> Rgba8 {
    Rgba8 {
        r: rgb[0],
        g: rgb[1],
        b: rgb[2],
        a: 255,
    }
}

fn layer_kind(layer_type: u32) -> LayerKind {
    match layer_type {
        LAYER_TYPE_RASTER => LayerKind::Raster,
        LAYER_TYPE_RASTER_MASKED => LayerKind::MaskedRaster,
        LAYER_TYPE_LAYER_FOLDER => LayerKind::Folder,
        LAYER_TYPE_GROUP => LayerKind::Group,
        LAYER_TYPE_FOLDER => LayerKind::Folder,
        LAYER_TYPE_PAPER => LayerKind::Paper,
        LAYER_TYPE_FILTER => LayerKind::Filter,
        other => LayerKind::Unsupported(other),
    }
}

fn string_from_value(value: ValueRef<'_>) -> rusqlite::Result<String> {
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

fn parse_offscreen_pixel_size(attribute: &[u8], canvas_size: CanvasSize) -> Option<CanvasSize> {
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

fn parse_offscreen_init_fill(attribute: &[u8]) -> Option<u8> {
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

fn read_be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn checked_canvas_extent(value: f64, field: &'static str) -> Result<u32, ClipFileError> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > f64::from(u32::MAX) {
        return Err(ClipFileError::InvalidMetadata(field));
    }
    Ok(value as u32)
}

fn checked_i64_to_u32(value: i64, field: &'static str) -> Result<u32, ClipFileError> {
    value
        .try_into()
        .map_err(|_| ClipFileError::InvalidMetadata(field))
}

fn checked_i64_to_usize(value: i64, field: &'static str) -> Result<usize, ClipFileError> {
    value
        .try_into()
        .map_err(|_| ClipFileError::InvalidMetadata(field))
}

fn checked_i64_to_u16(value: i64, field: &'static str) -> Result<u16, ClipFileError> {
    value
        .try_into()
        .map_err(|_| ClipFileError::InvalidMetadata(field))
}

fn checked_i64_to_i32(value: i64, field: &'static str) -> Result<i32, ClipFileError> {
    value
        .try_into()
        .map_err(|_| ClipFileError::InvalidMetadata(field))
}

fn optional_layer_id(value: i64, field: &'static str) -> Result<Option<LayerId>, ClipFileError> {
    Ok(optional_u32(value, field)?.map(LayerId))
}

fn optional_u32(value: i64, field: &'static str) -> Result<Option<u32>, ClipFileError> {
    if value == 0 {
        return Ok(None);
    }
    checked_i64_to_u32(value, field).map(Some)
}
