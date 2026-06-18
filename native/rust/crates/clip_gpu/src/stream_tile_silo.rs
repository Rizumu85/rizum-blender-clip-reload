use std::collections::HashMap;

use clip_model::CanvasSize;

use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::{CanvasRect, union_optional};
use crate::stream_context::StreamingExecutionContext;
use crate::stream_tile_silo_buffers::{
    create_params_buffer, create_tile_event_storage_buffers, create_u32_storage_buffer,
};
pub(crate) use crate::stream_tile_silo_plan::raster_silo_run_len;
use crate::stream_tile_silo_plan::{
    MIN_SILO_RUN_LEN, PreparedSiloSource, TILE_SIZE, plan_atlas_layout, source_bounds,
    source_local_bounds, tile_event_program, tile_work_lists,
};
use crate::stream_tile_silo_upload::{
    copy_sources_to_atlas, create_atlas_texture, rgba8_texture_byte_len, upload_atlas_texture,
    upload_atlas_tile_texture, upload_lut_atlas_texture, upload_mask_atlas_tile_texture,
};
use crate::stream_utils::local_pass_bounds;
use crate::{
    GpuNormalStackSource, GpuRasterAtlasSource, GpuRasterAtlasTileChunk, GpuRasterResourceInfo,
    GpuRenderError,
};

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_raster_silo_run_with_provider<P>(
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
    if sources.len() < MIN_SILO_RUN_LEN {
        return Ok(false);
    }

    let output_size = context.output_size;
    let Some(layout) = plan_atlas_layout(
        &*context.provider,
        output_size,
        target_origin,
        target_size,
        sources,
    ) else {
        return Ok(false);
    };
    let Some(requests) = atlas_requests(
        &*context.provider,
        output_size,
        target_origin,
        target_size,
        sources,
        &layout.sources,
    ) else {
        return Ok(false);
    };
    let run_has_masks = sources_have_masks(sources);
    let (prepared, atlas, mask_atlas, mask_atlas_bytes, drawn_resources) = if let Some(upload) =
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
        (
            prepared,
            atlas,
            mask_atlas,
            mask_atlas_bytes,
            upload.resources,
        )
    } else if let Some(upload) = context
        .provider
        .raster_run_atlas_pixels(&requests, layout.size)?
    {
        if upload.size != layout.size {
            return Err(P::Error::from(GpuRenderError::RasterAtlasSizeMismatch {
                expected: layout.size,
                actual: upload.size,
            }));
        }
        let atlas = upload_atlas_texture(context.renderer, &upload).map_err(P::Error::from)?;
        let (mask_atlas, mask_atlas_bytes) =
            upload_mask_atlas_tile_texture(context.renderer, upload.size, &[])
                .map_err(P::Error::from)?;
        let drawn_resources = upload.resources.clone();
        let prepared = prepared_sources_from_atlas_upload(
            &requests,
            output_size,
            target_origin,
            target_size,
            upload.resources,
        )
        .map_err(P::Error::from)?;
        (
            prepared,
            atlas,
            mask_atlas,
            mask_atlas_bytes,
            drawn_resources,
        )
    } else {
        if run_has_masks {
            return Ok(false);
        }
        let prepared = prepare_sources_with_caches(
            context,
            output_size,
            target_origin,
            target_size,
            sources,
            &layout.sources,
        )?;
        let atlas = create_atlas_texture(context.state.device(), layout.size);
        let (mask_atlas, mask_atlas_bytes) =
            upload_mask_atlas_tile_texture(context.renderer, layout.size, &[])
                .map_err(P::Error::from)?;
        copy_sources_to_atlas(context.state.encoder_mut(), &prepared, &atlas);
        let drawn_resources = prepared.iter().map(|source| source.info).collect();
        (
            prepared,
            atlas,
            mask_atlas,
            mask_atlas_bytes,
            drawn_resources,
        )
    };
    for info in drawn_resources {
        context.state.push_drawn_resource(info);
    }

    let Some(source_bounds) = prepared
        .iter()
        .map(|source| source.bounds)
        .reduce(CanvasRect::union)
    else {
        return Ok(false);
    };
    let Some(pass_bounds) = context
        .state
        .clip_pass_bounds(union_optional(*dirty_bounds, Some(source_bounds)))
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

    let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor::default());
    let mask_atlas_view = mask_atlas.create_view(&wgpu::TextureViewDescriptor::default());
    let (lut_atlas, lut_atlas_bytes) =
        upload_lut_atlas_texture(context.renderer, &[]).map_err(P::Error::from)?;
    let lut_atlas_view = lut_atlas.create_view(&wgpu::TextureViewDescriptor::default());
    let event_program = tile_event_program(&prepared);
    let event_buffers = create_tile_event_storage_buffers(
        context.state.device(),
        "rizum_clip_tile_silo_event_headers",
        "rizum_clip_tile_silo_raster_payloads",
        &event_program,
    );
    let work_buffer = create_u32_storage_buffer(
        context.state.device(),
        "rizum_clip_tile_silo_work_indices",
        &work_indices,
    );
    let span_buffer = create_u32_storage_buffer(
        context.state.device(),
        "rizum_clip_tile_silo_spans",
        &tile_spans,
    );
    let params_buffer = create_params_buffer(context.state.device(), target_origin, tile_cols);
    let pipeline = context.state.tile_silo_pipeline();
    let bind_group = context
        .state
        .device()
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rizum_clip_tile_silo_bind_group"),
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
                label: Some("rizum_clip_tile_silo_raster_pass"),
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
    for source in prepared {
        if let Some(cache) = source.cache {
            context.state.retain_raster_cache(cache);
        }
    }
    context.state.retain_texture(atlas, atlas_bytes);
    context.state.retain_texture(mask_atlas, mask_atlas_bytes);
    context.state.retain_texture(lut_atlas, lut_atlas_bytes);
    context.state.finish_pass()?;
    *dirty_bounds = Some(pass_bounds);
    Ok(true)
}

fn sources_have_masks(sources: &[GpuNormalStackSource]) -> bool {
    sources.iter().any(|source| match source {
        GpuNormalStackSource::Raster(raster) => raster.mask_key.is_some(),
        _ => false,
    })
}

#[allow(clippy::too_many_arguments)]
fn prepare_sources_with_caches<P>(
    context: &mut StreamingExecutionContext<'_, '_, P>,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    sources: &[GpuNormalStackSource],
    placements: &[crate::stream_tile_silo_plan::AtlasSourcePlacement],
) -> Result<Vec<PreparedSiloSource>, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let mut prepared = Vec::with_capacity(sources.len());
    for (index, source) in sources.iter().enumerate() {
        let GpuNormalStackSource::Raster(raster) = source else {
            return Ok(Vec::new());
        };
        let cache = context
            .state
            .retained_raster_cache(raster.key)
            .map(Ok)
            .unwrap_or_else(|| context.provider.raster_resource(context.renderer, *raster))?;
        let resource = cache
            .resource(raster.key)
            .ok_or(GpuRenderError::MissingRasterResource {
                layer_id: raster.key.layer_id,
                render_mipmap_id: raster.key.render_mipmap_id,
            })
            .map_err(P::Error::from)?;
        let info = resource.info();
        let offset = context
            .provider
            .raster_resource_offset(*raster)
            .unwrap_or((raster.offset_x, raster.offset_y));
        let bounds = source_bounds(offset, info.size, output_size)
            .ok_or(GpuRenderError::InvalidImageSize)
            .map_err(P::Error::from)?;
        let local_bounds = source_local_bounds(offset, info.size, target_origin, target_size)
            .ok_or(GpuRenderError::InvalidImageSize)
            .map_err(P::Error::from)?;
        prepared.push(PreparedSiloSource {
            source: *raster,
            cache: Some(cache),
            info,
            offset,
            bounds,
            local_bounds,
            atlas: placements[index],
            mask_atlas: None,
        });
    }
    Ok(prepared)
}

pub(crate) fn atlas_requests<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    sources: &[GpuNormalStackSource],
    placements: &[crate::stream_tile_silo_plan::AtlasSourcePlacement],
) -> Option<Vec<GpuRasterAtlasSource>>
where
    P: GpuNormalStackResourceProvider,
{
    let mut requests = Vec::with_capacity(sources.len());
    for (index, source) in sources.iter().enumerate() {
        let GpuNormalStackSource::Raster(raster) = source else {
            return None;
        };
        let size = provider.raster_resource_size(*raster)?;
        let offset = provider
            .raster_resource_offset(*raster)
            .unwrap_or((raster.offset_x, raster.offset_y));
        source_bounds(offset, size, output_size)?;
        source_local_bounds(offset, size, target_origin, target_size)?;
        let placement = placements[index];
        requests.push(GpuRasterAtlasSource {
            source: *raster,
            atlas_x: placement.x,
            atlas_y: placement.y,
            size,
            offset_x: offset.0,
            offset_y: offset.1,
        });
    }
    Some(requests)
}

pub(crate) fn prepared_sources_from_atlas_upload(
    requests: &[GpuRasterAtlasSource],
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    resources: Vec<crate::GpuRasterResourceInfo>,
) -> Result<Vec<PreparedSiloSource>, GpuRenderError> {
    if resources.len() != requests.len() {
        return Err(GpuRenderError::RasterAtlasResourceCountMismatch {
            expected: requests.len(),
            actual: resources.len(),
        });
    }
    requests
        .iter()
        .zip(resources)
        .map(|(request, info)| {
            if info.key != request.source.key {
                return Err(GpuRenderError::MissingRasterResource {
                    layer_id: request.source.key.layer_id,
                    render_mipmap_id: request.source.key.render_mipmap_id,
                });
            }
            if info.size != request.size {
                return Err(GpuRenderError::RasterResourceSizeMismatch {
                    layer_id: request.source.key.layer_id,
                    expected: request.size,
                    actual: info.size,
                });
            }
            let offset = (request.offset_x, request.offset_y);
            let bounds = source_bounds(offset, request.size, output_size)
                .ok_or(GpuRenderError::InvalidImageSize)?;
            let local_bounds =
                source_local_bounds(offset, request.size, target_origin, target_size)
                    .ok_or(GpuRenderError::InvalidImageSize)?;
            Ok(PreparedSiloSource {
                source: request.source,
                cache: None,
                info,
                offset,
                bounds,
                local_bounds,
                atlas: crate::stream_tile_silo_plan::AtlasSourcePlacement {
                    x: request.atlas_x,
                    y: request.atlas_y,
                },
                mask_atlas: None,
            })
        })
        .collect()
}

pub(crate) fn prepared_sources_from_atlas_tiles(
    chunks: &[GpuRasterAtlasTileChunk],
    resources: &[GpuRasterResourceInfo],
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
) -> Result<Vec<PreparedSiloSource>, GpuRenderError> {
    let resources_by_key: HashMap<_, _> = resources.iter().map(|info| (info.key, *info)).collect();
    chunks
        .iter()
        .filter_map(|chunk| {
            let Some(resource_info) = resources_by_key.get(&chunk.source.key) else {
                return Some(Err(GpuRenderError::MissingRasterResource {
                    layer_id: chunk.source.key.layer_id,
                    render_mipmap_id: chunk.source.key.render_mipmap_id,
                }));
            };
            let offset = (chunk.offset_x, chunk.offset_y);
            let Some(bounds) = source_bounds(offset, chunk.size, output_size) else {
                return Some(Err(GpuRenderError::InvalidImageSize));
            };
            let local_bounds = source_local_bounds(offset, chunk.size, target_origin, target_size)?;
            Some(Ok(PreparedSiloSource {
                source: chunk.source,
                cache: None,
                info: GpuRasterResourceInfo {
                    key: resource_info.key,
                    render_node_id: resource_info.render_node_id,
                    size: chunk.size,
                    byte_len: chunk.pixels.len(),
                },
                offset,
                bounds,
                local_bounds,
                atlas: crate::stream_tile_silo_plan::AtlasSourcePlacement {
                    x: chunk.atlas_x,
                    y: chunk.atlas_y,
                },
                mask_atlas: chunk
                    .mask_atlas_x
                    .zip(chunk.mask_atlas_y)
                    .map(|(x, y)| crate::stream_tile_silo_plan::AtlasSourcePlacement { x, y }),
            }))
        })
        .collect()
}
