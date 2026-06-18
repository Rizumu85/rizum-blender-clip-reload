use clip_model::CanvasSize;

use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::{CanvasRect, target_canvas_bounds, union_optional};
use crate::stream_context::StreamingExecutionContext;
use crate::stream_tile_event::{PointFilterTileEventPayload, TileEventPayload};
use crate::stream_tile_filter_program::{FilterTileProgramInputs, encode_filter_tile_program};
use crate::stream_tile_silo::{
    atlas_requests, prepared_sources_from_atlas_tiles, prepared_sources_from_atlas_upload,
};
use crate::stream_tile_silo_plan::{
    MAX_SILO_EVENTS, MIN_SILO_RUN_LEN, PreparedSiloSource, plan_atlas_layout,
    source_is_silo_eligible,
};
use crate::stream_tile_silo_upload::{
    upload_atlas_texture, upload_atlas_tile_texture, upload_mask_atlas_tile_texture,
};
use crate::stream_utils::local_pass_bounds;
use crate::{
    GpuMaskAtlasTileChunk, GpuMaskResourceKey, GpuNormalStackSource, GpuRasterAtlasPixels,
    GpuRasterAtlasSource, GpuRasterResourceInfo, GpuRenderError,
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

pub(crate) fn point_filter_silo_run_len<P>(
    provider: &P,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    sources: &[GpuNormalStackSource],
) -> usize
where
    P: GpuNormalStackResourceProvider,
{
    if target_canvas_bounds(target_origin, target_size).is_none() {
        return 0;
    }

    let mut len = 0usize;
    for source in sources.iter().take(MAX_SILO_EVENTS) {
        match source {
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
                len += 1;
            }
            _ => break,
        }
    }

    len
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

    let Some(pass_bounds) = encode_filter_tile_program(
        context,
        target_origin,
        target_size,
        layout.size,
        upload.atlas,
        upload.mask_atlas,
        upload.mask_atlas_bytes,
        program_inputs,
        previous_view,
        output_view,
    )?
    else {
        return Ok(false);
    };
    *dirty_bounds = Some(pass_bounds);
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_point_filter_silo_run_with_provider<P>(
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
    if point_filter_silo_run_len(&*context.provider, target_origin, target_size, sources)
        != sources.len()
    {
        return Ok(false);
    }

    let Some(program_inputs) = build_filter_event_program_inputs(
        context,
        target_origin,
        target_size,
        sources,
        Vec::new(),
        *dirty_bounds,
    )?
    else {
        return Ok(false);
    };
    let atlas_size = CanvasSize::new(1, 1);
    let atlas = upload_atlas_texture(
        context.renderer,
        &GpuRasterAtlasPixels {
            size: atlas_size,
            pixels: vec![0, 0, 0, 0],
            resources: Vec::new(),
        },
    )
    .map_err(P::Error::from)?;
    let (mask_atlas, mask_atlas_bytes) =
        upload_mask_atlas_tile_texture(context.renderer, atlas_size, &[])
            .map_err(P::Error::from)?;
    let Some(pass_bounds) = encode_filter_tile_program(
        context,
        target_origin,
        target_size,
        atlas_size,
        atlas,
        mask_atlas,
        mask_atlas_bytes,
        program_inputs,
        previous_view,
        output_view,
    )?
    else {
        return Ok(false);
    };
    *dirty_bounds = Some(pass_bounds);
    Ok(true)
}

pub(crate) struct RasterUploadBundle {
    pub(crate) prepared: Vec<PreparedSiloSource>,
    pub(crate) atlas: wgpu::Texture,
    pub(crate) mask_atlas: wgpu::Texture,
    pub(crate) mask_atlas_bytes: usize,
    pub(crate) mask_atlas_size: CanvasSize,
    pub(crate) mask_chunks: Vec<GpuMaskAtlasTileChunk>,
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
        let mask_chunks = upload.mask_chunks.clone();
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
            mask_atlas_size: upload.size,
            mask_chunks,
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
        mask_atlas_size: upload.size,
        mask_chunks: Vec::new(),
        drawn_resources: upload.resources,
    }))
}

fn build_filter_event_program_inputs<'a, P>(
    context: &StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    sources: &'a [GpuNormalStackSource],
    prepared: Vec<PreparedSiloSource>,
    initial_dirty_bounds: Option<CanvasRect>,
) -> Result<Option<FilterTileProgramInputs<'a>>, P::Error>
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
    Ok(Some(FilterTileProgramInputs {
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
