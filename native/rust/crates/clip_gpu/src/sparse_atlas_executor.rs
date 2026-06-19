use clip_model::{CanvasSize, Rect};

use crate::blend::blend_kind;
use crate::pass::create_rgba8_texture;
use crate::sparse_atlas_prepare::{
    PreparedSparseAtlasRasterEvents, prepare_sparse_atlas_raster_event_batch,
    prepare_sparse_atlas_raster_events,
};
use crate::sparse_atlas_targets::{
    SparseAtlasRenderedTextures, initialize_sparse_atlas_executor_targets, rgba8_patch_pixels,
    rgba8_texture_byte_len,
};
use crate::stream_bounds::CanvasRect;
use crate::stream_tile_event::TileEventProgram;
use crate::stream_tile_silo_buffers::{
    create_params_buffer_with_mode_and_resolve, create_tile_event_storage_buffers,
    create_u32_storage_buffer,
};
use crate::stream_tile_silo_pipeline::TileSiloPipeline;
use crate::stream_tile_silo_upload::{upload_lut_atlas_texture, upload_mask_atlas_tile_texture};
use crate::{
    GpuRasterBlendMode, GpuRasterPatchOutput, GpuRasterStackOutput, GpuRenderError, GpuRenderer,
    GpuSparseAtlasRasterEventBatch, GpuSparseAtlasRasterEventBatchKind, GpuSparseAtlasTextureKey,
    GpuSparseAtlasTexturePool,
};

const TILE_SILO_MODE_NORMAL: u32 = 0;
const TILE_SILO_MODE_CLIPPING_RUN: u32 = 2;

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

#[derive(Clone, Debug, PartialEq)]
pub struct GpuSparseAtlasPointFilterEvent {
    pub lut_rgba: Vec<u8>,
    pub opacity: f32,
    pub filter_mode: crate::GpuLutFilterMode,
    pub local_bounds: Rect,
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
            .filter(|batch| !batch.is_empty())
            .map(|batch| prepare_sparse_atlas_raster_event_batch(output_size, pool, batch))
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
            .filter(|batch| !batch.is_empty())
            .map(|batch| prepare_sparse_atlas_raster_event_batch(output_size, pool, batch))
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

    pub fn draw_sparse_atlas_raster_event_batch_patches_over_rgba8(
        &self,
        output_size: CanvasSize,
        pool: &GpuSparseAtlasTexturePool,
        batches: &[GpuSparseAtlasRasterEventBatch],
        base_pixels: &[u8],
        rects: &[Rect],
    ) -> Result<GpuRasterPatchOutput, GpuRenderError> {
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
            .filter(|batch| !batch.is_empty())
            .map(|batch| prepare_sparse_atlas_raster_event_batch(output_size, pool, batch))
            .collect::<Result<Vec<_>, _>>()?;
        if prepared_batches.is_empty() {
            return Ok(GpuRasterPatchOutput {
                size: output_size,
                payload: rgba8_patch_pixels(output_size, base_pixels, rects)?,
            });
        }

        self.draw_prepared_sparse_atlas_raster_batch_patches_over_rgba8(
            output_size,
            &prepared_batches,
            base_pixels,
            rects,
        )
    }

    fn draw_prepared_sparse_atlas_raster_batches_to_rgba8(
        &self,
        output_size: CanvasSize,
        prepared_batches: &[PreparedSparseAtlasRasterEvents<'_>],
        base_pixels: Option<&[u8]>,
    ) -> Result<GpuRasterStackOutput, GpuRenderError> {
        let rendered = self.render_prepared_sparse_atlas_raster_batches(
            output_size,
            prepared_batches,
            base_pixels,
        )?;
        let pixels = self.read_texture_rgba8(
            rendered.final_texture(),
            output_size.width,
            output_size.height,
        )?;
        Ok(GpuRasterStackOutput {
            drawn_resources: Vec::new(),
            size: output_size,
            pixels,
        })
    }

    fn draw_prepared_sparse_atlas_raster_batch_patches_over_rgba8(
        &self,
        output_size: CanvasSize,
        prepared_batches: &[PreparedSparseAtlasRasterEvents<'_>],
        base_pixels: &[u8],
        rects: &[Rect],
    ) -> Result<GpuRasterPatchOutput, GpuRenderError> {
        let rendered = self.render_prepared_sparse_atlas_raster_batches(
            output_size,
            prepared_batches,
            Some(base_pixels),
        )?;
        let payload =
            self.read_texture_rgba8_regions(rendered.final_texture(), output_size, rects)?;
        Ok(GpuRasterPatchOutput {
            size: output_size,
            payload,
        })
    }

    fn render_prepared_sparse_atlas_raster_batches(
        &self,
        output_size: CanvasSize,
        prepared_batches: &[PreparedSparseAtlasRasterEvents<'_>],
        base_pixels: Option<&[u8]>,
    ) -> Result<SparseAtlasRenderedTextures, GpuRenderError> {
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
        );

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
        Ok(SparseAtlasRenderedTextures::new(
            previous,
            output,
            drawable_batches.len(),
        ))
    }

    fn encode_sparse_atlas_raster_events(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        prepared: &PreparedSparseAtlasRasterEvents<'_>,
        previous_view: &wgpu::TextureView,
        output_view: &wgpu::TextureView,
        pass_bounds: CanvasRect,
    ) -> Result<(), GpuRenderError> {
        let (owned_atlas, atlas_view) = match prepared.atlas {
            Some(atlas) => (
                None,
                atlas
                    .texture()
                    .create_view(&wgpu::TextureViewDescriptor::default()),
            ),
            None => {
                let (texture, _) = upload_lut_atlas_texture(self, &[])?;
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                (Some(texture), view)
            }
        };
        let _owned_atlas = owned_atlas;
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
        let (lut_atlas, _) = upload_lut_atlas_texture(self, &prepared.lut_rows)?;
        let lut_atlas_view = lut_atlas.create_view(&wgpu::TextureViewDescriptor::default());
        let event_program = TileEventProgram::from_payloads(prepared.payloads.clone());
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
        let (mode, resolve_blend_kind, base_event_count) =
            sparse_atlas_batch_tile_silo_params(prepared.kind);
        let params_buffer = create_params_buffer_with_mode_and_resolve(
            &self.context.device,
            (0, 0),
            prepared.tile_cols,
            mode,
            resolve_blend_kind,
            base_event_count,
        );
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

fn sparse_atlas_batch_tile_silo_params(
    kind: GpuSparseAtlasRasterEventBatchKind,
) -> (u32, u32, u32) {
    match kind {
        GpuSparseAtlasRasterEventBatchKind::RasterRun => (TILE_SILO_MODE_NORMAL, 0, 0),
        GpuSparseAtlasRasterEventBatchKind::RasterClippingRun {
            base_event_count,
            resolve_blend_mode,
        } => (
            TILE_SILO_MODE_CLIPPING_RUN,
            blend_kind(resolve_blend_mode),
            base_event_count,
        ),
        GpuSparseAtlasRasterEventBatchKind::PointFilterRun => (TILE_SILO_MODE_NORMAL, 0, 0),
        GpuSparseAtlasRasterEventBatchKind::SimpleContainerScope
        | GpuSparseAtlasRasterEventBatchKind::SimpleThroughScope => (TILE_SILO_MODE_NORMAL, 0, 0),
    }
}
