use clip_model::CanvasSize;

use crate::stream_tile_silo_plan::PreparedSiloSource;
use crate::{
    GpuMaskAtlasTileChunk, GpuRasterAtlasPixels, GpuRasterAtlasTilePixels, GpuRenderError,
    GpuRenderer,
};

pub(crate) fn create_atlas_texture(device: &wgpu::Device, size: CanvasSize) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rizum_clip_tile_silo_atlas"),
        size: wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn create_mask_atlas_texture(device: &wgpu::Device, size: CanvasSize) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rizum_clip_tile_silo_mask_atlas"),
        size: wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

pub(crate) fn upload_atlas_texture(
    renderer: &GpuRenderer,
    upload: &GpuRasterAtlasPixels,
) -> Result<wgpu::Texture, GpuRenderError> {
    let expected_len = rgba8_texture_byte_len(upload.size)?;
    if upload.pixels.len() != expected_len {
        return Err(GpuRenderError::InputBufferSizeMismatch {
            expected: expected_len,
            actual: upload.pixels.len(),
        });
    }

    let texture = create_atlas_texture(&renderer.context.device, upload.size);
    renderer.context.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &upload.pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(upload.size.width * 4),
            rows_per_image: Some(upload.size.height),
        },
        wgpu::Extent3d {
            width: upload.size.width,
            height: upload.size.height,
            depth_or_array_layers: 1,
        },
    );
    Ok(texture)
}

pub(crate) fn upload_atlas_tile_texture(
    renderer: &GpuRenderer,
    upload: &GpuRasterAtlasTilePixels,
) -> Result<wgpu::Texture, GpuRenderError> {
    let texture = create_atlas_texture(&renderer.context.device, upload.size);
    for chunk in &upload.chunks {
        let expected_len = rgba8_texture_byte_len(chunk.size)?;
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
        if right > upload.size.width || bottom > upload.size.height {
            return Err(GpuRenderError::UploadRegionOutOfBounds {
                texture_size: upload.size,
                origin_x: chunk.atlas_x,
                origin_y: chunk.atlas_y,
                upload_size: chunk.size,
            });
        }
        renderer.context.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
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
                bytes_per_row: Some(chunk.size.width * 4),
                rows_per_image: Some(chunk.size.height),
            },
            wgpu::Extent3d {
                width: chunk.size.width,
                height: chunk.size.height,
                depth_or_array_layers: 1,
            },
        );
    }
    Ok(texture)
}

pub(crate) fn upload_mask_atlas_tile_texture(
    renderer: &GpuRenderer,
    atlas_size: CanvasSize,
    chunks: &[GpuMaskAtlasTileChunk],
) -> Result<(wgpu::Texture, usize), GpuRenderError> {
    if chunks.is_empty() {
        let texture = create_mask_atlas_texture(&renderer.context.device, CanvasSize::new(1, 1));
        renderer.context.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(1),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        return Ok((texture, 1));
    }

    let texture = create_mask_atlas_texture(&renderer.context.device, atlas_size);
    for chunk in chunks {
        write_mask_chunk(renderer, &texture, atlas_size, chunk)?;
    }
    Ok((texture, r8_texture_byte_len(atlas_size)?))
}

pub(crate) fn rgba8_texture_byte_len(size: CanvasSize) -> Result<usize, GpuRenderError> {
    if size.width == 0 || size.height == 0 {
        return Err(GpuRenderError::InvalidImageSize);
    }
    usize::try_from(
        u64::from(size.width)
            .checked_mul(u64::from(size.height))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(GpuRenderError::TextureSizeOverflow)?,
    )
    .map_err(|_| GpuRenderError::TextureSizeOverflow)
}

fn r8_texture_byte_len(size: CanvasSize) -> Result<usize, GpuRenderError> {
    if size.width == 0 || size.height == 0 {
        return Err(GpuRenderError::InvalidImageSize);
    }
    usize::try_from(
        u64::from(size.width)
            .checked_mul(u64::from(size.height))
            .ok_or(GpuRenderError::TextureSizeOverflow)?,
    )
    .map_err(|_| GpuRenderError::TextureSizeOverflow)
}

fn write_mask_chunk(
    renderer: &GpuRenderer,
    texture: &wgpu::Texture,
    atlas_size: CanvasSize,
    chunk: &GpuMaskAtlasTileChunk,
) -> Result<(), GpuRenderError> {
    let expected_len = r8_texture_byte_len(chunk.size)?;
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
    if right > atlas_size.width || bottom > atlas_size.height {
        return Err(GpuRenderError::UploadRegionOutOfBounds {
            texture_size: atlas_size,
            origin_x: chunk.atlas_x,
            origin_y: chunk.atlas_y,
            upload_size: chunk.size,
        });
    }
    renderer.context.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
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
            bytes_per_row: Some(chunk.size.width),
            rows_per_image: Some(chunk.size.height),
        },
        wgpu::Extent3d {
            width: chunk.size.width,
            height: chunk.size.height,
            depth_or_array_layers: 1,
        },
    );
    Ok(())
}

pub(crate) fn copy_sources_to_atlas(
    encoder: &mut wgpu::CommandEncoder,
    sources: &[PreparedSiloSource],
    atlas: &wgpu::Texture,
) {
    for source in sources {
        let resource = source
            .cache
            .as_ref()
            .expect("copied silo source must retain a raster cache")
            .resource(source.source.key)
            .expect("prepared source cache must contain raster resource");
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: resource.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: atlas,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: source.atlas.x,
                    y: source.atlas.y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: source.info.size.width,
                height: source.info.size.height,
                depth_or_array_layers: 1,
            },
        );
    }
}
