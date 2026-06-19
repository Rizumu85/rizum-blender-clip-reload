use std::sync::mpsc;

use clip_model::{CanvasSize, Rect};

use crate::{GpuRenderError, GpuRenderer};

impl GpuRenderer {
    pub(crate) fn read_texture_rgba8(
        &self,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, GpuRenderError> {
        let layout = RgbaReadbackLayout::new(width, height)?;
        let readback = self.context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rizum_clip_roundtrip_rgba8_readback"),
            size: layout.padded_len as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder =
            self.context
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("rizum_clip_roundtrip_rgba8_encoder"),
                });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(layout.padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.context.queue.submit([encoder.finish()]);

        let mut output = vec![0u8; layout.unpadded_len];
        append_mapped_rgba8_rows(self, &readback, layout, height, &mut output, 0)?;
        Ok(output)
    }

    pub(crate) fn read_texture_rgba8_regions(
        &self,
        texture: &wgpu::Texture,
        texture_size: CanvasSize,
        regions: &[Rect],
    ) -> Result<Vec<u8>, GpuRenderError> {
        let mut encoder =
            self.context
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("rizum_clip_rgba8_region_readback_encoder"),
                });
        let mut pending = Vec::new();
        let mut output_len = 0usize;
        for region in regions.iter().copied().filter(|region| !region.is_empty()) {
            validate_readback_region(texture_size, region)?;
            let layout = RgbaReadbackLayout::new(region.width, region.height)?;
            let readback = self.context.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rizum_clip_rgba8_region_readback"),
                size: layout.padded_len as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: region.x,
                        y: region.y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(layout.padded_bytes_per_row),
                        rows_per_image: Some(region.height),
                    },
                },
                wgpu::Extent3d {
                    width: region.width,
                    height: region.height,
                    depth_or_array_layers: 1,
                },
            );
            output_len = output_len
                .checked_add(layout.unpadded_len)
                .ok_or(GpuRenderError::ReadbackSizeOverflow)?;
            pending.push(PendingRegionReadback {
                buffer: readback,
                layout,
                height: region.height,
            });
        }
        if pending.is_empty() {
            return Ok(Vec::new());
        }
        self.context.queue.submit([encoder.finish()]);

        let mut output = vec![0u8; output_len];
        let mut output_offset = 0;
        for readback in &pending {
            append_mapped_rgba8_rows(
                self,
                &readback.buffer,
                readback.layout,
                readback.height,
                &mut output,
                output_offset,
            )?;
            output_offset += readback.layout.unpadded_len;
        }
        Ok(output)
    }
}

pub(crate) fn rgba8_unpadded_len(width: u32, height: u32) -> Result<usize, GpuRenderError> {
    RgbaReadbackLayout::new(width, height).map(|layout| layout.unpadded_len)
}

fn append_mapped_rgba8_rows(
    renderer: &GpuRenderer,
    readback: &wgpu::Buffer,
    layout: RgbaReadbackLayout,
    height: u32,
    output: &mut [u8],
    output_offset: usize,
) -> Result<(), GpuRenderError> {
    let slice = readback.slice(..);
    let (tx, rx) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result.map_err(|err| err.to_string()));
    });
    renderer
        .context
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|err| GpuRenderError::PollFailed(err.to_string()))?;
    rx.recv()
        .map_err(|err| GpuRenderError::MapFailed(err.to_string()))?
        .map_err(GpuRenderError::MapFailed)?;

    let mapped = slice.get_mapped_range();
    for row in 0..height as usize {
        let src_start = row * layout.padded_bytes_per_row as usize;
        let src_end = src_start + layout.unpadded_bytes_per_row as usize;
        let dst_start = output_offset + row * layout.unpadded_bytes_per_row as usize;
        let dst_end = dst_start + layout.unpadded_bytes_per_row as usize;
        output[dst_start..dst_end].copy_from_slice(&mapped[src_start..src_end]);
    }
    drop(mapped);
    readback.unmap();
    Ok(())
}

fn validate_readback_region(texture_size: CanvasSize, region: Rect) -> Result<(), GpuRenderError> {
    let right = region
        .x
        .checked_add(region.width)
        .ok_or(GpuRenderError::ReadbackSizeOverflow)?;
    let bottom = region
        .y
        .checked_add(region.height)
        .ok_or(GpuRenderError::ReadbackSizeOverflow)?;
    if right > texture_size.width || bottom > texture_size.height {
        return Err(GpuRenderError::ReadbackRegionOutOfBounds {
            texture_size,
            origin_x: region.x,
            origin_y: region.y,
            read_size: CanvasSize::new(region.width, region.height),
        });
    }
    Ok(())
}

struct PendingRegionReadback {
    buffer: wgpu::Buffer,
    layout: RgbaReadbackLayout,
    height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RgbaReadbackLayout {
    unpadded_bytes_per_row: u32,
    padded_bytes_per_row: u32,
    unpadded_len: usize,
    padded_len: usize,
}

impl RgbaReadbackLayout {
    fn new(width: u32, height: u32) -> Result<Self, GpuRenderError> {
        if width == 0 || height == 0 {
            return Err(GpuRenderError::InvalidImageSize);
        }
        let unpadded_bytes_per_row = width
            .checked_mul(4)
            .ok_or(GpuRenderError::ReadbackSizeOverflow)?;
        let padded_bytes_per_row =
            align_u32(unpadded_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)?;
        let unpadded_len = usize::try_from(
            u64::from(unpadded_bytes_per_row)
                .checked_mul(u64::from(height))
                .ok_or(GpuRenderError::ReadbackSizeOverflow)?,
        )
        .map_err(|_| GpuRenderError::ReadbackSizeOverflow)?;
        let padded_len = usize::try_from(
            u64::from(padded_bytes_per_row)
                .checked_mul(u64::from(height))
                .ok_or(GpuRenderError::ReadbackSizeOverflow)?,
        )
        .map_err(|_| GpuRenderError::ReadbackSizeOverflow)?;
        Ok(Self {
            unpadded_bytes_per_row,
            padded_bytes_per_row,
            unpadded_len,
            padded_len,
        })
    }
}

fn align_u32(value: u32, alignment: u32) -> Result<u32, GpuRenderError> {
    let mask = alignment
        .checked_sub(1)
        .ok_or(GpuRenderError::ReadbackSizeOverflow)?;
    value
        .checked_add(mask)
        .map(|value| value & !mask)
        .ok_or(GpuRenderError::ReadbackSizeOverflow)
}

#[cfg(test)]
mod tests {
    use super::{RgbaReadbackLayout, align_u32};

    #[test]
    fn readback_layout_pads_rows_to_wgpu_alignment() {
        let layout = RgbaReadbackLayout::new(62, 3).unwrap();

        assert_eq!(layout.unpadded_bytes_per_row, 248);
        assert_eq!(layout.padded_bytes_per_row, 256);
        assert_eq!(layout.unpadded_len, 744);
        assert_eq!(layout.padded_len, 768);
    }

    #[test]
    fn align_u32_keeps_aligned_values() {
        assert_eq!(align_u32(256, 256).unwrap(), 256);
        assert_eq!(align_u32(257, 256).unwrap(), 512);
    }
}
