use super::RuntimeGpuRenderer;
use crate::gpu_provider::{
    atlas_events::{sparse_atlas_raster_event_plan, sparse_atlas_raster_suffix_event_plan},
    atlas_upload::sparse_atlas_texture_pool_updates,
};
use crate::{
    ClipSession, GpuTextureCacheStats, NormalRasterStackGpuPatchResult, RuntimeError,
    stack_plan::GpuRenderStackSelection,
};

impl RuntimeGpuRenderer {
    pub fn plan_sparse_atlas_reload(
        &self,
        plan: &crate::ReloadDiffPlan,
    ) -> crate::GpuSparseAtlasReloadPlan {
        self.sparse_atlas_cache
            .borrow_mut()
            .plan_reload_diff(plan)
            .into()
    }

    pub fn prepare_sparse_atlas_raster_event_plan(
        &self,
        session: &ClipSession,
        plan: &crate::ReloadDiffPlan,
    ) -> Result<crate::GpuSparseAtlasPreparedRasterEventPlan, RuntimeError> {
        let selection =
            session.select_gpu_normal_render_stack(crate::tile_silo_options::tile_silo_options())?;
        let reload = self.sparse_atlas_cache.borrow_mut().plan_reload_diff(plan);
        let updates = sparse_atlas_texture_pool_updates(session, &reload.cache)?;
        let texture_pool_stats = self
            .renderer
            .update_sparse_atlas_texture_pool(
                &mut self.sparse_atlas_textures.borrow_mut(),
                &updates,
            )
            .map_err(RuntimeError::from)?;
        Ok(crate::GpuSparseAtlasPreparedRasterEventPlan {
            texture_pool_stats,
            event_plan: sparse_atlas_raster_event_plan(plan, &reload, &selection.sources).into(),
        })
    }

    pub fn prepare_sparse_atlas_raster_suffix_patch_plan(
        &self,
        session: &ClipSession,
        plan: &crate::ReloadDiffPlan,
    ) -> Result<crate::GpuSparseAtlasPreparedRasterEventPlan, RuntimeError> {
        let selection =
            session.select_gpu_normal_render_stack(crate::tile_silo_options::tile_silo_options())?;
        let reload = self.sparse_atlas_cache.borrow_mut().plan_reload_diff(plan);
        let updates = sparse_atlas_texture_pool_updates(session, &reload.cache)?;
        let texture_pool_stats = self
            .renderer
            .update_sparse_atlas_texture_pool(
                &mut self.sparse_atlas_textures.borrow_mut(),
                &updates,
            )
            .map_err(RuntimeError::from)?;
        Ok(crate::GpuSparseAtlasPreparedRasterEventPlan {
            texture_pool_stats,
            event_plan: sparse_atlas_raster_suffix_event_plan(plan, &reload, &selection.sources)
                .into(),
        })
    }

    pub fn draw_sparse_atlas_initial_suffix_patches(
        &self,
        session: &ClipSession,
        plan: &crate::ReloadDiffPlan,
    ) -> Result<
        Option<(
            NormalRasterStackGpuPatchResult,
            crate::GpuSparseAtlasReloadPlan,
        )>,
        RuntimeError,
    > {
        if plan.mode != crate::ReloadDiffMode::Patch
            || plan.dirty_rects.is_empty()
            || !suffix_starts_at_initial_accumulator(plan)
            || !suffix_manifest_is_raster_only(plan)
        {
            return Ok(None);
        }

        let selection =
            session.select_gpu_normal_render_stack(crate::tile_silo_options::tile_silo_options())?;
        let GpuRenderStackSelection {
            sources,
            resource_plan,
            unsupported,
        } = selection;
        if !unsupported.is_empty() {
            return Ok(None);
        }
        let source_count = sources.len();
        let resource_stats = resource_plan.resource_stats();
        let reload = self.sparse_atlas_cache.borrow_mut().plan_reload_diff(plan);
        let sparse_atlas = reload.clone().into();
        let event_plan = sparse_atlas_raster_suffix_event_plan(plan, &reload, &sources);
        if !event_plan.skipped_segments.is_empty() || event_plan.segments.is_empty() {
            return Ok(None);
        }

        let updates = sparse_atlas_texture_pool_updates(session, &reload.cache)?;
        self.renderer
            .update_sparse_atlas_texture_pool(
                &mut self.sparse_atlas_textures.borrow_mut(),
                &updates,
            )
            .map_err(RuntimeError::from)?;
        let batches = event_plan
            .segments
            .iter()
            .flat_map(|segment| segment.batches.iter().cloned())
            .collect::<Vec<_>>();
        let rects = plan
            .dirty_rects
            .iter()
            .map(|rect| clip_model::Rect::new(rect.x, rect.y, rect.width, rect.height))
            .collect::<Vec<_>>();
        let base = initial_transparent_rgba8(session.summary.canvas)?;
        let output = self
            .renderer
            .draw_sparse_atlas_raster_event_batch_patches_over_rgba8(
                session.summary.canvas,
                &self.sparse_atlas_textures.borrow(),
                &batches,
                &base,
                &rects,
            )?;
        Ok(Some((
            NormalRasterStackGpuPatchResult {
                payload: output.payload,
                source_count,
                resource_stats,
                texture_cache_stats: GpuTextureCacheStats::default(),
                drawn_resources: Vec::new(),
                mask_resources: Vec::new(),
                unsupported: Vec::new(),
            },
            sparse_atlas,
        )))
    }

    pub fn prepare_sparse_atlas_textures(
        &self,
        session: &ClipSession,
        plan: &crate::ReloadDiffPlan,
    ) -> Result<clip_gpu::GpuSparseAtlasTexturePoolStats, RuntimeError> {
        let reload = self.sparse_atlas_cache.borrow_mut().plan_reload_diff(plan);
        let updates = sparse_atlas_texture_pool_updates(session, &reload.cache)?;
        self.renderer
            .update_sparse_atlas_texture_pool(
                &mut self.sparse_atlas_textures.borrow_mut(),
                &updates,
            )
            .map_err(RuntimeError::from)
    }

    pub fn draw_sparse_atlas_raster_event_segment_to_rgba8(
        &self,
        session: &ClipSession,
        segment: &crate::GpuSparseAtlasRasterEventSegment,
    ) -> Result<clip_file::tiles::RgbaTileImage, RuntimeError> {
        let output = self
            .renderer
            .draw_sparse_atlas_raster_event_batches_to_rgba8(
                session.summary.canvas,
                &self.sparse_atlas_textures.borrow(),
                &segment.batches,
            )?;
        Ok(clip_file::tiles::RgbaTileImage {
            width: output.size.width,
            height: output.size.height,
            pixels: output.pixels,
        })
    }

    pub fn draw_sparse_atlas_raster_event_segment_over_rgba8(
        &self,
        session: &ClipSession,
        segment: &crate::GpuSparseAtlasRasterEventSegment,
        base_pixels: &[u8],
    ) -> Result<clip_file::tiles::RgbaTileImage, RuntimeError> {
        let output = self
            .renderer
            .draw_sparse_atlas_raster_event_batches_over_rgba8(
                session.summary.canvas,
                &self.sparse_atlas_textures.borrow(),
                &segment.batches,
                base_pixels,
            )?;
        Ok(clip_file::tiles::RgbaTileImage {
            width: output.size.width,
            height: output.size.height,
            pixels: output.pixels,
        })
    }

    pub fn draw_sparse_atlas_raster_event_segment_patches_over_rgba8(
        &self,
        session: &ClipSession,
        segment: &crate::GpuSparseAtlasRasterEventSegment,
        base_pixels: &[u8],
        rects: &[crate::ReloadPatchRect],
    ) -> Result<Vec<u8>, RuntimeError> {
        let rects = rects
            .iter()
            .map(|rect| clip_model::Rect::new(rect.x, rect.y, rect.width, rect.height))
            .collect::<Vec<_>>();
        let output = self
            .renderer
            .draw_sparse_atlas_raster_event_batch_patches_over_rgba8(
                session.summary.canvas,
                &self.sparse_atlas_textures.borrow(),
                &segment.batches,
                base_pixels,
                &rects,
            )?;
        Ok(output.payload)
    }
}

fn suffix_starts_at_initial_accumulator(plan: &crate::ReloadDiffPlan) -> bool {
    let Some(first_dirty_ordinal) = plan
        .dirty_segments
        .iter()
        .map(|segment| segment.ordinal)
        .min()
    else {
        return false;
    };
    plan.manifest
        .segments
        .iter()
        .find(|segment| segment.ordinal == first_dirty_ordinal)
        .is_some_and(|segment| segment.source_start == 0)
}

fn suffix_manifest_is_raster_only(plan: &crate::ReloadDiffPlan) -> bool {
    let Some(first_dirty_ordinal) = plan
        .dirty_segments
        .iter()
        .map(|segment| segment.ordinal)
        .min()
    else {
        return false;
    };
    plan.manifest
        .segments
        .iter()
        .filter(|segment| segment.ordinal >= first_dirty_ordinal)
        .all(|segment| segment.kind == "RasterRun")
}

fn initial_transparent_rgba8(size: clip_model::CanvasSize) -> Result<Vec<u8>, RuntimeError> {
    let len = usize::try_from(
        u64::from(size.width)
            .checked_mul(u64::from(size.height))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(clip_gpu::GpuRenderError::TextureSizeOverflow)?,
    )
    .map_err(|_| clip_gpu::GpuRenderError::TextureSizeOverflow)?;
    let mut pixels = vec![0u8; len];
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[255, 255, 255, 0]);
    }
    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::{
        initial_transparent_rgba8, suffix_manifest_is_raster_only,
        suffix_starts_at_initial_accumulator,
    };

    #[test]
    fn initial_transparent_base_uses_white_rgb_zero_alpha() {
        let pixels =
            initial_transparent_rgba8(clip_model::CanvasSize::new(2, 1)).expect("base pixels");

        assert_eq!(pixels, vec![255, 255, 255, 0, 255, 255, 255, 0]);
    }

    #[test]
    fn suffix_initial_base_requires_first_dirty_segment_at_source_zero() {
        let mut plan = patch_plan_with_segment_start(0);
        assert!(suffix_starts_at_initial_accumulator(&plan));

        plan.manifest.segments[0].source_start = 1;
        assert!(!suffix_starts_at_initial_accumulator(&plan));
    }

    #[test]
    fn suffix_initial_base_requires_raster_only_suffix() {
        let mut plan = patch_plan_with_segment_start(0);
        assert!(suffix_manifest_is_raster_only(&plan));

        plan.manifest.segments.push(crate::ReloadDiffSegment {
            ordinal: 8,
            depth: 0,
            source_start: 1,
            source_end: 2,
            kind: "Barrier".to_string(),
            barrier_reason: Some("SolidColorNotLowered".to_string()),
            expected_passes: 1,
            tile_events: 0,
            legacy_sources: 1,
            resources: Vec::new(),
            tile_work_list_source_count: 0,
            tile_work_list_tile_count: 0,
            tile_work_list_signature: 0,
            tile_work_list: Vec::new(),
            signature: 0,
        });
        assert!(!suffix_manifest_is_raster_only(&plan));
    }

    fn patch_plan_with_segment_start(source_start: u32) -> crate::ReloadDiffPlan {
        crate::ReloadDiffPlan {
            manifest: crate::ReloadDiffManifest {
                abi: 4,
                tile_size: 256,
                tile_event_abi_version: clip_gpu::TILE_EVENT_ABI_VERSION,
                width: 2,
                height: 1,
                root_layer_id: 1,
                nodes: Vec::new(),
                sources: Vec::new(),
                segments: vec![crate::ReloadDiffSegment {
                    ordinal: 7,
                    depth: 0,
                    source_start,
                    source_end: source_start + 1,
                    kind: "RasterRun".to_string(),
                    barrier_reason: None,
                    expected_passes: 1,
                    tile_events: 1,
                    legacy_sources: 0,
                    resources: Vec::new(),
                    tile_work_list_source_count: 0,
                    tile_work_list_tile_count: 0,
                    tile_work_list_signature: 0,
                    tile_work_list: Vec::new(),
                    signature: 0,
                }],
            },
            mode: crate::ReloadDiffMode::Patch,
            reason: "test".to_string(),
            dirty_rects: vec![crate::ReloadPatchRect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            }],
            dirty_segments: vec![crate::ReloadDirtySegment {
                ordinal: 7,
                dirty_tile_count: 1,
                dirty_resource_count: 0,
                dirty_event_ranges: vec![crate::ReloadDirtySegmentEventRange { start: 0, end: 1 }],
            }],
        }
    }
}
