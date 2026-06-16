use clip_file::ClipFileError;
use clip_model::{CanvasSize, Rect};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RasterSourceDecodeRegion {
    pub(crate) source_rect: Rect,
    pub(crate) offset_x: i32,
    pub(crate) offset_y: i32,
}

pub(crate) fn visible_raster_source_decode_region(
    source_size: CanvasSize,
    source_offset_x: i32,
    source_offset_y: i32,
    canvas: CanvasSize,
) -> Result<Option<RasterSourceDecodeRegion>, ClipFileError> {
    let source_width =
        usize::try_from(source_size.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let source_height =
        usize::try_from(source_size.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let canvas_width =
        usize::try_from(canvas.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let canvas_height =
        usize::try_from(canvas.height).map_err(|_| ClipFileError::TileSizeOverflow)?;

    let src_x0 =
        usize::try_from((-source_offset_x).max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let src_y0 =
        usize::try_from((-source_offset_y).max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let dst_x0 =
        usize::try_from(source_offset_x.max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let dst_y0 =
        usize::try_from(source_offset_y.max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;

    let visible_width = source_width
        .saturating_sub(src_x0)
        .min(canvas_width.saturating_sub(dst_x0));
    let visible_height = source_height
        .saturating_sub(src_y0)
        .min(canvas_height.saturating_sub(dst_y0));

    if visible_width == 0 || visible_height == 0 {
        return Ok(None);
    }

    Ok(Some(RasterSourceDecodeRegion {
        source_rect: Rect::new(
            u32::try_from(src_x0).map_err(|_| ClipFileError::TileSizeOverflow)?,
            u32::try_from(src_y0).map_err(|_| ClipFileError::TileSizeOverflow)?,
            u32::try_from(visible_width).map_err(|_| ClipFileError::TileSizeOverflow)?,
            u32::try_from(visible_height).map_err(|_| ClipFileError::TileSizeOverflow)?,
        ),
        offset_x: i32::try_from(dst_x0).map_err(|_| ClipFileError::TileSizeOverflow)?,
        offset_y: i32::try_from(dst_y0).map_err(|_| ClipFileError::TileSizeOverflow)?,
    }))
}

#[cfg(test)]
mod tests {
    use clip_model::{CanvasSize, Rect};

    use super::{RasterSourceDecodeRegion, visible_raster_source_decode_region};

    #[test]
    fn keeps_fully_visible_source_as_full_region() {
        let region =
            visible_raster_source_decode_region(CanvasSize::new(3, 2), 1, 1, CanvasSize::new(8, 8))
                .unwrap();

        assert_eq!(
            region,
            Some(RasterSourceDecodeRegion {
                source_rect: Rect::new(0, 0, 3, 2),
                offset_x: 1,
                offset_y: 1,
            })
        );
    }

    #[test]
    fn clips_negative_offset_to_visible_source_region() {
        let region = visible_raster_source_decode_region(
            CanvasSize::new(4, 3),
            -1,
            -1,
            CanvasSize::new(8, 8),
        )
        .unwrap();

        assert_eq!(
            region,
            Some(RasterSourceDecodeRegion {
                source_rect: Rect::new(1, 1, 3, 2),
                offset_x: 0,
                offset_y: 0,
            })
        );
    }

    #[test]
    fn clips_positive_overflow_to_canvas_edge() {
        let region =
            visible_raster_source_decode_region(CanvasSize::new(4, 3), 2, 1, CanvasSize::new(5, 3))
                .unwrap();

        assert_eq!(
            region,
            Some(RasterSourceDecodeRegion {
                source_rect: Rect::new(0, 0, 3, 2),
                offset_x: 2,
                offset_y: 1,
            })
        );
    }

    #[test]
    fn returns_none_for_fully_off_canvas_source() {
        let region = visible_raster_source_decode_region(
            CanvasSize::new(2, 2),
            10,
            0,
            CanvasSize::new(5, 5),
        )
        .unwrap();

        assert_eq!(region, None);
    }
}
