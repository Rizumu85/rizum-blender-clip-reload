use std::error::Error;
use std::fmt;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GpuDeviceConfig {
    pub preferred_backend: Option<String>,
}

#[derive(Debug)]
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl GpuContext {
    pub fn new(config: &GpuDeviceConfig) -> Result<Self, GpuDeviceError> {
        pollster::block_on(Self::new_async(config))
    }

    async fn new_async(config: &GpuDeviceConfig) -> Result<Self, GpuDeviceError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: backend_selection(config)?,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .map_err(|_| GpuDeviceError::BackendUnavailable)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("rizum_clip_gpu_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|err| GpuDeviceError::RequestDevice(err.to_string()))?;
        Ok(Self { device, queue })
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum GpuDeviceError {
    BackendUnavailable,
    InvalidPreferredBackend(String),
    RequestDevice(String),
}

impl fmt::Display for GpuDeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BackendUnavailable => f.write_str("no compatible GPU backend is available"),
            Self::InvalidPreferredBackend(backend) => {
                write!(f, "invalid preferred GPU backend {backend}")
            }
            Self::RequestDevice(err) => write!(f, "failed to request GPU device: {err}"),
        }
    }
}

impl Error for GpuDeviceError {}

fn backend_selection(config: &GpuDeviceConfig) -> Result<wgpu::Backends, GpuDeviceError> {
    let Some(preferred_backend) = config.preferred_backend.as_deref() else {
        return Ok(wgpu::Backends::all());
    };
    match preferred_backend.to_ascii_lowercase().as_str() {
        "dx12" | "d3d12" => Ok(wgpu::Backends::DX12),
        "metal" => Ok(wgpu::Backends::METAL),
        "vulkan" | "vk" => Ok(wgpu::Backends::VULKAN),
        "gles" | "gl" | "opengl" => Ok(wgpu::Backends::GL),
        other => Err(GpuDeviceError::InvalidPreferredBackend(other.to_owned())),
    }
}
