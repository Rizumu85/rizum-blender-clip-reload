use clip_model::{LayerId, LayerKind, LayerOpacity, LayerVisibility};

use crate::ClipFileError;

use super::paper_color::read_paper_color;
use super::records::{LayerGraphRecord, layer_kind};
use super::schema::{
    checked_i64_to_u16, checked_i64_to_u32, connect_sqlite, has_columns, optional_i64_expr,
    optional_layer_id, optional_text_expr, optional_u32, table_columns,
};

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
    let text_present_expr = if layer_columns.contains("TextLayerString")
        || layer_columns.contains("TextLayerStringArray")
        || layer_columns.contains("TextLayerAttributes")
        || layer_columns.contains("TextLayerAttributesArray")
    {
        format!(
            "CASE WHEN {} IS NOT NULL OR {} IS NOT NULL OR {} IS NOT NULL OR {} IS NOT NULL THEN 1 ELSE 0 END AS TextLayerPresent",
            optional_text_presence_column(&layer_columns, "TextLayerString"),
            optional_text_presence_column(&layer_columns, "TextLayerStringArray"),
            optional_text_presence_column(&layer_columns, "TextLayerAttributes"),
            optional_text_presence_column(&layer_columns, "TextLayerAttributesArray"),
        )
    } else {
        "0 AS TextLayerPresent".to_owned()
    };
    let query = format!(
        "SELECT \
            MainId, {}, LayerType, LayerVisibility, LayerClip, LayerOpacity, \
            LayerComposite, LayerNextIndex, LayerFirstChildIndex, \
            LayerRenderMipmap, LayerLayerMaskMipmap, \
            {}, {}, {}, {}, {}, {}, {}, {} \
         FROM Layer \
         ORDER BY MainId",
        optional_text_expr(&layer_columns, "LayerName"),
        optional_i64_expr(&layer_columns, "DrawColorEnable"),
        optional_i64_expr(&layer_columns, "DrawColorMainRed"),
        optional_i64_expr(&layer_columns, "DrawColorMainGreen"),
        optional_i64_expr(&layer_columns, "DrawColorMainBlue"),
        optional_i64_expr(&layer_columns, "LayerPaletteRed"),
        optional_i64_expr(&layer_columns, "LayerPaletteGreen"),
        optional_i64_expr(&layer_columns, "LayerPaletteBlue"),
        text_present_expr,
    );
    let mut stmt = conn.prepare(&query)?;

    let rows = stmt.query_map([], |row| {
        let id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        let layer_type: i64 = row.get(2)?;
        let visibility: i64 = row.get(3)?;
        let clip: i64 = row.get(4)?;
        let opacity: i64 = row.get(5)?;
        let composite: i64 = row.get(6)?;
        let next_layer_id: i64 = row.get(7)?;
        let first_child_layer_id: i64 = row.get(8)?;
        let render_mipmap_id: i64 = row.get(9)?;
        let mask_mipmap_id: i64 = row.get(10)?;
        let draw_color_enable: i64 = row.get::<_, Option<i64>>(11)?.unwrap_or(0);
        let draw_color_red: i64 = row.get::<_, Option<i64>>(12)?.unwrap_or(0);
        let draw_color_green: i64 = row.get::<_, Option<i64>>(13)?.unwrap_or(0);
        let draw_color_blue: i64 = row.get::<_, Option<i64>>(14)?.unwrap_or(0);
        let palette_red: i64 = row.get::<_, Option<i64>>(15)?.unwrap_or(0);
        let palette_green: i64 = row.get::<_, Option<i64>>(16)?.unwrap_or(0);
        let palette_blue: i64 = row.get::<_, Option<i64>>(17)?.unwrap_or(0);
        let text_present: i64 = row.get(18)?;
        Ok((
            id,
            name,
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
            text_present,
        ))
    })?;

    let mut records = Vec::new();
    for row in rows {
        let (
            id,
            name,
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
            text_present,
        ) = row?;
        let layer_type = checked_i64_to_u32(layer_type, "Layer.LayerType")?;
        let id = LayerId(checked_i64_to_u32(id, "Layer.MainId")?);
        let kind = if text_present != 0 {
            LayerKind::Text
        } else {
            layer_kind(layer_type)
        };
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
            name,
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

fn optional_text_presence_column(
    columns: &std::collections::HashSet<String>,
    column: &str,
) -> String {
    if columns.contains(column) {
        column.to_owned()
    } else {
        "NULL".to_owned()
    }
}
