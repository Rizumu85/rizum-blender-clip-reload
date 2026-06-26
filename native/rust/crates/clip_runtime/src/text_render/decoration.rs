use clip_model::Rgba8;
use skia_safe::{Canvas, Paint, Point};

use super::{
    TextCharStyle,
    font::{TextFontMetrics, text_paint},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TextDecorationCommand {
    pub(super) x0: f32,
    pub(super) x1: f32,
    pub(super) y: f32,
    pub(super) thickness: f32,
    pub(super) color: Rgba8,
    pub(super) inset_ends: bool,
}

pub(super) fn plan_text_decoration_commands(
    origin: (f32, f32),
    styles: &[TextCharStyle],
    decoration_styles: &[TextCharStyle],
    char_positions: &[(f32, f32)],
    char_metrics: &[Option<TextFontMetrics>],
) -> Vec<TextDecorationCommand> {
    let mut commands = Vec::new();
    let mut start = 0usize;
    while start < styles.len() {
        if !styles[start].underline && !styles[start].strikethrough {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < styles.len()
            && styles[end].underline == styles[start].underline
            && styles[end].strikethrough == styles[start].strikethrough
            && styles[end].color == styles[start].color
            && (styles[end].font_size_px - styles[start].font_size_px).abs() < f32::EPSILON
            && (decoration_styles[end].font_size_px - decoration_styles[start].font_size_px).abs()
                < f32::EPSILON
        {
            end += 1;
        }
        let x0 = char_positions
            .get(start)
            .map(|pos| pos.0)
            .unwrap_or(origin.0);
        let x1 = char_positions
            .get(end.saturating_sub(1))
            .map(|pos| pos.1)
            .unwrap_or(x0);
        let metrics = char_metrics.get(start).and_then(|metrics| *metrics);
        let fitted_decoration_span =
            (styles[start].font_size_px - decoration_styles[start].font_size_px).abs()
                > f32::EPSILON;
        if styles[start].underline {
            let y = decoration_y(
                origin.1,
                decoration_styles[start].font_size_px,
                0.90,
                metrics.and_then(|metrics| metrics.underline_position),
                metrics.and_then(|metrics| metrics.underline_thickness),
            );
            let thickness = decoration_thickness(
                decoration_styles[start].font_size_px,
                metrics.and_then(|metrics| metrics.underline_thickness),
                24.0,
                DecorationThicknessQuantize::Floor,
            );
            commands.push(TextDecorationCommand {
                x0,
                x1,
                y,
                thickness,
                color: styles[start].color,
                inset_ends: fitted_decoration_span,
            });
        }
        if styles[start].strikethrough {
            let strikethrough_position = metrics.and_then(|metrics| metrics.strikethrough_position);
            // CSP follows unusually high strikeout metrics for display fonts, but
            // ordinary fonts match the legacy fallback better in the focused samples.
            let metric_position = strikethrough_position.filter(|position| *position > 0.45);
            let y = decoration_y(
                origin.1,
                styles[start].font_size_px,
                0.66,
                metric_position,
                metrics.and_then(|metrics| metrics.strikethrough_thickness),
            );
            let thickness = decoration_thickness(
                decoration_styles[start].font_size_px,
                metrics.and_then(|metrics| metrics.strikethrough_thickness),
                24.0,
                DecorationThicknessQuantize::Round,
            );
            commands.push(TextDecorationCommand {
                x0,
                x1,
                y,
                thickness,
                color: styles[start].color,
                inset_ends: fitted_decoration_span,
            });
        }
        start = end;
    }
    commands
}

pub(super) fn decoration_y(
    origin_y: f32,
    font_size_px: f32,
    fallback_ratio: f32,
    metric_position: Option<f32>,
    metric_thickness: Option<f32>,
) -> f32 {
    let Some(position) = metric_position else {
        return origin_y + font_size_px * fallback_ratio;
    };
    let thickness = metric_thickness.unwrap_or(1.0 / 24.0) * font_size_px;
    let baseline_y = origin_y + font_size_px;
    baseline_y - position * font_size_px - thickness * 0.5
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DecorationThicknessQuantize {
    Floor,
    Round,
}

pub(super) fn decoration_thickness(
    font_size_px: f32,
    metric: Option<f32>,
    fallback_divisor: f32,
    quantize: DecorationThicknessQuantize,
) -> f32 {
    let thickness = metric
        .map(|ratio| ratio * font_size_px)
        .unwrap_or(font_size_px / fallback_divisor);
    match quantize {
        DecorationThicknessQuantize::Floor => thickness.floor(),
        DecorationThicknessQuantize::Round => thickness.round(),
    }
    .clamp(1.0, 16.0)
}

pub(super) fn draw_decoration_command(canvas: &Canvas, command: &TextDecorationCommand) {
    let mut paint = decoration_line_paint(command.color, command.inset_ends);
    let thickness = command.thickness.max(1.0);
    paint.set_stroke_width(thickness);
    let center_y = command.y + thickness * 0.5;
    let inset = decoration_line_end_inset(thickness, command.inset_ends);
    canvas.draw_line(
        Point::new(command.x0 + inset, center_y),
        Point::new((command.x1 - inset).max(command.x0 + inset), center_y),
        &paint,
    );
}

pub(super) fn decoration_line_paint(color: Rgba8, fitted_decoration_span: bool) -> Paint {
    let mut paint = text_paint(color);
    paint.set_anti_alias(!fitted_decoration_span);
    paint.set_stroke(true);
    paint
}

pub(super) fn decoration_line_end_inset(thickness: f32, inset_ends: bool) -> f32 {
    if inset_ends {
        thickness.max(1.0) * 0.5
    } else {
        0.0
    }
}
