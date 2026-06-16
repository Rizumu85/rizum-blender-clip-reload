use std::collections::{HashMap, HashSet};

use clip_graph::RenderNodeId;
use clip_model::{CanvasSize, LayerId};

use crate::{RuntimeError, source_crop};

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
        })
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

    fn raster_resource(
        &mut self,
        renderer: &clip_gpu::GpuRenderer,
        source: clip_gpu::GpuNormalRasterSource,
    ) -> Result<clip_gpu::GpuRasterResourceCache, Self::Error> {
        let meta = self.plan.rasters.get(&source.key).cloned().ok_or_else(|| {
            RuntimeError::Gpu(clip_gpu::GpuRenderError::MissingRasterResource {
                layer_id: source.key.layer_id,
                render_mipmap_id: source.key.render_mipmap_id,
            })
        })?;
        let visible = self
            .decode_region_for_source(source, &meta.source)?
            .ok_or(clip_gpu::GpuRenderError::InvalidImageSize)?;
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
        Ok(renderer.upload_raster_resources(&[upload])?)
    }

    fn mask_resource(
        &mut self,
        renderer: &clip_gpu::GpuRenderer,
        key: clip_gpu::GpuMaskResourceKey,
    ) -> Result<clip_gpu::GpuMaskResourceCache, Self::Error> {
        let meta = self.plan.masks.get(&key).cloned().ok_or_else(|| {
            RuntimeError::Gpu(clip_gpu::GpuRenderError::MissingMaskResource {
                layer_id: key.layer_id,
                mask_mipmap_id: key.mask_mipmap_id,
            })
        })?;
        let mask_payload = read_mask_payload_for_upload(self.container, self.canvas, &meta.source)?;
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
        for info in cache.resource_infos() {
            if self.reported_masks.insert(info.key) {
                self.mask_resources.push(info);
            }
        }
        Ok(cache)
    }
}

impl RuntimeGpuResourceProvider<'_> {
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
    ) -> Result<Option<source_crop::RasterSourceDecodeRegion>, RuntimeError> {
        if let Some(Some(region)) = self.sparse_region_for_source(source) {
            return Ok(Some(region));
        }
        if matches!(self.sparse_region_for_source(source), Some(None)) {
            return Ok(None);
        }
        Ok(source_crop::visible_raster_source_decode_region(
            metadata.pixel_size,
            metadata.offset_x,
            metadata.offset_y,
            self.canvas,
        )?)
    }
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
) -> Result<MaskUploadPayload, RuntimeError> {
    let Some(visible) = sparse::sparse_mask_source_decode_region(container, canvas, source)? else {
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
