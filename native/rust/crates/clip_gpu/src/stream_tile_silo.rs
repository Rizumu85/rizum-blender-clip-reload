use clip_model::CanvasSize;

use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::{CanvasRect, union_optional};
use crate::stream_state::StreamingEncoder;
pub(crate) use crate::stream_tile_silo_plan::raster_silo_run_len;
use crate::stream_tile_silo_plan::{
    MIN_SILO_RUN_LEN, PreparedSiloSource, TILE_SIZE, event_words, plan_atlas_layout, source_bounds,
    source_local_bounds, tile_work_lists,
};
use crate::stream_utils::local_pass_bounds;
use crate::{GpuNormalStackSource, GpuRenderError, GpuRenderer};

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_raster_silo_run_with_provider<P>(
    renderer: &GpuRenderer,
    provider: &mut P,
    state: &mut StreamingEncoder<'_, P::Error>,
    output_size: CanvasSize,
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

    let Some(layout) =
        plan_atlas_layout(provider, output_size, target_origin, target_size, sources)
    else {
        return Ok(false);
    };
    let mut prepared = Vec::with_capacity(sources.len());
    for (index, source) in sources.iter().enumerate() {
        let GpuNormalStackSource::Raster(raster) = source else {
            return Ok(false);
        };
        let cache = state
            .retained_raster_cache(raster.key)
            .map(Ok)
            .unwrap_or_else(|| provider.raster_resource(renderer, *raster))?;
        let resource = cache
            .resource(raster.key)
            .ok_or(GpuRenderError::MissingRasterResource {
                layer_id: raster.key.layer_id,
                render_mipmap_id: raster.key.render_mipmap_id,
            })
            .map_err(P::Error::from)?;
        let info = resource.info();
        state.push_drawn_resource(info);
        let offset = provider
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
            cache,
            info,
            offset,
            bounds,
            local_bounds,
            atlas: layout.sources[index],
        });
    }

    let Some(source_bounds) = prepared
        .iter()
        .map(|source| source.bounds)
        .reduce(CanvasRect::union)
    else {
        return Ok(false);
    };
    let Some(pass_bounds) = union_optional(*dirty_bounds, Some(source_bounds)) else {
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

    let atlas = create_atlas_texture(state.device(), layout.size);
    copy_sources_to_atlas(state.encoder_mut(), &prepared, &atlas);
    let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor::default());
    let event_words = event_words(&prepared);
    let event_buffer =
        create_u32_storage_buffer(state.device(), "rizum_clip_tile_silo_events", &event_words);
    let work_buffer = create_u32_storage_buffer(
        state.device(),
        "rizum_clip_tile_silo_work_indices",
        &work_indices,
    );
    let span_buffer =
        create_u32_storage_buffer(state.device(), "rizum_clip_tile_silo_spans", &tile_spans);
    let params_buffer = create_params_buffer(state.device(), target_origin, tile_cols);
    let pipeline = state.tile_silo_pipeline();
    let bind_group = state
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
                    resource: event_buffer.as_entire_binding(),
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
            ],
        });

    {
        let mut pass = state
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

    let atlas_bytes = usize::try_from(
        u64::from(layout.size.width)
            .saturating_mul(u64::from(layout.size.height))
            .saturating_mul(4),
    )
    .unwrap_or(usize::MAX);
    for source in prepared {
        state.retain_raster_cache(source.cache);
    }
    state.retain_texture(atlas, atlas_bytes);
    state.finish_pass()?;
    *dirty_bounds = Some(pass_bounds);
    Ok(true)
}

fn create_atlas_texture(device: &wgpu::Device, size: CanvasSize) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rizum_clip_tile_silo_atlas"),
        size: wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn copy_sources_to_atlas(
    encoder: &mut wgpu::CommandEncoder,
    sources: &[PreparedSiloSource],
    atlas: &wgpu::Texture,
) {
    for source in sources {
        let resource = source
            .cache
            .resource(source.source.key)
            .expect("prepared source cache must contain raster resource");
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: resource.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: atlas,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: source.atlas.x,
                    y: source.atlas.y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: source.info.size.width,
                height: source.info.size.height,
                depth_or_array_layers: 1,
            },
        );
    }
}

fn create_u32_storage_buffer(
    device: &wgpu::Device,
    label: &'static str,
    values: &[u32],
) -> wgpu::Buffer {
    create_buffer_with_bytes(
        device,
        label,
        wgpu::BufferUsages::STORAGE,
        &u32_bytes(values),
    )
}

fn create_params_buffer(
    device: &wgpu::Device,
    target_origin: (i32, i32),
    tile_cols: u32,
) -> wgpu::Buffer {
    let mut bytes = Vec::with_capacity(16);
    bytes.extend_from_slice(&target_origin.0.to_ne_bytes());
    bytes.extend_from_slice(&target_origin.1.to_ne_bytes());
    bytes.extend_from_slice(&TILE_SIZE.to_ne_bytes());
    bytes.extend_from_slice(&tile_cols.to_ne_bytes());
    create_buffer_with_bytes(
        device,
        "rizum_clip_tile_silo_params",
        wgpu::BufferUsages::UNIFORM,
        &bytes,
    )
}

fn u32_bytes(values: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }
    bytes
}

fn create_buffer_with_bytes(
    device: &wgpu::Device,
    label: &'static str,
    usage: wgpu::BufferUsages,
    bytes: &[u8],
) -> wgpu::Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes.len() as wgpu::BufferAddress,
        usage,
        mapped_at_creation: true,
    });
    {
        let mut mapped = buffer.slice(..).get_mapped_range_mut();
        mapped.copy_from_slice(bytes);
    }
    buffer.unmap();
    buffer
}
