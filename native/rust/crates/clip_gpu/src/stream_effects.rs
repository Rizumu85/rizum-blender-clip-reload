use crate::{GpuNormalRasterSource, GpuNormalStackSource};

pub(crate) fn stack_can_affect_output(sources: &[GpuNormalStackSource]) -> bool {
    sources.iter().any(source_can_affect_output)
}

pub(crate) fn source_can_affect_output(source: &GpuNormalStackSource) -> bool {
    match source {
        GpuNormalStackSource::Raster(raster) => raster_can_affect_output(*raster),
        GpuNormalStackSource::ClippingRun { base, .. } => raster_can_affect_output(*base),
        GpuNormalStackSource::Container {
            children, opacity, ..
        }
        | GpuNormalStackSource::ThroughGroup {
            children, opacity, ..
        } => opacity_can_affect_output(*opacity) && stack_can_affect_output(children),
        GpuNormalStackSource::SolidColor { color, opacity } => {
            opacity_can_affect_output(*opacity) && color.a != 0
        }
        GpuNormalStackSource::LutFilter { opacity, .. } => opacity_can_affect_output(*opacity),
    }
}

pub(crate) fn raster_can_affect_output(source: GpuNormalRasterSource) -> bool {
    opacity_can_affect_output(source.opacity)
}

fn opacity_can_affect_output(opacity: f32) -> bool {
    opacity > 0.0
}
