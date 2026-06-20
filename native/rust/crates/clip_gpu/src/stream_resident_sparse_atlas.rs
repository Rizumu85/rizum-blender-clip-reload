use std::time::Instant;

use clip_model::CanvasSize;

use crate::sparse_atlas_prepare::PreparedSparseAtlasRasterEvents;
use crate::sparse_atlas_prepare_payloads::{validate_sparse_atlas_format, validate_tile_ref};
use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::{CanvasRect, target_canvas_bounds, union_optional};
use crate::stream_context::StreamingExecutionContext;
use crate::stream_tile_event::{RasterTileEventPayload, TileEventPayload, TileEventProgram};
use crate::stream_tile_silo_buffers::{
    create_params_buffer, create_tile_event_storage_buffers, create_u32_storage_buffer,
};
use crate::stream_tile_silo_plan::{TILE_SIZE, tile_work_lists_for_bounds};
use crate::stream_tile_silo_upload::{upload_lut_atlas_texture, upload_mask_atlas_tile_texture};
use crate::stream_utils::local_pass_bounds;
use crate::{
    GpuRenderError, GpuSparseAtlasFormat, GpuSparseAtlasRasterEvent,
    GpuSparseAtlasRasterEventBatch, GpuSparseAtlasRasterEventBatchKind, GpuSparseAtlasTexture,
    GpuSparseAtlasTextureKey, GpuSparseAtlasTexturePool,
};

const MAX_REGION_RESIDENT_RASTER_RUN_SOURCES: usize = 32;
const MAX_REGION_RESIDENT_RASTER_RUN_EVENTS: usize = 64;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RasterSiloEncodeOutcome {
    pub(crate) wrote: bool,
    pub(crate) uses_sparse_resident_atlas: bool,
    pub(crate) uses_per_run_atlas: bool,
    pub(crate) gpu_batches: u32,
}

impl RasterSiloEncodeOutcome {
    pub(crate) fn not_written() -> Self {
        Self::default()
    }

    pub(crate) fn resident_sparse_atlas(gpu_batches: u32) -> Self {
        Self {
            wrote: true,
            uses_sparse_resident_atlas: true,
            uses_per_run_atlas: false,
            gpu_batches,
        }
    }

    pub(crate) fn per_run_atlas(gpu_batches: u32) -> Self {
        Self {
            wrote: true,
            uses_sparse_resident_atlas: false,
            uses_per_run_atlas: true,
            gpu_batches,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_resident_sparse_atlas_raster_run_with_provider<P>(
    context: &mut StreamingExecutionContext<'_, '_, P>,
    sources: &[crate::GpuNormalStackSource],
    target_origin: (i32, i32),
    target_size: CanvasSize,
    previous_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    dirty_bounds: &mut Option<CanvasRect>,
) -> Result<Option<RasterSiloEncodeOutcome>, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    if context.state.render_bounds().is_none()
        || sources.len() > MAX_REGION_RESIDENT_RASTER_RUN_SOURCES
    {
        return Ok(None);
    }
    let output_size = context.output_size;
    let renderer = context.renderer;
    let state = &mut context.state;
    let encoded = context.provider.with_resident_sparse_atlas_raster_run(
        output_size,
        target_origin,
        target_size,
        sources,
        |pool, batches| {
            if batches.is_empty() || batches.len() != 1 {
                return Ok(None);
            }
            if batches[0].events.len() > MAX_REGION_RESIDENT_RASTER_RUN_EVENTS {
                return Ok(None);
            }
            let prepared = prepare_region_sparse_atlas_raster_event_batch(
                output_size,
                target_origin,
                target_size,
                pool,
                &batches[0],
            )
            .map_err(P::Error::from)?;
            if prepared.prepared.payloads.is_empty() {
                return Ok(None);
            }
            let Some(pass_bounds) =
                state.clip_pass_bounds(union_optional(*dirty_bounds, Some(prepared.global_bounds)))
            else {
                return Ok(None);
            };

            encode_prepared_resident_sparse_atlas_batch::<P>(
                renderer,
                state,
                target_origin,
                &prepared.prepared,
                previous_view,
                output_view,
                local_pass_bounds(pass_bounds, target_origin),
            )?;
            state.finish_pass()?;
            *dirty_bounds = Some(pass_bounds);
            crate::render_profile::record_region_raster_resident_atlas_segment();
            Ok(Some(RasterSiloEncodeOutcome::resident_sparse_atlas(1)))
        },
    )?;
    Ok(encoded.flatten())
}

struct RegionPreparedSparseAtlasRasterEvents<'a> {
    prepared: PreparedSparseAtlasRasterEvents<'a>,
    global_bounds: CanvasRect,
}

fn prepare_region_sparse_atlas_raster_event_batch<'a>(
    canvas_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    pool: &'a GpuSparseAtlasTexturePool,
    batch: &'a GpuSparseAtlasRasterEventBatch,
) -> Result<RegionPreparedSparseAtlasRasterEvents<'a>, GpuRenderError> {
    if batch.kind != GpuSparseAtlasRasterEventBatchKind::RasterRun
        || !batch.filters.is_empty()
        || batch.scope.is_some()
        || !batch.tile_events.is_empty()
    {
        return Err(GpuRenderError::NotImplemented);
    }
    let atlas = common_raster_atlas(pool, &batch.events)?;
    let mask_atlas = common_mask_atlas(pool, &batch.events)?;
    let target_bounds =
        target_canvas_bounds(target_origin, target_size).ok_or(GpuRenderError::InvalidImageSize)?;

    let mut payloads = Vec::new();
    let mut local_bounds = Vec::new();
    let mut global_bounds = None;
    for event in &batch.events {
        append_region_raster_payload(
            canvas_size,
            target_bounds,
            atlas,
            mask_atlas,
            *event,
            &mut payloads,
            &mut local_bounds,
            &mut global_bounds,
        )?;
    }
    let Some(global_bounds) = global_bounds else {
        return Ok(RegionPreparedSparseAtlasRasterEvents {
            prepared: empty_prepared_batch(batch.kind, target_size),
            global_bounds: target_bounds,
        });
    };
    let tile_cols = target_size.width.div_ceil(TILE_SIZE);
    let tile_count =
        usize::try_from(u64::from(tile_cols) * u64::from(target_size.height.div_ceil(TILE_SIZE)))
            .map_err(|_| GpuRenderError::TextureSizeOverflow)?;
    let (work_indices, tile_spans) =
        tile_work_lists_for_bounds(tile_count, tile_cols, &local_bounds)?;
    Ok(RegionPreparedSparseAtlasRasterEvents {
        prepared: PreparedSparseAtlasRasterEvents {
            kind: batch.kind,
            atlas: Some(atlas),
            mask_atlas,
            payloads,
            lut_rows: Vec::new(),
            work_indices,
            tile_spans,
            tile_cols,
            pass_bounds: global_bounds
                .intersection(target_bounds)
                .and_then(|bounds| bounds.translate_to_local(target_origin))
                .ok_or(GpuRenderError::InvalidImageSize)?,
        },
        global_bounds,
    })
}

fn empty_prepared_batch(
    kind: GpuSparseAtlasRasterEventBatchKind,
    target_size: CanvasSize,
) -> PreparedSparseAtlasRasterEvents<'static> {
    PreparedSparseAtlasRasterEvents {
        kind,
        atlas: None,
        mask_atlas: None,
        payloads: Vec::new(),
        lut_rows: Vec::new(),
        work_indices: Vec::new(),
        tile_spans: Vec::new(),
        tile_cols: target_size.width.div_ceil(TILE_SIZE),
        pass_bounds: CanvasRect {
            x: 0,
            y: 0,
            width: target_size.width,
            height: target_size.height,
        },
    }
}

fn append_region_raster_payload(
    canvas_size: CanvasSize,
    target_bounds: CanvasRect,
    atlas: &GpuSparseAtlasTexture,
    mask_atlas: Option<&GpuSparseAtlasTexture>,
    event: GpuSparseAtlasRasterEvent,
    payloads: &mut Vec<TileEventPayload>,
    local_bounds: &mut Vec<CanvasRect>,
    global_bounds: &mut Option<CanvasRect>,
) -> Result<(), GpuRenderError> {
    validate_tile_ref(atlas, event.raster)?;
    if let (Some(mask_atlas), Some(mask)) = (mask_atlas, event.mask) {
        validate_tile_ref(mask_atlas, mask)?;
    }
    let Some(source_bounds) = CanvasRect::from_source(
        event.source_offset_x,
        event.source_offset_y,
        event.raster.size,
        canvas_size,
    ) else {
        return Ok(());
    };
    let Some(intersection) = source_bounds.intersection(target_bounds) else {
        return Ok(());
    };
    let local = intersection
        .translate_to_local((target_bounds.x as i32, target_bounds.y as i32))
        .ok_or(GpuRenderError::InvalidImageSize)?;
    *global_bounds = union_optional(*global_bounds, Some(source_bounds));
    local_bounds.push(local);
    payloads.push(TileEventPayload::Raster(RasterTileEventPayload {
        atlas_origin: (event.raster.atlas_x, event.raster.atlas_y),
        source_size: event.raster.size,
        source_offset: (event.source_offset_x, event.source_offset_y),
        opacity: event.opacity,
        blend_mode: event.blend_mode,
        mask_atlas_origin: event.mask.map(|mask| (mask.atlas_x, mask.atlas_y)),
    }));
    Ok(())
}

fn common_raster_atlas<'a>(
    pool: &'a GpuSparseAtlasTexturePool,
    events: &[GpuSparseAtlasRasterEvent],
) -> Result<&'a GpuSparseAtlasTexture, GpuRenderError> {
    let key = common_raster_atlas_key(events)?;
    let atlas = pool
        .texture(key)
        .ok_or(GpuRenderError::MissingSparseAtlasTexture { key })?;
    validate_sparse_atlas_format(atlas, GpuSparseAtlasFormat::Rgba8)?;
    Ok(atlas)
}

fn common_mask_atlas<'a>(
    pool: &'a GpuSparseAtlasTexturePool,
    events: &[GpuSparseAtlasRasterEvent],
) -> Result<Option<&'a GpuSparseAtlasTexture>, GpuRenderError> {
    let Some(key) = common_mask_atlas_key(events)? else {
        return Ok(None);
    };
    let atlas = pool
        .texture(key)
        .ok_or(GpuRenderError::MissingSparseAtlasTexture { key })?;
    validate_sparse_atlas_format(atlas, GpuSparseAtlasFormat::R8)?;
    Ok(Some(atlas))
}

fn common_raster_atlas_key(
    events: &[GpuSparseAtlasRasterEvent],
) -> Result<GpuSparseAtlasTextureKey, GpuRenderError> {
    let first = events.first().ok_or(GpuRenderError::EmptyRasterStack)?;
    let key = first.raster.key;
    if events.iter().any(|event| event.raster.key != key) {
        return Err(GpuRenderError::NotImplemented);
    }
    Ok(key)
}

fn common_mask_atlas_key(
    events: &[GpuSparseAtlasRasterEvent],
) -> Result<Option<GpuSparseAtlasTextureKey>, GpuRenderError> {
    let mut key = None;
    for event in events {
        let Some(mask_key) = event.mask.map(|mask| mask.key) else {
            continue;
        };
        if let Some(current) = key {
            if current != mask_key {
                return Err(GpuRenderError::NotImplemented);
            }
        } else {
            key = Some(mask_key);
        }
    }
    Ok(key)
}

fn encode_prepared_resident_sparse_atlas_batch<P>(
    renderer: &crate::GpuRenderer,
    state: &mut crate::stream_state::StreamingEncoder<'_, P::Error>,
    target_origin: (i32, i32),
    prepared: &PreparedSparseAtlasRasterEvents<'_>,
    previous_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    pass_bounds: CanvasRect,
) -> Result<(), P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let atlas = prepared.atlas.ok_or(GpuRenderError::EmptyRasterStack)?;
    let atlas_view = atlas.create_view();
    let (owned_mask_atlas, mask_atlas_bytes, mask_atlas_view) = match prepared.mask_atlas {
        Some(mask_atlas) => (None, 0, mask_atlas.create_view()),
        None => {
            let (texture, bytes) =
                upload_mask_atlas_tile_texture(renderer, CanvasSize::new(1, 1), &[])
                    .map_err(P::Error::from)?;
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            (Some(texture), bytes, view)
        }
    };
    let (lut_atlas, lut_atlas_bytes) =
        upload_lut_atlas_texture(renderer, &[]).map_err(P::Error::from)?;
    let lut_atlas_view = lut_atlas.create_view(&wgpu::TextureViewDescriptor::default());
    let event_program = TileEventProgram::from_payloads(prepared.payloads.clone());
    let event_buffers = create_tile_event_storage_buffers(
        state.device(),
        "rizum_clip_resident_sparse_atlas_event_headers",
        "rizum_clip_resident_sparse_atlas_raster_payloads",
        &event_program,
    );
    let work_buffer = create_u32_storage_buffer(
        state.device(),
        "rizum_clip_resident_sparse_atlas_work_indices",
        &prepared.work_indices,
    );
    let span_buffer = create_u32_storage_buffer(
        state.device(),
        "rizum_clip_resident_sparse_atlas_spans",
        &prepared.tile_spans,
    );
    let params_buffer = create_params_buffer(state.device(), target_origin, prepared.tile_cols);
    let pipeline = state.tile_silo_pipeline();
    let bind_group = state
        .device()
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rizum_clip_resident_sparse_atlas_bind_group"),
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
        let mut pass = state
            .encoder_mut()
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rizum_clip_resident_sparse_atlas_raster_pass"),
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
    }
    crate::render_profile::record_gpu_pass_encode(pass_encode_start.elapsed());

    if let Some(texture) = owned_mask_atlas {
        state.retain_texture(texture, mask_atlas_bytes);
    }
    state.retain_texture(lut_atlas, lut_atlas_bytes);
    Ok(())
}
