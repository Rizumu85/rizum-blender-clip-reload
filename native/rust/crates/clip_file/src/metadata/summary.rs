use clip_model::{CanvasSize, LayerId};

use crate::{ClipFileError, ClipFileSummary};

use super::records::CanvasRecord;
use super::schema::{
    checked_canvas_extent, checked_i64_to_u32, checked_i64_to_usize, connect_sqlite,
};

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
