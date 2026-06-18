use clip_model::CanvasSize;

use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::CanvasRect;
use crate::stream_context::StreamingExecutionContext;
use crate::stream_tile_event::TileEventProgram;
use crate::stream_tile_filter_silo::upload_raster_sources;
use crate::stream_tile_scope_silo_plan::{
    simple_container_scope_event_count, simple_through_scope_event_count,
};
use crate::stream_tile_scope_silo_program::{
    ScopeProgramInputs, ScopeProgramKind, build_scope_event_program_inputs,
    raster_sources_from_scope_children, raster_sources_have_masks,
};
use crate::stream_tile_silo::atlas_requests;
use crate::stream_tile_silo_buffers::{
    create_params_buffer, create_tile_event_storage_buffers, create_u32_storage_buffer,
};
use crate::stream_tile_silo_plan::{TILE_SIZE, plan_atlas_layout, tile_work_lists_for_bounds};
use crate::stream_tile_silo_upload::{
    rgba8_texture_byte_len, upload_lut_atlas_texture, upload_mask_atlas_tile_texture,
};
use crate::stream_utils::local_pass_bounds;
use crate::{GpuMaskAtlasTileChunk, GpuNormalStackSource, GpuRenderError};

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_simple_container_scope_silo_with_provider<P>(
    context: &mut StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    source: &GpuNormalStackSource,
    previous_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    dirty_bounds: &mut Option<CanvasRect>,
) -> Result<bool, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let GpuNormalStackSource::Container {
        children,
        opacity,
        mask_key,
        blend_mode,
    } = source
    else {
        return Ok(false);
    };
    if simple_container_scope_event_count(
        &*context.provider,
        context.output_size,
        target_origin,
        target_size,
        source,
    )
    .is_none()
    {
        return Ok(false);
    }

    let raster_sources = raster_sources_from_scope_children(children);
    if raster_sources.is_empty() {
        return Ok(false);
    }
    let output_size = context.output_size;
    let Some(layout) = plan_atlas_layout(
        &*context.provider,
        output_size,
        target_origin,
        target_size,
        &raster_sources,
    ) else {
        return Ok(false);
    };
    let Some(requests) = atlas_requests(
        &*context.provider,
        output_size,
        target_origin,
        target_size,
        &raster_sources,
        &layout.sources,
    ) else {
        return Ok(false);
    };

    let run_has_masks = raster_sources_have_masks(&raster_sources);
    let Some(upload) = upload_raster_sources(
        context,
        output_size,
        target_origin,
        target_size,
        &requests,
        layout.size,
        run_has_masks,
    )?
    else {
        return Ok(false);
    };

    let Some(program_inputs) = build_scope_event_program_inputs(
        context,
        target_origin,
        target_size,
        ScopeProgramKind::Container {
            opacity: *opacity,
            blend_mode: *blend_mode,
            mask_key: *mask_key,
        },
        children,
        upload.prepared,
        upload.mask_atlas_size,
        context.renderer.max_texture_dimension_2d(),
        *dirty_bounds,
    )?
    else {
        return Ok(false);
    };
    let Some(pass_bounds) = context
        .state
        .clip_pass_bounds(program_inputs.final_dirty_bounds)
    else {
        return Ok(false);
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
        return Ok(false);
    }

    let Some((mask_atlas, mask_atlas_bytes)) = scope_mask_atlas(
        context,
        upload.mask_atlas,
        upload.mask_atlas_bytes,
        upload.mask_atlas_size,
        upload.mask_chunks,
        &program_inputs,
    )?
    else {
        return Ok(false);
    };

    encode_scope_tile_program(
        context,
        target_origin,
        layout.size,
        upload.atlas,
        mask_atlas,
        mask_atlas_bytes,
        &program_inputs,
        &work_indices,
        &tile_spans,
        previous_view,
        output_view,
        tile_cols,
        pass_bounds,
    )?;
    for info in upload.drawn_resources {
        context.state.push_drawn_resource(info);
    }
    *dirty_bounds = Some(pass_bounds);
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_simple_through_scope_silo_with_provider<P>(
    context: &mut StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    source: &GpuNormalStackSource,
    previous_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    dirty_bounds: &mut Option<CanvasRect>,
) -> Result<bool, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let GpuNormalStackSource::ThroughGroup {
        children,
        opacity,
        mask_key,
    } = source
    else {
        return Ok(false);
    };
    if simple_through_scope_event_count(
        &*context.provider,
        context.output_size,
        target_origin,
        target_size,
        source,
    )
    .is_none()
    {
        return Ok(false);
    }

    let raster_sources = raster_sources_from_scope_children(children);
    if raster_sources.is_empty() {
        return Ok(false);
    }
    let output_size = context.output_size;
    let Some(layout) = plan_atlas_layout(
        &*context.provider,
        output_size,
        target_origin,
        target_size,
        &raster_sources,
    ) else {
        return Ok(false);
    };
    let Some(requests) = atlas_requests(
        &*context.provider,
        output_size,
        target_origin,
        target_size,
        &raster_sources,
        &layout.sources,
    ) else {
        return Ok(false);
    };

    let run_has_masks = raster_sources_have_masks(&raster_sources);
    let Some(upload) = upload_raster_sources(
        context,
        output_size,
        target_origin,
        target_size,
        &requests,
        layout.size,
        run_has_masks,
    )?
    else {
        return Ok(false);
    };

    let Some(program_inputs) = build_scope_event_program_inputs(
        context,
        target_origin,
        target_size,
        ScopeProgramKind::Through {
            opacity: *opacity,
            mask_key: *mask_key,
        },
        children,
        upload.prepared,
        upload.mask_atlas_size,
        context.renderer.max_texture_dimension_2d(),
        *dirty_bounds,
    )?
    else {
        return Ok(false);
    };
    let Some(pass_bounds) = context
        .state
        .clip_pass_bounds(program_inputs.final_dirty_bounds)
    else {
        return Ok(false);
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
        return Ok(false);
    }

    let Some((mask_atlas, mask_atlas_bytes)) = scope_mask_atlas(
        context,
        upload.mask_atlas,
        upload.mask_atlas_bytes,
        upload.mask_atlas_size,
        upload.mask_chunks,
        &program_inputs,
    )?
    else {
        return Ok(false);
    };

    encode_scope_tile_program(
        context,
        target_origin,
        layout.size,
        upload.atlas,
        mask_atlas,
        mask_atlas_bytes,
        &program_inputs,
        &work_indices,
        &tile_spans,
        previous_view,
        output_view,
        tile_cols,
        pass_bounds,
    )?;
    for info in upload.drawn_resources {
        context.state.push_drawn_resource(info);
    }
    *dirty_bounds = Some(pass_bounds);
    Ok(true)
}

fn scope_mask_atlas<P>(
    context: &mut StreamingExecutionContext<'_, '_, P>,
    existing_mask_atlas: wgpu::Texture,
    existing_mask_atlas_bytes: usize,
    existing_mask_atlas_size: CanvasSize,
    mut mask_chunks: Vec<GpuMaskAtlasTileChunk>,
    program_inputs: &ScopeProgramInputs<'_>,
) -> Result<Option<(wgpu::Texture, usize)>, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    if program_inputs.scope_mask_sources.is_empty() {
        return Ok(Some((existing_mask_atlas, existing_mask_atlas_bytes)));
    }
    let Some(scope_chunks) = context.provider.mask_atlas_tile_pixels(
        &program_inputs.scope_mask_sources,
        program_inputs.mask_atlas_size,
    )?
    else {
        return Ok(None);
    };
    mask_chunks.extend(scope_chunks);
    let mask_atlas_size = CanvasSize::new(
        existing_mask_atlas_size
            .width
            .max(program_inputs.mask_atlas_size.width),
        existing_mask_atlas_size
            .height
            .max(program_inputs.mask_atlas_size.height),
    );
    upload_mask_atlas_tile_texture(context.renderer, mask_atlas_size, &mask_chunks)
        .map(Some)
        .map_err(P::Error::from)
}

#[allow(clippy::too_many_arguments)]
fn encode_scope_tile_program<P>(
    context: &mut StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    atlas_size: CanvasSize,
    atlas: wgpu::Texture,
    mask_atlas: wgpu::Texture,
    mask_atlas_bytes: usize,
    program_inputs: &ScopeProgramInputs<'_>,
    work_indices: &[u32],
    tile_spans: &[u32],
    previous_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    tile_cols: u32,
    pass_bounds: CanvasRect,
) -> Result<(), P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor::default());
    let mask_atlas_view = mask_atlas.create_view(&wgpu::TextureViewDescriptor::default());
    let (lut_atlas, lut_atlas_bytes) =
        upload_lut_atlas_texture(context.renderer, &program_inputs.lut_rows)
            .map_err(P::Error::from)?;
    let lut_atlas_view = lut_atlas.create_view(&wgpu::TextureViewDescriptor::default());
    let event_program = TileEventProgram::from_payloads(program_inputs.payloads.clone());
    let event_buffers = create_tile_event_storage_buffers(
        context.state.device(),
        "rizum_clip_tile_scope_silo_event_headers",
        "rizum_clip_tile_scope_silo_raster_payloads",
        &event_program,
    );
    let work_buffer = create_u32_storage_buffer(
        context.state.device(),
        "rizum_clip_tile_scope_silo_work_indices",
        work_indices,
    );
    let span_buffer = create_u32_storage_buffer(
        context.state.device(),
        "rizum_clip_tile_scope_silo_spans",
        tile_spans,
    );
    let params_buffer = create_params_buffer(context.state.device(), target_origin, tile_cols);
    let pipeline = context.state.tile_silo_pipeline();
    let bind_group = context
        .state
        .device()
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rizum_clip_tile_scope_silo_bind_group"),
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
                label: Some("rizum_clip_tile_scope_silo_pass"),
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
    context.state.finish_pass()
}
