use crate::gpu_provider::RuntimeGpuResourceProvider;
use crate::results::{NativePerformancePlanResult, NativeTileSiloEstimateResult};
use crate::stack_plan::GpuRenderStackSelection;
use crate::tile_silo_options::tile_silo_options;
use crate::{ClipSession, RuntimeError};

impl ClipSession {
    pub fn performance_plan(
        &self,
        tile_size: u32,
    ) -> Result<NativePerformancePlanResult, RuntimeError> {
        let tile_estimate = self.estimate_tile_silo_plan(tile_size)?;
        let selection = self.select_gpu_normal_render_stack(tile_silo_options())?;
        let GpuRenderStackSelection {
            sources,
            resource_plan,
            unsupported: _,
        } = selection;
        let provider =
            RuntimeGpuResourceProvider::new(&self.container, self.summary.canvas, resource_plan)?;
        let render_program_stats = clip_gpu::inspect_normal_stack_render_program(
            &provider,
            self.summary.canvas,
            (0, 0),
            self.summary.canvas,
            &sources,
        );
        let estimated_atlas_upload_bytes = estimated_sparse_atlas_upload_bytes(&tile_estimate);
        let estimated_tile_events = tile_estimate
            .compressed_raster_tile_event_count
            .saturating_add(tile_estimate.solid_tile_event_count);

        Ok(NativePerformancePlanResult {
            tile_estimate,
            render_program_stats,
            estimated_atlas_upload_bytes,
            estimated_tile_events,
        })
    }
}

fn estimated_sparse_atlas_upload_bytes(estimate: &NativeTileSiloEstimateResult) -> u64 {
    let tile_pixels = u64::from(estimate.tile_size).saturating_mul(u64::from(estimate.tile_size));
    let raster_bytes = estimate
        .raster_compressed_tile_slot_count
        .saturating_mul(tile_pixels)
        .saturating_mul(4);
    let mask_bytes = estimate
        .mask_compressed_tile_slot_count
        .saturating_mul(tile_pixels);
    raster_bytes.saturating_add(mask_bytes)
}
