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

pub(crate) fn clip_decode_region_to_canvas_rect(
    region: RasterSourceDecodeRegion,
    canvas_rect: Rect,
) -> Result<Option<RasterSourceDecodeRegion>, ClipFileError> {
    if canvas_rect.width == 0 || canvas_rect.height == 0 {
        return Ok(None);
    }

    let region_x0 = i64::from(region.offset_x);
    let region_y0 = i64::from(region.offset_y);
    let region_x1 = region_x0 + i64::from(region.source_rect.width);
    let region_y1 = region_y0 + i64::from(region.source_rect.height);
    let clip_x0 = region_x0.max(i64::from(canvas_rect.x));
    let clip_y0 = region_y0.max(i64::from(canvas_rect.y));
    let clip_x1 = region_x1.min(i64::from(canvas_rect.x) + i64::from(canvas_rect.width));
    let clip_y1 = region_y1.min(i64::from(canvas_rect.y) + i64::from(canvas_rect.height));
    if clip_x1 <= clip_x0 || clip_y1 <= clip_y0 {
        return Ok(None);
    }

    let source_x = i64::from(region.source_rect.x) + (clip_x0 - region_x0);
    let source_y = i64::from(region.source_rect.y) + (clip_y0 - region_y0);
    Ok(Some(RasterSourceDecodeRegion {
        source_rect: Rect::new(
            u32::try_from(source_x).map_err(|_| ClipFileError::TileSizeOverflow)?,
            u32::try_from(source_y).map_err(|_| ClipFileError::TileSizeOverflow)?,
            u32::try_from(clip_x1 - clip_x0).map_err(|_| ClipFileError::TileSizeOverflow)?,
            u32::try_from(clip_y1 - clip_y0).map_err(|_| ClipFileError::TileSizeOverflow)?,
        ),
        offset_x: i32::try_from(clip_x0).map_err(|_| ClipFileError::TileSizeOverflow)?,
        offset_y: i32::try_from(clip_y0).map_err(|_| ClipFileError::TileSizeOverflow)?,
    }))
}

#[cfg(test)]
mod tests {
    use clip_model::{CanvasSize, Rect};

    use super::{
        RasterSourceDecodeRegion, clip_decode_region_to_canvas_rect,
        visible_raster_source_decode_region,
    };

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

    #[test]
    fn clips_decode_region_to_canvas_rect() {
        let clipped = clip_decode_region_to_canvas_rect(
            RasterSourceDecodeRegion {
                source_rect: Rect::new(10, 20, 100, 80),
                offset_x: 5,
                offset_y: 7,
            },
            Rect::new(25, 30, 40, 50),
        )
        .unwrap();

        assert_eq!(
            clipped,
            Some(RasterSourceDecodeRegion {
                source_rect: Rect::new(30, 43, 40, 50),
                offset_x: 25,
                offset_y: 30,
            })
        );
    }

    #[test]
    fn rejects_decode_region_outside_canvas_rect() {
        let clipped = clip_decode_region_to_canvas_rect(
            RasterSourceDecodeRegion {
                source_rect: Rect::new(0, 0, 10, 10),
                offset_x: 20,
                offset_y: 20,
            },
            Rect::new(0, 0, 10, 10),
        )
        .unwrap();

        assert_eq!(clipped, None);
    }
}
