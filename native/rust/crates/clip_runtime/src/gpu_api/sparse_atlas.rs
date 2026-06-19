use super::RuntimeGpuRenderer;
use super::checkpoint::initial_transparent_rgba8;
use super::checkpoint_selection::{
    dirty_checkpoint_candidate, dirty_starts_at_initial_accumulator,
};
use crate::gpu_provider::{
    atlas_events::{sparse_atlas_raster_affected_event_plan, sparse_atlas_raster_event_plan},
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

    pub fn prepare_sparse_atlas_raster_affected_patch_plan(
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
            event_plan: sparse_atlas_raster_affected_event_plan(plan, &reload, &selection.sources)
                .into(),
        })
    }

    pub fn draw_sparse_atlas_initial_segment_patches(
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
            || !dirty_starts_at_initial_accumulator(plan)
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
        let updates = sparse_atlas_texture_pool_updates(session, &reload.cache)?;
        self.renderer
            .update_sparse_atlas_texture_pool(
                &mut self.sparse_atlas_textures.borrow_mut(),
                &updates,
            )
            .map_err(RuntimeError::from)?;
        let event_plan = sparse_atlas_raster_affected_event_plan(plan, &reload, &sources);
        if !event_plan.skipped_segments.is_empty() || event_plan.segments.is_empty() {
            return Ok(None);
        }
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

    pub fn draw_sparse_atlas_reconstructed_segment_patches(
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
        if plan.mode != crate::ReloadDiffMode::Patch || plan.dirty_rects.is_empty() {
            return Ok(None);
        }
        let Some(checkpoint_candidate) = dirty_checkpoint_candidate(plan) else {
            return Ok(None);
        };
        if checkpoint_candidate.source_start == 0 {
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
        let checkpoint_source_start = usize::try_from(checkpoint_candidate.source_start)
            .map_err(|_| clip_gpu::GpuRenderError::TextureSizeOverflow)?;
        if checkpoint_source_start > sources.len() {
            return Ok(None);
        }
        let source_count = sources.len();
        let resource_stats = resource_plan.resource_stats();
        let reload = self.sparse_atlas_cache.borrow_mut().plan_reload_diff(plan);
        let sparse_atlas = reload.clone().into();
        let event_plan = sparse_atlas_raster_affected_event_plan(plan, &reload, &sources);
        if !event_plan.skipped_segments.is_empty() || event_plan.segments.is_empty() {
            return Ok(None);
        }

        let checkpoint = self.prefix_checkpoint_rgba8(
            session,
            plan,
            u32::try_from(checkpoint_source_start)
                .map_err(|_| clip_gpu::GpuRenderError::TextureSizeOverflow)?,
            &sources[..checkpoint_source_start],
            resource_plan,
            checkpoint_candidate.priority,
        )?;
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
        let output = self
            .renderer
            .draw_sparse_atlas_raster_event_batch_patches_over_rgba8(
                session.summary.canvas,
                &self.sparse_atlas_textures.borrow(),
                &batches,
                &checkpoint.pixels,
                &rects,
            )?;
        Ok(Some((
            NormalRasterStackGpuPatchResult {
                payload: output.payload,
                source_count,
                resource_stats,
                texture_cache_stats: checkpoint.texture_cache_stats,
                drawn_resources: checkpoint.drawn_resources,
                mask_resources: checkpoint.mask_resources,
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
