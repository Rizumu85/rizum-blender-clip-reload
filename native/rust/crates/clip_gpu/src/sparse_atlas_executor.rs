use clip_model::CanvasSize;

use crate::pass::{WHITE_TRANSPARENT, create_rgba8_texture};
use crate::sparse_atlas_prepare::{
    PreparedSparseAtlasRasterEvents, prepare_sparse_atlas_raster_events,
};
use crate::stream_bounds::CanvasRect;
use crate::stream_tile_event::TileEventProgram;
use crate::stream_tile_silo_buffers::{
    create_params_buffer, create_tile_event_storage_buffers, create_u32_storage_buffer,
};
use crate::stream_tile_silo_pipeline::TileSiloPipeline;
use crate::stream_tile_silo_upload::{upload_lut_atlas_texture, upload_mask_atlas_tile_texture};
use crate::{
    GpuRasterBlendMode, GpuRasterStackOutput, GpuRenderError, GpuRenderer,
    GpuSparseAtlasRasterEventBatch, GpuSparseAtlasTextureKey, GpuSparseAtlasTexturePool,
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
        self.draw_prepared_sparse_atlas_raster_batches_to_rgba8(
            output_size,
            &prepared_batches,
            None,
        )
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

        self.draw_prepared_sparse_atlas_raster_batches_to_rgba8(
            output_size,
            &prepared_batches,
            None,
        )
    }

    pub fn draw_sparse_atlas_raster_event_batches_over_rgba8(
        &self,
        output_size: CanvasSize,
        pool: &GpuSparseAtlasTexturePool,
        batches: &[GpuSparseAtlasRasterEventBatch],
        base_pixels: &[u8],
    ) -> Result<GpuRasterStackOutput, GpuRenderError> {
        if output_size.width == 0 || output_size.height == 0 {
            return Err(GpuRenderError::InvalidImageSize);
        }
        let expected_len = rgba8_texture_byte_len(output_size)?;
        if base_pixels.len() != expected_len {
            return Err(GpuRenderError::InputBufferSizeMismatch {
                expected: expected_len,
                actual: base_pixels.len(),
            });
        }
        let prepared_batches = batches
            .iter()
            .filter(|batch| !batch.events.is_empty())
            .map(|batch| prepare_sparse_atlas_raster_events(output_size, pool, &batch.events))
            .collect::<Result<Vec<_>, _>>()?;
        if prepared_batches.is_empty() {
            return Ok(GpuRasterStackOutput {
                drawn_resources: Vec::new(),
                size: output_size,
                pixels: base_pixels.to_vec(),
            });
        }

        self.draw_prepared_sparse_atlas_raster_batches_to_rgba8(
            output_size,
            &prepared_batches,
            Some(base_pixels),
        )
    }

    fn draw_prepared_sparse_atlas_raster_batches_to_rgba8(
        &self,
        output_size: CanvasSize,
        prepared_batches: &[PreparedSparseAtlasRasterEvents<'_>],
        base_pixels: Option<&[u8]>,
    ) -> Result<GpuRasterStackOutput, GpuRenderError> {
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST;
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
        initialize_sparse_atlas_executor_targets(
            self,
            &mut encoder,
            &previous,
            &previous_view,
            &output_view,
            output_size,
            base_pixels,
        )?;

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
                if base_pixels.is_none() && drawable_batches.len() == 1 {
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

fn rgba8_texture_byte_len(size: CanvasSize) -> Result<usize, GpuRenderError> {
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

fn initialize_sparse_atlas_executor_targets(
    renderer: &GpuRenderer,
    encoder: &mut wgpu::CommandEncoder,
    previous: &wgpu::Texture,
    previous_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    output_size: CanvasSize,
    base_pixels: Option<&[u8]>,
) -> Result<(), GpuRenderError> {
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
    Ok(())
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
