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
    pub pixels: &'a [u8],
}

#[derive(Debug)]
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

#[derive(Debug)]
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

#[derive(Debug)]
pub struct GpuRasterResourceCache {
    resources: Vec<GpuRasterResource>,
}

#[derive(Debug, Default)]
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
            let staging = self.context.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rizum_clip_raster_resource_upload_staging"),
                size: layout.padded_len as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: true,
            });
            {
                let mut mapped = staging.slice(..).get_mapped_range_mut();
                write_padded_rows(mapped.slice(..), upload.pixels, layout);
            }
            staging.unmap();

            let mut encoder =
                self.context
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("rizum_clip_raster_resource_upload_encoder"),
                    });
            encoder.copy_buffer_to_texture(
                wgpu::TexelCopyBufferInfo {
                    buffer: &staging,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(layout.padded_bytes_per_row),
                        rows_per_image: Some(upload.size.height),
                    },
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: upload.size.width,
                    height: upload.size.height,
                    depth_or_array_layers: 1,
                },
            );
            self.context.queue.submit([encoder.finish()]);
            self.context
                .device
                .poll(wgpu::PollType::wait_indefinitely())
                .map_err(|err| GpuRenderError::PollFailed(err.to_string()))?;
            drop(staging);

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
            let staging = self.context.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rizum_clip_mask_resource_upload_staging"),
                size: layout.padded_len as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: true,
            });
            {
                let mut mapped = staging.slice(..).get_mapped_range_mut();
                write_padded_mask_rows(mapped.slice(..), upload.pixels, layout);
            }
            staging.unmap();

            let mut encoder =
                self.context
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("rizum_clip_mask_resource_upload_encoder"),
                    });
            encoder.copy_buffer_to_texture(
                wgpu::TexelCopyBufferInfo {
                    buffer: &staging,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(layout.padded_bytes_per_row),
                        rows_per_image: Some(upload.size.height),
                    },
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: upload.size.width,
                    height: upload.size.height,
                    depth_or_array_layers: 1,
                },
            );
            self.context.queue.submit([encoder.finish()]);
            self.context
                .device
                .poll(wgpu::PollType::wait_indefinitely())
                .map_err(|err| GpuRenderError::PollFailed(err.to_string()))?;
            drop(staging);

            resources.push(GpuMaskResource {
                info: GpuMaskResourceInfo {
                    key,
                    render_node_id: upload.render_node_id,
                    size: upload.size,
                    byte_len: upload.pixels.len(),
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
    padded_bytes_per_row: u32,
    unpadded_len: usize,
    padded_len: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MaskTextureLayout {
    unpadded_bytes_per_row: u32,
    padded_bytes_per_row: u32,
    unpadded_len: usize,
    padded_len: usize,
}

impl MaskTextureLayout {
    fn new(size: CanvasSize) -> Result<Self, GpuRenderError> {
        if size.width == 0 || size.height == 0 {
            return Err(GpuRenderError::InvalidImageSize);
        }
        let unpadded_bytes_per_row = size.width;
        let padded_bytes_per_row =
            align_u32(unpadded_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)?;
        let unpadded_len = byte_len(unpadded_bytes_per_row, size.height)?;
        let padded_len = byte_len(padded_bytes_per_row, size.height)?;
        Ok(Self {
            unpadded_bytes_per_row,
            padded_bytes_per_row,
            unpadded_len,
            padded_len,
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
        let padded_bytes_per_row =
            align_u32(unpadded_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)?;
        let unpadded_len = byte_len(unpadded_bytes_per_row, size.height)?;
        let padded_len = byte_len(padded_bytes_per_row, size.height)?;
        Ok(Self {
            unpadded_bytes_per_row,
            padded_bytes_per_row,
            unpadded_len,
            padded_len,
        })
    }
}

fn write_padded_rows(dst: wgpu::WriteOnly<'_, [u8]>, src: &[u8], layout: RgbaTextureLayout) {
    write_padded_texture_rows(
        dst,
        src,
        layout.unpadded_bytes_per_row as usize,
        layout.padded_bytes_per_row as usize,
        layout.unpadded_len,
    );
}

fn write_padded_mask_rows(dst: wgpu::WriteOnly<'_, [u8]>, src: &[u8], layout: MaskTextureLayout) {
    write_padded_texture_rows(
        dst,
        src,
        layout.unpadded_bytes_per_row as usize,
        layout.padded_bytes_per_row as usize,
        layout.unpadded_len,
    );
}

fn write_padded_texture_rows(
    mut dst: wgpu::WriteOnly<'_, [u8]>,
    src: &[u8],
    unpadded_bytes_per_row: usize,
    padded_bytes_per_row: usize,
    unpadded_len: usize,
) {
    if padded_bytes_per_row == unpadded_bytes_per_row {
        dst.copy_from_slice(src);
        return;
    }

    dst.fill(0);
    for row in 0..(unpadded_len / unpadded_bytes_per_row) {
        let src_start = row * unpadded_bytes_per_row;
        let src_end = src_start + unpadded_bytes_per_row;
        let dst_start = row * padded_bytes_per_row;
        let dst_end = dst_start + unpadded_bytes_per_row;
        dst.slice(dst_start..dst_end)
            .copy_from_slice(&src[src_start..src_end]);
    }
}

#[cfg(test)]
fn padded_rows(src: &[u8], layout: RgbaTextureLayout) -> Vec<u8> {
    let mut dst = vec![255u8; layout.padded_len];
    write_padded_rows(wgpu::WriteOnly::from_mut(dst.as_mut_slice()), src, layout);
    dst
}

#[cfg(test)]
fn padded_mask_rows(src: &[u8], layout: MaskTextureLayout) -> Vec<u8> {
    let mut dst = vec![255u8; layout.padded_len];
    write_padded_mask_rows(wgpu::WriteOnly::from_mut(dst.as_mut_slice()), src, layout);
    dst
}

fn align_u32(value: u32, alignment: u32) -> Result<u32, GpuRenderError> {
    let mask = alignment
        .checked_sub(1)
        .ok_or(GpuRenderError::TextureSizeOverflow)?;
    value
        .checked_add(mask)
        .map(|value| value & !mask)
        .ok_or(GpuRenderError::TextureSizeOverflow)
}

fn byte_len(bytes_per_row: u32, rows: u32) -> Result<usize, GpuRenderError> {
    usize::try_from(
        u64::from(bytes_per_row)
            .checked_mul(u64::from(rows))
            .ok_or(GpuRenderError::TextureSizeOverflow)?,
    )
    .map_err(|_| GpuRenderError::TextureSizeOverflow)
}

#[cfg(test)]
mod tests {
    use clip_model::CanvasSize;

    use super::{MaskTextureLayout, RgbaTextureLayout, padded_mask_rows, padded_rows};

    #[test]
    fn upload_layout_pads_unaligned_rows() {
        let layout = RgbaTextureLayout::new(CanvasSize::new(62, 3)).unwrap();

        assert_eq!(layout.unpadded_bytes_per_row, 248);
        assert_eq!(layout.padded_bytes_per_row, 256);
        assert_eq!(layout.unpadded_len, 744);
        assert_eq!(layout.padded_len, 768);
    }

    #[test]
    fn write_padded_rows_preserves_row_content() {
        let layout = RgbaTextureLayout::new(CanvasSize::new(2, 2)).unwrap();
        let src: Vec<u8> = (0..16).collect();

        let dst = padded_rows(&src, layout);

        assert_eq!(&dst[0..8], &src[0..8]);
        assert_eq!(&dst[8..256], &[0u8; 248]);
        assert_eq!(&dst[256..264], &src[8..16]);
    }

    #[test]
    fn mask_upload_layout_pads_unaligned_rows() {
        let layout = MaskTextureLayout::new(CanvasSize::new(62, 3)).unwrap();

        assert_eq!(layout.unpadded_bytes_per_row, 62);
        assert_eq!(layout.padded_bytes_per_row, 256);
        assert_eq!(layout.unpadded_len, 186);
        assert_eq!(layout.padded_len, 768);
    }

    #[test]
    fn write_padded_mask_rows_preserves_row_content() {
        let layout = MaskTextureLayout::new(CanvasSize::new(2, 2)).unwrap();
        let src: Vec<u8> = (0..4).collect();

        let dst = padded_mask_rows(&src, layout);

        assert_eq!(&dst[0..2], &src[0..2]);
        assert_eq!(&dst[2..256], &[0u8; 254]);
        assert_eq!(&dst[256..258], &src[2..4]);
    }
}
