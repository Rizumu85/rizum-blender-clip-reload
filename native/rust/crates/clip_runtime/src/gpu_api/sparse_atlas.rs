use super::RuntimeGpuRenderer;
use crate::gpu_provider::{
    atlas_events::{sparse_atlas_raster_event_plan, sparse_atlas_raster_suffix_event_plan},
    atlas_upload::sparse_atlas_texture_pool_updates,
};
use crate::{ClipSession, RuntimeError};

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
