use clip_model::{CanvasSize, Rect};

use crate::pass::WHITE_TRANSPARENT;
use crate::{GpuRenderError, GpuRenderer};

pub(crate) struct SparseAtlasRenderedTextures {
    previous: wgpu::Texture,
    output: wgpu::Texture,
    drawable_batch_count: usize,
}

impl SparseAtlasRenderedTextures {
    pub(crate) fn new(
        previous: wgpu::Texture,
        output: wgpu::Texture,
        drawable_batch_count: usize,
    ) -> Self {
        Self {
            previous,
            output,
            drawable_batch_count,
        }
    }

    pub(crate) fn final_texture(&self) -> &wgpu::Texture {
        if self.drawable_batch_count % 2 == 0 {
            &self.previous
        } else {
            &self.output
        }
    }
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

pub(crate) fn rgba8_patch_pixels(
    image_size: CanvasSize,
    pixels: &[u8],
    rects: &[Rect],
) -> Result<Vec<u8>, GpuRenderError> {
    let stride = usize::try_from(
        u64::from(image_size.width)
            .checked_mul(4)
            .ok_or(GpuRenderError::TextureSizeOverflow)?,
    )
    .map_err(|_| GpuRenderError::TextureSizeOverflow)?;
    let mut output = Vec::new();
    for rect in rects.iter().copied().filter(|rect| !rect.is_empty()) {
        let bottom = validate_patch_rect(image_size, rect)?;
        let row_bytes = usize::try_from(
            u64::from(rect.width)
                .checked_mul(4)
                .ok_or(GpuRenderError::TextureSizeOverflow)?,
        )
        .map_err(|_| GpuRenderError::TextureSizeOverflow)?;
        let x_bytes = usize::try_from(
            u64::from(rect.x)
                .checked_mul(4)
                .ok_or(GpuRenderError::TextureSizeOverflow)?,
        )
        .map_err(|_| GpuRenderError::TextureSizeOverflow)?;
        for row in rect.y..bottom {
            let row_start = usize::try_from(row)
                .map_err(|_| GpuRenderError::TextureSizeOverflow)?
                .checked_mul(stride)
                .ok_or(GpuRenderError::TextureSizeOverflow)?;
            let start = row_start
                .checked_add(x_bytes)
                .ok_or(GpuRenderError::TextureSizeOverflow)?;
            let end = start
                .checked_add(row_bytes)
                .ok_or(GpuRenderError::TextureSizeOverflow)?;
            let row_pixels =
                pixels
                    .get(start..end)
                    .ok_or(GpuRenderError::InputBufferSizeMismatch {
                        expected: rgba8_texture_byte_len(image_size)?,
                        actual: pixels.len(),
                    })?;
            output.extend_from_slice(row_pixels);
        }
    }
    Ok(output)
}

fn validate_patch_rect(image_size: CanvasSize, rect: Rect) -> Result<u32, GpuRenderError> {
    let right = rect
        .x
        .checked_add(rect.width)
        .ok_or(GpuRenderError::TextureSizeOverflow)?;
    let bottom = rect
        .y
        .checked_add(rect.height)
        .ok_or(GpuRenderError::TextureSizeOverflow)?;
    if right > image_size.width || bottom > image_size.height {
        return Err(GpuRenderError::ReadbackRegionOutOfBounds {
            texture_size: image_size,
            origin_x: rect.x,
            origin_y: rect.y,
            read_size: CanvasSize::new(rect.width, rect.height),
        });
    }
    Ok(bottom)
}

pub(crate) fn initialize_sparse_atlas_executor_targets(
    renderer: &GpuRenderer,
    encoder: &mut wgpu::CommandEncoder,
    previous: &wgpu::Texture,
    previous_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    output_size: CanvasSize,
    base_pixels: Option<&[u8]>,
) {
    if let Some(base_pixels) = base_pixels {
        renderer.context.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: previous,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            base_pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(output_size.width * 4),
                rows_per_image: Some(output_size.height),
            },
            wgpu::Extent3d {
                width: output_size.width,
                height: output_size.height,
                depth_or_array_layers: 1,
            },
        );
        clear_sparse_atlas_executor_output_target(encoder, output_view);
    } else {
        clear_sparse_atlas_executor_targets(encoder, previous_view, output_view);
    }
}

fn clear_sparse_atlas_executor_output_target(
    encoder: &mut wgpu::CommandEncoder,
    output_view: &wgpu::TextureView,
) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("rizum_clip_sparse_atlas_clear_output"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: output_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(WHITE_TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        timestamp_writes: None,
        multiview_mask: None,
    });
}

fn clear_sparse_atlas_executor_targets(
    encoder: &mut wgpu::CommandEncoder,
    previous_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("rizum_clip_sparse_atlas_clear"),
        color_attachments: &[
            Some(wgpu::RenderPassColorAttachment {
                view: previous_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(WHITE_TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            }),
            Some(wgpu::RenderPassColorAttachment {
                view: output_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(WHITE_TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            }),
        ],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        timestamp_writes: None,
        multiview_mask: None,
    });
}
