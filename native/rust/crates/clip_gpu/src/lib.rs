#![forbid(unsafe_code)]

mod blend;
pub mod device;
mod lut_filter;
pub mod pass;
pub mod renderer;
pub mod resource;
mod shaders;
mod source_params;
pub mod stream;
mod stream_bounds;
mod stream_effects;
mod stream_extents;
mod stream_groups;
mod stream_resources;
#[cfg(test)]
mod stream_resources_tests;
mod stream_state;
#[cfg(test)]
mod stream_tests;
pub mod types;
mod validation;

pub use device::{GpuContext, GpuDeviceConfig, GpuDeviceError};
pub use renderer::{GpuRenderError, GpuRenderer};
pub use resource::{
    GpuMaskResourceCache, GpuMaskResourceInfo, GpuMaskResourceKey, GpuMaskSamplingInfo,
    GpuMaskUpload, GpuRasterResourceCache, GpuRasterResourceInfo, GpuRasterResourceKey,
    GpuRasterUpload,
};
pub use stream::GpuNormalStackResourceProvider;
pub use types::{
    GpuLutFilterMode, GpuNormalRasterSource, GpuNormalStackChunk, GpuNormalStackSource,
    GpuRasterBlendMode, GpuRasterDrawOutput, GpuRasterStackOutput,
};
