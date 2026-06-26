use skia_safe::{Canvas, Font, Paint, Point, TextBlob};

use crate::RuntimeError;

use super::{
    TextCharStyle, TextRasterLayout,
    decoration::{TextDecorationCommand, draw_decoration_command, plan_text_decoration_commands},
    font::{FontResolver, TextFontMetrics, horizontal_glyph_font, skia_font, text_paint},
    shaped, shaped_text_probe_enabled, text_lines,
};

fn entry_quad_top_px(entry: &clip_file::metadata::TextLayerEntry) -> Option<f32> {
    let quad = entry.attributes.quad_verts_100?;
    Some(
        [quad[1], quad[3], quad[5], quad[7]]
            .into_iter()
            .min()
            .unwrap_or(0) as f32
            / 100.0,
    )
}

fn fit_single_line_to_quad_width(
    entry: &clip_file::metadata::TextLayerEntry,
    chars: &[char],
    lines: &[(usize, usize)],
    styles: &mut [TextCharStyle],
    fonts: &mut FontResolver,
) {
    let Some(&(start, end)) = lines.first() else {
        return;
    };
    if lines.len() != 1 || start >= end {
        return;
    }
    let Some(quad) = entry.attributes.quad_verts_100 else {
        return;
    };
    let xs = [quad[0], quad[2], quad[4], quad[6]];
    let Some(min_x) = xs.iter().min() else {
        return;
    };
    let Some(max_x) = xs.iter().max() else {
        return;
    };
    let target_width = (*max_x - *min_x) as f32 / 100.0;
    if target_width <= 1.0 {
        return;
    }
    let measured = measure_line_width(&chars[start..end], &styles[start..end], fonts);
    if measured <= 1.0 {
        return;
    }
    let scale = (target_width / measured).clamp(0.5, 2.0);
    if (scale - 1.0).abs() < 0.05 {
        return;
    }
    for style in &mut styles[start..end] {
        style.font_size_px = (style.font_size_px * scale).max(1.0);
    }
}

fn measure_line_width(chars: &[char], styles: &[TextCharStyle], fonts: &mut FontResolver) -> f32 {
    let mut width = 0.0f32;
    let mut start = 0usize;
    while start < chars.len() {
        let style = &styles[start];
        let end = glyph_run_end(start, chars.len(), styles);
        let text = chars[start..end].iter().collect::<String>();
        let run_width = fonts.resolve(style).map_or_else(
            || style.font_size_px * 0.55 * (end - start) as f32,
            |resolved| {
                let font = skia_font(&resolved, style);
                let paint = text_paint(style.color);
                font.measure_str(&text, Some(&paint)).0
            },
        );
        width += run_width;
        start = end;
    }
    width
}

#[derive(Clone, Debug)]
pub(super) struct HorizontalTextPlan {
    chars: Vec<char>,
    styles: Vec<TextCharStyle>,
    logical_styles: Vec<TextCharStyle>,
    pub(super) lines: Vec<HorizontalTextLinePlan>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct HorizontalTextLinePlan {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) origin: (f32, f32),
    pub(super) runs: Vec<HorizontalTextRunPlan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct HorizontalTextRunPlan {
    pub(super) start: usize,
    pub(super) end: usize,
}

pub(super) fn build_horizontal_text_plan(
    entry: &clip_file::metadata::TextLayerEntry,
    layout: &TextRasterLayout,
    chars: Vec<char>,
    mut styles: Vec<TextCharStyle>,
    logical_styles: Vec<TextCharStyle>,
    fonts: &mut FontResolver,
) -> HorizontalTextPlan {
    let text_lines = text_lines(&chars);
    fit_single_line_to_quad_width(entry, &chars, &text_lines, &mut styles, fonts);
    let default_font_size = styles
        .iter()
        .map(|style| style.font_size_px)
        .fold(1.0f32, f32::max);
    let line_height = (default_font_size * 1.2).max(1.0);
    let mut y = -entry_quad_top_px(entry).unwrap_or(0.0);
    let mut lines = Vec::with_capacity(text_lines.len());
    for (start, end) in text_lines {
        let line_width = measure_line_width(&chars[start..end], &styles[start..end], fonts);
        let x = match entry.attributes.align {
            Some(1) => layout.size.width as f32 - line_width,
            Some(2) => (layout.size.width as f32 - line_width) * 0.5,
            _ => 0.0,
        }
        .max(0.0);
        lines.push(HorizontalTextLinePlan {
            start,
            end,
            origin: (x, y),
            runs: horizontal_text_run_plans(start, end, &styles),
        });
        y += line_height;
    }
    HorizontalTextPlan {
        chars,
        styles,
        logical_styles,
        lines,
    }
}

fn horizontal_text_run_plans(
    start: usize,
    end: usize,
    styles: &[TextCharStyle],
) -> Vec<HorizontalTextRunPlan> {
    let mut runs = Vec::new();
    let mut run_start = start;
    while run_start < end {
        let run_end = glyph_run_end(run_start, end, styles);
        runs.push(HorizontalTextRunPlan {
            start: run_start,
            end: run_end,
        });
        run_start = run_end;
    }
    runs
}

pub(super) fn draw_horizontal_text_plan(
    canvas: &Canvas,
    plan: &HorizontalTextPlan,
    fonts: &mut FontResolver,
) -> Result<(), RuntimeError> {
    for line in &plan.lines {
        draw_horizontal_text_line(canvas, plan, line, fonts)?;
    }
    Ok(())
}

fn draw_horizontal_text_line(
    canvas: &Canvas,
    plan: &HorizontalTextPlan,
    line: &HorizontalTextLinePlan,
    fonts: &mut FontResolver,
) -> Result<(), RuntimeError> {
    let commands = plan_horizontal_text_line_commands(plan, line, fonts)?;
    for command in &commands.glyphs {
        draw_glyph_command(canvas, command);
    }
    for command in &commands.decorations {
        draw_decoration_command(canvas, command);
    }
    Ok(())
}

pub(super) struct HorizontalTextLineCommands {
    pub(super) glyphs: Vec<TextGlyphCommand>,
    pub(super) decorations: Vec<TextDecorationCommand>,
}

pub(super) struct TextGlyphCommand {
    pub(super) x: f32,
    baseline_y: f32,
    paint: Paint,
    payload: TextGlyphPayload,
}

enum TextGlyphPayload {
    Plain { text: String, font: Font },
    Shaped { blob: TextBlob },
}

pub(super) fn plan_horizontal_text_line_commands(
    plan: &HorizontalTextPlan,
    line: &HorizontalTextLinePlan,
    fonts: &mut FontResolver,
) -> Result<HorizontalTextLineCommands, RuntimeError> {
    if shaped_text_probe_enabled()
        && let Some(commands) = plan_shaped_horizontal_text_line_commands(plan, line, fonts)
    {
        return Ok(commands);
    }
    plan_plain_horizontal_text_line_commands(plan, line, fonts)
}

fn plan_plain_horizontal_text_line_commands(
    plan: &HorizontalTextPlan,
    line: &HorizontalTextLinePlan,
    fonts: &mut FontResolver,
) -> Result<HorizontalTextLineCommands, RuntimeError> {
    let mut x = line.origin.0;
    let styles = &plan.styles[line.start..line.end];
    let decoration_styles = &plan.logical_styles[line.start..line.end];
    let mut glyphs = Vec::with_capacity(line.runs.len());
    let mut char_positions = Vec::with_capacity(line.end.saturating_sub(line.start));
    let mut char_metrics = Vec::with_capacity(line.end.saturating_sub(line.start));
    for run in &line.runs {
        let start = run.start;
        let end = run.end;
        let style = &plan.styles[start];
        let Some(resolved) = fonts.resolve(style) else {
            for _ in start..end {
                let next_x = x + style.font_size_px * 0.55;
                char_positions.push((x, next_x));
                char_metrics.push(None);
                x = next_x;
            }
            continue;
        };
        let font = horizontal_glyph_font(&resolved, style, &plan.logical_styles[start]);
        let paint = text_paint(style.color);
        let baseline_y = horizontal_glyph_baseline_y(line.origin.1, &plan.logical_styles[start]);
        let text = plan.chars[start..end].iter().collect::<String>();
        let run_width = font.measure_str(&text, Some(&paint)).0;
        let char_advances = plan.chars[start..end]
            .iter()
            .map(|ch| font.measure_str(ch.to_string(), Some(&paint)).0)
            .collect::<Vec<_>>();
        glyphs.push(TextGlyphCommand {
            x,
            baseline_y,
            paint,
            payload: TextGlyphPayload::Plain { text, font },
        });
        let char_sum = char_advances.iter().sum::<f32>();
        let scale = if char_sum > 0.0 {
            run_width / char_sum
        } else {
            1.0
        };
        for advance in char_advances {
            let next_x = x + advance * scale;
            char_positions.push((x, next_x));
            char_metrics.push(Some(resolved.metrics));
            x = next_x;
        }
    }
    let decorations = plan_text_decoration_commands(
        line.origin,
        styles,
        decoration_styles,
        &char_positions,
        &char_metrics,
    );
    Ok(HorizontalTextLineCommands {
        glyphs,
        decorations,
    })
}

struct PreparedShapedHorizontalRun {
    start: usize,
    end: usize,
    text: String,
    font: Font,
    baseline_y: f32,
    paint: Paint,
    metrics: TextFontMetrics,
}

pub(super) fn plan_shaped_horizontal_text_line_commands(
    plan: &HorizontalTextPlan,
    line: &HorizontalTextLinePlan,
    fonts: &mut FontResolver,
) -> Option<HorizontalTextLineCommands> {
    let mut prepared_runs = Vec::with_capacity(line.runs.len());
    for run in &line.runs {
        let start = run.start;
        let end = run.end;
        let style = &plan.styles[start];
        let resolved = fonts.resolve(style)?;
        prepared_runs.push(PreparedShapedHorizontalRun {
            start,
            end,
            text: plan.chars[start..end].iter().collect::<String>(),
            font: horizontal_glyph_font(&resolved, style, &plan.logical_styles[start]),
            baseline_y: horizontal_glyph_baseline_y(line.origin.1, &plan.logical_styles[start]),
            paint: text_paint(style.color),
            metrics: resolved.metrics,
        });
    }
    let shaped_inputs = prepared_runs
        .iter()
        .map(|run| shaped::ShapedTextLineRunInput {
            text: &run.text,
            font: &run.font,
        })
        .collect::<Vec<_>>();
    let shaped_line = shaped::shape_text_line(&shaped_inputs)?;
    let line_end_x = line.origin.0 + shaped_line.advance_x;
    let mut glyphs = Vec::with_capacity(shaped_line.runs.len());
    for shaped_run in shaped_line.runs {
        let prepared = prepared_runs.get(shaped_run.input_index)?;
        glyphs.push(TextGlyphCommand {
            x: line.origin.0 + shaped_run.x,
            baseline_y: prepared.baseline_y,
            paint: prepared.paint.clone(),
            payload: TextGlyphPayload::Shaped {
                blob: shaped_run.blob,
            },
        });
    }
    let char_positions = shaped_line
        .char_positions
        .iter()
        .map(|(start_x, end_x)| (line.origin.0 + start_x, line.origin.0 + end_x))
        .collect::<Vec<_>>();
    let mut char_metrics = Vec::with_capacity(line.end.saturating_sub(line.start));
    for prepared in &prepared_runs {
        char_metrics.extend(
            std::iter::repeat(Some(prepared.metrics))
                .take(prepared.end.saturating_sub(prepared.start)),
        );
    }
    if char_positions.len() != line.end.saturating_sub(line.start)
        || char_metrics.len() != char_positions.len()
    {
        return None;
    }
    if char_positions
        .last()
        .map(|(_, end_x)| *end_x > line_end_x + 1.0)
        .unwrap_or(false)
    {
        return None;
    }
    let decorations = plan_text_decoration_commands(
        line.origin,
        &plan.styles[line.start..line.end],
        &plan.logical_styles[line.start..line.end],
        &char_positions,
        &char_metrics,
    );
    Some(HorizontalTextLineCommands {
        glyphs,
        decorations,
    })
}

fn draw_glyph_command(canvas: &Canvas, command: &TextGlyphCommand) {
    let point = Point::new(command.x, command.baseline_y);
    match &command.payload {
        TextGlyphPayload::Plain { text, font } => {
            canvas.draw_str(text, point, font, &command.paint);
        }
        TextGlyphPayload::Shaped { blob } => {
            canvas.draw_text_blob(blob, point, &command.paint);
        }
    }
}

pub(super) fn horizontal_glyph_baseline_y(origin_y: f32, logical_style: &TextCharStyle) -> f32 {
    origin_y + logical_style.font_size_px
}

pub(super) fn glyph_run_end(start: usize, len: usize, styles: &[TextCharStyle]) -> usize {
    let mut end = start + 1;
    while end < len && glyph_style_matches(&styles[start], &styles[end]) {
        end += 1;
    }
    end
}

fn glyph_style_matches(a: &TextCharStyle, b: &TextCharStyle) -> bool {
    a.font_name == b.font_name
        && a.fallback_font == b.fallback_font
        && (a.font_size_px - b.font_size_px).abs() < f32::EPSILON
        && a.color == b.color
        && a.bold == b.bold
        && a.italic == b.italic
}
