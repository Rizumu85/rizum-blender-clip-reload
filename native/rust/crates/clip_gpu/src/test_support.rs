use std::sync::OnceLock;

use crate::{GpuDeviceConfig, GpuRenderer};

/// The one renderer every GPU test draws with.
///
/// A renderer is only a wgpu device and queue, so tests cannot observe each
/// other through it. Creating a device per test made the suite slow, and
/// several tests creating devices at once could hang the GPU driver.
pub(crate) fn shared_renderer() -> &'static GpuRenderer {
    static RENDERER: OnceLock<GpuRenderer> = OnceLock::new();
    RENDERER
        .get_or_init(|| GpuRenderer::new(GpuDeviceConfig::default()).expect("create GPU renderer"))
}
