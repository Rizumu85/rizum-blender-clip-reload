use std::error::Error;
use std::fmt;

use clip_model::LayerId;

use crate::results::SimpleRasterStackUnsupported;

#[derive(Debug)]
pub enum RuntimeError {
    File(clip_file::ClipFileError),
    Graph(clip_graph::RenderPlanError),
    Gpu(clip_gpu::GpuRenderError),
    MissingRasterRenderMipmap {
        layer_id: LayerId,
    },
    MissingPlannedRasterLayer {
        layer_id: LayerId,
    },
    UnsupportedRenderPlan {
        unsupported: Vec<SimpleRasterStackUnsupported>,
    },
    EmptyRenderPlan,
    InvalidRegion,
    OutputBufferTooSmall {
        expected: usize,
        actual: usize,
    },
}

impl From<clip_file::ClipFileError> for RuntimeError {
    fn from(value: clip_file::ClipFileError) -> Self {
        Self::File(value)
    }
}

impl From<clip_graph::RenderPlanError> for RuntimeError {
    fn from(value: clip_graph::RenderPlanError) -> Self {
        Self::Graph(value)
    }
}

impl From<clip_gpu::GpuRenderError> for RuntimeError {
    fn from(value: clip_gpu::GpuRenderError) -> Self {
        Self::Gpu(value)
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(err) => write!(f, "{err}"),
            Self::Graph(err) => write!(f, "{err}"),
            Self::Gpu(err) => write!(f, "{err}"),
            Self::MissingRasterRenderMipmap { layer_id } => {
                write!(
                    f,
                    "planned raster layer {} has no render mipmap",
                    layer_id.0
                )
            }
            Self::MissingPlannedRasterLayer { layer_id } => {
                write!(
                    f,
                    "layer {} is not a visible planned raster layer",
                    layer_id.0
                )
            }
            Self::UnsupportedRenderPlan { unsupported } => write!(
                f,
                "strict native NORMAL renderer does not yet support {} planned nodes",
                unsupported.len(),
            ),
            Self::EmptyRenderPlan => f.write_str("render plan has no drawable native sources"),
            Self::InvalidRegion => f.write_str("requested image region is outside the canvas"),
            Self::OutputBufferTooSmall { expected, actual } => write!(
                f,
                "output buffer too small: expected at least {expected} bytes, got {actual}",
            ),
        }
    }
}

impl Error for RuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::File(err) => Some(err),
            Self::Graph(err) => Some(err),
            Self::Gpu(err) => Some(err),
            _ => None,
        }
    }
}
