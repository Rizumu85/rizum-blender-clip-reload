use clip_model::CanvasSize;

use crate::{
    GpuMaskResourceCache, GpuMaskResourceKey, GpuNormalRasterSource, GpuRasterResourceCache,
    GpuRasterResourceInfo, GpuRenderError, GpuRenderer,
};

#[derive(Clone, Copy, Debug)]
pub struct GpuRasterAtlasSource {
    pub source: GpuNormalRasterSource,
    pub atlas_x: u32,
    pub atlas_y: u32,
    pub size: CanvasSize,
    pub offset_x: i32,
    pub offset_y: i32,
}

#[derive(Debug)]
pub struct GpuRasterAtlasPixels {
    pub size: CanvasSize,
    pub pixels: Vec<u8>,
    pub resources: Vec<GpuRasterResourceInfo>,
}

#[derive(Debug)]
pub struct GpuRasterAtlasTileChunk {
    pub source: GpuNormalRasterSource,
    pub atlas_x: u32,
    pub atlas_y: u32,
    pub size: CanvasSize,
    pub offset_x: i32,
    pub offset_y: i32,
    pub pixels: Vec<u8>,
}

#[derive(Debug)]
pub struct GpuRasterAtlasTilePixels {
    pub size: CanvasSize,
    pub chunks: Vec<GpuRasterAtlasTileChunk>,
    pub resources: Vec<GpuRasterResourceInfo>,
}

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

    fn raster_run_atlas_pixels(
        &mut self,
        sources: &[GpuRasterAtlasSource],
        atlas_size: CanvasSize,
    ) -> Result<Option<GpuRasterAtlasPixels>, Self::Error> {
        let _ = sources;
        let _ = atlas_size;
        Ok(None)
    }

    fn raster_run_atlas_tile_pixels(
        &mut self,
        sources: &[GpuRasterAtlasSource],
        atlas_size: CanvasSize,
    ) -> Result<Option<GpuRasterAtlasTilePixels>, Self::Error> {
        let _ = sources;
        let _ = atlas_size;
        Ok(None)
    }

    fn mask_resource(
        &mut self,
        renderer: &GpuRenderer,
        key: GpuMaskResourceKey,
    ) -> Result<GpuMaskResourceCache, Self::Error>;
}
