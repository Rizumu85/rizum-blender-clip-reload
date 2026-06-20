use clip_model::Rect;

use crate::{ClipSession, RuntimeError};

impl ClipSession {
    pub fn read_rgba8_region(&mut self, region: Rect, out: &mut [u8]) -> Result<(), RuntimeError> {
        let x_end = region
            .x
            .checked_add(region.width)
            .ok_or(RuntimeError::InvalidRegion)?;
        let y_end = region
            .y
            .checked_add(region.height)
            .ok_or(RuntimeError::InvalidRegion)?;
        if x_end > self.summary.canvas.width || y_end > self.summary.canvas.height {
            return Err(RuntimeError::InvalidRegion);
        }

        let expected = u64::from(region.width)
            .checked_mul(u64::from(region.height))
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .ok_or(RuntimeError::InvalidRegion)?;
        if out.len() < expected {
            return Err(RuntimeError::OutputBufferTooSmall {
                expected,
                actual: out.len(),
            });
        }

        let image = self.rendered_image()?;
        let width = usize::try_from(region.width).map_err(|_| RuntimeError::InvalidRegion)?;
        let height = usize::try_from(region.height).map_err(|_| RuntimeError::InvalidRegion)?;
        let image_width = usize::try_from(image.width).map_err(|_| RuntimeError::InvalidRegion)?;
        let x = usize::try_from(region.x).map_err(|_| RuntimeError::InvalidRegion)?;
        let base_y = usize::try_from(region.y).map_err(|_| RuntimeError::InvalidRegion)?;
        for row in 0..height {
            let src_start = ((base_y + row) * image_width + x) * 4;
            let src_end = src_start + width * 4;
            let dst_start = row * width * 4;
            let dst_end = dst_start + width * 4;
            out[dst_start..dst_end].copy_from_slice(&image.pixels[src_start..src_end]);
        }
        Ok(())
    }

    fn rendered_image(&mut self) -> Result<&clip_file::tiles::RgbaTileImage, RuntimeError> {
        if self.rendered_image.is_none() {
            let result = self.draw_normal_raster_stack_via_gpu()?;
            let image = result.image.ok_or(RuntimeError::EmptyRenderPlan)?;
            self.rendered_image = Some(image);
        }
        Ok(self
            .rendered_image
            .as_ref()
            .expect("rendered image was populated"))
    }
}
