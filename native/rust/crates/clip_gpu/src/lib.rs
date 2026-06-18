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
mod stream_clipped_tile_silo;
mod stream_clipping;
mod stream_clipping_tile_silo;
mod stream_context;
mod stream_effects;
mod stream_extents;
mod stream_groups;
mod stream_program;
mod stream_program_barriers;
mod stream_program_inspect;
mod stream_program_lowering;
#[cfg(test)]
mod stream_program_tests;
mod stream_provider;
mod stream_resources;
#[cfg(test)]
mod stream_resources_tests;
mod stream_sequence;
mod stream_state;
#[cfg(test)]
mod stream_tests;
mod stream_through;
mod stream_tile_event;
mod stream_tile_filter_program;
mod stream_tile_filter_silo;
mod stream_tile_scope_silo;
mod stream_tile_scope_silo_plan;
mod stream_tile_scope_silo_program;
mod stream_tile_silo;
mod stream_tile_silo_buffers;
mod stream_tile_silo_pipeline;
mod stream_tile_silo_plan;
mod stream_tile_silo_upload;
mod stream_utils;
pub mod types;
mod validation;

pub use device::{GpuContext, GpuDeviceConfig, GpuDeviceError};
pub use renderer::{GpuRenderError, GpuRenderer};
pub use resource::{
    GpuMaskResourceCache, GpuMaskResourceInfo, GpuMaskResourceKey, GpuMaskSamplingInfo,
    GpuMaskUpload, GpuRasterResourceCache, GpuRasterResourceInfo, GpuRasterResourceKey,
    GpuRasterUpload,
};
pub use stream::{
    GpuMaskAtlasTileChunk, GpuNormalStackResourceProvider, GpuRasterAtlasPixels,
    GpuRasterAtlasSource, GpuRasterAtlasTileChunk, GpuRasterAtlasTilePixels,
};
pub use stream_program::RenderProgramStats;
pub use stream_program_barriers::{RenderProgramBarrierCounts, RenderProgramBarrierReason};
pub use stream_program_inspect::inspect_normal_stack_render_program;
pub use stream_tile_event::TILE_EVENT_ABI_VERSION;
pub use types::{
    GpuClippedStackSource, GpuHslFilterParams, GpuLutFilterMode, GpuNormalRasterSource,
    GpuNormalStackChunk, GpuNormalStackSource, GpuRasterBlendMode, GpuRasterDrawOutput,
    GpuRasterStackOutput,
};
