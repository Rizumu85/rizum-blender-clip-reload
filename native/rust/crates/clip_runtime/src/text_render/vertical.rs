use skia_safe::{Canvas, Point};

use crate::RuntimeError;

use super::{
    TextCharStyle, TextRasterLayout, char_advances,
    font::{
        FontResolver, ResolvedFont, skia_font, text_paint, vertical_horizontal_run_font,
        vertical_upright_font,
    },
    horizontal::glyph_run_end,
    shaped, shaped_text_probe_enabled,
};

mod layout;

pub(super) use layout::{
    CJK_VERTICAL_ITEM_ADVANCE_EM, VerticalTextItem, VerticalTextItemKind,
    cjk_vertical_horizontal_run_offset, cjk_vertical_item_advance_em, cjk_vertical_midpoint_y_em,
    cjk_vertical_right_column_x_em, vertical_column_row_positions, vertical_text_box,
    vertical_text_uses_upright_layout, vertical_upright_column_step,
    vertical_upright_column_y_offset, vertical_upright_item_columns, vertical_upright_row_y_offset,
};
#[cfg(test)]
pub(super) use layout::{
    CJK_VERTICAL_MIDPOINT_Y_EM, CJK_VERTICAL_MIXED_MIDPOINT_OFFSET_EM,
    CJK_VERTICAL_MIXED_RIGHT_COLUMN_X_EM, CJK_VERTICAL_PURE_ITEM_ADVANCE_EM,
    CJK_VERTICAL_PURE_MIDPOINT_Y_EM, CJK_VERTICAL_PURE_RIGHT_COLUMN_X_EM,
};
use layout::{vertical_text_columns, vertical_upright_items_have_horizontal_run};

pub(super) fn text_layout_is_vertical(entry: &clip_file::metadata::TextLayerEntry) -> bool {
    entry
        .attributes
        .layout_flags
        .map(|flags| flags & 0x10 != 0)
        .unwrap_or(false)
}

pub(super) fn render_vertical_entry_surface(
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
    let profile_text_layout = std::env::var_os("RIZUM_CLIP_TEXT_PROFILE").is_some();
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
        let column_midpoint_y =
            midpoint_y + vertical_upright_column_y_offset(has_horizontal_run, max_font_size);
        let row_positions = vertical_column_row_positions(&advances, column_midpoint_y);
        let center_x = right_column_x - column as f32 * column_step;
        if profile_text_layout {
            eprintln!(
                "text vertical column -> column={} center_x={:.3} midpoint_y={:.3} rows={} has_horizontal_run={}",
                column,
                center_x,
                column_midpoint_y,
                items.len(),
                has_horizontal_run
            );
        }
        for (row, item) in items.iter().enumerate() {
            let center_y = row_positions.get(row).copied().unwrap_or(midpoint_y)
                + vertical_upright_row_y_offset(
                    row,
                    items.len(),
                    has_horizontal_run,
                    max_font_size,
                );
            if profile_text_layout {
                let item_text = chars[item.start..item.end].iter().collect::<String>();
                eprintln!(
                    "text vertical item -> column={} row={} kind={:?} range={}..{} text={:?} advance={:.3} center=({:.3},{:.3})",
                    column,
                    row,
                    item.kind,
                    item.start,
                    item.end,
                    item_text,
                    advances.get(row).copied().unwrap_or(0.0),
                    center_x,
                    center_y
                );
            }
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
        let shaped = shaped_text_probe_enabled()
            .then(|| shaped::shape_text_run(&text, &font))
            .flatten();
        if let Some(shaped) = shaped {
            canvas.draw_text_blob(&shaped.blob, Point::new(x, baseline_y), &paint);
            x += shaped.advance_x;
        } else {
            canvas.draw_str(&text, Point::new(x, baseline_y), &font, &paint);
            x += font.measure_str(&text, Some(&paint)).0;
        }
        start = end;
    }
    Ok(())
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
