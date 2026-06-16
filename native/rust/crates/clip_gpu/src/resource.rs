use std::collections::HashSet;

use clip_graph::RenderNodeId;
use clip_model::{CanvasSize, LayerId};

use crate::{GpuRenderError, GpuRenderer};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GpuRasterResourceKey {
    pub layer_id: LayerId,
    pub render_mipmap_id: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GpuMaskResourceKey {
    pub layer_id: LayerId,
    pub mask_mipmap_id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuRasterResourceInfo {
    pub key: GpuRasterResourceKey,
    pub render_node_id: RenderNodeId,
    pub size: CanvasSize,
    pub byte_len: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuMaskResourceInfo {
    pub key: GpuMaskResourceKey,
    pub render_node_id: RenderNodeId,
    pub size: CanvasSize,
    pub byte_len: usize,
}

#[derive(Debug)]
pub struct GpuRasterUpload<'a> {
    pub layer_id: LayerId,
    pub render_node_id: RenderNodeId,
    pub render_mipmap_id: u32,
    pub size: CanvasSize,
    pub pixels: &'a [u8],
}

#[derive(Debug)]
pub struct GpuMaskUpload<'a> {
    pub layer_id: LayerId,
    pub render_node_id: RenderNodeId,
    pub mask_mipmap_id: u32,
    pub size: CanvasSize,
    pub upload_origin_x: u32,
    pub upload_origin_y: u32,
    pub upload_size: CanvasSize,
    pub pixels: &'a [u8],
}

#[derive(Clone, Debug)]
pub struct GpuRasterResource {
    info: GpuRasterResourceInfo,
    texture: wgpu::Texture,
}

impl GpuRasterResource {
    pub fn info(&self) -> GpuRasterResourceInfo {
        self.info
    }

    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }
}

#[derive(Clone, Debug)]
pub struct GpuMaskResource {
    info: GpuMaskResourceInfo,
    texture: wgpu::Texture,
}

impl GpuMaskResource {
    pub fn info(&self) -> GpuMaskResourceInfo {
        self.info
    }

    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }
}

#[derive(Clone, Debug)]
pub struct GpuRasterResourceCache {
    resources: Vec<GpuRasterResource>,
}

#[derive(Clone, Debug, Default)]
pub struct GpuMaskResourceCache {
    resources: Vec<GpuMaskResource>,
}

impl GpuRasterResourceCache {
    pub fn empty() -> Self {
        Self {
            resources: Vec::new(),
        }
    }

    pub fn resource_infos(&self) -> impl Iterator<Item = GpuRasterResourceInfo> + '_ {
        self.resources.iter().map(GpuRasterResource::info)
    }

    pub fn len(&self) -> usize {
        self.resources.len()
    }

    pub fn is_empty(&self) -> bool {
        self.resources.is_empty()
    }

    pub fn resource(&self, key: GpuRasterResourceKey) -> Option<&GpuRasterResource> {
        self.resources
            .iter()
            .find(|resource| resource.info.key == key)
    }
}

impl GpuMaskResourceCache {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn resource_infos(&self) -> impl Iterator<Item = GpuMaskResourceInfo> + '_ {
        self.resources.iter().map(GpuMaskResource::info)
    }

    pub fn len(&self) -> usize {
        self.resources.len()
    }

    pub fn is_empty(&self) -> bool {
        self.resources.is_empty()
    }

    pub fn resource(&self, key: GpuMaskResourceKey) -> Option<&GpuMaskResource> {
        self.resources
            .iter()
            .find(|resource| resource.info.key == key)
    }
}

impl GpuRenderer {
    pub fn upload_raster_resources(
        &self,
        uploads: &[GpuRasterUpload<'_>],
    ) -> Result<GpuRasterResourceCache, GpuRenderError> {
        let mut seen = HashSet::with_capacity(uploads.len());
        let mut resources = Vec::with_capacity(uploads.len());

        for upload in uploads {
            let key = GpuRasterResourceKey {
                layer_id: upload.layer_id,
                render_mipmap_id: upload.render_mipmap_id,
            };
            if !seen.insert(key) {
                return Err(GpuRenderError::DuplicateRasterResource {
                    layer_id: upload.layer_id,
                    render_mipmap_id: upload.render_mipmap_id,
                });
            }

            let layout = RgbaTextureLayout::new(upload.size)?;
            if upload.pixels.len() != layout.unpadded_len {
                return Err(GpuRenderError::InputBufferSizeMismatch {
                    expected: layout.unpadded_len,
                    actual: upload.pixels.len(),
                });
            }

            let texture = self
                .context
                .device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("rizum_clip_raster_resource_texture"),
                    size: wgpu::Extent3d {
                        width: upload.size.width,
                        height: upload.size.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
            self.context.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                upload.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(layout.unpadded_bytes_per_row),
                    rows_per_image: Some(upload.size.height),
                },
                wgpu::Extent3d {
                    width: upload.size.width,
                    height: upload.size.height,
                    depth_or_array_layers: 1,
                },
            );

            resources.push(GpuRasterResource {
                info: GpuRasterResourceInfo {
                    key,
                    render_node_id: upload.render_node_id,
                    size: upload.size,
                    byte_len: upload.pixels.len(),
                },
                texture,
            });
        }

        Ok(GpuRasterResourceCache { resources })
    }

    pub fn upload_mask_resources(
        &self,
        uploads: &[GpuMaskUpload<'_>],
    ) -> Result<GpuMaskResourceCache, GpuRenderError> {
        let mut seen = HashSet::with_capacity(uploads.len());
        let mut resources = Vec::with_capacity(uploads.len());

        for upload in uploads {
            let key = GpuMaskResourceKey {
                layer_id: upload.layer_id,
                mask_mipmap_id: upload.mask_mipmap_id,
            };
            if !seen.insert(key) {
                return Err(GpuRenderError::DuplicateMaskResource {
                    layer_id: upload.layer_id,
                    mask_mipmap_id: upload.mask_mipmap_id,
                });
            }

            let layout = MaskTextureLayout::new(upload.size)?;
            let upload_layout = MaskTextureLayout::new(upload.upload_size)?;
            if upload.pixels.len() != upload_layout.unpadded_len {
                return Err(GpuRenderError::InputBufferSizeMismatch {
                    expected: upload_layout.unpadded_len,
                    actual: upload.pixels.len(),
                });
            }
            validate_upload_region(
                upload.size,
                upload.upload_origin_x,
                upload.upload_origin_y,
                upload.upload_size,
            )?;

            let texture = self
                .context
                .device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("rizum_clip_mask_resource_texture"),
                    size: wgpu::Extent3d {
                        width: upload.size.width,
                        height: upload.size.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R8Unorm,
                    usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
            self.context.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: upload.upload_origin_x,
                        y: upload.upload_origin_y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                upload.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(upload_layout.unpadded_bytes_per_row),
                    rows_per_image: Some(upload.upload_size.height),
                },
                wgpu::Extent3d {
                    width: upload.upload_size.width,
                    height: upload.upload_size.height,
                    depth_or_array_layers: 1,
                },
            );

            resources.push(GpuMaskResource {
                info: GpuMaskResourceInfo {
                    key,
                    render_node_id: upload.render_node_id,
                    size: upload.size,
                    byte_len: layout.unpadded_len,
                },
                texture,
            });
        }

        Ok(GpuMaskResourceCache { resources })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RgbaTextureLayout {
    unpadded_bytes_per_row: u32,
    unpadded_len: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MaskTextureLayout {
    unpadded_bytes_per_row: u32,
    unpadded_len: usize,
}

impl MaskTextureLayout {
    fn new(size: CanvasSize) -> Result<Self, GpuRenderError> {
        if size.width == 0 || size.height == 0 {
            return Err(GpuRenderError::InvalidImageSize);
        }
        let unpadded_bytes_per_row = size.width;
        let unpadded_len = byte_len(unpadded_bytes_per_row, size.height)?;
        Ok(Self {
            unpadded_bytes_per_row,
            unpadded_len,
        })
    }
}

impl RgbaTextureLayout {
    fn new(size: CanvasSize) -> Result<Self, GpuRenderError> {
        if size.width == 0 || size.height == 0 {
            return Err(GpuRenderError::InvalidImageSize);
        }
        let unpadded_bytes_per_row = size
            .width
            .checked_mul(4)
            .ok_or(GpuRenderError::TextureSizeOverflow)?;
        let unpadded_len = byte_len(unpadded_bytes_per_row, size.height)?;
        Ok(Self {
            unpadded_bytes_per_row,
            unpadded_len,
        })
    }
}

fn byte_len(bytes_per_row: u32, rows: u32) -> Result<usize, GpuRenderError> {
    usize::try_from(
        u64::from(bytes_per_row)
            .checked_mul(u64::from(rows))
            .ok_or(GpuRenderError::TextureSizeOverflow)?,
    )
    .map_err(|_| GpuRenderError::TextureSizeOverflow)
}

fn validate_upload_region(
    texture_size: CanvasSize,
    origin_x: u32,
    origin_y: u32,
    upload_size: CanvasSize,
) -> Result<(), GpuRenderError> {
    let right = origin_x
        .checked_add(upload_size.width)
        .ok_or(GpuRenderError::TextureSizeOverflow)?;
    let bottom = origin_y
        .checked_add(upload_size.height)
        .ok_or(GpuRenderError::TextureSizeOverflow)?;
    if right > texture_size.width || bottom > texture_size.height {
        return Err(GpuRenderError::UploadRegionOutOfBounds {
            texture_size,
            origin_x,
            origin_y,
            upload_size,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clip_model::CanvasSize;

    use super::{MaskTextureLayout, RgbaTextureLayout, validate_upload_region};
    use crate::GpuRenderError;

    #[test]
    fn upload_layout_tracks_unpadded_rgba_rows() {
        let layout = RgbaTextureLayout::new(CanvasSize::new(62, 3)).unwrap();

        assert_eq!(layout.unpadded_bytes_per_row, 248);
        assert_eq!(layout.unpadded_len, 744);
    }

    #[test]
    fn mask_upload_layout_tracks_unpadded_rows() {
        let layout = MaskTextureLayout::new(CanvasSize::new(62, 3)).unwrap();

        assert_eq!(layout.unpadded_bytes_per_row, 62);
        assert_eq!(layout.unpadded_len, 186);
    }

    #[test]
    fn mask_upload_region_must_fit_texture() {
        let err =
            validate_upload_region(CanvasSize::new(8, 8), 6, 4, CanvasSize::new(3, 2)).unwrap_err();

        assert_eq!(
            err,
            GpuRenderError::UploadRegionOutOfBounds {
                texture_size: CanvasSize::new(8, 8),
                origin_x: 6,
                origin_y: 4,
                upload_size: CanvasSize::new(3, 2),
            }
        );
    }
}
