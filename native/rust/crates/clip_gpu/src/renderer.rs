use std::{error::Error, fmt};

use clip_graph::RenderPlan;
use clip_model::Rect;

use crate::{GpuContext, GpuDeviceConfig, GpuDeviceError};
use crate::{GpuSparseAtlasFormat, GpuSparseAtlasTextureKey};

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
    SparseAtlasSizeMismatch {
        expected: clip_model::CanvasSize,
        actual: clip_model::CanvasSize,
    },
    MissingSparseAtlasTexture {
        key: GpuSparseAtlasTextureKey,
    },
    SparseAtlasFormatMismatch {
        expected: GpuSparseAtlasFormat,
        actual: GpuSparseAtlasFormat,
    },
    SparseAtlasMixedTextureKeys,
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
    ReadbackRegionOutOfBounds {
        texture_size: clip_model::CanvasSize,
        origin_x: u32,
        origin_y: u32,
        read_size: clip_model::CanvasSize,
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
            Self::SparseAtlasSizeMismatch { expected, actual } => write!(
                f,
                "GPU sparse atlas has size {}x{}, expected {}x{}",
                actual.width, actual.height, expected.width, expected.height,
            ),
            Self::MissingSparseAtlasTexture { key } => write!(
                f,
                "missing GPU sparse atlas texture for format {:?} atlas {}",
                key.format, key.atlas_id,
            ),
            Self::SparseAtlasFormatMismatch { expected, actual } => write!(
                f,
                "GPU sparse atlas format mismatch: expected {:?}, got {:?}",
                expected, actual,
            ),
            Self::SparseAtlasMixedTextureKeys => {
                f.write_str("GPU sparse atlas tile pass cannot bind multiple atlas keys")
            }
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
            Self::ReadbackRegionOutOfBounds {
                texture_size,
                origin_x,
                origin_y,
                read_size,
            } => write!(
                f,
                "GPU readback region {}x{} at {},{} exceeds texture {}x{}",
                read_size.width,
                read_size.height,
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
        let expected_len = crate::readback::rgba8_unpadded_len(width, height)?;
        if pixels.len() != expected_len {
            return Err(GpuRenderError::InputBufferSizeMismatch {
                expected: expected_len,
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
                bytes_per_row: Some(width * 4),
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

    pub fn read_rgba8_region(
        &self,
        _plan: &RenderPlan,
        _region: Rect,
        _out: &mut [u8],
    ) -> Result<(), GpuRenderError> {
        Err(GpuRenderError::NotImplemented)
    }
}
