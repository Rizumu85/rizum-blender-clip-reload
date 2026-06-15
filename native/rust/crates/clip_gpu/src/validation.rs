use clip_model::{CanvasSize, LayerId};

use crate::{
    GpuMaskResourceCache, GpuMaskResourceKey, GpuNormalRasterSource, GpuNormalStackSource,
    GpuRasterResourceCache, GpuRasterResourceInfo, GpuRenderError,
};

pub(crate) fn validate_normal_stack_sources(
    cache: &GpuRasterResourceCache,
    mask_cache: Option<&GpuMaskResourceCache>,
    output_size: CanvasSize,
    sources: &[GpuNormalStackSource],
) -> Result<Vec<GpuRasterResourceInfo>, GpuRenderError> {
    let mut drawn_resources = Vec::new();
    for source in sources {
        match source {
            GpuNormalStackSource::Raster(raster) => {
                validate_normal_raster_source(
                    cache,
                    mask_cache,
                    output_size,
                    *raster,
                    &mut drawn_resources,
                )?;
            }
            GpuNormalStackSource::ClippingRun { base, clipped } => {
                validate_normal_raster_source(
                    cache,
                    mask_cache,
                    output_size,
                    *base,
                    &mut drawn_resources,
                )?;
                for clipped_source in clipped {
                    validate_normal_raster_source(
                        cache,
                        mask_cache,
                        output_size,
                        *clipped_source,
                        &mut drawn_resources,
                    )?;
                }
            }
            GpuNormalStackSource::Container {
                children, mask_key, ..
            }
            | GpuNormalStackSource::ThroughGroup {
                children, mask_key, ..
            } => {
                if let Some(mask_key) = *mask_key {
                    validate_mask_source(mask_cache, output_size, mask_key, mask_key.layer_id)?;
                }
                drawn_resources.extend(validate_normal_stack_sources(
                    cache,
                    mask_cache,
                    output_size,
                    children,
                )?);
            }
            GpuNormalStackSource::SolidColor { .. } => {}
            GpuNormalStackSource::LutFilter {
                lut_rgba, mask_key, ..
            } => {
                if lut_rgba.len() != 256 * 4 {
                    return Err(GpuRenderError::InvalidToneCurveLutLength {
                        expected: 256 * 4,
                        actual: lut_rgba.len(),
                    });
                }
                if let Some(mask_key) = *mask_key {
                    validate_mask_source(mask_cache, output_size, mask_key, mask_key.layer_id)?;
                }
            }
        }
    }
    Ok(drawn_resources)
}

pub(crate) fn validate_mask_source(
    mask_cache: Option<&GpuMaskResourceCache>,
    output_size: CanvasSize,
    mask_key: GpuMaskResourceKey,
    owner_layer_id: LayerId,
) -> Result<(), GpuRenderError> {
    let mask_cache = mask_cache.ok_or(GpuRenderError::MissingMaskResource {
        layer_id: mask_key.layer_id,
        mask_mipmap_id: mask_key.mask_mipmap_id,
    })?;
    let mask = mask_cache
        .resource(mask_key)
        .ok_or(GpuRenderError::MissingMaskResource {
            layer_id: mask_key.layer_id,
            mask_mipmap_id: mask_key.mask_mipmap_id,
        })?;
    let info = mask.info();
    if info.size != output_size {
        return Err(GpuRenderError::MaskResourceSizeMismatch {
            layer_id: owner_layer_id,
            expected: output_size,
            actual: info.size,
        });
    }
    Ok(())
}

fn validate_normal_raster_source(
    cache: &GpuRasterResourceCache,
    mask_cache: Option<&GpuMaskResourceCache>,
    output_size: CanvasSize,
    source: GpuNormalRasterSource,
    drawn_resources: &mut Vec<GpuRasterResourceInfo>,
) -> Result<(), GpuRenderError> {
    let resource = cache
        .resource(source.key)
        .ok_or(GpuRenderError::MissingRasterResource {
            layer_id: source.key.layer_id,
            render_mipmap_id: source.key.render_mipmap_id,
        })?;
    let info = resource.info();
    drawn_resources.push(info);

    if let Some(mask_key) = source.mask_key {
        validate_mask_source(mask_cache, output_size, mask_key, source.key.layer_id)?;
    }

    Ok(())
}
