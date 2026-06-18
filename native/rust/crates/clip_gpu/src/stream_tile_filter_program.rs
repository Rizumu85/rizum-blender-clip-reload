use clip_model::CanvasSize;

use crate::GpuRenderError;
use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::CanvasRect;
use crate::stream_context::StreamingExecutionContext;
use crate::stream_tile_event::{TileEventPayload, TileEventProgram};
use crate::stream_tile_silo_buffers::{
    create_params_buffer, create_tile_event_storage_buffers, create_u32_storage_buffer,
};
use crate::stream_tile_silo_plan::{TILE_SIZE, tile_work_lists_for_bounds};
use crate::stream_tile_silo_upload::{rgba8_texture_byte_len, upload_lut_atlas_texture};
use crate::stream_utils::local_pass_bounds;

pub(crate) struct FilterTileProgramInputs<'a> {
    pub(crate) payloads: Vec<TileEventPayload>,
    pub(crate) event_bounds: Vec<CanvasRect>,
    pub(crate) lut_rows: Vec<&'a [u8]>,
    pub(crate) final_dirty_bounds: Option<CanvasRect>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_filter_tile_program<P>(
    context: &mut StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    atlas_size: CanvasSize,
    atlas: wgpu::Texture,
    mask_atlas: wgpu::Texture,
    mask_atlas_bytes: usize,
    program_inputs: FilterTileProgramInputs<'_>,
    previous_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
) -> Result<Option<CanvasRect>, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let Some(pass_bounds) = context
        .state
        .clip_pass_bounds(program_inputs.final_dirty_bounds)
    else {
        return Ok(None);
    };

    let tile_cols = target_size.width.div_ceil(TILE_SIZE);
    let tile_count =
        usize::try_from(u64::from(tile_cols) * u64::from(target_size.height.div_ceil(TILE_SIZE)))
            .map_err(|_| GpuRenderError::TextureSizeOverflow)
            .map_err(P::Error::from)?;
    let (work_indices, tile_spans) =
        tile_work_lists_for_bounds(tile_count, tile_cols, &program_inputs.event_bounds)
            .map_err(P::Error::from)?;
    if work_indices.is_empty() {
        return Ok(None);
    }

    let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor::default());
    let mask_atlas_view = mask_atlas.create_view(&wgpu::TextureViewDescriptor::default());
    let (lut_atlas, lut_atlas_bytes) =
        upload_lut_atlas_texture(context.renderer, &program_inputs.lut_rows)
            .map_err(P::Error::from)?;
    let lut_atlas_view = lut_atlas.create_view(&wgpu::TextureViewDescriptor::default());
    let event_program = TileEventProgram::from_payloads(program_inputs.payloads);
    let event_buffers = create_tile_event_storage_buffers(
        context.state.device(),
        "rizum_clip_tile_filter_silo_event_headers",
        "rizum_clip_tile_filter_silo_raster_payloads",
        &event_program,
    );
    let work_buffer = create_u32_storage_buffer(
        context.state.device(),
        "rizum_clip_tile_filter_silo_work_indices",
        &work_indices,
    );
    let span_buffer = create_u32_storage_buffer(
        context.state.device(),
        "rizum_clip_tile_filter_silo_spans",
        &tile_spans,
    );
    let params_buffer = create_params_buffer(context.state.device(), target_origin, tile_cols);
    let pipeline = context.state.tile_silo_pipeline();
    let bind_group = context
        .state
        .device()
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rizum_clip_tile_filter_silo_bind_group"),
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

    {
        let mut pass = context
            .state
            .encoder_mut()
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rizum_clip_tile_filter_silo_pass"),
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
        let local_bounds = local_pass_bounds(pass_bounds, target_origin);
        pass.set_scissor_rect(
            local_bounds.x,
            local_bounds.y,
            local_bounds.width,
            local_bounds.height,
        );
        pass.draw(0..3, 0..1);
    }

    let atlas_bytes = rgba8_texture_byte_len(atlas_size).map_err(P::Error::from)?;
    context.state.retain_texture(atlas, atlas_bytes);
    context.state.retain_texture(mask_atlas, mask_atlas_bytes);
    context.state.retain_texture(lut_atlas, lut_atlas_bytes);
    context.state.finish_pass()?;
    Ok(Some(pass_bounds))
}
