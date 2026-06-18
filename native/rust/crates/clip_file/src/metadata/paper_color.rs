use std::collections::HashSet;

use clip_model::{LayerId, Rgba8};
use rusqlite::Connection;

use crate::ClipFileError;

use super::schema::has_columns;

pub(crate) fn read_paper_color(
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
