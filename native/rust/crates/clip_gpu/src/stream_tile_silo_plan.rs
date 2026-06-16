use clip_model::CanvasSize;

use crate::blend::blend_kind;
use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::CanvasRect;
use crate::stream_effects::raster_can_affect_output;
use crate::{
    GpuNormalRasterSource, GpuNormalStackSource, GpuRasterBlendMode, GpuRasterResourceCache,
    GpuRasterResourceInfo, GpuRenderError,
};

pub(crate) const TILE_SIZE: u32 = 256;
pub(crate) const MIN_SILO_RUN_LEN: usize = 2;
const MAX_SILO_EVENTS: usize = 256;
const EVENT_WORDS: usize = 8;

#[derive(Clone, Copy)]
pub(crate) struct AtlasSourcePlacement {
    pub(crate) x: u32,
    pub(crate) y: u32,
}

pub(crate) struct AtlasLayout {
    pub(crate) size: CanvasSize,
    pub(crate) sources: Vec<AtlasSourcePlacement>,
}

#[derive(Clone)]
pub(crate) struct PreparedSiloSource {
    pub(crate) source: GpuNormalRasterSource,
    pub(crate) cache: Option<GpuRasterResourceCache>,
    pub(crate) info: GpuRasterResourceInfo,
    pub(crate) offset: (i32, i32),
    pub(crate) bounds: CanvasRect,
    pub(crate) local_bounds: CanvasRect,
    pub(crate) atlas: AtlasSourcePlacement,
}

pub(crate) fn raster_silo_run_len<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    sources: &[GpuNormalStackSource],
) -> usize
where
    P: GpuNormalStackResourceProvider,
{
    sources
        .iter()
        .take(MAX_SILO_EVENTS)
        .take_while(|source| {
            source_is_silo_eligible(provider, output_size, target_origin, target_size, source)
        })
        .count()
}

pub(crate) fn plan_atlas_layout<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    sources: &[GpuNormalStackSource],
) -> Option<AtlasLayout>
where
    P: GpuNormalStackResourceProvider,
{
    let max_texture_size = 8192u32;
    let sizes: Vec<_> = sources
        .iter()
        .map(|source| {
            let GpuNormalStackSource::Raster(raster) = source else {
                return None;
            };
            let size = provider.raster_resource_size(*raster)?;
            let offset = provider
                .raster_resource_offset(*raster)
                .unwrap_or((raster.offset_x, raster.offset_y));
            source_bounds(offset, size, output_size)?;
            source_local_bounds(offset, size, target_origin, target_size)?;
            Some(size)
        })
        .collect::<Option<_>>()?;
    let max_width = sizes.iter().map(|size| size.width).max()?;
    if max_width > max_texture_size {
        return None;
    }
    let total_area = sizes.iter().try_fold(0u64, |total, size| {
        total.checked_add(u64::from(size.width) * u64::from(size.height))
    })?;
    let target_width = ceil_sqrt_u64(total_area)
        .max(u64::from(max_width))
        .min(u64::from(max_texture_size)) as u32;
    let mut x = 0u32;
    let mut y = 0u32;
    let mut row_height = 0u32;
    let mut placements = Vec::with_capacity(sizes.len());
    for size in sizes {
        if x > 0 && x.checked_add(size.width)? > target_width {
            y = y.checked_add(row_height)?;
            x = 0;
            row_height = 0;
        }
        let bottom = y.checked_add(size.height)?;
        if bottom > max_texture_size {
            return None;
        }
        placements.push(AtlasSourcePlacement { x, y });
        x = x.checked_add(size.width)?;
        row_height = row_height.max(size.height);
    }
    let height = y.checked_add(row_height)?;
    if height == 0 || height > max_texture_size {
        return None;
    }
    Some(AtlasLayout {
        size: CanvasSize::new(target_width, height),
        sources: placements,
    })
}

pub(crate) fn source_bounds(
    offset: (i32, i32),
    size: CanvasSize,
    output_size: CanvasSize,
) -> Option<CanvasRect> {
    let x0 = i64::from(offset.0).max(0);
    let y0 = i64::from(offset.1).max(0);
    let x1 = (i64::from(offset.0) + i64::from(size.width)).min(i64::from(output_size.width));
    let y1 = (i64::from(offset.1) + i64::from(size.height)).min(i64::from(output_size.height));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(CanvasRect {
        x: u32::try_from(x0).ok()?,
        y: u32::try_from(y0).ok()?,
        width: u32::try_from(x1 - x0).ok()?,
        height: u32::try_from(y1 - y0).ok()?,
    })
}

pub(crate) fn source_local_bounds(
    offset: (i32, i32),
    size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
) -> Option<CanvasRect> {
    let target_x0 = i64::from(target_origin.0);
    let target_y0 = i64::from(target_origin.1);
    let target_x1 = target_x0 + i64::from(target_size.width);
    let target_y1 = target_y0 + i64::from(target_size.height);
    let x0 = i64::from(offset.0).max(target_x0);
    let y0 = i64::from(offset.1).max(target_y0);
    let x1 = (i64::from(offset.0) + i64::from(size.width)).min(target_x1);
    let y1 = (i64::from(offset.1) + i64::from(size.height)).min(target_y1);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(CanvasRect {
        x: u32::try_from(x0 - target_x0).ok()?,
        y: u32::try_from(y0 - target_y0).ok()?,
        width: u32::try_from(x1 - x0).ok()?,
        height: u32::try_from(y1 - y0).ok()?,
    })
}

pub(crate) fn event_words(sources: &[PreparedSiloSource]) -> Vec<u32> {
    let mut words = Vec::with_capacity(sources.len() * EVENT_WORDS);
    for source in sources {
        words.extend_from_slice(&[
            source.atlas.x,
            source.atlas.y,
            source.info.size.width,
            source.info.size.height,
            i32_bits(source.offset.0),
            i32_bits(source.offset.1),
            source.source.opacity.to_bits(),
            blend_kind(source.source.blend_mode),
        ]);
    }
    words
}

pub(crate) fn tile_work_lists(
    tile_count: usize,
    tile_cols: u32,
    sources: &[PreparedSiloSource],
) -> Result<(Vec<u32>, Vec<u32>), GpuRenderError> {
    let mut by_tile = vec![Vec::<u32>::new(); tile_count];
    for (event_index, source) in sources.iter().enumerate() {
        let tile_x0 = source.local_bounds.x / TILE_SIZE;
        let tile_y0 = source.local_bounds.y / TILE_SIZE;
        let tile_x1 = (source.local_bounds.x + source.local_bounds.width - 1) / TILE_SIZE;
        let tile_y1 = (source.local_bounds.y + source.local_bounds.height - 1) / TILE_SIZE;
        for tile_y in tile_y0..=tile_y1 {
            for tile_x in tile_x0..=tile_x1 {
                let tile_index =
                    usize::try_from(u64::from(tile_y) * u64::from(tile_cols) + u64::from(tile_x))
                        .map_err(|_| GpuRenderError::TextureSizeOverflow)?;
                by_tile
                    .get_mut(tile_index)
                    .ok_or(GpuRenderError::TextureSizeOverflow)?
                    .push(
                        u32::try_from(event_index)
                            .map_err(|_| GpuRenderError::TextureSizeOverflow)?,
                    );
            }
        }
    }

    let mut work_indices = Vec::new();
    let mut spans = Vec::with_capacity(tile_count * 2);
    for entries in by_tile {
        spans.push(
            u32::try_from(work_indices.len()).map_err(|_| GpuRenderError::TextureSizeOverflow)?,
        );
        spans.push(u32::try_from(entries.len()).map_err(|_| GpuRenderError::TextureSizeOverflow)?);
        work_indices.extend(entries);
    }
    Ok((work_indices, spans))
}

fn source_is_silo_eligible<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    source: &GpuNormalStackSource,
) -> bool
where
    P: GpuNormalStackResourceProvider,
{
    let GpuNormalStackSource::Raster(raster) = source else {
        return false;
    };
    if !raster_can_affect_output(*raster)
        || raster.mask_key.is_some()
        || !blend_is_silo_eligible(raster.blend_mode)
    {
        return false;
    }
    let Some(size) = provider.raster_resource_size(*raster) else {
        return false;
    };
    if size.width == 0 || size.height == 0 {
        return false;
    }
    let offset = provider
        .raster_resource_offset(*raster)
        .unwrap_or((raster.offset_x, raster.offset_y));
    source_bounds(offset, size, output_size).is_some()
        && source_local_bounds(offset, size, target_origin, target_size).is_some()
}

fn blend_is_silo_eligible(blend_mode: GpuRasterBlendMode) -> bool {
    !matches!(
        blend_mode,
        GpuRasterBlendMode::AddGlow
            | GpuRasterBlendMode::ColorBurn
            | GpuRasterBlendMode::ColorDodge
            | GpuRasterBlendMode::GlowDodge
    )
}

fn ceil_sqrt_u64(value: u64) -> u64 {
    let mut root = (value as f64).sqrt().ceil() as u64;
    while root.saturating_mul(root) < value {
        root += 1;
    }
    root
}

fn i32_bits(value: i32) -> u32 {
    u32::from_ne_bytes(value.to_ne_bytes())
}
