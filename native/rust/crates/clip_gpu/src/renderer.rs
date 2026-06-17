use std::sync::mpsc;
use std::{error::Error, fmt};

use clip_graph::RenderPlan;
use clip_model::Rect;

use crate::{GpuContext, GpuDeviceConfig, GpuDeviceError};

#[derive(Debug, Eq, PartialEq)]
pub enum GpuRenderError {
    Device(GpuDeviceError),
    InvalidImageSize,
    InputBufferSizeMismatch {
        expected: usize,
        actual: usize,
    },
    ReadbackSizeOverflow,
    TextureSizeOverflow,
    InvalidToneCurveLutLength {
        expected: usize,
        actual: usize,
    },
    DuplicateRasterResource {
        layer_id: clip_model::LayerId,
        render_mipmap_id: u32,
    },
    DuplicateMaskResource {
        layer_id: clip_model::LayerId,
        mask_mipmap_id: u32,
    },
    MissingRasterResource {
        layer_id: clip_model::LayerId,
        render_mipmap_id: u32,
    },
    MissingMaskResource {
        layer_id: clip_model::LayerId,
        mask_mipmap_id: u32,
    },
    EmptyRasterStack,
    RasterResourceSizeMismatch {
        layer_id: clip_model::LayerId,
        expected: clip_model::CanvasSize,
        actual: clip_model::CanvasSize,
    },
    RasterAtlasSizeMismatch {
        expected: clip_model::CanvasSize,
        actual: clip_model::CanvasSize,
    },
    RasterAtlasResourceCountMismatch {
        expected: usize,
        actual: usize,
    },
    MaskResourceSizeMismatch {
        layer_id: clip_model::LayerId,
        expected: clip_model::CanvasSize,
        actual: clip_model::CanvasSize,
    },
    UploadRegionOutOfBounds {
        texture_size: clip_model::CanvasSize,
        origin_x: u32,
        origin_y: u32,
        upload_size: clip_model::CanvasSize,
    },
    MapFailed(String),
    PollFailed(String),
    NotImplemented,
}

impl From<GpuDeviceError> for GpuRenderError {
    fn from(value: GpuDeviceError) -> Self {
        Self::Device(value)
    }
}

impl fmt::Display for GpuRenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Device(err) => write!(f, "{err}"),
            Self::InvalidImageSize => f.write_str("GPU image size must be non-zero"),
            Self::InputBufferSizeMismatch { expected, actual } => write!(
                f,
                "GPU input buffer size mismatch: expected {expected} bytes, got {actual}",
            ),
            Self::ReadbackSizeOverflow => f.write_str("GPU readback size calculation overflow"),
            Self::TextureSizeOverflow => f.write_str("GPU texture size calculation overflow"),
            Self::InvalidToneCurveLutLength { expected, actual } => {
                write!(f, "tone curve LUT must be {expected} bytes, got {actual}",)
            }
            Self::DuplicateRasterResource {
                layer_id,
                render_mipmap_id,
            } => write!(
                f,
                "duplicate GPU raster resource for layer {} render mipmap {}",
                layer_id.0, render_mipmap_id,
            ),
            Self::DuplicateMaskResource {
                layer_id,
                mask_mipmap_id,
            } => write!(
                f,
                "duplicate GPU mask resource for layer {} mask mipmap {}",
                layer_id.0, mask_mipmap_id,
            ),
            Self::MissingRasterResource {
                layer_id,
                render_mipmap_id,
            } => write!(
                f,
                "missing GPU raster resource for layer {} render mipmap {}",
                layer_id.0, render_mipmap_id,
            ),
            Self::MissingMaskResource {
                layer_id,
                mask_mipmap_id,
            } => write!(
                f,
                "missing GPU mask resource for layer {} mask mipmap {}",
                layer_id.0, mask_mipmap_id,
            ),
            Self::EmptyRasterStack => f.write_str("GPU raster stack has no drawable resources"),
            Self::RasterResourceSizeMismatch {
                layer_id,
                expected,
                actual,
            } => write!(
                f,
                "GPU raster resource {} has size {}x{}, expected {}x{}",
                layer_id.0, actual.width, actual.height, expected.width, expected.height,
            ),
            Self::RasterAtlasSizeMismatch { expected, actual } => write!(
                f,
                "GPU raster atlas has size {}x{}, expected {}x{}",
                actual.width, actual.height, expected.width, expected.height,
            ),
            Self::RasterAtlasResourceCountMismatch { expected, actual } => write!(
                f,
                "GPU raster atlas reported {actual} resources, expected {expected}",
            ),
            Self::MaskResourceSizeMismatch {
                layer_id,
                expected,
                actual,
            } => write!(
                f,
                "GPU mask resource {} has size {}x{}, expected {}x{}",
                layer_id.0, actual.width, actual.height, expected.width, expected.height,
            ),
            Self::UploadRegionOutOfBounds {
                texture_size,
                origin_x,
                origin_y,
                upload_size,
            } => write!(
                f,
                "GPU upload region {}x{} at {},{} exceeds texture {}x{}",
                upload_size.width,
                upload_size.height,
                origin_x,
                origin_y,
                texture_size.width,
                texture_size.height,
            ),
            Self::MapFailed(err) => write!(f, "GPU readback map failed: {err}"),
            Self::PollFailed(err) => write!(f, "GPU poll failed: {err}"),
            Self::NotImplemented => f.write_str("GPU renderer operation is not implemented"),
        }
    }
}

impl Error for GpuRenderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Device(err) => Some(err),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct GpuRenderer {
    pub(crate) context: GpuContext,
}

impl GpuRenderer {
    pub fn new(config: GpuDeviceConfig) -> Result<Self, GpuRenderError> {
        Ok(Self {
            context: GpuContext::new(&config)?,
        })
    }

    pub fn max_texture_dimension_2d(&self) -> u32 {
        self.context.device.limits().max_texture_dimension_2d
    }

    pub fn roundtrip_rgba8(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Vec<u8>, GpuRenderError> {
        let layout = RgbaReadbackLayout::new(width, height)?;
        if pixels.len() != layout.unpadded_len {
            return Err(GpuRenderError::InputBufferSizeMismatch {
                expected: layout.unpadded_len,
                actual: pixels.len(),
            });
        }

        let texture = self
            .context
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("rizum_clip_roundtrip_rgba8_texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
        self.context.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(layout.unpadded_bytes_per_row),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        self.read_texture_rgba8(&texture, width, height)
    }

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
                texture: &texture,
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

        let slice = readback.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result.map_err(|err| err.to_string()));
        });
        self.context
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|err| GpuRenderError::PollFailed(err.to_string()))?;
        rx.recv()
            .map_err(|err| GpuRenderError::MapFailed(err.to_string()))?
            .map_err(GpuRenderError::MapFailed)?;

        let mapped = slice.get_mapped_range();
        let mut output = vec![0u8; layout.unpadded_len];
        for row in 0..height as usize {
            let src_start = row * layout.padded_bytes_per_row as usize;
            let src_end = src_start + layout.unpadded_bytes_per_row as usize;
            let dst_start = row * layout.unpadded_bytes_per_row as usize;
            let dst_end = dst_start + layout.unpadded_bytes_per_row as usize;
            output[dst_start..dst_end].copy_from_slice(&mapped[src_start..src_end]);
        }
        drop(mapped);
        readback.unmap();
        Ok(output)
    }

    pub fn read_rgba8_region(
        &self,
        _plan: &RenderPlan,
        _region: Rect,
        _out: &mut [u8],
    ) -> Result<(), GpuRenderError> {
        Err(GpuRenderError::NotImplemented)
    }
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
