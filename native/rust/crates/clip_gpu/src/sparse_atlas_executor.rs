use clip_model::CanvasSize;

use crate::pass::{WHITE_TRANSPARENT, create_rgba8_texture};
use crate::stream_bounds::{CanvasRect, union_optional};
use crate::stream_tile_event::{RasterTileEventPayload, TileEventProgram};
use crate::stream_tile_silo_buffers::{
    create_params_buffer, create_tile_event_storage_buffers, create_u32_storage_buffer,
};
use crate::stream_tile_silo_pipeline::TileSiloPipeline;
use crate::stream_tile_silo_plan::{TILE_SIZE, tile_work_lists_for_bounds};
use crate::stream_tile_silo_upload::{upload_lut_atlas_texture, upload_mask_atlas_tile_texture};
use crate::{
    GpuRasterBlendMode, GpuRasterStackOutput, GpuRenderError, GpuRenderer, GpuSparseAtlasFormat,
    GpuSparseAtlasRasterEventBatch, GpuSparseAtlasTexture, GpuSparseAtlasTextureKey,
    GpuSparseAtlasTexturePool,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuSparseAtlasTileRef {
    pub key: GpuSparseAtlasTextureKey,
    pub atlas_x: u32,
    pub atlas_y: u32,
    pub size: CanvasSize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuSparseAtlasRasterEvent {
    pub raster: GpuSparseAtlasTileRef,
    pub source_offset_x: i32,
    pub source_offset_y: i32,
    pub opacity: f32,
    pub blend_mode: GpuRasterBlendMode,
    pub mask: Option<GpuSparseAtlasTileRef>,
}

impl GpuRenderer {
    pub fn draw_sparse_atlas_raster_events_to_rgba8(
        &self,
        output_size: CanvasSize,
        pool: &GpuSparseAtlasTexturePool,
        events: &[GpuSparseAtlasRasterEvent],
    ) -> Result<GpuRasterStackOutput, GpuRenderError> {
        if output_size.width == 0 || output_size.height == 0 {
            return Err(GpuRenderError::InvalidImageSize);
        }
        if events.is_empty() {
            return Err(GpuRenderError::EmptyRasterStack);
        }

        let prepared_batches = [prepare_sparse_atlas_raster_events(
            output_size,
            pool,
            events,
        )?];
        self.draw_prepared_sparse_atlas_raster_batches_to_rgba8(output_size, &prepared_batches)
    }

    pub fn draw_sparse_atlas_raster_event_batches_to_rgba8(
        &self,
        output_size: CanvasSize,
        pool: &GpuSparseAtlasTexturePool,
        batches: &[GpuSparseAtlasRasterEventBatch],
    ) -> Result<GpuRasterStackOutput, GpuRenderError> {
        if output_size.width == 0 || output_size.height == 0 {
            return Err(GpuRenderError::InvalidImageSize);
        }
        let prepared_batches = batches
            .iter()
            .filter(|batch| !batch.events.is_empty())
            .map(|batch| prepare_sparse_atlas_raster_events(output_size, pool, &batch.events))
            .collect::<Result<Vec<_>, _>>()?;
        if prepared_batches.is_empty() {
            return Err(GpuRenderError::EmptyRasterStack);
        }

        self.draw_prepared_sparse_atlas_raster_batches_to_rgba8(output_size, &prepared_batches)
    }

    fn draw_prepared_sparse_atlas_raster_batches_to_rgba8(
        &self,
        output_size: CanvasSize,
        prepared_batches: &[PreparedSparseAtlasRasterEvents<'_>],
    ) -> Result<GpuRasterStackOutput, GpuRenderError> {
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC;
        let previous = create_rgba8_texture(
            &self.context.device,
            "rizum_clip_sparse_atlas_previous",
            output_size,
            usage,
        );
        let output = create_rgba8_texture(
            &self.context.device,
            "rizum_clip_sparse_atlas_output",
            output_size,
            usage,
        );
        let previous_view = previous.create_view(&wgpu::TextureViewDescriptor::default());
        let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder =
            self.context
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("rizum_clip_sparse_atlas_executor_encoder"),
                });
        clear_sparse_atlas_executor_targets(&mut encoder, &previous_view, &output_view);

        let drawable_batches = prepared_batches
            .iter()
            .filter(|prepared| !prepared.payloads.is_empty())
            .collect::<Vec<_>>();
        let full_bounds = CanvasRect::full(output_size).ok_or(GpuRenderError::InvalidImageSize)?;
        for (index, prepared) in drawable_batches.iter().enumerate() {
            let (input_view, target_view) = if index % 2 == 0 {
                (&previous_view, &output_view)
            } else {
                (&output_view, &previous_view)
            };
            self.encode_sparse_atlas_raster_events(
                &mut encoder,
                prepared,
                input_view,
                target_view,
                if drawable_batches.len() == 1 {
                    prepared.pass_bounds
                } else {
                    full_bounds
                },
            )?;
        }

        self.context.queue.submit([encoder.finish()]);
        let final_texture = if drawable_batches.len() % 2 == 0 {
            &previous
        } else {
            &output
        };
        let pixels =
            self.read_texture_rgba8(final_texture, output_size.width, output_size.height)?;
        Ok(GpuRasterStackOutput {
            drawn_resources: Vec::new(),
            size: output_size,
            pixels,
        })
    }

    fn encode_sparse_atlas_raster_events(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        prepared: &PreparedSparseAtlasRasterEvents<'_>,
        previous_view: &wgpu::TextureView,
        output_view: &wgpu::TextureView,
        pass_bounds: CanvasRect,
    ) -> Result<(), GpuRenderError> {
        let atlas_view = prepared
            .atlas
            .texture()
            .create_view(&wgpu::TextureViewDescriptor::default());
        let (owned_mask_atlas, mask_atlas_view) = match prepared.mask_atlas {
            Some(mask_atlas) => (
                None,
                mask_atlas
                    .texture()
                    .create_view(&wgpu::TextureViewDescriptor::default()),
            ),
            None => {
                let (texture, _) =
                    upload_mask_atlas_tile_texture(self, CanvasSize::new(1, 1), &[])?;
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                (Some(texture), view)
            }
        };
        let _owned_mask_atlas = owned_mask_atlas;
        let (lut_atlas, _) = upload_lut_atlas_texture(self, &[])?;
        let lut_atlas_view = lut_atlas.create_view(&wgpu::TextureViewDescriptor::default());
        let event_program = TileEventProgram::from_raster_payloads(prepared.payloads.clone());
        let event_buffers = create_tile_event_storage_buffers(
            &self.context.device,
            "rizum_clip_sparse_atlas_event_headers",
            "rizum_clip_sparse_atlas_raster_payloads",
            &event_program,
        );
        let work_buffer = create_u32_storage_buffer(
            &self.context.device,
            "rizum_clip_sparse_atlas_work_indices",
            &prepared.work_indices,
        );
        let span_buffer = create_u32_storage_buffer(
            &self.context.device,
            "rizum_clip_sparse_atlas_spans",
            &prepared.tile_spans,
        );
        let params_buffer = create_params_buffer(&self.context.device, (0, 0), prepared.tile_cols);
        let pipeline = TileSiloPipeline::new(&self.context.device);
        let bind_group = self
            .context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("rizum_clip_sparse_atlas_bind_group"),
                layout: &pipeline.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&atlas_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(previous_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: event_buffers.headers.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: work_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: span_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: params_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::TextureView(&mask_atlas_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: event_buffers.raster_payloads.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: event_buffers.filter_payloads.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 9,
                        resource: wgpu::BindingResource::TextureView(&lut_atlas_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 10,
                        resource: event_buffers.scope_payloads.as_entire_binding(),
                    },
                ],
            });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rizum_clip_sparse_atlas_raster_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline.render_pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_scissor_rect(
            pass_bounds.x,
            pass_bounds.y,
            pass_bounds.width,
            pass_bounds.height,
        );
        pass.draw(0..3, 0..1);
        Ok(())
    }
}

struct PreparedSparseAtlasRasterEvents<'a> {
    atlas: &'a GpuSparseAtlasTexture,
    mask_atlas: Option<&'a GpuSparseAtlasTexture>,
    payloads: Vec<RasterTileEventPayload>,
    work_indices: Vec<u32>,
    tile_spans: Vec<u32>,
    tile_cols: u32,
    pass_bounds: CanvasRect,
}

fn prepare_sparse_atlas_raster_events<'a>(
    output_size: CanvasSize,
    pool: &'a GpuSparseAtlasTexturePool,
    events: &[GpuSparseAtlasRasterEvent],
) -> Result<PreparedSparseAtlasRasterEvents<'a>, GpuRenderError> {
    let raster_key = common_raster_atlas_key(events)?;
    let atlas = pool
        .texture(raster_key)
        .ok_or(GpuRenderError::MissingSparseAtlasTexture { key: raster_key })?;
    validate_sparse_atlas_format(atlas, GpuSparseAtlasFormat::Rgba8)?;
    let mask_key = common_mask_atlas_key(events)?;
    let mask_atlas = match mask_key {
        Some(key) => {
            let atlas = pool
                .texture(key)
                .ok_or(GpuRenderError::MissingSparseAtlasTexture { key })?;
            validate_sparse_atlas_format(atlas, GpuSparseAtlasFormat::R8)?;
            Some(atlas)
        }
        None => None,
    };

    let mut payloads = Vec::new();
    let mut bounds = Vec::new();
    let mut pass_bounds = None;
    for event in events {
        validate_tile_ref(atlas, event.raster)?;
        if let (Some(mask_atlas), Some(mask)) = (mask_atlas, event.mask) {
            validate_tile_ref(mask_atlas, mask)?;
        }
        let Some(source_bounds) = CanvasRect::from_source(
            event.source_offset_x,
            event.source_offset_y,
            event.raster.size,
            output_size,
        ) else {
            continue;
        };
        pass_bounds = union_optional(pass_bounds, Some(source_bounds));
        bounds.push(source_bounds);
        payloads.push(RasterTileEventPayload {
            atlas_origin: (event.raster.atlas_x, event.raster.atlas_y),
            source_size: event.raster.size,
            source_offset: (event.source_offset_x, event.source_offset_y),
            opacity: event.opacity,
            blend_mode: event.blend_mode,
            mask_atlas_origin: event.mask.map(|mask| (mask.atlas_x, mask.atlas_y)),
        });
    }
    let pass_bounds = match pass_bounds {
        Some(bounds) => bounds,
        None => CanvasRect::full(output_size).ok_or(GpuRenderError::InvalidImageSize)?,
    };
    let tile_cols = output_size.width.div_ceil(TILE_SIZE);
    let tile_count =
        usize::try_from(u64::from(tile_cols) * u64::from(output_size.height.div_ceil(TILE_SIZE)))
            .map_err(|_| GpuRenderError::TextureSizeOverflow)?;
    let (work_indices, tile_spans) = tile_work_lists_for_bounds(tile_count, tile_cols, &bounds)?;
    Ok(PreparedSparseAtlasRasterEvents {
        atlas,
        mask_atlas,
        payloads,
        work_indices,
        tile_spans,
        tile_cols,
        pass_bounds,
    })
}

fn common_raster_atlas_key(
    events: &[GpuSparseAtlasRasterEvent],
) -> Result<GpuSparseAtlasTextureKey, GpuRenderError> {
    let key = events[0].raster.key;
    if key.format != GpuSparseAtlasFormat::Rgba8 {
        return Err(GpuRenderError::SparseAtlasFormatMismatch {
            expected: GpuSparseAtlasFormat::Rgba8,
            actual: key.format,
        });
    }
    if events.iter().any(|event| event.raster.key != key) {
        return Err(GpuRenderError::SparseAtlasMixedTextureKeys);
    }
    Ok(key)
}

fn common_mask_atlas_key(
    events: &[GpuSparseAtlasRasterEvent],
) -> Result<Option<GpuSparseAtlasTextureKey>, GpuRenderError> {
    let mut key = None;
    for mask in events.iter().filter_map(|event| event.mask) {
        if mask.key.format != GpuSparseAtlasFormat::R8 {
            return Err(GpuRenderError::SparseAtlasFormatMismatch {
                expected: GpuSparseAtlasFormat::R8,
                actual: mask.key.format,
            });
        }
        if let Some(existing) = key {
            if existing != mask.key {
                return Err(GpuRenderError::SparseAtlasMixedTextureKeys);
            }
        } else {
            key = Some(mask.key);
        }
    }
    Ok(key)
}

fn validate_sparse_atlas_format(
    atlas: &GpuSparseAtlasTexture,
    expected: GpuSparseAtlasFormat,
) -> Result<(), GpuRenderError> {
    if atlas.format() != expected {
        return Err(GpuRenderError::SparseAtlasFormatMismatch {
            expected,
            actual: atlas.format(),
        });
    }
    Ok(())
}

fn validate_tile_ref(
    atlas: &GpuSparseAtlasTexture,
    tile: GpuSparseAtlasTileRef,
) -> Result<(), GpuRenderError> {
    let right = tile
        .atlas_x
        .checked_add(tile.size.width)
        .ok_or(GpuRenderError::TextureSizeOverflow)?;
    let bottom = tile
        .atlas_y
        .checked_add(tile.size.height)
        .ok_or(GpuRenderError::TextureSizeOverflow)?;
    if right > atlas.size().width || bottom > atlas.size().height {
        return Err(GpuRenderError::UploadRegionOutOfBounds {
            texture_size: atlas.size(),
            origin_x: tile.atlas_x,
            origin_y: tile.atlas_y,
            upload_size: tile.size,
        });
    }
    Ok(())
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
