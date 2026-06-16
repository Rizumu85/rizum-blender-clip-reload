use clip_file::tiles::RgbaTileImage;
use clip_file::{ClipFileError, PlacedRgbaTileImage};
use clip_model::CanvasSize;

pub(crate) fn crop_raster_to_canvas(
    placed: PlacedRgbaTileImage,
    canvas: CanvasSize,
) -> Result<PlacedRgbaTileImage, ClipFileError> {
    let image_width =
        usize::try_from(placed.image.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let image_height =
        usize::try_from(placed.image.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let canvas_width =
        usize::try_from(canvas.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let canvas_height =
        usize::try_from(canvas.height).map_err(|_| ClipFileError::TileSizeOverflow)?;

    let src_x0 =
        usize::try_from((-placed.offset_x).max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let src_y0 =
        usize::try_from((-placed.offset_y).max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let dst_x0 =
        usize::try_from(placed.offset_x.max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let dst_y0 =
        usize::try_from(placed.offset_y.max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;

    let visible_width = image_width
        .saturating_sub(src_x0)
        .min(canvas_width.saturating_sub(dst_x0));
    let visible_height = image_height
        .saturating_sub(src_y0)
        .min(canvas_height.saturating_sub(dst_y0));

    if visible_width == 0 || visible_height == 0 {
        return Ok(placed);
    }
    if src_x0 == 0 && src_y0 == 0 && visible_width == image_width && visible_height == image_height
    {
        return Ok(placed);
    }

    let mut pixels = vec![
        0u8;
        visible_width
            .checked_mul(visible_height)
            .and_then(|pixel_count| pixel_count.checked_mul(4))
            .ok_or(ClipFileError::TileSizeOverflow)?
    ];
    for row in 0..visible_height {
        let src_start = ((src_y0 + row) * image_width + src_x0) * 4;
        let src_end = src_start + visible_width * 4;
        let dst_start = row * visible_width * 4;
        let dst_end = dst_start + visible_width * 4;
        pixels[dst_start..dst_end].copy_from_slice(&placed.image.pixels[src_start..src_end]);
    }

    Ok(PlacedRgbaTileImage {
        image: RgbaTileImage {
            width: u32::try_from(visible_width).map_err(|_| ClipFileError::TileSizeOverflow)?,
            height: u32::try_from(visible_height).map_err(|_| ClipFileError::TileSizeOverflow)?,
            pixels,
        },
        offset_x: i32::try_from(dst_x0).map_err(|_| ClipFileError::TileSizeOverflow)?,
        offset_y: i32::try_from(dst_y0).map_err(|_| ClipFileError::TileSizeOverflow)?,
    })
}

#[cfg(test)]
mod tests {
    use clip_file::PlacedRgbaTileImage;
    use clip_file::tiles::RgbaTileImage;
    use clip_model::CanvasSize;

    use super::crop_raster_to_canvas;

    #[test]
    fn keeps_fully_visible_source_unchanged() {
        let placed = placed_image(3, 2, 1, 1);
        let cropped = crop_raster_to_canvas(placed.clone(), CanvasSize::new(8, 8)).unwrap();

        assert_eq!(cropped, placed);
    }

    #[test]
    fn crops_negative_offset_to_visible_canvas_region() {
        let placed = placed_image(4, 3, -1, -1);
        let cropped = crop_raster_to_canvas(placed, CanvasSize::new(8, 8)).unwrap();

        assert_eq!(cropped.offset_x, 0);
        assert_eq!(cropped.offset_y, 0);
        assert_eq!(cropped.image.width, 3);
        assert_eq!(cropped.image.height, 2);
        assert_eq!(pixels_as_ids(&cropped.image), vec![5, 6, 7, 9, 10, 11]);
    }

    #[test]
    fn crops_positive_overflow_to_canvas_edge() {
        let placed = placed_image(4, 3, 2, 1);
        let cropped = crop_raster_to_canvas(placed, CanvasSize::new(5, 3)).unwrap();

        assert_eq!(cropped.offset_x, 2);
        assert_eq!(cropped.offset_y, 1);
        assert_eq!(cropped.image.width, 3);
        assert_eq!(cropped.image.height, 2);
        assert_eq!(pixels_as_ids(&cropped.image), vec![0, 1, 2, 4, 5, 6]);
    }

    fn placed_image(width: u32, height: u32, offset_x: i32, offset_y: i32) -> PlacedRgbaTileImage {
        let mut pixels = Vec::new();
        for value in 0..(width * height) {
            pixels.extend_from_slice(&[value as u8, 0, 0, 255]);
        }
        PlacedRgbaTileImage {
            image: RgbaTileImage {
                width,
                height,
                pixels,
            },
            offset_x,
            offset_y,
        }
    }

    fn pixels_as_ids(image: &RgbaTileImage) -> Vec<u8> {
        image.pixels.chunks_exact(4).map(|pixel| pixel[0]).collect()
    }
}
