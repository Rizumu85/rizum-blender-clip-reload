#![forbid(unsafe_code)]

pub mod container;
mod error;
pub mod external;
pub mod metadata;
mod placement;
mod read;
mod tile_region;
pub mod tiles;

pub use error::ClipFileError;
pub use read::{
    ClipFileSummary, PlacedRgbaTileImage, RasterLayerSourceInfo, read_layer_mask_alpha,
    read_layer_mask_alpha_from_container, read_raster_layer_rgba, read_raster_layer_source_info,
    read_raster_layer_source_info_from_container, read_raster_layer_source_rgba,
    read_raster_layer_source_rgba_from_container, read_resolved_layer_mask_alpha_from_container,
    read_resolved_raster_layer_source_rgba_from_container,
    read_resolved_raster_layer_source_rgba_region_from_container, read_summary,
};
