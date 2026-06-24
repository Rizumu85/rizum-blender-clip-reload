use std::collections::HashMap;

use clip_model::{CanvasSize, Rect, Rgba8};
use skia_safe::{
    Canvas, Color, Font, FontHinting, FontMgr, Paint, Point, Typeface, font, surfaces,
};

use crate::RuntimeError;

const SKIA_SYNTHETIC_ITALIC_SKEW: f32 = -0.17;
const SKIA_FITTED_SYNTHETIC_ITALIC_SKEW: f32 = -0.18;
const CJK_VERTICAL_ITEM_ADVANCE_EM: f32 = 0.90;
const CJK_VERTICAL_PURE_ITEM_ADVANCE_EM: f32 = 0.99;
const CJK_VERTICAL_MIXED_RIGHT_COLUMN_X_EM: f32 = 0.04;
const CJK_VERTICAL_PURE_RIGHT_COLUMN_X_EM: f32 = 0.10;
const CJK_VERTICAL_COLUMN_ADVANCE_EM: f32 = 1.22;
const CJK_VERTICAL_MIDPOINT_Y_EM: f32 = 0.93;
const CJK_VERTICAL_PURE_MIDPOINT_Y_EM: f32 = 1.00;
const CJK_VERTICAL_MIXED_MIDPOINT_OFFSET_EM: f32 = 0.01;
const CJK_VERTICAL_HORIZONTAL_RUN_X_OFFSET_EM: f32 = -0.02;
const CJK_VERTICAL_HORIZONTAL_RUN_Y_OFFSET_EM: f32 = 0.02;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TextRasterLayout {
    pub(crate) size: CanvasSize,
    pub(crate) offset_x: i32,
    pub(crate) offset_y: i32,
}

pub(crate) fn measure_text_source(
    source: &clip_file::metadata::TextLayerSource,
    canvas: CanvasSize,
) -> Result<TextRasterLayout, RuntimeError> {
    let Some(entry) = source.entries.first() else {
        return Ok(TextRasterLayout {
            size: CanvasSize::new(0, 0),
            offset_x: 0,
            offset_y: 0,
        });
    };
    if let Some(bbox) = entry.attributes.bbox {
        let width = bbox.right.saturating_sub(bbox.left).max(0) as u32;
        let height = bbox.bottom.saturating_sub(bbox.top).max(0) as u32;
        if width > 0 && height > 0 {
            if std::env::var_os("RIZUM_CLIP_TEXT_PROFILE").is_some() {
                eprintln!(
                    "text layout bbox -> layer={} text={:?} align={:?} layout_flags={:?} left={} top={} right={} bottom={} width={} height={} font_size_100={:?} quad={:?} box_size={:?} runs={} underline_spans={} strike_spans={}",
                    source.layer.id.0,
                    entry.text,
                    entry.attributes.align,
                    entry.attributes.layout_flags,
                    bbox.left,
                    bbox.top,
                    bbox.right,
                    bbox.bottom,
                    width,
                    height,
                    entry.attributes.font_size_100,
                    entry.attributes.quad_verts_100,
                    entry.attributes.box_size,
                    entry.attributes.runs.len(),
                    entry.attributes.underline_spans.len(),
                    entry.attributes.strikethrough_spans.len(),
                );
            }
            return Ok(TextRasterLayout {
                size: CanvasSize::new(width, height),
                offset_x: bbox.left,
                offset_y: bbox.top,
            });
        }
    }
    if let Some(quad) = entry.attributes.quad_verts_100 {
        let xs = [quad[0], quad[2], quad[4], quad[6]];
        let ys = [quad[1], quad[3], quad[5], quad[7]];
        let min_x = div_floor_100(*xs.iter().min().unwrap_or(&0));
        let max_x = div_ceil_100(*xs.iter().max().unwrap_or(&0));
        let min_y = div_floor_100(*ys.iter().min().unwrap_or(&0));
        let max_y = div_ceil_100(*ys.iter().max().unwrap_or(&0));
        let width = max_x.saturating_sub(min_x).max(0) as u32;
        let height = max_y.saturating_sub(min_y).max(0) as u32;
        if width > 0 && height > 0 {
            return Ok(TextRasterLayout {
                size: CanvasSize::new(width, height),
                offset_x: min_x,
                offset_y: min_y,
            });
        }
    }
    Ok(TextRasterLayout {
        size: canvas,
        offset_x: 0,
        offset_y: 0,
    })
}

pub(crate) fn render_text_source_region(
    source: &clip_file::metadata::TextLayerSource,
    layout: &TextRasterLayout,
    region: Rect,
) -> Result<clip_file::tiles::RgbaTileImage, RuntimeError> {
    let len = rgba_len(region.width, region.height)?;
    if layout.size.width == 0 || layout.size.height == 0 || region.width == 0 || region.height == 0
    {
        return Ok(clip_file::tiles::RgbaTileImage {
            width: region.width,
            height: region.height,
            pixels: vec![0; len],
        });
    }
    if region.x.saturating_add(region.width) > layout.size.width
        || region.y.saturating_add(region.height) > layout.size.height
    {
        return Err(RuntimeError::InvalidRegion);
    }
    let surface_size = (
        i32::try_from(layout.size.width)
            .map_err(|_| clip_gpu::GpuRenderError::TextureSizeOverflow)?,
        i32::try_from(layout.size.height)
            .map_err(|_| clip_gpu::GpuRenderError::TextureSizeOverflow)?,
    );
    let mut surface = surfaces::raster_n32_premul(surface_size)
        .ok_or(clip_gpu::GpuRenderError::TextureSizeOverflow)?;
    surface.canvas().clear(Color::TRANSPARENT);
    let mut fonts = FontResolver::new();
    for entry in &source.entries {
        render_entry_surface(
            surface.canvas(),
            source.resolution_dpi,
            layout,
            entry,
            &mut fonts,
        )?;
    }
    let image = surface.image_snapshot();
    let pixmap = image
        .peek_pixels()
        .ok_or(clip_gpu::GpuRenderError::ReadbackSizeOverflow)?;
    let bytes = pixmap
        .bytes()
        .ok_or(clip_gpu::GpuRenderError::ReadbackSizeOverflow)?;
    let mut pixels = vec![0; len];
    copy_skia_region_to_rgba(bytes, layout.size.width, region, &mut pixels);
    Ok(clip_file::tiles::RgbaTileImage {
        width: region.width,
        height: region.height,
        pixels,
    })
}

fn render_entry_surface(
    canvas: &Canvas,
    resolution_dpi: u32,
    layout: &TextRasterLayout,
    entry: &clip_file::metadata::TextLayerEntry,
    fonts: &mut FontResolver,
) -> Result<(), RuntimeError> {
    let mut styles = text_char_styles(entry, resolution_dpi);
    if styles.is_empty() {
        return Ok(());
    }
    let logical_styles = styles.clone();
    let chars: Vec<char> = entry.text.chars().collect();
    if text_layout_is_arc(entry) {
        return render_arc_entry_surface(canvas, entry, &chars, &styles, fonts);
    }
    if text_layout_is_vertical(entry) {
        return render_vertical_entry_surface(canvas, layout, entry, &chars, &styles, fonts);
    }
    let lines = text_lines(&chars);
    fit_single_line_to_quad_width(entry, &chars, &lines, &mut styles, fonts);
    let default_font_size = styles
        .iter()
        .map(|style| style.font_size_px)
        .fold(1.0f32, f32::max);
    let line_height = (default_font_size * 1.2).max(1.0);
    let mut y = -entry_quad_top_px(entry).unwrap_or(0.0);
    for (start, end) in lines {
        let line_width = measure_line_width(&chars[start..end], &styles[start..end], fonts);
        let x = match entry.attributes.align {
            Some(1) => layout.size.width as f32 - line_width,
            Some(2) => (layout.size.width as f32 - line_width) * 0.5,
            _ => 0.0,
        }
        .max(0.0);
        draw_line(
            canvas,
            (x, y),
            &chars[start..end],
            &styles[start..end],
            &logical_styles[start..end],
            fonts,
        )?;
        y += line_height;
    }
    Ok(())
}

fn text_layout_is_vertical(entry: &clip_file::metadata::TextLayerEntry) -> bool {
    entry
        .attributes
        .layout_flags
        .map(|flags| flags & 0x10 != 0)
        .unwrap_or(false)
}

fn text_layout_is_arc(entry: &clip_file::metadata::TextLayerEntry) -> bool {
    entry.attributes.path_mode == Some(1) && entry.attributes.path_center.is_some()
}

fn render_arc_entry_surface(
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

fn arc_outer_radius(entry: &clip_file::metadata::TextLayerEntry) -> Option<f32> {
    let (width, height) = entry.attributes.box_size?;
    let from_box = width.min(height) as f32 * 0.5;
    if from_box > 0.0 {
        return Some(from_box);
    }
    let (center_x, center_y) = entry.attributes.path_center?;
    Some(center_x.min(center_y).max(0) as f32)
}

fn char_advances(chars: &[char], styles: &[TextCharStyle], fonts: &mut FontResolver) -> Vec<f32> {
    chars
        .iter()
        .zip(styles.iter())
        .map(|(ch, style)| {
            let Some(resolved) = fonts.resolve(style) else {
                return style.font_size_px * 0.55;
            };
            let font = skia_font(&resolved, style);
            let paint = text_paint(style.color);
            let advance = font.measure_str(ch.to_string(), Some(&paint)).0;
            advance.max(style.font_size_px * 0.25)
        })
        .collect()
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

fn render_vertical_entry_surface(
    canvas: &Canvas,
    layout: &TextRasterLayout,
    entry: &clip_file::metadata::TextLayerEntry,
    chars: &[char],
    styles: &[TextCharStyle],
    fonts: &mut FontResolver,
) -> Result<(), RuntimeError> {
    let upright_layout = vertical_text_uses_upright_layout(chars);
    if upright_layout {
        return render_upright_vertical_entry_surface(canvas, layout, entry, chars, styles, fonts);
    }
    let Some((box_x, box_y, box_width, box_height)) = vertical_text_box(entry, layout, false)
    else {
        return Ok(());
    };
    let max_font_size = styles
        .iter()
        .map(|style| style.font_size_px)
        .fold(1.0f32, f32::max);
    let row_step = (max_font_size * 0.55).max(1.0);
    let right_column_inset = max_font_size * 0.522;
    let column_step = (max_font_size * 1.208).max(1.0);
    let rows_per_column = ((box_height / row_step).round() as usize).max(1);
    let advances = char_advances(chars, styles, fonts);
    let columns = vertical_text_columns(chars, rows_per_column);
    for (column, (column_start, column_end)) in columns.into_iter().enumerate() {
        let row_positions = vertical_column_row_positions(
            &advances[column_start..column_end],
            box_y
                + max_font_size * 0.318
                + row_step * (rows_per_column.saturating_sub(1)) as f32 * 0.5,
        );
        let center_x = box_x + box_width - right_column_inset - column as f32 * column_step;
        for (row, index) in (column_start..column_end).enumerate() {
            let ch = chars[index];
            let style = &styles[index];
            if ch == '\r' || ch == '\n' {
                continue;
            }
            let Some(resolved) = fonts.resolve(style) else {
                continue;
            };
            let center_y = row_positions
                .get(row)
                .copied()
                .unwrap_or_else(|| box_y + max_font_size * 0.31 + row as f32 * row_step);
            draw_vertical_char(canvas, ch, style, &resolved, center_x, center_y, false);
        }
    }
    Ok(())
}

fn render_upright_vertical_entry_surface(
    canvas: &Canvas,
    layout: &TextRasterLayout,
    entry: &clip_file::metadata::TextLayerEntry,
    chars: &[char],
    styles: &[TextCharStyle],
    fonts: &mut FontResolver,
) -> Result<(), RuntimeError> {
    let Some((_, box_y, _, box_height)) = vertical_text_box(entry, layout, true) else {
        return Ok(());
    };
    let max_font_size = styles
        .iter()
        .map(|style| style.font_size_px)
        .fold(1.0f32, f32::max);
    let has_horizontal_run = vertical_upright_items_have_horizontal_run(chars);
    let item_step = (max_font_size * cjk_vertical_item_advance_em(has_horizontal_run)).max(1.0);
    let rows_per_column = ((box_height / item_step).floor() as usize).max(1);
    let columns = vertical_upright_item_columns(chars, rows_per_column);
    let right_column_x = layout.size.width as f32 * 0.5
        + max_font_size * cjk_vertical_right_column_x_em(has_horizontal_run);
    let column_step = vertical_upright_column_step(max_font_size);
    let midpoint_y = box_y + max_font_size * cjk_vertical_midpoint_y_em(has_horizontal_run);
    for (column, items) in columns.into_iter().enumerate() {
        let advances = items
            .iter()
            .map(|item| {
                vertical_upright_item_advance(
                    item,
                    chars,
                    styles,
                    max_font_size,
                    fonts,
                    has_horizontal_run,
                )
            })
            .collect::<Vec<_>>();
        let row_positions = vertical_column_row_positions(&advances, midpoint_y);
        let center_x = right_column_x - column as f32 * column_step;
        for (row, item) in items.iter().enumerate() {
            let center_y = row_positions.get(row).copied().unwrap_or(midpoint_y);
            match item.kind {
                VerticalTextItemKind::UprightChar => {
                    let ch = chars[item.start];
                    let style = &styles[item.start];
                    let Some(resolved) = fonts.resolve(style) else {
                        continue;
                    };
                    draw_upright_vertical_char(
                        canvas,
                        ch,
                        style,
                        &resolved,
                        center_x,
                        center_y,
                        !has_horizontal_run,
                    );
                }
                VerticalTextItemKind::HorizontalRun => {
                    draw_horizontal_vertical_run(
                        canvas,
                        &chars[item.start..item.end],
                        &styles[item.start..item.end],
                        fonts,
                        center_x,
                        center_y,
                    )?;
                }
                VerticalTextItemKind::RotatedChar => {
                    let ch = chars[item.start];
                    let style = &styles[item.start];
                    let Some(resolved) = fonts.resolve(style) else {
                        continue;
                    };
                    draw_rotated_vertical_char(canvas, ch, style, &resolved, center_x, center_y);
                }
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VerticalTextItem {
    start: usize,
    end: usize,
    kind: VerticalTextItemKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VerticalTextItemKind {
    UprightChar,
    HorizontalRun,
    RotatedChar,
}

fn vertical_upright_column_step(max_font_size: f32) -> f32 {
    (max_font_size * CJK_VERTICAL_COLUMN_ADVANCE_EM).max(1.0)
}

fn vertical_text_uses_upright_layout(chars: &[char]) -> bool {
    let cjk_count = chars
        .iter()
        .filter(|ch| vertical_char_is_upright(**ch))
        .count();
    let visible_count = chars
        .iter()
        .filter(|ch| **ch != '\r' && **ch != '\n')
        .count();
    cjk_count > 0 && cjk_count * 2 >= visible_count
}

fn vertical_char_is_upright(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3000..=0x303f
            | 0x3040..=0x30ff
            | 0x3400..=0x4dbf
            | 0x4e00..=0x9fff
            | 0xf900..=0xfaff
            | 0xff00..=0xffef
    )
}

fn vertical_char_starts_horizontal_run(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
}

fn vertical_upright_item_columns(
    chars: &[char],
    rows_per_column: usize,
) -> Vec<Vec<VerticalTextItem>> {
    let mut columns = Vec::new();
    for (line_start, line_end) in text_lines(chars) {
        let mut line_items = Vec::new();
        let mut index = line_start;
        while index < line_end {
            let ch = chars[index];
            if vertical_char_is_upright(ch) {
                line_items.push(VerticalTextItem {
                    start: index,
                    end: index + 1,
                    kind: VerticalTextItemKind::UprightChar,
                });
                index += 1;
                continue;
            }
            if vertical_char_starts_horizontal_run(ch) {
                let start = index;
                index += 1;
                while index < line_end && vertical_char_starts_horizontal_run(chars[index]) {
                    index += 1;
                }
                line_items.push(VerticalTextItem {
                    start,
                    end: index,
                    kind: VerticalTextItemKind::HorizontalRun,
                });
                continue;
            }
            line_items.push(VerticalTextItem {
                start: index,
                end: index + 1,
                kind: VerticalTextItemKind::RotatedChar,
            });
            index += 1;
        }
        let mut start = 0usize;
        while start < line_items.len() {
            let end = start.saturating_add(rows_per_column).min(line_items.len());
            columns.push(line_items[start..end].to_vec());
            start = end;
        }
    }
    columns
}

fn vertical_upright_item_advance(
    item: &VerticalTextItem,
    chars: &[char],
    styles: &[TextCharStyle],
    max_font_size: f32,
    fonts: &mut FontResolver,
    has_horizontal_run: bool,
) -> f32 {
    match item.kind {
        VerticalTextItemKind::HorizontalRun => {
            let height = text_run_bounds(
                &chars[item.start..item.end],
                &styles[item.start..item.end],
                fonts,
            )
            .map(|(_, top, bottom)| bottom - top)
            .unwrap_or(max_font_size * 0.75);
            height.max(max_font_size * CJK_VERTICAL_ITEM_ADVANCE_EM)
        }
        VerticalTextItemKind::UprightChar | VerticalTextItemKind::RotatedChar => {
            max_font_size * cjk_vertical_item_advance_em(has_horizontal_run)
        }
    }
}

fn vertical_upright_items_have_horizontal_run(chars: &[char]) -> bool {
    chars
        .iter()
        .any(|ch| vertical_char_starts_horizontal_run(*ch))
}

fn cjk_vertical_item_advance_em(has_horizontal_run: bool) -> f32 {
    if has_horizontal_run {
        CJK_VERTICAL_ITEM_ADVANCE_EM
    } else {
        CJK_VERTICAL_PURE_ITEM_ADVANCE_EM
    }
}

fn cjk_vertical_right_column_x_em(has_horizontal_run: bool) -> f32 {
    if has_horizontal_run {
        CJK_VERTICAL_MIXED_RIGHT_COLUMN_X_EM
    } else {
        CJK_VERTICAL_PURE_RIGHT_COLUMN_X_EM
    }
}

fn cjk_vertical_midpoint_y_em(has_horizontal_run: bool) -> f32 {
    if has_horizontal_run {
        CJK_VERTICAL_MIDPOINT_Y_EM + CJK_VERTICAL_MIXED_MIDPOINT_OFFSET_EM
    } else {
        CJK_VERTICAL_PURE_MIDPOINT_Y_EM
    }
}

fn vertical_text_columns(chars: &[char], rows_per_column: usize) -> Vec<(usize, usize)> {
    let mut columns = Vec::new();
    for (line_start, line_end) in text_lines(chars) {
        if line_start >= line_end {
            continue;
        }
        let mut start = line_start;
        while start < line_end {
            let end = start.saturating_add(rows_per_column).min(line_end);
            columns.push((start, end));
            start = end;
        }
    }
    columns
}

fn vertical_column_row_positions(advances: &[f32], midpoint_y: f32) -> Vec<f32> {
    if advances.is_empty() {
        return Vec::new();
    }
    if advances.len() == 1 {
        return vec![midpoint_y];
    }
    let gaps = advances
        .windows(2)
        .map(|pair| ((pair[0] + pair[1]) * 0.5).max(1.0))
        .collect::<Vec<_>>();
    let total_span = gaps.iter().sum::<f32>();
    let mut y = midpoint_y - total_span * 0.5;
    let mut positions = Vec::with_capacity(advances.len());
    positions.push(y);
    for gap in gaps {
        y += gap;
        positions.push(y);
    }
    positions
}

fn vertical_text_box(
    entry: &clip_file::metadata::TextLayerEntry,
    layout: &TextRasterLayout,
    center_in_layout: bool,
) -> Option<(f32, f32, f32, f32)> {
    let (box_width, box_height) = entry.attributes.box_size?;
    if box_width <= 0 || box_height <= 0 {
        return None;
    }
    let quad = entry.attributes.quad_verts_100?;
    let xs = [quad[0], quad[2], quad[4], quad[6]];
    let ys = [quad[1], quad[3], quad[5], quad[7]];
    let min_x = *xs.iter().min()? as f32 / 100.0;
    let min_y = *ys.iter().min()? as f32 / 100.0;
    let box_width = box_width as f32;
    let box_height = box_height as f32;
    if center_in_layout {
        return Some((
            (layout.size.width as f32 - box_width) * 0.5,
            (layout.size.height as f32 - box_height) * 0.5,
            box_width,
            box_height,
        ));
    }
    let x = if min_x < 0.0 {
        layout.size.width as f32 + min_x
    } else {
        min_x
    };
    let y = if min_y < 0.0 {
        layout.size.height as f32 + min_y
    } else {
        min_y
    };
    Some((x, y, box_width, box_height))
}

fn draw_vertical_char(
    canvas: &Canvas,
    ch: char,
    style: &TextCharStyle,
    resolved: &ResolvedFont,
    center_x: f32,
    center_y: f32,
    upright: bool,
) {
    if upright {
        draw_upright_vertical_char(canvas, ch, style, resolved, center_x, center_y, false);
    } else {
        draw_rotated_vertical_char(canvas, ch, style, resolved, center_x, center_y);
    }
}

fn draw_upright_vertical_char(
    canvas: &Canvas,
    ch: char,
    style: &TextCharStyle,
    resolved: &ResolvedFont,
    center_x: f32,
    center_y: f32,
    disable_baseline_snap: bool,
) {
    let font = vertical_upright_font(resolved, style, disable_baseline_snap);
    let paint = text_paint(style.color);
    let text = ch.to_string();
    let (_, bounds) = font.measure_str(&text, Some(&paint));
    canvas.draw_str(
        &text,
        Point::new(
            center_x - bounds.left - bounds.width() * 0.5,
            center_y - bounds.top - bounds.height() * 0.5,
        ),
        &font,
        &paint,
    );
}

fn vertical_upright_font(
    resolved: &ResolvedFont,
    style: &TextCharStyle,
    disable_baseline_snap: bool,
) -> Font {
    let mut font = skia_font(resolved, style);
    if disable_baseline_snap {
        font.set_baseline_snap(false);
    }
    font
}

fn draw_horizontal_vertical_run(
    canvas: &Canvas,
    chars: &[char],
    styles: &[TextCharStyle],
    fonts: &mut FontResolver,
    center_x: f32,
    center_y: f32,
) -> Result<(), RuntimeError> {
    let Some((width, top, bottom)) = text_run_bounds(chars, styles, fonts) else {
        return Ok(());
    };
    let max_font_size = styles
        .iter()
        .map(|style| style.font_size_px)
        .fold(1.0f32, f32::max);
    let (offset_x, offset_y) = cjk_vertical_horizontal_run_offset(max_font_size);
    let adjusted_center_x = center_x + offset_x;
    let adjusted_center_y = center_y + offset_y;
    let baseline_y = adjusted_center_y - (top + bottom) * 0.5;
    let mut x = adjusted_center_x - width * 0.5;
    let mut start = 0usize;
    while start < chars.len() {
        let style = &styles[start];
        let end = glyph_run_end(start, chars.len(), styles);
        let Some(resolved) = fonts.resolve(style) else {
            x += style.font_size_px * 0.55 * (end - start) as f32;
            start = end;
            continue;
        };
        let font = vertical_horizontal_run_font(&resolved, style);
        let paint = text_paint(style.color);
        let text = chars[start..end].iter().collect::<String>();
        canvas.draw_str(&text, Point::new(x, baseline_y), &font, &paint);
        x += font.measure_str(&text, Some(&paint)).0;
        start = end;
    }
    Ok(())
}

fn cjk_vertical_horizontal_run_offset(max_font_size: f32) -> (f32, f32) {
    (
        max_font_size * CJK_VERTICAL_HORIZONTAL_RUN_X_OFFSET_EM,
        max_font_size * CJK_VERTICAL_HORIZONTAL_RUN_Y_OFFSET_EM,
    )
}

fn text_run_bounds(
    chars: &[char],
    styles: &[TextCharStyle],
    fonts: &mut FontResolver,
) -> Option<(f32, f32, f32)> {
    let mut width = 0.0f32;
    let mut top = f32::INFINITY;
    let mut bottom = f32::NEG_INFINITY;
    let mut start = 0usize;
    while start < chars.len() {
        let style = &styles[start];
        let end = glyph_run_end(start, chars.len(), styles);
        let Some(resolved) = fonts.resolve(style) else {
            width += style.font_size_px * 0.55 * (end - start) as f32;
            top = top.min(0.0);
            bottom = bottom.max(style.font_size_px);
            start = end;
            continue;
        };
        let font = vertical_horizontal_run_font(&resolved, style);
        let paint = text_paint(style.color);
        let text = chars[start..end].iter().collect::<String>();
        let (advance, bounds) = font.measure_str(&text, Some(&paint));
        width += advance;
        top = top.min(bounds.top);
        bottom = bottom.max(bounds.bottom);
        start = end;
    }
    (width > 0.0 && top.is_finite() && bottom.is_finite()).then_some((width, top, bottom))
}

fn draw_rotated_vertical_char(
    canvas: &Canvas,
    ch: char,
    style: &TextCharStyle,
    resolved: &ResolvedFont,
    center_x: f32,
    center_y: f32,
) {
    let font = skia_font(resolved, style);
    let paint = text_paint(style.color);
    let text = ch.to_string();
    let (_, bounds) = font.measure_str(&text, Some(&paint));
    let save_count = canvas.save();
    canvas.translate(Point::new(center_x, center_y));
    canvas.rotate(90.0, None);
    canvas.draw_str(
        &text,
        Point::new(
            -bounds.width() * 0.5 - bounds.left,
            style.font_size_px * 0.35,
        ),
        &font,
        &paint,
    );
    canvas.restore_to_count(save_count);
}

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

fn draw_line(
    canvas: &Canvas,
    origin: (f32, f32),
    chars: &[char],
    styles: &[TextCharStyle],
    decoration_styles: &[TextCharStyle],
    fonts: &mut FontResolver,
) -> Result<(), RuntimeError> {
    let mut x = origin.0;
    let mut char_positions = Vec::with_capacity(chars.len());
    let mut char_metrics = Vec::with_capacity(chars.len());
    let mut start = 0usize;
    while start < chars.len() {
        let style = &styles[start];
        let end = glyph_run_end(start, chars.len(), styles);
        let Some(resolved) = fonts.resolve(style) else {
            for _ in start..end {
                let next_x = x + style.font_size_px * 0.55;
                char_positions.push((x, next_x));
                char_metrics.push(None);
                x = next_x;
            }
            start = end;
            continue;
        };
        let font = horizontal_glyph_font(&resolved, style, &decoration_styles[start]);
        let paint = text_paint(style.color);
        let baseline_y = horizontal_glyph_baseline_y(origin.1, &decoration_styles[start]);
        let text = chars[start..end].iter().collect::<String>();
        canvas.draw_str(&text, Point::new(x, baseline_y), &font, &paint);
        let run_width = font.measure_str(&text, Some(&paint)).0;
        let char_advances = chars[start..end]
            .iter()
            .map(|ch| font.measure_str(ch.to_string(), Some(&paint)).0)
            .collect::<Vec<_>>();
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
        start = end;
    }
    draw_text_decorations(
        canvas,
        origin,
        styles,
        decoration_styles,
        &char_positions,
        &char_metrics,
    );
    Ok(())
}

fn horizontal_glyph_baseline_y(origin_y: f32, logical_style: &TextCharStyle) -> f32 {
    origin_y + logical_style.font_size_px
}

fn glyph_run_end(start: usize, len: usize, styles: &[TextCharStyle]) -> usize {
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

fn skia_font(resolved: &ResolvedFont, style: &TextCharStyle) -> Font {
    let mut font = Font::from_typeface(resolved.typeface.clone(), style.font_size_px);
    font.set_subpixel(true);
    font.set_edging(font::Edging::AntiAlias);
    font.set_hinting(FontHinting::None);
    if resolved.synthetic_italic {
        font.set_skew_x(SKIA_SYNTHETIC_ITALIC_SKEW);
    }
    font
}

fn horizontal_glyph_font(
    resolved: &ResolvedFont,
    style: &TextCharStyle,
    logical_style: &TextCharStyle,
) -> Font {
    let mut font = skia_font(resolved, style);
    if resolved.synthetic_italic && text_style_is_quad_fitted(style, logical_style) {
        font.set_skew_x(horizontal_synthetic_italic_skew(style, logical_style));
    }
    font
}

fn horizontal_synthetic_italic_skew(style: &TextCharStyle, logical_style: &TextCharStyle) -> f32 {
    if text_style_is_quad_fitted(style, logical_style) {
        SKIA_FITTED_SYNTHETIC_ITALIC_SKEW
    } else {
        SKIA_SYNTHETIC_ITALIC_SKEW
    }
}

fn text_style_is_quad_fitted(style: &TextCharStyle, logical_style: &TextCharStyle) -> bool {
    (style.font_size_px - logical_style.font_size_px).abs() > f32::EPSILON
}

fn vertical_horizontal_run_font(resolved: &ResolvedFont, style: &TextCharStyle) -> Font {
    let mut font = skia_font(resolved, style);
    font.set_baseline_snap(false);
    font
}

fn text_paint(color: Rgba8) -> Paint {
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_color(Color::from_argb(color.a, color.r, color.g, color.b));
    paint
}

fn copy_skia_region_to_rgba(bytes: &[u8], source_width: u32, region: Rect, output: &mut [u8]) {
    let source_stride = usize::try_from(u64::from(source_width) * 4).unwrap_or(0);
    let output_stride = usize::try_from(u64::from(region.width) * 4).unwrap_or(0);
    for row in 0..region.height {
        let source_start = usize::try_from(
            (u64::from(region.y) + u64::from(row)) * u64::from(source_width) * 4
                + u64::from(region.x) * 4,
        )
        .unwrap_or(usize::MAX);
        let output_start =
            usize::try_from(u64::from(row) * u64::from(region.width) * 4).unwrap_or(usize::MAX);
        let source_end = source_start.saturating_add(output_stride);
        let output_end = output_start.saturating_add(output_stride);
        let Some(source_row) = bytes.get(source_start..source_end) else {
            continue;
        };
        let Some(output_row) = output.get_mut(output_start..output_end) else {
            continue;
        };
        if source_stride == 0 {
            continue;
        }
        for (source, target) in source_row
            .chunks_exact(4)
            .zip(output_row.chunks_exact_mut(4))
        {
            target[0] = source[2];
            target[1] = source[1];
            target[2] = source[0];
            target[3] = source[3];
        }
    }
}

fn draw_text_decorations(
    canvas: &Canvas,
    origin: (f32, f32),
    styles: &[TextCharStyle],
    decoration_styles: &[TextCharStyle],
    char_positions: &[(f32, f32)],
    char_metrics: &[Option<TextFontMetrics>],
) {
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
            draw_decoration_line(
                canvas,
                x0,
                x1,
                y,
                thickness,
                styles[start].color,
                fitted_decoration_span,
            );
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
            draw_decoration_line(
                canvas,
                x0,
                x1,
                y,
                thickness,
                styles[start].color,
                fitted_decoration_span,
            );
        }
        start = end;
    }
}

fn decoration_y(
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
enum DecorationThicknessQuantize {
    Floor,
    Round,
}

fn decoration_thickness(
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

fn draw_decoration_line(
    canvas: &Canvas,
    x0: f32,
    x1: f32,
    y: f32,
    thickness: f32,
    color: Rgba8,
    inset_ends: bool,
) {
    let mut paint = decoration_line_paint(color, inset_ends);
    let thickness = thickness.max(1.0);
    paint.set_stroke_width(thickness);
    let center_y = y + thickness * 0.5;
    let inset = decoration_line_end_inset(thickness, inset_ends);
    canvas.draw_line(
        Point::new(x0 + inset, center_y),
        Point::new((x1 - inset).max(x0 + inset), center_y),
        &paint,
    );
}

fn decoration_line_paint(color: Rgba8, fitted_decoration_span: bool) -> Paint {
    let mut paint = text_paint(color);
    paint.set_anti_alias(!fitted_decoration_span);
    paint.set_stroke(true);
    paint
}

fn decoration_line_end_inset(thickness: f32, inset_ends: bool) -> f32 {
    if inset_ends {
        thickness.max(1.0) * 0.5
    } else {
        0.0
    }
}

#[derive(Clone, Debug, PartialEq)]
struct TextCharStyle {
    font_name: Option<String>,
    fallback_font: Option<String>,
    font_size_px: f32,
    color: Rgba8,
    bold: bool,
    italic: bool,
    underline: bool,
    strikethrough: bool,
}

fn text_char_styles(
    entry: &clip_file::metadata::TextLayerEntry,
    resolution_dpi: u32,
) -> Vec<TextCharStyle> {
    let char_count = entry.text.chars().count();
    if char_count == 0 {
        return Vec::new();
    }
    let attrs = &entry.attributes;
    let default_font_size_px = attrs
        .font_size_100
        .map(|value| value as f32 / 100.0 * resolution_dpi as f32 / 72.0)
        .unwrap_or(12.0)
        .max(1.0);
    let default_color = attrs.color.unwrap_or(Rgba8 {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    });
    let default_style = TextCharStyle {
        font_name: attrs.default_font.clone(),
        fallback_font: attrs.fallback_font.clone(),
        font_size_px: default_font_size_px,
        color: default_color,
        bold: font_name_implies_bold(attrs.default_font.as_deref()),
        italic: font_name_implies_italic(attrs.default_font.as_deref()),
        underline: false,
        strikethrough: false,
    };
    let mut styles = vec![default_style; char_count];
    for run in &attrs.runs {
        let start = run.start.max(0) as usize;
        let end = start.saturating_add(run.length as usize).min(char_count);
        if start >= end {
            continue;
        }
        let run_font = run
            .font
            .as_ref()
            .and_then(|font| (!font.is_empty()).then_some(font.clone()));
        let mapped_font = run_font
            .as_ref()
            .and_then(|font| font_mapping(attrs, font).or_else(|| Some(font.clone())));
        let color = if (run.field_defaults_flags & 1) != 0 {
            run.color
        } else {
            default_color
        };
        let font_size_px = if (run.field_defaults_flags & 2) != 0 && run.font_scale != 0 {
            (default_font_size_px * run.font_scale as f32 / 100.0).max(1.0)
        } else {
            default_font_size_px
        };
        for style in &mut styles[start..end] {
            let font_name = mapped_font.clone().or_else(|| attrs.default_font.clone());
            let bold = (run.style_flags & 1) != 0 || font_name_implies_bold(font_name.as_deref());
            let italic =
                (run.style_flags & 2) != 0 || font_name_implies_italic(font_name.as_deref());
            style.font_name = font_name;
            style.font_size_px = font_size_px;
            style.color = color;
            style.bold = bold;
            style.italic = italic;
        }
    }
    apply_text_decoration_spans(
        &mut styles,
        &attrs.underline_spans,
        TextDecoration::Underline,
    );
    apply_text_decoration_spans(
        &mut styles,
        &attrs.strikethrough_spans,
        TextDecoration::Strikethrough,
    );
    styles
}

#[derive(Clone, Copy)]
enum TextDecoration {
    Underline,
    Strikethrough,
}

fn apply_text_decoration_spans(
    styles: &mut [TextCharStyle],
    spans: &[clip_file::metadata::TextLayerSpan],
    decoration: TextDecoration,
) {
    for span in spans {
        let start = span.start.max(0) as usize;
        let end = start.saturating_add(span.length as usize).min(styles.len());
        if start >= end {
            continue;
        }
        for style in &mut styles[start..end] {
            match decoration {
                TextDecoration::Underline => style.underline = true,
                TextDecoration::Strikethrough => style.strikethrough = true,
            }
        }
    }
}

fn font_name_implies_bold(name: Option<&str>) -> bool {
    name.map(|name| {
        let normalized = name.to_ascii_lowercase();
        normalized.contains("bold")
            || normalized.contains("semibold")
            || normalized.contains("demibold")
            || normalized.contains("black")
            || normalized.contains("heavy")
    })
    .unwrap_or(false)
}

fn font_name_implies_italic(name: Option<&str>) -> bool {
    name.map(|name| {
        let normalized = name.to_ascii_lowercase();
        normalized.contains("italic") || normalized.contains("oblique")
    })
    .unwrap_or(false)
}

fn font_mapping(
    attrs: &clip_file::metadata::TextLayerAttributes,
    display_name: &str,
) -> Option<String> {
    attrs
        .fonts
        .iter()
        .find(|font| font.display_name == display_name)
        .map(|font| font.font_name.clone())
}

fn text_lines(chars: &[char]) -> Vec<(usize, usize)> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut index = 0usize;
    while index < chars.len() {
        match chars[index] {
            '\r' => {
                lines.push((start, index));
                index += usize::from(index + 1 < chars.len() && chars[index + 1] == '\n');
                start = index + 1;
            }
            '\n' => {
                lines.push((start, index));
                start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    lines.push((start, chars.len()));
    lines
}

struct FontResolver {
    db: fontdb::Database,
    font_mgr: FontMgr,
    cache: HashMap<FontRequest, Option<ResolvedFont>>,
}

#[derive(Clone)]
struct ResolvedFont {
    typeface: Typeface,
    metrics: TextFontMetrics,
    synthetic_italic: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct TextFontMetrics {
    underline_thickness: Option<f32>,
    underline_position: Option<f32>,
    strikethrough_thickness: Option<f32>,
    strikethrough_position: Option<f32>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct FontRequest {
    name: Option<String>,
    fallback: Option<String>,
    bold: bool,
    italic: bool,
}

impl FontResolver {
    fn new() -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        Self {
            db,
            font_mgr: FontMgr::new(),
            cache: HashMap::new(),
        }
    }

    fn resolve(&mut self, style: &TextCharStyle) -> Option<ResolvedFont> {
        let request = FontRequest {
            name: style.font_name.clone(),
            fallback: style.fallback_font.clone(),
            bold: style.bold,
            italic: style.italic,
        };
        if !self.cache.contains_key(&request) {
            let font = self.load_font(&request);
            self.cache.insert(request.clone(), font);
        }
        self.cache.get(&request).cloned().flatten()
    }

    fn load_font(&self, request: &FontRequest) -> Option<ResolvedFont> {
        let mut names = Vec::new();
        if let Some(name) = &request.name {
            names.extend(font_name_candidates(name));
        }
        if let Some(name) = &request.fallback {
            names.extend(font_name_candidates(name));
        }
        names.extend([
            "Microsoft YaHei".to_owned(),
            "Arial".to_owned(),
            "Tahoma".to_owned(),
        ]);
        for name in names {
            if let Some(font) = self.load_named_font(&name, request.bold, request.italic) {
                return Some(font);
            }
        }
        self.load_family_font(fontdb::Family::SansSerif, request.bold, request.italic)
    }

    fn load_named_font(&self, name: &str, bold: bool, italic: bool) -> Option<ResolvedFont> {
        self.load_family_font(fontdb::Family::Name(name), bold, italic)
            .or_else(|| self.load_postscript_font(name, bold, italic))
    }

    fn load_postscript_font(&self, name: &str, bold: bool, italic: bool) -> Option<ResolvedFont> {
        let normalized_name = normalize_font_lookup_name(name);
        let face = self.db.faces().find(|face| {
            normalize_font_lookup_name(&face.post_script_name) == normalized_name
                && (!bold || face.weight >= fontdb::Weight::BOLD)
        })?;
        self.load_font_id(face.id, italic && face.style != fontdb::Style::Italic)
    }

    fn load_family_font(
        &self,
        family: fontdb::Family<'_>,
        bold: bool,
        italic: bool,
    ) -> Option<ResolvedFont> {
        let families = [family];
        let query = fontdb::Query {
            families: &families,
            weight: if bold {
                fontdb::Weight::BOLD
            } else {
                fontdb::Weight::NORMAL
            },
            stretch: fontdb::Stretch::Normal,
            style: if italic {
                fontdb::Style::Italic
            } else {
                fontdb::Style::Normal
            },
        };
        if let Some(id) = self.db.query(&query) {
            let synthetic_italic = italic
                && self
                    .db
                    .face(id)
                    .map(|face| face.style != fontdb::Style::Italic)
                    .unwrap_or(false);
            return self.load_font_id(id, synthetic_italic);
        }
        if !italic {
            return None;
        }
        let normal_query = fontdb::Query {
            families: &families,
            weight: if bold {
                fontdb::Weight::BOLD
            } else {
                fontdb::Weight::NORMAL
            },
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        };
        let id = self.db.query(&normal_query)?;
        self.load_font_id(id, true)
    }

    fn load_font_id(&self, id: fontdb::ID, synthetic_italic: bool) -> Option<ResolvedFont> {
        if std::env::var_os("RIZUM_CLIP_TEXT_PROFILE").is_some()
            && let Some(face) = self.db.face(id)
        {
            eprintln!(
                "text font -> families={:?} post_script={} weight={:?} style={:?} index={} source={:?} synthetic_italic={}",
                face.families,
                face.post_script_name,
                face.weight,
                face.style,
                face.index,
                face.source,
                synthetic_italic,
            );
        }
        let mut font = None;
        let mut metrics = TextFontMetrics::default();
        self.db.with_face_data(id, |data, face_index| {
            if let Ok(face) = ttf_parser::Face::parse(data, face_index) {
                metrics = text_font_metrics(&face);
            }
            font = self.font_mgr.new_from_data(data, Some(face_index as usize));
        });
        font.map(|typeface| ResolvedFont {
            typeface,
            metrics,
            synthetic_italic,
        })
    }
}

fn text_font_metrics(face: &ttf_parser::Face<'_>) -> TextFontMetrics {
    let units_per_em = f32::from(face.units_per_em()).max(1.0);
    let underline = face.underline_metrics();
    let strikethrough = face.strikeout_metrics();
    TextFontMetrics {
        underline_thickness: underline
            .map(|metrics| f32::from(metrics.thickness.max(1)) / units_per_em),
        underline_position: underline.map(|metrics| f32::from(metrics.position) / units_per_em),
        strikethrough_thickness: strikethrough
            .map(|metrics| f32::from(metrics.thickness.max(1)) / units_per_em),
        strikethrough_position: strikethrough
            .map(|metrics| f32::from(metrics.position) / units_per_em),
    }
}

fn font_name_candidates(name: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    push_unique_font_candidate(&mut candidates, name.trim());
    let without_vf_dash = name.replace("VF-", "-").replace("VF_", "_");
    push_unique_font_candidate(&mut candidates, without_vf_dash.trim());
    let spaced = name.replace(['_', '-'], " ");
    push_unique_font_candidate(&mut candidates, spaced.trim());
    push_unique_font_candidate(&mut candidates, spaced.replace("VF ", " VF ").trim());
    push_unique_font_candidate(&mut candidates, spaced.replace("VF ", " ").trim());
    let without_style = strip_font_style_suffix(&spaced);
    push_unique_font_candidate(&mut candidates, without_style.trim());
    candidates
}

fn push_unique_font_candidate(candidates: &mut Vec<String>, name: &str) {
    if name.is_empty() {
        return;
    }
    if !candidates
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
    {
        candidates.push(name.to_owned());
    }
}

fn strip_font_style_suffix(name: &str) -> String {
    let mut words: Vec<&str> = name.split_whitespace().collect();
    while words
        .last()
        .map(|word| font_style_suffix(word))
        .unwrap_or(false)
    {
        words.pop();
    }
    words.join(" ")
}

fn font_style_suffix(word: &str) -> bool {
    matches!(
        word.to_ascii_lowercase().as_str(),
        "regular"
            | "normal"
            | "bold"
            | "semibold"
            | "demibold"
            | "black"
            | "heavy"
            | "light"
            | "medium"
            | "italic"
            | "oblique"
    )
}

fn normalize_font_lookup_name(name: &str) -> String {
    name.chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn rgba_len(width: u32, height: u32) -> Result<usize, RuntimeError> {
    usize::try_from(
        u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(clip_gpu::GpuRenderError::TextureSizeOverflow)?,
    )
    .map_err(|_| clip_gpu::GpuRenderError::TextureSizeOverflow.into())
}

fn div_floor_100(value: i32) -> i32 {
    value.div_euclid(100)
}

fn div_ceil_100(value: i32) -> i32 {
    value.div_euclid(100) + i32::from(value.rem_euclid(100) != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clip_model::{LayerKind, LayerVisibility};
    use skia_safe::FontStyle;

    #[test]
    fn renders_basic_text_with_nonzero_alpha() {
        let source = clip_file::metadata::TextLayerSource {
            layer: clip_file::metadata::LayerRecord {
                id: clip_model::LayerId(5),
                kind: LayerKind::Text,
                visibility: LayerVisibility(1),
            },
            resolution_dpi: 72,
            entries: vec![clip_file::metadata::TextLayerEntry {
                text: "Test".to_owned(),
                attributes: clip_file::metadata::TextLayerAttributes {
                    default_font: Some("Arial".to_owned()),
                    fallback_font: Some("Tahoma".to_owned()),
                    fonts: Vec::new(),
                    layout_flags: None,
                    path_mode: None,
                    path_angle_a_degrees: None,
                    path_angle_b_degrees: None,
                    path_center: None,
                    font_size_100: Some(3200),
                    color: Some(Rgba8 {
                        r: 39,
                        g: 39,
                        b: 39,
                        a: 255,
                    }),
                    bbox: Some(clip_file::metadata::TextLayerRect {
                        left: 0,
                        top: 0,
                        right: 160,
                        bottom: 64,
                    }),
                    quad_verts_100: None,
                    box_size: None,
                    align: Some(0),
                    underline_spans: Vec::new(),
                    strikethrough_spans: Vec::new(),
                    runs: Vec::new(),
                },
            }],
        };
        let layout = measure_text_source(&source, CanvasSize::new(200, 200)).unwrap();
        let image = render_text_source_region(
            &source,
            &layout,
            Rect::new(0, 0, layout.size.width, layout.size.height),
        )
        .unwrap();

        assert!(image.pixels.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn font_name_candidates_strip_style_suffixes() {
        assert_eq!(
            font_name_candidates("HarmonyOS_Sans_Bold"),
            vec![
                "HarmonyOS_Sans_Bold".to_owned(),
                "HarmonyOS Sans Bold".to_owned(),
                "HarmonyOS Sans".to_owned(),
            ],
        );
    }

    #[test]
    fn font_name_candidates_map_variable_font_style_names() {
        assert!(
            font_name_candidates("MiSansVF-ExtraLight").contains(&"MiSans-ExtraLight".to_owned())
        );
        assert!(
            font_name_candidates("MiSansVF-ExtraLight").contains(&"MiSans ExtraLight".to_owned())
        );
    }

    #[test]
    fn skia_font_uses_unhinted_grayscale_text_rasterization() {
        let typeface = FontMgr::new()
            .legacy_make_typeface(None, FontStyle::normal())
            .expect("default typeface");
        let resolved = ResolvedFont {
            typeface,
            metrics: TextFontMetrics::default(),
            synthetic_italic: false,
        };
        let style = TextCharStyle {
            font_name: Some("Default".to_owned()),
            fallback_font: None,
            font_size_px: 24.0,
            color: Rgba8 {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
        };

        let font = skia_font(&resolved, &style);

        assert!(font.is_subpixel());
        assert_eq!(font.edging(), font::Edging::AntiAlias);
        assert_eq!(font.hinting(), FontHinting::None);
    }

    #[test]
    fn vertical_horizontal_run_font_disables_baseline_snap() {
        let typeface = FontMgr::new()
            .legacy_make_typeface(None, FontStyle::normal())
            .expect("default typeface");
        let resolved = ResolvedFont {
            typeface,
            metrics: TextFontMetrics::default(),
            synthetic_italic: false,
        };
        let style = TextCharStyle {
            font_name: Some("Default".to_owned()),
            fallback_font: None,
            font_size_px: 24.0,
            color: Rgba8 {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
        };

        let font = vertical_horizontal_run_font(&resolved, &style);

        assert!(!font.is_baseline_snap());
        assert_eq!(font.hinting(), FontHinting::None);
    }

    #[test]
    fn pure_cjk_vertical_upright_font_can_disable_baseline_snap() {
        let typeface = FontMgr::new()
            .legacy_make_typeface(None, FontStyle::normal())
            .expect("default typeface");
        let resolved = ResolvedFont {
            typeface,
            metrics: TextFontMetrics::default(),
            synthetic_italic: false,
        };
        let style = TextCharStyle {
            font_name: Some("Default".to_owned()),
            fallback_font: None,
            font_size_px: 24.0,
            color: Rgba8 {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
        };

        let pure_cjk_font = vertical_upright_font(&resolved, &style, true);
        let mixed_run_font = vertical_upright_font(&resolved, &style, false);

        assert!(!pure_cjk_font.is_baseline_snap());
        assert!(mixed_run_font.is_baseline_snap());
    }

    #[test]
    fn glyph_runs_ignore_decoration_but_split_glyph_style() {
        let base = TextCharStyle {
            font_name: Some("Arial".to_owned()),
            fallback_font: None,
            font_size_px: 24.0,
            color: Rgba8 {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
        };
        let mut underlined = base.clone();
        underlined.underline = true;
        let mut red = base.clone();
        red.color = Rgba8 {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        };
        let styles = vec![base.clone(), underlined, red];

        assert_eq!(glyph_run_end(0, styles.len(), &styles), 2);
        assert_eq!(glyph_run_end(2, styles.len(), &styles), 3);
    }

    #[test]
    fn horizontal_glyph_baseline_uses_logical_prefit_size() {
        let logical = TextCharStyle {
            font_name: Some("MiSans".to_owned()),
            fallback_font: None,
            font_size_px: 75.0,
            color: Rgba8 {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            bold: false,
            italic: true,
            underline: true,
            strikethrough: true,
        };

        assert_eq!(horizontal_glyph_baseline_y(3.0, &logical), 78.0);
    }

    #[test]
    fn text_char_styles_expand_decoration_spans() {
        let entry = clip_file::metadata::TextLayerEntry {
            text: "Test".to_owned(),
            attributes: clip_file::metadata::TextLayerAttributes {
                default_font: Some("Arial".to_owned()),
                fallback_font: None,
                fonts: Vec::new(),
                layout_flags: None,
                path_mode: None,
                path_angle_a_degrees: None,
                path_angle_b_degrees: None,
                path_center: None,
                font_size_100: Some(3200),
                color: None,
                bbox: None,
                quad_verts_100: None,
                box_size: None,
                align: None,
                underline_spans: vec![clip_file::metadata::TextLayerSpan {
                    start: 0,
                    length: 2,
                }],
                strikethrough_spans: vec![clip_file::metadata::TextLayerSpan {
                    start: 1,
                    length: 2,
                }],
                runs: Vec::new(),
            },
        };

        let styles = text_char_styles(&entry, 72);

        assert!(styles[0].underline);
        assert!(styles[1].underline);
        assert!(!styles[2].underline);
        assert!(!styles[0].strikethrough);
        assert!(styles[1].strikethrough);
        assert!(styles[2].strikethrough);
    }

    #[test]
    fn vertical_text_box_maps_negative_quad_x_from_right_edge() {
        let entry = clip_file::metadata::TextLayerEntry {
            text: "Test".to_owned(),
            attributes: clip_file::metadata::TextLayerAttributes {
                default_font: Some("Arial".to_owned()),
                fallback_font: None,
                fonts: Vec::new(),
                layout_flags: Some(0x10),
                path_mode: None,
                path_angle_a_degrees: None,
                path_angle_b_degrees: None,
                path_center: None,
                font_size_100: Some(900),
                color: None,
                bbox: None,
                quad_verts_100: Some([-16500, 1000, 0, 1000, 0, 9800, -16500, 9800]),
                box_size: Some((165, 98)),
                align: Some(2),
                underline_spans: Vec::new(),
                strikethrough_spans: Vec::new(),
                runs: Vec::new(),
            },
        };
        let layout = TextRasterLayout {
            size: CanvasSize::new(222, 108),
            offset_x: -8,
            offset_y: 35,
        };

        let (x, y, width, height) = vertical_text_box(&entry, &layout, false).unwrap();

        assert_eq!(x.round() as i32, 57);
        assert_eq!(y.round() as i32, 10);
        assert_eq!(width.round() as i32, 165);
        assert_eq!(height.round() as i32, 98);
    }

    #[test]
    fn arc_text_mode_uses_path_center_and_box_radius() {
        let entry = clip_file::metadata::TextLayerEntry {
            text: "Test".to_owned(),
            attributes: clip_file::metadata::TextLayerAttributes {
                default_font: Some("Arial".to_owned()),
                fallback_font: None,
                fonts: Vec::new(),
                layout_flags: Some(0),
                path_mode: Some(1),
                path_angle_a_degrees: Some(195),
                path_angle_b_degrees: Some(165),
                path_center: Some((171, 171)),
                font_size_100: Some(900),
                color: None,
                bbox: None,
                quad_verts_100: Some([-17100, -17100, 17100, -17100, 17100, 17100, -17100, 17100]),
                box_size: Some((342, 342)),
                align: Some(2),
                underline_spans: Vec::new(),
                strikethrough_spans: Vec::new(),
                runs: Vec::new(),
            },
        };

        assert!(text_layout_is_arc(&entry));
        assert_eq!(arc_outer_radius(&entry).unwrap().round() as i32, 171);
    }

    #[test]
    fn vertical_row_positions_follow_adjacent_glyph_advances() {
        let wide_pair = vertical_column_row_positions(&[55.0, 50.0], 100.0);
        let narrow_pair = vertical_column_row_positions(&[42.0, 39.0], 100.0);

        assert!((wide_pair[1] - wide_pair[0]) > (narrow_pair[1] - narrow_pair[0]));
        assert_eq!(
            (wide_pair[0] + wide_pair[1]).round() as i32,
            (narrow_pair[0] + narrow_pair[1]).round() as i32
        );
    }

    #[test]
    fn vertical_upright_layout_detects_cjk_majority() {
        let pure_cjk = "测试一下".chars().collect::<Vec<_>>();
        let mixed_cjk = "测试hu\r\n一下".chars().collect::<Vec<_>>();
        let latin = "Test".chars().collect::<Vec<_>>();

        assert!(vertical_text_uses_upright_layout(&pure_cjk));
        assert!(vertical_text_uses_upright_layout(&mixed_cjk));
        assert!(!vertical_text_uses_upright_layout(&latin));
    }

    #[test]
    fn vertical_upright_items_group_short_ascii_runs() {
        let chars = "测试hu\r\n一下".chars().collect::<Vec<_>>();
        let columns = vertical_upright_item_columns(&chars, 16);

        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0][0].kind, VerticalTextItemKind::UprightChar);
        assert_eq!(columns[0][1].kind, VerticalTextItemKind::UprightChar);
        assert_eq!(columns[0][2].kind, VerticalTextItemKind::HorizontalRun);
        assert_eq!(&chars[columns[0][2].start..columns[0][2].end], &['h', 'u']);
        assert_eq!(columns[1].len(), 2);
        assert!(
            columns[1]
                .iter()
                .all(|item| item.kind == VerticalTextItemKind::UprightChar)
        );
    }

    #[test]
    fn cjk_vertical_horizontal_runs_use_focused_center_offset() {
        let (x, y) = cjk_vertical_horizontal_run_offset(50.0);

        assert_eq!(x.round() as i32, -1);
        assert_eq!(y.round() as i32, 1);
    }

    #[test]
    fn cjk_vertical_columns_use_wide_line_advance() {
        let font_size = 50.0;
        let column_step = vertical_upright_column_step(font_size);
        let item_step = font_size * CJK_VERTICAL_ITEM_ADVANCE_EM;

        assert!(column_step > item_step);
        assert_eq!(column_step.round() as i32, 61);
    }

    #[test]
    fn mixed_cjk_vertical_uses_latin_run_midpoint_offset() {
        assert!(
            (cjk_vertical_item_advance_em(false) - CJK_VERTICAL_PURE_ITEM_ADVANCE_EM).abs() < 0.001
        );
        assert!((cjk_vertical_item_advance_em(true) - CJK_VERTICAL_ITEM_ADVANCE_EM).abs() < 0.001);
        assert!(
            (cjk_vertical_right_column_x_em(false) - CJK_VERTICAL_PURE_RIGHT_COLUMN_X_EM).abs()
                < 0.001
        );
        assert!(
            (cjk_vertical_right_column_x_em(true) - CJK_VERTICAL_MIXED_RIGHT_COLUMN_X_EM).abs()
                < 0.001
        );
        assert!(
            (cjk_vertical_midpoint_y_em(false) - CJK_VERTICAL_PURE_MIDPOINT_Y_EM).abs() < 0.001
        );
        assert!(
            (cjk_vertical_midpoint_y_em(true)
                - (CJK_VERTICAL_MIDPOINT_Y_EM + CJK_VERTICAL_MIXED_MIDPOINT_OFFSET_EM))
                .abs()
                < 0.001
        );
    }

    #[test]
    fn underline_uses_logical_size_after_layout_fit() {
        let mut surface = surfaces::raster_n32_premul((40, 40)).unwrap();
        surface.canvas().clear(Color::TRANSPARENT);
        let fitted = TextCharStyle {
            font_name: None,
            fallback_font: None,
            font_size_px: 20.0,
            color: Rgba8 {
                r: 39,
                g: 39,
                b: 39,
                a: 255,
            },
            bold: false,
            italic: true,
            underline: true,
            strikethrough: false,
        };
        let logical = TextCharStyle {
            font_size_px: 10.0,
            ..fitted.clone()
        };

        draw_text_decorations(
            surface.canvas(),
            (0.0, 0.0),
            &[fitted],
            &[logical],
            &[(2.0, 20.0)],
            &[Some(TextFontMetrics {
                underline_thickness: Some(0.2),
                underline_position: None,
                strikethrough_thickness: None,
                strikethrough_position: None,
            })],
        );

        let image = surface.image_snapshot();
        let pixmap = image.peek_pixels().unwrap();
        let pixels = pixmap.bytes().unwrap();
        let dark_rows = (0..40)
            .filter_map(|y| {
                (0..40)
                    .any(|x| {
                        let index = (y * 40 + x) * 4;
                        pixels[index + 3] != 0
                    })
                    .then_some(y)
            })
            .collect::<Vec<_>>();
        assert!((2..=3).contains(&dark_rows.len()));
        assert!(
            dark_rows.iter().all(|row| *row <= 12),
            "decoration should use the logical size, not the fitted size"
        );
    }

    #[test]
    fn strikethrough_position_uses_fitted_size_after_layout_fit() {
        let mut surface = surfaces::raster_n32_premul((40, 40)).unwrap();
        surface.canvas().clear(Color::TRANSPARENT);
        let fitted = TextCharStyle {
            font_name: None,
            fallback_font: None,
            font_size_px: 20.0,
            color: Rgba8 {
                r: 39,
                g: 39,
                b: 39,
                a: 255,
            },
            bold: false,
            italic: true,
            underline: false,
            strikethrough: true,
        };
        let logical = TextCharStyle {
            font_size_px: 10.0,
            ..fitted.clone()
        };

        draw_text_decorations(
            surface.canvas(),
            (0.0, 0.0),
            &[fitted],
            &[logical],
            &[(2.0, 20.0)],
            &[Some(TextFontMetrics {
                underline_thickness: None,
                underline_position: None,
                strikethrough_thickness: None,
                strikethrough_position: None,
            })],
        );

        let image = surface.image_snapshot();
        let pixmap = image.peek_pixels().unwrap();
        let pixels = pixmap.bytes().unwrap();
        let dark_rows = (0..40)
            .filter_map(|y| {
                (0..40)
                    .any(|x| {
                        let index = (y * 40 + x) * 4;
                        pixels[index + 3] != 0
                    })
                    .then_some(y)
            })
            .collect::<Vec<_>>();
        assert!(!dark_rows.is_empty());
        assert!(
            dark_rows.iter().all(|row| *row >= 12),
            "strikethrough position should follow the fitted glyph body"
        );
    }

    #[test]
    fn decoration_thickness_quantization_differs_by_decoration_kind() {
        let underline =
            decoration_thickness(90.0, Some(0.05), 24.0, DecorationThicknessQuantize::Floor);
        let strikethrough =
            decoration_thickness(90.0, Some(0.05), 24.0, DecorationThicknessQuantize::Round);

        assert_eq!(underline as i32, 4);
        assert_eq!(strikethrough as i32, 5);
    }

    #[test]
    fn fitted_decoration_lines_inset_ends_by_half_stroke() {
        assert_eq!(decoration_line_end_inset(4.0, false), 0.0);
        assert_eq!(decoration_line_end_inset(4.0, true), 2.0);
        assert_eq!(decoration_line_end_inset(0.25, true), 0.5);
    }

    #[test]
    fn fitted_decoration_lines_use_hard_edges() {
        let color = Rgba8 {
            r: 39,
            g: 39,
            b: 39,
            a: 255,
        };

        let regular = decoration_line_paint(color, false);
        let fitted = decoration_line_paint(color, true);

        assert!(regular.is_anti_alias());
        assert!(!fitted.is_anti_alias());
    }

    #[test]
    fn synthetic_italic_skew_matches_focused_text_matrix() {
        assert!((SKIA_SYNTHETIC_ITALIC_SKEW + 0.17).abs() < f32::EPSILON);
    }

    #[test]
    fn fitted_synthetic_italic_uses_stronger_skew() {
        let fitted = TextCharStyle {
            font_name: None,
            fallback_font: None,
            font_size_px: 80.0,
            color: Rgba8 {
                r: 39,
                g: 39,
                b: 39,
                a: 255,
            },
            bold: false,
            italic: true,
            underline: false,
            strikethrough: false,
        };
        let logical = TextCharStyle {
            font_size_px: 75.0,
            ..fitted.clone()
        };

        assert!(
            (horizontal_synthetic_italic_skew(&logical, &logical) - SKIA_SYNTHETIC_ITALIC_SKEW)
                .abs()
                < f32::EPSILON
        );
        assert!(
            (horizontal_synthetic_italic_skew(&fitted, &logical)
                - SKIA_FITTED_SYNTHETIC_ITALIC_SKEW)
                .abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn high_strikethrough_metric_position_overrides_fallback_y() {
        let fallback_y = decoration_y(0.0, 100.0, 0.66, None, Some(0.1));
        let metric_y = decoration_y(0.0, 100.0, 0.66, Some(0.512), Some(0.102));

        assert_eq!(fallback_y.round() as i32, 66);
        assert_eq!(metric_y.round() as i32, 44);
    }

    #[test]
    fn underline_metric_position_overrides_fallback_y() {
        let fallback_y = decoration_y(0.0, 100.0, 0.90, None, Some(0.05));
        let metric_y = decoration_y(0.0, 100.0, 0.90, Some(-0.075), Some(0.05));

        assert_eq!(fallback_y.round() as i32, 90);
        assert_eq!(metric_y.round() as i32, 105);
    }
}
