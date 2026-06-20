#![forbid(unsafe_code)]

mod atlas_chunks;
pub mod container;
pub mod decode_profile;
mod error;
pub mod external;
pub mod metadata;
mod placement;
mod read;
mod tile_region;
pub mod tiles;

pub use error::ClipFileError;
pub use read::{
    ClipFileSummary, PlacedRgbaTileImage, RasterAtlasTileChunk, RasterLayerSourceInfo,
    read_layer_mask_alpha, read_layer_mask_alpha_from_container, read_layer_render_rgba,
    read_layer_render_rgba_from_container, read_raster_layer_rgba, read_raster_layer_source_info,
    read_raster_layer_source_info_from_container, read_raster_layer_source_rgba,
    read_raster_layer_source_rgba_from_container, read_resolved_layer_mask_alpha_from_container,
    read_resolved_layer_mask_alpha_region_from_container,
    read_resolved_raster_layer_source_rgba_from_container,
    read_resolved_raster_layer_source_rgba_region_atlas_chunks_from_container,
    read_resolved_raster_layer_source_rgba_region_from_container, read_summary,
};
