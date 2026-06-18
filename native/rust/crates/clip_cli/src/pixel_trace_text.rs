use clip_model::Rgba8;
use clip_runtime::{ClipSession, NormalRasterStackPixelTraceResult};

use crate::layer_labels::{layer_label, optional_raw_layer_label};

pub(crate) fn print_pixel_trace_result(
    session: &ClipSession,
    label: &str,
    result: NormalRasterStackPixelTraceResult,
) {
    println!(
        "{} sources={} unsupported={}",
        label,
        result.source_count,
        result.unsupported.len(),
    );
    for sample in result.samples {
        println!(
            "  prefix={} before={} rgba=[{},{},{},{}] source={}",
            sample.source_index,
            format_optional_rgba(sample.before_rgba),
            sample.rgba.r,
            sample.rgba.g,
            sample.rgba.b,
            sample.rgba.a,
            sample.source,
        );
        for input in sample.inputs {
            println!(
                "    input role={} node={} layer={} blend={} opacity={} rgba={} mask_alpha={}",
                input.role,
                format_optional_u32(input.render_node_id),
                optional_raw_layer_label(session, input.layer_id),
                input.blend_mode.as_deref().unwrap_or("-"),
                format_optional_f32(input.opacity),
                format_optional_rgba(input.rgba),
                format_optional_u8(input.mask_alpha),
            );
        }
    }
    for unsupported in result.unsupported {
        println!(
            "  unsupported node={} layer={} kind={:?} reason={}",
            unsupported.render_node_id.0,
            layer_label(session, unsupported.layer_id),
            unsupported.kind,
            unsupported.reason,
        );
    }
}

fn format_optional_u32(value: Option<u32>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => "-".to_string(),
    }
}

fn format_optional_u8(value: Option<u8>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => "-".to_string(),
    }
}

fn format_optional_f32(value: Option<f32>) -> String {
    match value {
        Some(value) => format!("{value:.6}"),
        None => "-".to_string(),
    }
}

fn format_optional_rgba(value: Option<Rgba8>) -> String {
    match value {
        Some(rgba) => format!("[{},{},{},{}]", rgba.r, rgba.g, rgba.b, rgba.a),
        None => "-".to_string(),
    }
}
