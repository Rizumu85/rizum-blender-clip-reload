use std::collections::{HashMap, HashSet};

use clip_graph::RenderNodeId;
use clip_model::{CanvasSize, LayerId, Rect};

use crate::{NormalRasterStackResourceStats, RuntimeError, source_crop};

pub(crate) mod cache;
mod sparse;

#[derive(Clone, Debug)]
struct PlannedRasterResourceMeta {
    render_node_id: RenderNodeId,
    layer_id: LayerId,
    render_mipmap_id: u32,
    source: clip_file::metadata::RasterLayerSource,
}

#[derive(Clone, Debug)]
struct PlannedMaskResourceMeta {
    render_node_id: RenderNodeId,
    layer_id: LayerId,
    mask_mipmap_id: u32,
    source: clip_file::metadata::MaskLayerSource,
}

#[derive(Debug, Default)]
pub(crate) struct GpuResourcePlan {
    rasters: HashMap<clip_gpu::GpuRasterResourceKey, PlannedRasterResourceMeta>,
    masks: HashMap<clip_gpu::GpuMaskResourceKey, PlannedMaskResourceMeta>,
}

impl GpuResourcePlan {
    pub(crate) fn insert_raster(
        &mut self,
        key: clip_gpu::GpuRasterResourceKey,
        render_node_id: RenderNodeId,
        layer_id: LayerId,
        render_mipmap_id: u32,
        source: clip_file::metadata::RasterLayerSource,
    ) {
        self.rasters.insert(
            key,
            PlannedRasterResourceMeta {
                render_node_id,
                layer_id,
                render_mipmap_id,
                source,
            },
        );
    }

    #[cfg(test)]
    pub(crate) fn mask_resource_count(&self) -> usize {
        self.masks.len()
    }

    pub(crate) fn resource_stats(&self) -> NormalRasterStackResourceStats {
        let mut stats = NormalRasterStackResourceStats::default();
        let mut rasters: Vec<_> = self.rasters.values().collect();
        rasters.sort_by_key(|meta| meta.render_node_id.0);
        for meta in rasters {
            stats.add_raster_source(&meta.source);
        }
        let mut masks: Vec<_> = self.masks.values().collect();
        masks.sort_by_key(|meta| meta.render_node_id.0);
        for meta in masks {
            stats.add_mask_source(&meta.source);
        }
        stats
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlannedGpuMaskResource {
    None,
    Key(clip_gpu::GpuMaskResourceKey),
    FullyTransparent,
    FullyOpaque,
}

#[derive(Debug)]
pub(crate) struct RuntimeGpuResourceProvider<'a> {
    container: &'a clip_file::container::ClipContainer,
    canvas: CanvasSize,
    plan: GpuResourcePlan,
    raster_regions:
        HashMap<clip_gpu::GpuRasterResourceKey, Option<source_crop::RasterSourceDecodeRegion>>,
    raster_offsets: HashMap<clip_gpu::GpuRasterResourceKey, (i32, i32)>,
    pub(crate) mask_resources: Vec<clip_gpu::GpuMaskResourceInfo>,
    reported_masks: HashSet<clip_gpu::GpuMaskResourceKey>,
    texture_cache: Option<&'a mut cache::PersistentGpuTextureCache>,
}

impl<'a> RuntimeGpuResourceProvider<'a> {
    pub(crate) fn new(
        container: &'a clip_file::container::ClipContainer,
        canvas: CanvasSize,
        plan: GpuResourcePlan,
    ) -> Result<Self, RuntimeError> {
        let raster_regions = sparse::planned_sparse_raster_regions(container, canvas, &plan)?;
        Ok(Self {
            container,
            canvas,
            plan,
            raster_regions,
            raster_offsets: HashMap::new(),
            mask_resources: Vec::new(),
            reported_masks: HashSet::new(),
            texture_cache: None,
        })
    }

    pub(crate) fn with_texture_cache(
        container: &'a clip_file::container::ClipContainer,
        canvas: CanvasSize,
        plan: GpuResourcePlan,
        texture_cache: &'a mut cache::PersistentGpuTextureCache,
    ) -> Result<Self, RuntimeError> {
        let mut provider = Self::new(container, canvas, plan)?;
        provider.texture_cache = Some(texture_cache);
        Ok(provider)
    }
}

impl clip_gpu::GpuNormalStackResourceProvider for RuntimeGpuResourceProvider<'_> {
    type Error = RuntimeError;

    fn raster_resource_size(&self, source: clip_gpu::GpuNormalRasterSource) -> Option<CanvasSize> {
        if let Some(region) = self.sparse_region_for_source(source) {
            return Some(region.map_or(CanvasSize::new(0, 0), |region| {
                CanvasSize::new(region.source_rect.width, region.source_rect.height)
            }));
        }
        self.plan
            .rasters
            .get(&source.key)
            .map(|meta| meta.source.pixel_size)
    }

    fn raster_resource_offset(
        &self,
        source: clip_gpu::GpuNormalRasterSource,
    ) -> Option<(i32, i32)> {
        if let Some(Some(region)) = self.sparse_region_for_source(source) {
            return Some((region.offset_x, region.offset_y));
        }
        self.raster_offsets.get(&source.key).copied()
    }

    fn uploaded_raster_resource_offset(
        &self,
        source: clip_gpu::GpuNormalRasterSource,
    ) -> Option<(i32, i32)> {
        self.raster_offsets
            .get(&source.key)
            .copied()
            .or_else(|| self.raster_resource_offset(source))
    }

    fn raster_resource(
        &mut self,
        renderer: &clip_gpu::GpuRenderer,
        source: clip_gpu::GpuNormalRasterSource,
    ) -> Result<clip_gpu::GpuRasterResourceCache, Self::Error> {
        self.raster_resource_for_bounds(renderer, source, None)
    }

    fn raster_resource_region(
        &mut self,
        renderer: &clip_gpu::GpuRenderer,
        source: clip_gpu::GpuNormalRasterSource,
        render_bounds: Rect,
    ) -> Result<clip_gpu::GpuRasterResourceCache, Self::Error> {
        self.raster_resource_for_bounds(renderer, source, Some(render_bounds))
    }

    fn raster_run_atlas_tile_pixels(
        &mut self,
        sources: &[clip_gpu::GpuRasterAtlasSource],
        atlas_size: CanvasSize,
    ) -> Result<Option<clip_gpu::GpuRasterAtlasTilePixels>, Self::Error> {
        if self.texture_cache.is_some() {
            return Ok(None);
        }
        self.build_raster_run_atlas_tile_pixels(sources, atlas_size)
    }

    fn mask_resource(
        &mut self,
        renderer: &clip_gpu::GpuRenderer,
        key: clip_gpu::GpuMaskResourceKey,
    ) -> Result<clip_gpu::GpuMaskResourceCache, Self::Error> {
        self.mask_resource_for_bounds(renderer, key, None)
    }

    fn mask_resource_region(
        &mut self,
        renderer: &clip_gpu::GpuRenderer,
        key: clip_gpu::GpuMaskResourceKey,
        render_bounds: Rect,
    ) -> Result<clip_gpu::GpuMaskResourceCache, Self::Error> {
        self.mask_resource_for_bounds(renderer, key, Some(render_bounds))
    }
}

impl RuntimeGpuResourceProvider<'_> {
    fn raster_resource_for_bounds(
        &mut self,
        renderer: &clip_gpu::GpuRenderer,
        source: clip_gpu::GpuNormalRasterSource,
        render_bounds: Option<Rect>,
    ) -> Result<clip_gpu::GpuRasterResourceCache, RuntimeError> {
        let meta = self.plan.rasters.get(&source.key).cloned().ok_or_else(|| {
            RuntimeError::Gpu(clip_gpu::GpuRenderError::MissingRasterResource {
                layer_id: source.key.layer_id,
                render_mipmap_id: source.key.render_mipmap_id,
            })
        })?;
        let visible = self
            .decode_region_for_source(source, &meta.source, render_bounds)?
            .ok_or(clip_gpu::GpuRenderError::InvalidImageSize)?;
        let cache_key = cache::raster_texture_cache_key(self.container, &meta.source, visible)?;
        if let Some(texture_cache) = self.texture_cache.as_mut() {
            if let Some(cache) = texture_cache.raster_cache(&cache_key) {
                self.raster_offsets
                    .insert(source.key, (visible.offset_x, visible.offset_y));
                return Ok(cache);
            }
        }
        let image = clip_file::read_resolved_raster_layer_source_rgba_region_from_container(
            self.container,
            &meta.source,
            visible.source_rect,
        )?;
        self.raster_offsets
            .insert(source.key, (visible.offset_x, visible.offset_y));
        let upload = clip_gpu::GpuRasterUpload {
            layer_id: meta.layer_id,
            render_node_id: meta.render_node_id,
            render_mipmap_id: meta.render_mipmap_id,
            size: CanvasSize::new(image.width, image.height),
            pixels: &image.pixels,
        };
        let cache = renderer.upload_raster_resources(&[upload])?;
        if let Some(texture_cache) = self.texture_cache.as_mut() {
            texture_cache.insert_raster(cache_key, cache.clone());
        }
        Ok(cache)
    }

    fn mask_resource_for_bounds(
        &mut self,
        renderer: &clip_gpu::GpuRenderer,
        key: clip_gpu::GpuMaskResourceKey,
        render_bounds: Option<Rect>,
    ) -> Result<clip_gpu::GpuMaskResourceCache, RuntimeError> {
        let meta = self.plan.masks.get(&key).cloned().ok_or_else(|| {
            RuntimeError::Gpu(clip_gpu::GpuRenderError::MissingMaskResource {
                layer_id: key.layer_id,
                mask_mipmap_id: key.mask_mipmap_id,
            })
        })?;
        let mask_payload =
            read_mask_payload_for_upload(self.container, self.canvas, &meta.source, render_bounds)?;
        let cache_key = cache::mask_texture_cache_key(self.container, &meta.source, &mask_payload)?;
        if let Some(texture_cache) = self.texture_cache.as_mut() {
            if let Some(cache) = texture_cache.mask_cache(&cache_key) {
                self.report_mask_infos(&cache);
                return Ok(cache);
            }
        }
        let upload = clip_gpu::GpuMaskUpload {
            layer_id: meta.layer_id,
            render_node_id: meta.render_node_id,
            mask_mipmap_id: meta.mask_mipmap_id,
            size: CanvasSize::new(mask_payload.image.width, mask_payload.image.height),
            origin_x: mask_payload.origin_x,
            origin_y: mask_payload.origin_y,
            fill_value: mask_payload.fill_value,
            upload_origin_x: mask_payload.upload_origin_x,
            upload_origin_y: mask_payload.upload_origin_y,
            upload_size: CanvasSize::new(mask_payload.image.width, mask_payload.image.height),
            pixels: &mask_payload.image.pixels,
        };
        let cache = renderer.upload_mask_resources(&[upload])?;
        self.report_mask_infos(&cache);
        if let Some(texture_cache) = self.texture_cache.as_mut() {
            texture_cache.insert_mask(cache_key, cache.clone());
        }
        Ok(cache)
    }

    fn sparse_region_for_source(
        &self,
        source: clip_gpu::GpuNormalRasterSource,
    ) -> Option<Option<source_crop::RasterSourceDecodeRegion>> {
        self.raster_regions.get(&source.key).copied()
    }

    fn decode_region_for_source(
        &self,
        source: clip_gpu::GpuNormalRasterSource,
        metadata: &clip_file::metadata::RasterLayerSource,
        render_bounds: Option<Rect>,
    ) -> Result<Option<source_crop::RasterSourceDecodeRegion>, RuntimeError> {
        let region = if let Some(Some(region)) = self.sparse_region_for_source(source) {
            Some(region)
        } else if matches!(self.sparse_region_for_source(source), Some(None)) {
            None
        } else {
            source_crop::visible_raster_source_decode_region(
                metadata.pixel_size,
                metadata.offset_x,
                metadata.offset_y,
                self.canvas,
            )?
        };
        clip_region_to_render_bounds(region, render_bounds)
    }

    fn build_raster_run_atlas_tile_pixels(
        &mut self,
        sources: &[clip_gpu::GpuRasterAtlasSource],
        atlas_size: CanvasSize,
    ) -> Result<Option<clip_gpu::GpuRasterAtlasTilePixels>, RuntimeError> {
        if sources.is_empty() {
            return Ok(None);
        }

        let mut chunks = Vec::new();
        let mut resources = Vec::with_capacity(sources.len());
        for request in sources {
            let meta = self
                .plan
                .rasters
                .get(&request.source.key)
                .cloned()
                .ok_or_else(|| {
                    RuntimeError::Gpu(clip_gpu::GpuRenderError::MissingRasterResource {
                        layer_id: request.source.key.layer_id,
                        render_mipmap_id: request.source.key.render_mipmap_id,
                    })
                })?;
            let visible = self
                .decode_region_for_source(request.source, &meta.source, None)?
                .ok_or(clip_gpu::GpuRenderError::InvalidImageSize)?;
            if request.size
                != CanvasSize::new(visible.source_rect.width, visible.source_rect.height)
                || request.offset_x != visible.offset_x
                || request.offset_y != visible.offset_y
            {
                return Ok(None);
            }

            let source_chunks =
                clip_file::read_resolved_raster_layer_source_rgba_region_atlas_chunks_from_container(
                    self.container,
                    &meta.source,
                    visible.source_rect,
                    atlas_size,
                    request.atlas_x,
                    request.atlas_y,
                )?;
            for chunk in source_chunks {
                let local_x = chunk
                    .x
                    .checked_sub(request.atlas_x)
                    .ok_or(clip_gpu::GpuRenderError::TextureSizeOverflow)?;
                let local_y = chunk
                    .y
                    .checked_sub(request.atlas_y)
                    .ok_or(clip_gpu::GpuRenderError::TextureSizeOverflow)?;
                let offset_x = i32::try_from(i64::from(visible.offset_x) + i64::from(local_x))
                    .map_err(|_| clip_gpu::GpuRenderError::TextureSizeOverflow)?;
                let offset_y = i32::try_from(i64::from(visible.offset_y) + i64::from(local_y))
                    .map_err(|_| clip_gpu::GpuRenderError::TextureSizeOverflow)?;
                let size = CanvasSize::new(chunk.width, chunk.height);
                chunks.push(clip_gpu::GpuRasterAtlasTileChunk {
                    source: request.source,
                    atlas_x: chunk.x,
                    atlas_y: chunk.y,
                    size,
                    offset_x,
                    offset_y,
                    pixels: chunk.pixels,
                });
            }

            self.raster_offsets
                .insert(request.source.key, (visible.offset_x, visible.offset_y));
            resources.push(clip_gpu::GpuRasterResourceInfo {
                key: request.source.key,
                render_node_id: meta.render_node_id,
                size: request.size,
                byte_len: rgba_byte_len(request.size)?,
            });
        }

        Ok(Some(clip_gpu::GpuRasterAtlasTilePixels {
            size: atlas_size,
            chunks,
            resources,
        }))
    }

    fn report_mask_infos(&mut self, cache: &clip_gpu::GpuMaskResourceCache) {
        for info in cache.resource_infos() {
            if self.reported_masks.insert(info.key) {
                self.mask_resources.push(info);
            }
        }
    }
}

pub(crate) fn rgba_byte_len(size: CanvasSize) -> Result<usize, RuntimeError> {
    usize::try_from(
        u64::from(size.width)
            .checked_mul(u64::from(size.height))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(clip_gpu::GpuRenderError::TextureSizeOverflow)?,
    )
    .map_err(|_| clip_gpu::GpuRenderError::TextureSizeOverflow.into())
}

pub(crate) fn plan_gpu_mask_resource(
    mask_sources: &HashMap<LayerId, clip_file::metadata::MaskLayerSource>,
    node: &clip_graph::RenderNode,
    canvas: CanvasSize,
    resource_plan: &mut GpuResourcePlan,
) -> Result<PlannedGpuMaskResource, RuntimeError> {
    let Some(mask_mipmap_id) = node.mask_mipmap_id else {
        return Ok(PlannedGpuMaskResource::None);
    };
    let source = mask_sources.get(&node.layer_id).cloned().ok_or(
        clip_file::ClipFileError::LayerHasNoMask {
            layer_id: node.layer_id,
        },
    )?;
    if source_crop::visible_raster_source_decode_region(
        source.pixel_size,
        source.offset_x,
        source.offset_y,
        canvas,
    )?
    .is_none()
    {
        return Ok(match source.empty_fill {
            0 => PlannedGpuMaskResource::FullyTransparent,
            255 => PlannedGpuMaskResource::FullyOpaque,
            _ => planned_gpu_mask_key(node, mask_mipmap_id, source, resource_plan),
        });
    }

    Ok(planned_gpu_mask_key(
        node,
        mask_mipmap_id,
        source,
        resource_plan,
    ))
}

fn planned_gpu_mask_key(
    node: &clip_graph::RenderNode,
    mask_mipmap_id: u32,
    source: clip_file::metadata::MaskLayerSource,
    resource_plan: &mut GpuResourcePlan,
) -> PlannedGpuMaskResource {
    let key = clip_gpu::GpuMaskResourceKey {
        layer_id: node.layer_id,
        mask_mipmap_id,
    };
    resource_plan.masks.insert(
        key,
        PlannedMaskResourceMeta {
            render_node_id: node.id,
            layer_id: node.layer_id,
            mask_mipmap_id,
            source,
        },
    );
    PlannedGpuMaskResource::Key(key)
}

struct MaskUploadPayload {
    image: clip_file::tiles::AlphaTileImage,
    origin_x: i32,
    origin_y: i32,
    fill_value: u8,
    upload_origin_x: u32,
    upload_origin_y: u32,
}

fn read_mask_payload_for_upload(
    container: &clip_file::container::ClipContainer,
    canvas: CanvasSize,
    source: &clip_file::metadata::MaskLayerSource,
    render_bounds: Option<Rect>,
) -> Result<MaskUploadPayload, RuntimeError> {
    let visible = sparse::sparse_mask_source_decode_region(container, canvas, source)?;
    let Some(visible) = clip_region_to_render_bounds(visible, render_bounds)? else {
        return Ok(MaskUploadPayload {
            image: clip_file::tiles::AlphaTileImage {
                width: 1,
                height: 1,
                pixels: vec![source.empty_fill],
            },
            origin_x: 0,
            origin_y: 0,
            fill_value: source.empty_fill,
            upload_origin_x: 0,
            upload_origin_y: 0,
        });
    };

    let image = clip_file::read_resolved_layer_mask_alpha_region_from_container(
        container,
        source,
        visible.source_rect,
    )?;
    Ok(MaskUploadPayload {
        image,
        origin_x: visible.offset_x,
        origin_y: visible.offset_y,
        fill_value: source.empty_fill,
        upload_origin_x: 0,
        upload_origin_y: 0,
    })
}

fn clip_region_to_render_bounds(
    region: Option<source_crop::RasterSourceDecodeRegion>,
    render_bounds: Option<Rect>,
) -> Result<Option<source_crop::RasterSourceDecodeRegion>, RuntimeError> {
    let Some(region) = region else {
        return Ok(None);
    };
    let Some(render_bounds) = render_bounds else {
        return Ok(Some(region));
    };
    Ok(source_crop::clip_decode_region_to_canvas_rect(
        region,
        render_bounds,
    )?)
}
