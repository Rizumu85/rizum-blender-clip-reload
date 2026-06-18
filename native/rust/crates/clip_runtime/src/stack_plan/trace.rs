use clip_model::{CanvasSize, Rgba8};

use crate::error::RuntimeError;
pub(crate) fn sample_rgba8(
    pixels: &[u8],
    size: CanvasSize,
    x: u32,
    y: u32,
) -> Result<Rgba8, RuntimeError> {
    let width = usize::try_from(size.width).map_err(|_| RuntimeError::InvalidRegion)?;
    let x = usize::try_from(x).map_err(|_| RuntimeError::InvalidRegion)?;
    let y = usize::try_from(y).map_err(|_| RuntimeError::InvalidRegion)?;
    let pixel_offset = y
        .checked_mul(width)
        .and_then(|row| row.checked_add(x))
        .and_then(|pixel| pixel.checked_mul(4))
        .ok_or(RuntimeError::InvalidRegion)?;
    let pixel = pixels
        .get(pixel_offset..pixel_offset + 4)
        .ok_or(RuntimeError::InvalidRegion)?;
    Ok(Rgba8 {
        r: pixel[0],
        g: pixel[1],
        b: pixel[2],
        a: pixel[3],
    })
}
