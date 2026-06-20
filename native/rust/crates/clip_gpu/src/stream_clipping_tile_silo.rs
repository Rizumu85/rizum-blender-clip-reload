use std::time::Instant;

use clip_model::CanvasSize;

use crate::blend::blend_kind;
use crate::render_profile;
use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::{CanvasRect, union_optional};
use crate::stream_context::StreamingExecutionContext;
use crate::stream_tile_silo::{
    atlas_requests, prepared_sources_from_atlas_tiles, prepared_sources_from_atlas_upload,
    raster_silo_run_len,
};
use crate::stream_tile_silo_buffers::{
    create_params_buffer_with_mode_and_resolve, create_tile_event_storage_buffers,
    create_u32_storage_buffer,
};
use crate::stream_tile_silo_plan::{
    TILE_SIZE, plan_atlas_layout, tile_event_program, tile_work_lists,
};
use crate::stream_tile_silo_upload::{
    rgba8_texture_byte_len, upload_atlas_texture, upload_atlas_tile_texture,
    upload_lut_atlas_texture, upload_mask_atlas_tile_texture,
};
use crate::stream_utils::local_pass_bounds;
use crate::{GpuClippedStackSource, GpuNormalRasterSource, GpuNormalStackSource, GpuRenderError};

const TILE_SILO_MODE_CLIPPING_RUN: u32 = 2;

pub(crate) fn clipping_run_silo_is_eligible<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    base: GpuNormalRasterSource,
    clipped: &[GpuClippedStackSource],
) -> bool
where
    P: GpuNormalStackResourceProvider,
{
    let Some(normal_sources) = clipping_run_as_normal_sources(base, clipped) else {
        return false;
    };
    raster_silo_run_len(
        provider,
        output_size,
        target_origin,
        target_size,
        &normal_sources,
    ) == normal_sources.len()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_clipping_run_silo_with_provider<P>(
    context: &mut StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    base: GpuNormalRasterSource,
    clipped: &[GpuClippedStackSource],
    previous_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    dirty_bounds: &mut Option<CanvasRect>,
) -> Result<bool, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let Some(normal_sources) = clipping_run_as_normal_sources(base, clipped) else {
        return Ok(false);
    };

    let output_size = context.output_size;
    let Some(layout) = plan_atlas_layout(
        &*context.provider,
        output_size,
        target_origin,
        target_size,
        &normal_sources,
        crate::stream_tile_silo_plan::MAX_ATLAS_TEXTURE_SIZE,
    ) else {
        return Ok(false);
    };
    let Some(requests) = atlas_requests(
        &*context.provider,
        output_size,
        target_origin,
        target_size,
        &normal_sources,
        &layout.sources,
    ) else {
        return Ok(false);
    };
    let run_has_masks = normal_sources.iter().any(|source| match source {
        GpuNormalStackSource::Raster(raster) => raster.mask_key.is_some(),
        _ => false,
    });

    let (mut prepared, atlas, mask_atlas, mask_atlas_bytes, drawn_resources) = if let Some(upload) =
        context
            .provider
            .raster_run_atlas_tile_pixels(&requests, layout.size)?
    {
        if upload.size != layout.size {
            return Err(P::Error::from(GpuRenderError::RasterAtlasSizeMismatch {
                expected: layout.size,
                actual: upload.size,
            }));
        }
        let prepared = prepared_sources_from_atlas_tiles(
            &upload.chunks,
            &upload.resources,
            output_size,
            target_origin,
            target_size,
        )
        .map_err(P::Error::from)?;
        if prepared.is_empty() {
            return Ok(false);
        }
        let atlas = upload_atlas_tile_texture(context.renderer, &upload).map_err(P::Error::from)?;
        let (mask_atlas, mask_atlas_bytes) =
            upload_mask_atlas_tile_texture(context.renderer, upload.size, &upload.mask_chunks)
                .map_err(P::Error::from)?;
        (
            prepared,
            atlas,
            mask_atlas,
            mask_atlas_bytes,
            upload.resources,
        )
    } else {
        if run_has_masks {
            return Ok(false);
        }
        let Some(upload) = context
            .provider
            .raster_run_atlas_pixels(&requests, layout.size)?
        else {
            return Ok(false);
        };
        if upload.size != layout.size {
            return Err(P::Error::from(GpuRenderError::RasterAtlasSizeMismatch {
                expected: layout.size,
                actual: upload.size,
            }));
        }
        let prepared = prepared_sources_from_atlas_upload(
            &requests,
            output_size,
            target_origin,
            target_size,
            upload.resources.clone(),
        )
        .map_err(P::Error::from)?;
        if prepared.is_empty() {
            return Ok(false);
        }
        let atlas = upload_atlas_texture(context.renderer, &upload).map_err(P::Error::from)?;
        let (mask_atlas, mask_atlas_bytes) =
            upload_mask_atlas_tile_texture(context.renderer, upload.size, &[])
                .map_err(P::Error::from)?;
        (
            prepared,
            atlas,
            mask_atlas,
            mask_atlas_bytes,
            upload.resources,
        )
    };
    sort_prepared_by_source_order(&mut prepared, &normal_sources);

    let base_event_count = prepared
        .iter()
        .take_while(|source| source.source.key == base.key)
        .count();
    if base_event_count == 0 {
        return Ok(false);
    }
    let Some(base_bounds) = prepared
        .iter()
        .take(base_event_count)
        .map(|source| source.bounds)
        .reduce(CanvasRect::union)
    else {
        return Ok(false);
    };
    let Some(pass_bounds) = context
        .state
        .clip_pass_bounds(union_optional(*dirty_bounds, Some(base_bounds)))
    else {
        return Ok(false);
    };

    let tile_cols = target_size.width.div_ceil(TILE_SIZE);
    let tile_count =
        usize::try_from(u64::from(tile_cols) * u64::from(target_size.height.div_ceil(TILE_SIZE)))
            .map_err(|_| GpuRenderError::TextureSizeOverflow)
            .map_err(P::Error::from)?;
    let (work_indices, tile_spans) =
        tile_work_lists(tile_count, tile_cols, &prepared).map_err(P::Error::from)?;
    if work_indices.is_empty() {
        return Ok(false);
    }

    for info in drawn_resources {
        context.state.push_drawn_resource(info);
    }

    let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor::default());
    let mask_atlas_view = mask_atlas.create_view(&wgpu::TextureViewDescriptor::default());
    let (lut_atlas, lut_atlas_bytes) =
        upload_lut_atlas_texture(context.renderer, &[]).map_err(P::Error::from)?;
    let lut_atlas_view = lut_atlas.create_view(&wgpu::TextureViewDescriptor::default());
    let event_program = tile_event_program(&prepared);
    let event_buffers = create_tile_event_storage_buffers(
        context.state.device(),
        "rizum_clip_clipping_tile_silo_event_headers",
        "rizum_clip_clipping_tile_silo_raster_payloads",
        &event_program,
    );
    let work_buffer = create_u32_storage_buffer(
        context.state.device(),
        "rizum_clip_clipping_tile_silo_work_indices",
        &work_indices,
    );
    let span_buffer = create_u32_storage_buffer(
        context.state.device(),
        "rizum_clip_clipping_tile_silo_spans",
        &tile_spans,
    );
    let params_buffer = create_params_buffer_with_mode_and_resolve(
        context.state.device(),
        target_origin,
        tile_cols,
        TILE_SILO_MODE_CLIPPING_RUN,
        blend_kind(base.blend_mode),
        u32::try_from(base_event_count).map_err(|_| GpuRenderError::TextureSizeOverflow)?,
    );
    let pipeline = context.state.tile_silo_pipeline();
    let bind_group = context
        .state
        .device()
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rizum_clip_clipping_tile_silo_bind_group"),
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

    let pass_encode_start = Instant::now();
    {
        let mut pass = context
            .state
            .encoder_mut()
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rizum_clip_clipping_tile_silo_raster_pass"),
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
    render_profile::record_gpu_pass_encode(pass_encode_start.elapsed());

    let atlas_bytes = rgba8_texture_byte_len(layout.size).map_err(P::Error::from)?;
    context.state.retain_texture(atlas, atlas_bytes);
    context.state.retain_texture(mask_atlas, mask_atlas_bytes);
    context.state.retain_texture(lut_atlas, lut_atlas_bytes);
    context.state.finish_pass()?;
    *dirty_bounds = Some(pass_bounds);
    Ok(true)
}

pub(crate) fn clipping_run_as_normal_sources(
    base: GpuNormalRasterSource,
    clipped: &[GpuClippedStackSource],
) -> Option<Vec<GpuNormalStackSource>> {
    if clipped.is_empty() {
        return None;
    }
    let mut sources = Vec::with_capacity(clipped.len() + 1);
    sources.push(GpuNormalStackSource::Raster(base));
    for source in clipped {
        match source {
            GpuClippedStackSource::Raster(raster) => {
                sources.push(GpuNormalStackSource::Raster(*raster));
            }
            GpuClippedStackSource::Container { .. } => return None,
        }
    }
    Some(sources)
}

fn sort_prepared_by_source_order(
    prepared: &mut [crate::stream_tile_silo_plan::PreparedSiloSource],
    sources: &[GpuNormalStackSource],
) {
    prepared.sort_by_key(|prepared_source| {
        sources
            .iter()
            .position(|source| match source {
                GpuNormalStackSource::Raster(raster) => raster.key == prepared_source.source.key,
                _ => false,
            })
            .unwrap_or(usize::MAX)
    });
}
