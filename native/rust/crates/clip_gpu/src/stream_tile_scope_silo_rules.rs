use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_clipping_tile_silo::clipping_run_as_normal_sources;
use crate::{GpuClippedStackSource, GpuMaskResourceKey, GpuNormalStackSource};

pub(crate) fn scope_mask_can_lower<P>(provider: &P, mask_key: Option<GpuMaskResourceKey>) -> bool
where
    P: GpuNormalStackResourceProvider,
{
    match mask_key {
        Some(key) => {
            provider.mask_is_fully_opaque(key) == Some(true)
                || provider.mask_atlas_tiles_supported()
        }
        None => true,
    }
}

pub(crate) fn simple_scope_clipping_run_can_lower(clipped: &[GpuClippedStackSource]) -> bool {
    clipped
        .iter()
        .all(|source| matches!(source, GpuClippedStackSource::Raster(_)))
}

pub(crate) fn raster_sources_from_scope_children(
    children: &[GpuNormalStackSource],
) -> Vec<GpuNormalStackSource> {
    let mut sources = Vec::new();
    append_raster_sources(children, &mut sources);
    sources
}

fn append_raster_sources(
    children: &[GpuNormalStackSource],
    sources: &mut Vec<GpuNormalStackSource>,
) {
    for child in children {
        match child {
            GpuNormalStackSource::Raster(raster) => {
                sources.push(GpuNormalStackSource::Raster(*raster));
            }
            GpuNormalStackSource::Container { children, .. } => {
                append_raster_sources(children, sources);
            }
            GpuNormalStackSource::ThroughGroup { children, .. } => {
                append_raster_sources(children, sources);
            }
            GpuNormalStackSource::ClippingRun { base, clipped } => {
                if let Some(normal_sources) = clipping_run_as_normal_sources(*base, clipped) {
                    sources.extend(normal_sources);
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn raster_sources_have_masks(sources: &[GpuNormalStackSource]) -> bool {
    sources.iter().any(|source| match source {
        GpuNormalStackSource::Raster(raster) => raster.mask_key.is_some(),
        _ => false,
    })
}
