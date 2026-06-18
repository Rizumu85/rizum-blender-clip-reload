use clip_model::CanvasSize;

use super::{
    MaskUploadPayload, PlannedMaskResourceMeta, RuntimeError, RuntimeGpuResourceProvider,
    read_mask_payload_for_upload, rgba_byte_len,
};

impl RuntimeGpuResourceProvider<'_> {
    pub(super) fn build_raster_run_atlas_tile_pixels(
        &mut self,
        sources: &[clip_gpu::GpuRasterAtlasSource],
        atlas_size: CanvasSize,
    ) -> Result<Option<clip_gpu::GpuRasterAtlasTilePixels>, RuntimeError> {
        if sources.is_empty() {
            return Ok(None);
        }

        let mut chunks = Vec::new();
        let mut resources = Vec::with_capacity(sources.len());
        for request in sources {
            if request.source.mask_key.is_some()
                && request.source.blend_mode != clip_gpu::GpuRasterBlendMode::Normal
            {
                return Ok(None);
            }
            let meta = self
                .plan
                .rasters
                .get(&request.source.key)
                .cloned()
                .ok_or_else(|| {
                    RuntimeError::Gpu(clip_gpu::GpuRenderError::MissingRasterResource {
                        layer_id: request.source.key.layer_id,
                        render_mipmap_id: request.source.key.render_mipmap_id,
                    })
                })?;
            let visible = self
                .decode_region_for_source(request.source, &meta.source, None)?
                .ok_or(clip_gpu::GpuRenderError::InvalidImageSize)?;
            if request.size
                != CanvasSize::new(visible.source_rect.width, visible.source_rect.height)
                || request.offset_x != visible.offset_x
                || request.offset_y != visible.offset_y
            {
                return Ok(None);
            }
            let mask_payload = match request.source.mask_key {
                Some(mask_key) => {
                    let mask_meta = self.plan.masks.get(&mask_key).cloned().ok_or_else(|| {
                        RuntimeError::Gpu(clip_gpu::GpuRenderError::MissingMaskResource {
                            layer_id: mask_key.layer_id,
                            mask_mipmap_id: mask_key.mask_mipmap_id,
                        })
                    })?;
                    let payload = read_mask_payload_for_upload(
                        self.container,
                        self.canvas,
                        &mask_meta.source,
                        None,
                    )?;
                    self.report_mask_payload_info(mask_key, &mask_meta, &payload);
                    Some(payload)
                }
                None => None,
            };

            let source_chunks =
                clip_file::read_resolved_raster_layer_source_rgba_region_atlas_chunks_from_container(
                    self.container,
                    &meta.source,
                    visible.source_rect,
                    atlas_size,
                    request.atlas_x,
                    request.atlas_y,
                )?;
            for chunk in source_chunks {
                let local_x = chunk
                    .x
                    .checked_sub(request.atlas_x)
                    .ok_or(clip_gpu::GpuRenderError::TextureSizeOverflow)?;
                let local_y = chunk
                    .y
                    .checked_sub(request.atlas_y)
                    .ok_or(clip_gpu::GpuRenderError::TextureSizeOverflow)?;
                let offset_x = i32::try_from(i64::from(visible.offset_x) + i64::from(local_x))
                    .map_err(|_| clip_gpu::GpuRenderError::TextureSizeOverflow)?;
                let offset_y = i32::try_from(i64::from(visible.offset_y) + i64::from(local_y))
                    .map_err(|_| clip_gpu::GpuRenderError::TextureSizeOverflow)?;
                let size = CanvasSize::new(chunk.width, chunk.height);
                let mut pixels = chunk.pixels;
                if let Some(mask_payload) = &mask_payload {
                    apply_mask_to_rgba_chunk(
                        &mut pixels,
                        size,
                        (offset_x, offset_y),
                        mask_payload,
                    )?;
                }
                chunks.push(clip_gpu::GpuRasterAtlasTileChunk {
                    source: request.source,
                    atlas_x: chunk.x,
                    atlas_y: chunk.y,
                    size,
                    offset_x,
                    offset_y,
                    pixels,
                });
            }

            self.raster_offsets
                .insert(request.source.key, (visible.offset_x, visible.offset_y));
            resources.push(clip_gpu::GpuRasterResourceInfo {
                key: request.source.key,
                render_node_id: meta.render_node_id,
                size: request.size,
                byte_len: rgba_byte_len(request.size)?,
            });
        }

        Ok(Some(clip_gpu::GpuRasterAtlasTilePixels {
            size: atlas_size,
            chunks,
            resources,
        }))
    }

    fn report_mask_payload_info(
        &mut self,
        key: clip_gpu::GpuMaskResourceKey,
        meta: &PlannedMaskResourceMeta,
        payload: &MaskUploadPayload,
    ) {
        if self.reported_masks.insert(key) {
            self.mask_resources.push(clip_gpu::GpuMaskResourceInfo {
                key,
                render_node_id: meta.render_node_id,
                size: CanvasSize::new(payload.image.width, payload.image.height),
                origin_x: payload.origin_x,
                origin_y: payload.origin_y,
                fill_value: payload.fill_value,
                byte_len: payload.image.pixels.len(),
            });
        }
    }
}

fn apply_mask_to_rgba_chunk(
    pixels: &mut [u8],
    size: CanvasSize,
    canvas_offset: (i32, i32),
    mask: &MaskUploadPayload,
) -> Result<(), RuntimeError> {
    let width =
        usize::try_from(size.width).map_err(|_| clip_gpu::GpuRenderError::TextureSizeOverflow)?;
    let height =
        usize::try_from(size.height).map_err(|_| clip_gpu::GpuRenderError::TextureSizeOverflow)?;
    let expected_len = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(clip_gpu::GpuRenderError::TextureSizeOverflow)?;
    if pixels.len() != expected_len {
        return Err(clip_gpu::GpuRenderError::InputBufferSizeMismatch {
            expected: expected_len,
            actual: pixels.len(),
        }
        .into());
    }
    let mask_width = usize::try_from(mask.image.width)
        .map_err(|_| clip_gpu::GpuRenderError::TextureSizeOverflow)?;
    let mask_height = usize::try_from(mask.image.height)
        .map_err(|_| clip_gpu::GpuRenderError::TextureSizeOverflow)?;
    for y in 0..height {
        let global_y = canvas_offset
            .1
            .checked_add(
                i32::try_from(y).map_err(|_| clip_gpu::GpuRenderError::TextureSizeOverflow)?,
            )
            .ok_or(clip_gpu::GpuRenderError::TextureSizeOverflow)?;
        for x in 0..width {
            let global_x = canvas_offset
                .0
                .checked_add(
                    i32::try_from(x).map_err(|_| clip_gpu::GpuRenderError::TextureSizeOverflow)?,
                )
                .ok_or(clip_gpu::GpuRenderError::TextureSizeOverflow)?;
            let mask_value = mask_value_at(mask, mask_width, mask_height, global_x, global_y);
            let alpha_index = (y * width + x) * 4 + 3;
            pixels[alpha_index] =
                ((u16::from(pixels[alpha_index]) * u16::from(mask_value)) / 255) as u8;
        }
    }
    Ok(())
}

fn mask_value_at(
    mask: &MaskUploadPayload,
    mask_width: usize,
    mask_height: usize,
    global_x: i32,
    global_y: i32,
) -> u8 {
    let local_x = i64::from(global_x) - i64::from(mask.origin_x);
    let local_y = i64::from(global_y) - i64::from(mask.origin_y);
    if local_x < 0 || local_y < 0 {
        return mask.fill_value;
    }
    let (Ok(local_x), Ok(local_y)) = (usize::try_from(local_x), usize::try_from(local_y)) else {
        return mask.fill_value;
    };
    if local_x >= mask_width || local_y >= mask_height {
        return mask.fill_value;
    }
    mask.image.pixels[local_y * mask_width + local_x]
}
