use clip_model::CanvasSize;

use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::{CanvasRect, target_canvas_bounds, union_optional};
use crate::stream_context::StreamingExecutionContext;
use crate::stream_tile_event::{PointFilterTileEventPayload, TileEventPayload, TileEventProgram};
use crate::stream_tile_silo::{
    atlas_requests, prepared_sources_from_atlas_tiles, prepared_sources_from_atlas_upload,
};
use crate::stream_tile_silo_buffers::{
    create_params_buffer, create_tile_event_storage_buffers, create_u32_storage_buffer,
};
use crate::stream_tile_silo_plan::{
    MAX_SILO_EVENTS, MIN_SILO_RUN_LEN, PreparedSiloSource, TILE_SIZE, plan_atlas_layout,
    source_is_silo_eligible, tile_work_lists_for_bounds,
};
use crate::stream_tile_silo_upload::{
    rgba8_texture_byte_len, upload_atlas_texture, upload_atlas_tile_texture,
    upload_lut_atlas_texture, upload_mask_atlas_tile_texture,
};
use crate::stream_utils::local_pass_bounds;
use crate::{
    GpuMaskResourceKey, GpuNormalStackSource, GpuRasterAtlasSource, GpuRasterResourceInfo,
    GpuRenderError,
};

pub(crate) fn raster_filter_silo_run_len<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    sources: &[GpuNormalStackSource],
) -> usize
where
    P: GpuNormalStackResourceProvider,
{
    let mut len = 0usize;
    let mut saw_raster = false;
    let mut saw_filter = false;

    for source in sources.iter().take(MAX_SILO_EVENTS) {
        match source {
            GpuNormalStackSource::Raster(_) => {
                if !source_is_silo_eligible(
                    provider,
                    output_size,
                    target_origin,
                    target_size,
                    source,
                ) {
                    break;
                }
                saw_raster = true;
                len += 1;
            }
            GpuNormalStackSource::LutFilter {
                lut_rgba,
                opacity,
                mask_key,
                ..
            } => {
                if *opacity <= 0.0
                    || !filter_mask_can_lower(provider, *mask_key)
                    || lut_rgba.len() != 256 * 4
                {
                    break;
                }
                saw_filter = true;
                len += 1;
            }
            _ => break,
        }
    }

    if saw_filter && saw_raster && len >= MIN_SILO_RUN_LEN {
        len
    } else {
        0
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_raster_filter_silo_run_with_provider<P>(
    context: &mut StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    sources: &[GpuNormalStackSource],
    previous_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    dirty_bounds: &mut Option<CanvasRect>,
) -> Result<bool, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    if raster_filter_silo_run_len(
        &*context.provider,
        context.output_size,
        target_origin,
        target_size,
        sources,
    ) != sources.len()
    {
        return Ok(false);
    }

    let raster_sources = raster_sources_from_mixed_run(sources);
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

    for info in upload.drawn_resources {
        context.state.push_drawn_resource(info);
    }

    let Some(program_inputs) = build_filter_event_program_inputs(
        context,
        target_origin,
        target_size,
        sources,
        upload.prepared,
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

    let atlas_view = upload
        .atlas
        .create_view(&wgpu::TextureViewDescriptor::default());
    let mask_atlas_view = upload
        .mask_atlas
        .create_view(&wgpu::TextureViewDescriptor::default());
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

    let atlas_bytes = rgba8_texture_byte_len(layout.size).map_err(P::Error::from)?;
    context.state.retain_texture(upload.atlas, atlas_bytes);
    context
        .state
        .retain_texture(upload.mask_atlas, upload.mask_atlas_bytes);
    context.state.retain_texture(lut_atlas, lut_atlas_bytes);
    context.state.finish_pass()?;
    *dirty_bounds = Some(pass_bounds);
    Ok(true)
}

pub(crate) struct RasterUploadBundle {
    pub(crate) prepared: Vec<PreparedSiloSource>,
    pub(crate) atlas: wgpu::Texture,
    pub(crate) mask_atlas: wgpu::Texture,
    pub(crate) mask_atlas_bytes: usize,
    pub(crate) drawn_resources: Vec<GpuRasterResourceInfo>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn upload_raster_sources<P>(
    context: &mut StreamingExecutionContext<'_, '_, P>,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    requests: &[GpuRasterAtlasSource],
    atlas_size: CanvasSize,
    run_has_masks: bool,
) -> Result<Option<RasterUploadBundle>, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    if let Some(upload) = context
        .provider
        .raster_run_atlas_tile_pixels(requests, atlas_size)?
    {
        if upload.size != atlas_size {
            return Err(P::Error::from(GpuRenderError::RasterAtlasSizeMismatch {
                expected: atlas_size,
                actual: upload.size,
            }));
        }
        let atlas = upload_atlas_tile_texture(context.renderer, &upload).map_err(P::Error::from)?;
        let (mask_atlas, mask_atlas_bytes) =
            upload_mask_atlas_tile_texture(context.renderer, upload.size, &upload.mask_chunks)
                .map_err(P::Error::from)?;
        let prepared = prepared_sources_from_atlas_tiles(
            &upload.chunks,
            &upload.resources,
            output_size,
            target_origin,
            target_size,
        )
        .map_err(P::Error::from)?;
        if prepared.is_empty() {
            return Ok(None);
        }
        return Ok(Some(RasterUploadBundle {
            prepared,
            atlas,
            mask_atlas,
            mask_atlas_bytes,
            drawn_resources: upload.resources,
        }));
    }

    if run_has_masks {
        return Ok(None);
    }

    let Some(upload) = context
        .provider
        .raster_run_atlas_pixels(requests, atlas_size)?
    else {
        return Ok(None);
    };
    if upload.size != atlas_size {
        return Err(P::Error::from(GpuRenderError::RasterAtlasSizeMismatch {
            expected: atlas_size,
            actual: upload.size,
        }));
    }
    let atlas = upload_atlas_texture(context.renderer, &upload).map_err(P::Error::from)?;
    let (mask_atlas, mask_atlas_bytes) =
        upload_mask_atlas_tile_texture(context.renderer, upload.size, &[])
            .map_err(P::Error::from)?;
    let prepared = prepared_sources_from_atlas_upload(
        requests,
        output_size,
        target_origin,
        target_size,
        upload.resources.clone(),
    )
    .map_err(P::Error::from)?;
    if prepared.is_empty() {
        return Ok(None);
    }
    Ok(Some(RasterUploadBundle {
        prepared,
        atlas,
        mask_atlas,
        mask_atlas_bytes,
        drawn_resources: upload.resources,
    }))
}

struct FilterProgramInputs<'a> {
    payloads: Vec<TileEventPayload>,
    event_bounds: Vec<CanvasRect>,
    lut_rows: Vec<&'a [u8]>,
    final_dirty_bounds: Option<CanvasRect>,
}

fn build_filter_event_program_inputs<'a, P>(
    context: &StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    sources: &'a [GpuNormalStackSource],
    prepared: Vec<PreparedSiloSource>,
    initial_dirty_bounds: Option<CanvasRect>,
) -> Result<Option<FilterProgramInputs<'a>>, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let mut payloads = Vec::new();
    let mut event_bounds = Vec::new();
    let mut lut_rows = Vec::new();
    let mut current_dirty = initial_dirty_bounds;
    let mut saw_filter = false;

    for source in sources {
        match source {
            GpuNormalStackSource::Raster(raster) => {
                for prepared_source in prepared.iter().filter(|item| item.source == *raster) {
                    payloads.push(TileEventPayload::Raster(raster_payload(prepared_source)));
                    event_bounds.push(prepared_source.local_bounds);
                    current_dirty = union_optional(current_dirty, Some(prepared_source.bounds));
                }
            }
            GpuNormalStackSource::LutFilter {
                lut_rgba,
                opacity,
                mask_key,
                filter_mode,
            } => {
                if !filter_mask_can_lower(context.provider, *mask_key)
                    || *opacity <= 0.0
                    || lut_rgba.len() != 256 * 4
                {
                    return Ok(None);
                }
                let filter_bounds =
                    current_dirty.or_else(|| target_canvas_bounds(target_origin, target_size));
                let Some(filter_bounds) = context.state.clip_pass_bounds(filter_bounds) else {
                    return Ok(None);
                };
                let local_bounds = local_pass_bounds(filter_bounds, target_origin);
                let lut_row = u32::try_from(lut_rows.len())
                    .map_err(|_| GpuRenderError::TextureSizeOverflow)?;
                payloads.push(TileEventPayload::PointFilter(PointFilterTileEventPayload {
                    lut_row,
                    opacity: *opacity,
                    filter_mode: *filter_mode,
                    local_bounds,
                }));
                event_bounds.push(local_bounds);
                lut_rows.push(lut_rgba.as_slice());
                current_dirty = Some(filter_bounds);
                saw_filter = true;
            }
            _ => return Ok(None),
        }
    }

    if !saw_filter || payloads.is_empty() || event_bounds.is_empty() {
        return Ok(None);
    }
    if !event_bounds_fit_target(&event_bounds, target_size) {
        return Ok(None);
    }
    Ok(Some(FilterProgramInputs {
        payloads,
        event_bounds,
        lut_rows,
        final_dirty_bounds: current_dirty,
    }))
}

pub(crate) fn raster_payload(
    source: &PreparedSiloSource,
) -> crate::stream_tile_event::RasterTileEventPayload {
    crate::stream_tile_event::RasterTileEventPayload {
        atlas_origin: (source.atlas.x, source.atlas.y),
        source_size: source.info.size,
        source_offset: source.offset,
        opacity: source.source.opacity,
        blend_mode: source.source.blend_mode,
        mask_atlas_origin: source.mask_atlas.map(|mask| (mask.x, mask.y)),
    }
}

fn event_bounds_fit_target(bounds: &[CanvasRect], target_size: CanvasSize) -> bool {
    bounds.iter().all(|bounds| {
        bounds.width > 0
            && bounds.height > 0
            && bounds.x.saturating_add(bounds.width) <= target_size.width
            && bounds.y.saturating_add(bounds.height) <= target_size.height
    })
}

fn raster_sources_from_mixed_run(sources: &[GpuNormalStackSource]) -> Vec<GpuNormalStackSource> {
    sources
        .iter()
        .filter_map(|source| match source {
            GpuNormalStackSource::Raster(raster) => Some(GpuNormalStackSource::Raster(*raster)),
            _ => None,
        })
        .collect()
}

fn raster_sources_have_masks(sources: &[GpuNormalStackSource]) -> bool {
    sources.iter().any(|source| match source {
        GpuNormalStackSource::Raster(raster) => raster.mask_key.is_some(),
        _ => false,
    })
}

pub(crate) fn filter_mask_can_lower<P>(provider: &P, mask_key: Option<GpuMaskResourceKey>) -> bool
where
    P: GpuNormalStackResourceProvider,
{
    match mask_key {
        Some(key) => provider.mask_is_fully_opaque(key) == Some(true),
        None => true,
    }
}
