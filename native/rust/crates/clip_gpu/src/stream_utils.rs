use crate::{GpuLutFilterMode, GpuRenderer};

pub(crate) fn renderer_context_queue(renderer: &GpuRenderer) -> &wgpu::Queue {
    &renderer.context.queue
}

pub(crate) fn lut_filter_label(filter_mode: GpuLutFilterMode) -> &'static str {
    match filter_mode {
        GpuLutFilterMode::ToneCurveRgb => "rizum_clip_provider_tone_curve_pass",
        GpuLutFilterMode::GradientMapLum => "rizum_clip_provider_gradient_map_pass",
        GpuLutFilterMode::ThresholdLum => "rizum_clip_provider_threshold_pass",
        GpuLutFilterMode::Hsl(_) => "rizum_clip_provider_hsl_filter_pass",
    }
}

pub(crate) fn local_pass_bounds(
    pass_bounds: crate::stream_bounds::CanvasRect,
    target_origin: (i32, i32),
) -> crate::stream_bounds::CanvasRect {
    pass_bounds
        .translate_to_local(target_origin)
        .expect("global pass bounds must fit inside the streaming target")
}
