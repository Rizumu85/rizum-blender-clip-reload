use clip_model::{CanvasSize, Rgba8};

use crate::error::RuntimeError;
use crate::results::NormalRasterStackPixelTraceInput;

use super::{PlannedDecodedMask, PlannedDecodedRaster, StrictRasterStackDraw};
pub(crate) fn sample_rgba8(
    pixels: &[u8],
    size: CanvasSize,
    x: u32,
    y: u32,
) -> Result<Rgba8, RuntimeError> {
    let width = usize::try_from(size.width).map_err(|_| RuntimeError::InvalidRegion)?;
    let x = usize::try_from(x).map_err(|_| RuntimeError::InvalidRegion)?;
    let y = usize::try_from(y).map_err(|_| RuntimeError::InvalidRegion)?;
    let pixel_offset = y
        .checked_mul(width)
        .and_then(|row| row.checked_add(x))
        .and_then(|pixel| pixel.checked_mul(4))
        .ok_or(RuntimeError::InvalidRegion)?;
    let pixel = pixels
        .get(pixel_offset..pixel_offset + 4)
        .ok_or(RuntimeError::InvalidRegion)?;
    Ok(Rgba8 {
        r: pixel[0],
        g: pixel[1],
        b: pixel[2],
        a: pixel[3],
    })
}

fn sample_alpha8(pixels: &[u8], size: CanvasSize, x: u32, y: u32) -> Result<u8, RuntimeError> {
    let width = usize::try_from(size.width).map_err(|_| RuntimeError::InvalidRegion)?;
    let x = usize::try_from(x).map_err(|_| RuntimeError::InvalidRegion)?;
    let y = usize::try_from(y).map_err(|_| RuntimeError::InvalidRegion)?;
    let pixel_offset = y
        .checked_mul(width)
        .and_then(|row| row.checked_add(x))
        .ok_or(RuntimeError::InvalidRegion)?;
    pixels
        .get(pixel_offset)
        .copied()
        .ok_or(RuntimeError::InvalidRegion)
}

pub(crate) fn stack_draw_trace_label(draw: &StrictRasterStackDraw) -> String {
    match draw {
        StrictRasterStackDraw::Paper { color, opacity } => {
            format!(
                "paper color=[{},{},{},{}] opacity={opacity:.6}",
                color.r, color.g, color.b, color.a
            )
        }
        StrictRasterStackDraw::Raster(decoded) => format!(
            "raster node={} layer={} blend={:?} opacity={:.6}",
            decoded.render_node_id.0, decoded.layer_id.0, decoded.blend_mode, decoded.opacity
        ),
        StrictRasterStackDraw::ClippingRun(run) => format!(
            "clipping-run base_node={} base_layer={} clipped_count={}",
            run.base.render_node_id.0,
            run.base.layer_id.0,
            run.clipped.len()
        ),
        StrictRasterStackDraw::Container(container) => format!(
            "container node={} layer={} children={} opacity={:.6}",
            container.render_node_id.0,
            container.layer_id.0,
            container.draws.len(),
            container.opacity
        ),
        StrictRasterStackDraw::ThroughGroup(through_group) => format!(
            "through-group node={} layer={} children={} opacity={:.6}",
            through_group.render_node_id.0,
            through_group.layer_id.0,
            through_group.draws.len(),
            through_group.opacity
        ),
        StrictRasterStackDraw::LutFilter(filter) => format!(
            "{} filter node={} layer={} opacity={:.6}",
            filter.name, filter.render_node_id.0, filter.layer_id.0, filter.opacity
        ),
    }
}

pub(crate) fn stack_draw_trace_inputs(
    draw: &StrictRasterStackDraw,
    x: u32,
    y: u32,
) -> Result<Vec<NormalRasterStackPixelTraceInput>, RuntimeError> {
    let mut inputs = Vec::new();
    push_stack_draw_trace_inputs(draw, "", x, y, &mut inputs)?;
    Ok(inputs)
}

fn push_stack_draw_trace_inputs(
    draw: &StrictRasterStackDraw,
    role_prefix: &str,
    x: u32,
    y: u32,
    inputs: &mut Vec<NormalRasterStackPixelTraceInput>,
) -> Result<(), RuntimeError> {
    match draw {
        StrictRasterStackDraw::Paper { color, opacity } => {
            inputs.push(NormalRasterStackPixelTraceInput {
                role: prefixed_trace_role(role_prefix, "paper"),
                render_node_id: None,
                layer_id: None,
                blend_mode: None,
                opacity: Some(*opacity),
                rgba: Some(*color),
                mask_alpha: None,
            });
        }
        StrictRasterStackDraw::Raster(decoded) => {
            push_raster_trace_input("raster", role_prefix, decoded, x, y, inputs)?;
        }
        StrictRasterStackDraw::ClippingRun(run) => {
            push_raster_trace_input("clipping-base", role_prefix, &run.base, x, y, inputs)?;
            for (index, clipped) in run.clipped.iter().enumerate() {
                push_raster_trace_input(
                    &format!("clipped[{index}]"),
                    role_prefix,
                    clipped,
                    x,
                    y,
                    inputs,
                )?;
            }
        }
        StrictRasterStackDraw::Container(container) => {
            inputs.push(NormalRasterStackPixelTraceInput {
                role: prefixed_trace_role(role_prefix, "container"),
                render_node_id: Some(container.render_node_id.0),
                layer_id: Some(container.layer_id.0),
                blend_mode: Some("Normal".to_string()),
                opacity: Some(container.opacity),
                rgba: None,
                mask_alpha: sample_optional_mask_alpha(container.mask.as_ref(), x, y)?,
            });
            let child_prefix = prefixed_trace_role(role_prefix, "container");
            for child in &container.draws {
                push_stack_draw_trace_inputs(child, &child_prefix, x, y, inputs)?;
            }
        }
        StrictRasterStackDraw::ThroughGroup(through_group) => {
            inputs.push(NormalRasterStackPixelTraceInput {
                role: prefixed_trace_role(role_prefix, "through-group"),
                render_node_id: Some(through_group.render_node_id.0),
                layer_id: Some(through_group.layer_id.0),
                blend_mode: Some("Through".to_string()),
                opacity: Some(through_group.opacity),
                rgba: None,
                mask_alpha: sample_optional_mask_alpha(through_group.mask.as_ref(), x, y)?,
            });
            let child_prefix = prefixed_trace_role(role_prefix, "through-group");
            for child in &through_group.draws {
                push_stack_draw_trace_inputs(child, &child_prefix, x, y, inputs)?;
            }
        }
        StrictRasterStackDraw::LutFilter(filter) => {
            inputs.push(NormalRasterStackPixelTraceInput {
                role: prefixed_trace_role(role_prefix, filter.name),
                render_node_id: Some(filter.render_node_id.0),
                layer_id: Some(filter.layer_id.0),
                blend_mode: Some(filter.name.to_string()),
                opacity: Some(filter.opacity),
                rgba: None,
                mask_alpha: sample_optional_mask_alpha(filter.mask.as_ref(), x, y)?,
            });
        }
    }
    Ok(())
}

fn push_raster_trace_input(
    role: &str,
    role_prefix: &str,
    decoded: &PlannedDecodedRaster,
    x: u32,
    y: u32,
    inputs: &mut Vec<NormalRasterStackPixelTraceInput>,
) -> Result<(), RuntimeError> {
    let image_size = CanvasSize::new(decoded.image.width, decoded.image.height);
    inputs.push(NormalRasterStackPixelTraceInput {
        role: prefixed_trace_role(role_prefix, role),
        render_node_id: Some(decoded.render_node_id.0),
        layer_id: Some(decoded.layer_id.0),
        blend_mode: Some(format!("{:?}", decoded.blend_mode)),
        opacity: Some(decoded.opacity),
        rgba: Some(sample_raster_source_rgba(decoded, image_size, x, y)?),
        mask_alpha: sample_optional_mask_alpha(decoded.mask.as_ref(), x, y)?,
    });
    Ok(())
}

fn sample_raster_source_rgba(
    decoded: &PlannedDecodedRaster,
    image_size: CanvasSize,
    x: u32,
    y: u32,
) -> Result<Rgba8, RuntimeError> {
    let source_x = i64::from(x) - i64::from(decoded.offset_x);
    let source_y = i64::from(y) - i64::from(decoded.offset_y);
    if source_x < 0
        || source_y < 0
        || source_x >= i64::from(image_size.width)
        || source_y >= i64::from(image_size.height)
    {
        return Ok(Rgba8 {
            r: 255,
            g: 255,
            b: 255,
            a: 0,
        });
    }
    sample_rgba8(
        &decoded.image.pixels,
        image_size,
        u32::try_from(source_x).map_err(|_| RuntimeError::InvalidRegion)?,
        u32::try_from(source_y).map_err(|_| RuntimeError::InvalidRegion)?,
    )
}

fn sample_optional_mask_alpha(
    mask: Option<&PlannedDecodedMask>,
    x: u32,
    y: u32,
) -> Result<Option<u8>, RuntimeError> {
    match mask {
        Some(mask) => {
            let size = CanvasSize::new(mask.image.width, mask.image.height);
            Ok(Some(sample_alpha8(&mask.image.pixels, size, x, y)?))
        }
        None => Ok(None),
    }
}

fn prefixed_trace_role(prefix: &str, role: &str) -> String {
    if prefix.is_empty() {
        role.to_string()
    } else {
        format!("{prefix}/{role}")
    }
}
