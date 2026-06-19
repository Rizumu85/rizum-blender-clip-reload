use std::collections::HashMap;

use clip_model::CanvasSize;

use crate::{GpuRenderError, GpuRenderer};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GpuSparseAtlasFormat {
    Rgba8,
    R8,
}

impl GpuSparseAtlasFormat {
    fn texture_format(self) -> wgpu::TextureFormat {
        match self {
            Self::Rgba8 => wgpu::TextureFormat::Rgba8Unorm,
            Self::R8 => wgpu::TextureFormat::R8Unorm,
        }
    }

    fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::Rgba8 => 4,
            Self::R8 => 1,
        }
    }
}

#[derive(Debug)]
pub struct GpuSparseAtlasTexture {
    size: CanvasSize,
    format: GpuSparseAtlasFormat,
    byte_len: usize,
    texture: wgpu::Texture,
}

impl GpuSparseAtlasTexture {
    pub fn size(&self) -> CanvasSize {
        self.size
    }

    pub fn format(&self) -> GpuSparseAtlasFormat {
        self.format
    }

    pub fn byte_len(&self) -> usize {
        self.byte_len
    }

    pub(crate) fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    pub fn create_view(&self) -> wgpu::TextureView {
        self.texture
            .create_view(&wgpu::TextureViewDescriptor::default())
    }
}

#[derive(Clone, Debug)]
pub struct GpuSparseAtlasUpdateChunk {
    pub atlas_x: u32,
    pub atlas_y: u32,
    pub size: CanvasSize,
    pub pixels: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GpuSparseAtlasTextureKey {
    pub format: GpuSparseAtlasFormat,
    pub atlas_id: u32,
}

#[derive(Debug, Default)]
pub struct GpuSparseAtlasTexturePool {
    textures: HashMap<GpuSparseAtlasTextureKey, GpuSparseAtlasTexture>,
}

impl GpuSparseAtlasTexturePool {
    pub fn texture(&self, key: GpuSparseAtlasTextureKey) -> Option<&GpuSparseAtlasTexture> {
        self.textures.get(&key)
    }

    pub fn len(&self) -> usize {
        self.textures.len()
    }

    pub fn is_empty(&self) -> bool {
        self.textures.is_empty()
    }

    fn resident_bytes(&self) -> usize {
        self.textures
            .values()
            .map(GpuSparseAtlasTexture::byte_len)
            .sum()
    }
}

#[derive(Clone, Debug)]
pub struct GpuSparseAtlasTexturePoolUpdate {
    pub key: GpuSparseAtlasTextureKey,
    pub atlas_size: CanvasSize,
    pub chunks: Vec<GpuSparseAtlasUpdateChunk>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GpuSparseAtlasTexturePoolStats {
    pub created_atlases: usize,
    pub updated_chunks: usize,
    pub upload_bytes: usize,
    pub resident_atlases: usize,
    pub resident_bytes: usize,
}

impl GpuRenderer {
    pub fn create_sparse_atlas_texture(
        &self,
        size: CanvasSize,
        format: GpuSparseAtlasFormat,
    ) -> Result<GpuSparseAtlasTexture, GpuRenderError> {
        if size.width == 0 || size.height == 0 {
            return Err(GpuRenderError::InvalidImageSize);
        }
        let byte_len = texture_byte_len(size, format)?;
        let texture = self
            .context
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("rizum_clip_sparse_atlas_texture"),
                size: wgpu::Extent3d {
                    width: size.width,
                    height: size.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: format.texture_format(),
                usage: wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
        Ok(GpuSparseAtlasTexture {
            size,
            format,
            byte_len,
            texture,
        })
    }

    pub fn update_sparse_atlas_texture(
        &self,
        atlas: &GpuSparseAtlasTexture,
        chunks: &[GpuSparseAtlasUpdateChunk],
    ) -> Result<(), GpuRenderError> {
        for chunk in chunks {
            validate_chunk(atlas, chunk)?;
            self.context.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: atlas.texture(),
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: chunk.atlas_x,
                        y: chunk.atlas_y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &chunk.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(chunk.size.width * atlas.format.bytes_per_pixel()),
                    rows_per_image: Some(chunk.size.height),
                },
                wgpu::Extent3d {
                    width: chunk.size.width,
                    height: chunk.size.height,
                    depth_or_array_layers: 1,
                },
            );
        }
        Ok(())
    }

    pub fn read_sparse_rgba_atlas_texture(
        &self,
        atlas: &GpuSparseAtlasTexture,
    ) -> Result<Vec<u8>, GpuRenderError> {
        if atlas.format != GpuSparseAtlasFormat::Rgba8 {
            return Err(GpuRenderError::NotImplemented);
        }
        self.read_texture_rgba8(atlas.texture(), atlas.size.width, atlas.size.height)
    }

    pub fn update_sparse_atlas_texture_pool(
        &self,
        pool: &mut GpuSparseAtlasTexturePool,
        updates: &[GpuSparseAtlasTexturePoolUpdate],
    ) -> Result<GpuSparseAtlasTexturePoolStats, GpuRenderError> {
        let mut stats = GpuSparseAtlasTexturePoolStats::default();
        for update in updates {
            let texture = if let Some(texture) = pool.textures.get(&update.key) {
                if texture.size != update.atlas_size {
                    return Err(GpuRenderError::SparseAtlasSizeMismatch {
                        expected: texture.size,
                        actual: update.atlas_size,
                    });
                }
                texture
            } else {
                let texture =
                    self.create_sparse_atlas_texture(update.atlas_size, update.key.format)?;
                pool.textures.insert(update.key, texture);
                stats.created_atlases += 1;
                pool.textures
                    .get(&update.key)
                    .expect("sparse atlas texture was just inserted")
            };
            self.update_sparse_atlas_texture(texture, &update.chunks)?;
            stats.updated_chunks += update.chunks.len();
            stats.upload_bytes += update
                .chunks
                .iter()
                .map(|chunk| chunk.pixels.len())
                .sum::<usize>();
        }
        stats.resident_atlases = pool.textures.len();
        stats.resident_bytes = pool.resident_bytes();
        Ok(stats)
    }
}

fn validate_chunk(
    atlas: &GpuSparseAtlasTexture,
    chunk: &GpuSparseAtlasUpdateChunk,
) -> Result<(), GpuRenderError> {
    let expected_len = texture_byte_len(chunk.size, atlas.format)?;
    if chunk.pixels.len() != expected_len {
        return Err(GpuRenderError::InputBufferSizeMismatch {
            expected: expected_len,
            actual: chunk.pixels.len(),
        });
    }
    let right = chunk
        .atlas_x
        .checked_add(chunk.size.width)
        .ok_or(GpuRenderError::TextureSizeOverflow)?;
    let bottom = chunk
        .atlas_y
        .checked_add(chunk.size.height)
        .ok_or(GpuRenderError::TextureSizeOverflow)?;
    if right > atlas.size.width || bottom > atlas.size.height {
        return Err(GpuRenderError::UploadRegionOutOfBounds {
            texture_size: atlas.size,
            origin_x: chunk.atlas_x,
            origin_y: chunk.atlas_y,
            upload_size: chunk.size,
        });
    }
    Ok(())
}

fn texture_byte_len(
    size: CanvasSize,
    format: GpuSparseAtlasFormat,
) -> Result<usize, GpuRenderError> {
    if size.width == 0 || size.height == 0 {
        return Err(GpuRenderError::InvalidImageSize);
    }
    usize::try_from(
        u64::from(size.width)
            .checked_mul(u64::from(size.height))
            .and_then(|pixels| pixels.checked_mul(u64::from(format.bytes_per_pixel())))
            .ok_or(GpuRenderError::TextureSizeOverflow)?,
    )
    .map_err(|_| GpuRenderError::TextureSizeOverflow)
}
