use skia_safe::{Canvas, Point};

use crate::RuntimeError;

use super::{
    TextCharStyle, char_advances,
    font::{FontResolver, ResolvedFont, skia_font, text_paint},
};

pub(super) fn text_layout_is_arc(entry: &clip_file::metadata::TextLayerEntry) -> bool {
    entry.attributes.path_mode == Some(1) && entry.attributes.path_center.is_some()
}

pub(super) fn render_arc_entry_surface(
    canvas: &Canvas,
    entry: &clip_file::metadata::TextLayerEntry,
    chars: &[char],
    styles: &[TextCharStyle],
    fonts: &mut FontResolver,
) -> Result<(), RuntimeError> {
    let Some((center_x, center_y)) = entry.attributes.path_center else {
        return Ok(());
    };
    let outer_radius = arc_outer_radius(entry).unwrap_or(0.0);
    if outer_radius <= 1.0 {
        return Ok(());
    }
    let max_font_size = styles
        .iter()
        .map(|style| style.font_size_px)
        .fold(1.0f32, f32::max);
    let baseline_radius = (outer_radius - max_font_size * 0.945).max(max_font_size * 0.35);
    let angular_radius = (outer_radius * 0.57).max(baseline_radius);
    let advances = char_advances(chars, styles, fonts);
    let total_advance = advances.iter().sum::<f32>();
    if total_advance <= 0.0 {
        return Ok(());
    }
    let center_angle = -std::f32::consts::FRAC_PI_2;
    let mut advance_cursor = 0.0f32;
    for ((ch, style), advance) in chars.iter().zip(styles.iter()).zip(advances.iter()) {
        if *ch == '\r' || *ch == '\n' {
            advance_cursor += *advance;
            continue;
        }
        let Some(resolved) = fonts.resolve(style) else {
            advance_cursor += *advance;
            continue;
        };
        let arc_offset = advance_cursor + *advance * 0.5 - total_advance * 0.5;
        let angle = center_angle + arc_offset / angular_radius;
        let baseline_x = center_x as f32 + baseline_radius * angle.cos();
        let baseline_y = center_y as f32 + baseline_radius * angle.sin();
        draw_arc_char(canvas, *ch, style, &resolved, baseline_x, baseline_y, angle);
        advance_cursor += *advance;
    }
    Ok(())
}

pub(super) fn arc_outer_radius(entry: &clip_file::metadata::TextLayerEntry) -> Option<f32> {
    let (width, height) = entry.attributes.box_size?;
    let from_box = width.min(height) as f32 * 0.5;
    if from_box > 0.0 {
        return Some(from_box);
    }
    let (center_x, center_y) = entry.attributes.path_center?;
    Some(center_x.min(center_y).max(0) as f32)
}

fn draw_arc_char(
    canvas: &Canvas,
    ch: char,
    style: &TextCharStyle,
    resolved: &ResolvedFont,
    baseline_x: f32,
    baseline_y: f32,
    angle: f32,
) {
    let font = skia_font(resolved, style);
    let paint = text_paint(style.color);
    let text = ch.to_string();
    let (advance, bounds) = font.measure_str(&text, Some(&paint));
    let save_count = canvas.save();
    canvas.translate(Point::new(baseline_x, baseline_y));
    canvas.rotate(angle.to_degrees() + 90.0, None);
    canvas.draw_str(
        &text,
        Point::new(-advance * 0.5 - bounds.left, 0.0),
        &font,
        &paint,
    );
    canvas.restore_to_count(save_count);
}
