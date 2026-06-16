use clip_model::CanvasSize;

use crate::{
    GpuMaskResourceCache, GpuMaskResourceKey, GpuNormalRasterSource, GpuRasterResourceCache,
    GpuRenderError, GpuRenderer,
};

pub trait GpuNormalStackResourceProvider {
    type Error: From<GpuRenderError>;

    fn raster_resource(
        &mut self,
        renderer: &GpuRenderer,
        source: GpuNormalRasterSource,
    ) -> Result<GpuRasterResourceCache, Self::Error>;

    fn raster_resource_size(&self, source: GpuNormalRasterSource) -> Option<CanvasSize> {
        let _ = source;
        None
    }

    fn raster_resource_offset(&self, source: GpuNormalRasterSource) -> Option<(i32, i32)> {
        let _ = source;
        None
    }

    fn mask_resource(
        &mut self,
        renderer: &GpuRenderer,
        key: GpuMaskResourceKey,
    ) -> Result<GpuMaskResourceCache, Self::Error>;
}
