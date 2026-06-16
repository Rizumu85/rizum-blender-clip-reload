#![forbid(unsafe_code)]

mod blend;
pub mod device;
pub mod pass;
pub mod renderer;
pub mod resource;
mod shaders;
mod source_params;
pub mod stream;
mod stream_bounds;
mod stream_extents;
mod stream_groups;
mod stream_resources;
mod stream_state;
#[cfg(test)]
mod stream_tests;
pub mod types;
mod validation;

pub use device::{GpuContext, GpuDeviceConfig, GpuDeviceError};
pub use renderer::{GpuRenderError, GpuRenderer};
pub use resource::{
    GpuMaskResourceCache, GpuMaskResourceInfo, GpuMaskResourceKey, GpuMaskUpload,
    GpuRasterResourceCache, GpuRasterResourceInfo, GpuRasterResourceKey, GpuRasterUpload,
};
pub use stream::GpuNormalStackResourceProvider;
pub use types::{
    GpuLutFilterMode, GpuNormalRasterSource, GpuNormalStackChunk, GpuNormalStackSource,
    GpuRasterBlendMode, GpuRasterDrawOutput, GpuRasterStackOutput,
};
