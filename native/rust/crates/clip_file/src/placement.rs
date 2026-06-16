use clip_model::CanvasSize;

use crate::ClipFileError;
use crate::tiles::{AlphaTileImage, RgbaTileImage};

pub(crate) fn place_alpha_on_canvas(
    alpha: AlphaTileImage,
    canvas_size: CanvasSize,
    offset_x: i32,
    offset_y: i32,
    empty_fill: u8,
) -> Result<AlphaTileImage, ClipFileError> {
    if alpha.width == canvas_size.width
        && alpha.height == canvas_size.height
        && offset_x == 0
        && offset_y == 0
    {
        return Ok(alpha);
    }

    if offset_x == 0
        && offset_y == 0
        && alpha.width >= canvas_size.width
        && alpha.height >= canvas_size.height
    {
        return crop_alpha_top_left(alpha, canvas_size);
    }

    let canvas_width =
        usize::try_from(canvas_size.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let canvas_height =
        usize::try_from(canvas_size.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let alpha_width = usize::try_from(alpha.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let alpha_height =
        usize::try_from(alpha.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let mut pixels = vec![
        empty_fill;
        canvas_width
            .checked_mul(canvas_height)
            .ok_or(ClipFileError::TileSizeOverflow)?
    ];

    let src_x0 =
        usize::try_from((-offset_x).max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let src_y0 =
        usize::try_from((-offset_y).max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let dst_x0 = usize::try_from(offset_x.max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let dst_y0 = usize::try_from(offset_y.max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let paste_w = alpha_width
        .saturating_sub(src_x0)
        .min(canvas_width.saturating_sub(dst_x0));
    let paste_h = alpha_height
        .saturating_sub(src_y0)
        .min(canvas_height.saturating_sub(dst_y0));

    for row in 0..paste_h {
        let src_start = (src_y0 + row) * alpha_width + src_x0;
        let src_end = src_start + paste_w;
        let dst_start = (dst_y0 + row) * canvas_width + dst_x0;
        let dst_end = dst_start + paste_w;
        pixels[dst_start..dst_end].copy_from_slice(&alpha.pixels[src_start..src_end]);
    }

    Ok(AlphaTileImage {
        width: canvas_size.width,
        height: canvas_size.height,
        pixels,
    })
}

pub(crate) fn place_rgba_on_canvas(
    rgba: RgbaTileImage,
    canvas_size: CanvasSize,
    offset_x: i32,
    offset_y: i32,
) -> Result<RgbaTileImage, ClipFileError> {
    if rgba.width == canvas_size.width
        && rgba.height == canvas_size.height
        && offset_x == 0
        && offset_y == 0
    {
        return Ok(rgba);
    }

    if offset_x == 0
        && offset_y == 0
        && rgba.width >= canvas_size.width
        && rgba.height >= canvas_size.height
    {
        return crop_rgba_top_left(rgba, canvas_size);
    }

    let canvas_width =
        usize::try_from(canvas_size.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let canvas_height =
        usize::try_from(canvas_size.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let rgba_width = usize::try_from(rgba.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let rgba_height = usize::try_from(rgba.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let pixel_count = canvas_width
        .checked_mul(canvas_height)
        .ok_or(ClipFileError::TileSizeOverflow)?;
    let mut pixels = vec![
        0u8;
        pixel_count
            .checked_mul(4)
            .ok_or(ClipFileError::TileSizeOverflow)?
    ];
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[0] = 255;
        pixel[1] = 255;
        pixel[2] = 255;
    }

    let src_x0 =
        usize::try_from((-offset_x).max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let src_y0 =
        usize::try_from((-offset_y).max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let dst_x0 = usize::try_from(offset_x.max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let dst_y0 = usize::try_from(offset_y.max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let paste_w = rgba_width
        .saturating_sub(src_x0)
        .min(canvas_width.saturating_sub(dst_x0));
    let paste_h = rgba_height
        .saturating_sub(src_y0)
        .min(canvas_height.saturating_sub(dst_y0));

    for row in 0..paste_h {
        let src_start = ((src_y0 + row) * rgba_width + src_x0) * 4;
        let src_end = src_start + paste_w * 4;
        let dst_start = ((dst_y0 + row) * canvas_width + dst_x0) * 4;
        let dst_end = dst_start + paste_w * 4;
        pixels[dst_start..dst_end].copy_from_slice(&rgba.pixels[src_start..src_end]);
    }

    Ok(RgbaTileImage {
        width: canvas_size.width,
        height: canvas_size.height,
        pixels,
    })
}

fn crop_alpha_top_left(
    alpha: AlphaTileImage,
    canvas_size: CanvasSize,
) -> Result<AlphaTileImage, ClipFileError> {
    let canvas_width =
        usize::try_from(canvas_size.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let canvas_height =
        usize::try_from(canvas_size.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let alpha_width = usize::try_from(alpha.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let mut pixels = vec![
        0u8;
        canvas_width
            .checked_mul(canvas_height)
            .ok_or(ClipFileError::TileSizeOverflow)?
    ];
    for row in 0..canvas_height {
        let src_start = row * alpha_width;
        let src_end = src_start + canvas_width;
        let dst_start = row * canvas_width;
        let dst_end = dst_start + canvas_width;
        pixels[dst_start..dst_end].copy_from_slice(&alpha.pixels[src_start..src_end]);
    }
    Ok(AlphaTileImage {
        width: canvas_size.width,
        height: canvas_size.height,
        pixels,
    })
}

fn crop_rgba_top_left(
    rgba: RgbaTileImage,
    canvas_size: CanvasSize,
) -> Result<RgbaTileImage, ClipFileError> {
    let canvas_width =
        usize::try_from(canvas_size.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let canvas_height =
        usize::try_from(canvas_size.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let rgba_width = usize::try_from(rgba.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let pixel_count = canvas_width
        .checked_mul(canvas_height)
        .ok_or(ClipFileError::TileSizeOverflow)?;
    let mut pixels = vec![
        0u8;
        pixel_count
            .checked_mul(4)
            .ok_or(ClipFileError::TileSizeOverflow)?
    ];
    for row in 0..canvas_height {
        let src_start = row * rgba_width * 4;
        let src_end = src_start + canvas_width * 4;
        let dst_start = row * canvas_width * 4;
        let dst_end = dst_start + canvas_width * 4;
        pixels[dst_start..dst_end].copy_from_slice(&rgba.pixels[src_start..src_end]);
    }
    Ok(RgbaTileImage {
        width: canvas_size.width,
        height: canvas_size.height,
        pixels,
    })
}
