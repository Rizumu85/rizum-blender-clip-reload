use super::super::{TextRasterLayout, text_lines};

pub(crate) const CJK_VERTICAL_ITEM_ADVANCE_EM: f32 = 0.99;
pub(crate) const CJK_VERTICAL_PURE_ITEM_ADVANCE_EM: f32 = 0.99;
pub(crate) const CJK_VERTICAL_MIXED_RIGHT_COLUMN_X_EM: f32 = 0.04;
pub(crate) const CJK_VERTICAL_PURE_RIGHT_COLUMN_X_EM: f32 = 0.10;
pub(crate) const CJK_VERTICAL_COLUMN_ADVANCE_EM: f32 = 1.22;
pub(crate) const CJK_VERTICAL_MIDPOINT_Y_EM: f32 = 0.93;
pub(crate) const CJK_VERTICAL_PURE_MIDPOINT_Y_EM: f32 = 1.00;
pub(crate) const CJK_VERTICAL_MIXED_MIDPOINT_OFFSET_EM: f32 = 0.01;
pub(crate) const CJK_VERTICAL_HORIZONTAL_RUN_X_OFFSET_EM: f32 = -0.02;
pub(crate) const CJK_VERTICAL_HORIZONTAL_RUN_Y_OFFSET_EM: f32 = 0.04;
pub(crate) const CJK_VERTICAL_HORIZONTAL_RUN_COLUMN_Y_OFFSET_EM: f32 = -0.10;
pub(crate) const CJK_VERTICAL_PURE_LAST_ROW_Y_OFFSET_EM: f32 = 0.06;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VerticalTextItem {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) kind: VerticalTextItemKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VerticalTextItemKind {
    UprightChar,
    HorizontalRun,
    RotatedChar,
}

pub(crate) fn vertical_upright_column_step(max_font_size: f32) -> f32 {
    (max_font_size * CJK_VERTICAL_COLUMN_ADVANCE_EM).max(1.0)
}

pub(crate) fn vertical_text_uses_upright_layout(chars: &[char]) -> bool {
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

pub(crate) fn vertical_char_starts_horizontal_run(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
}

pub(crate) fn vertical_upright_item_columns(
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

pub(crate) fn vertical_upright_items_have_horizontal_run(chars: &[char]) -> bool {
    chars
        .iter()
        .any(|ch| vertical_char_starts_horizontal_run(*ch))
}

pub(crate) fn cjk_vertical_item_advance_em(has_horizontal_run: bool) -> f32 {
    if has_horizontal_run {
        CJK_VERTICAL_ITEM_ADVANCE_EM
    } else {
        CJK_VERTICAL_PURE_ITEM_ADVANCE_EM
    }
}

pub(crate) fn cjk_vertical_right_column_x_em(has_horizontal_run: bool) -> f32 {
    if has_horizontal_run {
        CJK_VERTICAL_MIXED_RIGHT_COLUMN_X_EM
    } else {
        CJK_VERTICAL_PURE_RIGHT_COLUMN_X_EM
    }
}

pub(crate) fn cjk_vertical_midpoint_y_em(has_horizontal_run: bool) -> f32 {
    if has_horizontal_run {
        CJK_VERTICAL_MIDPOINT_Y_EM + CJK_VERTICAL_MIXED_MIDPOINT_OFFSET_EM
    } else {
        CJK_VERTICAL_PURE_MIDPOINT_Y_EM
    }
}

pub(crate) fn vertical_text_columns(chars: &[char], rows_per_column: usize) -> Vec<(usize, usize)> {
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

pub(crate) fn vertical_column_row_positions(advances: &[f32], midpoint_y: f32) -> Vec<f32> {
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

pub(crate) fn vertical_text_box(
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

pub(crate) fn cjk_vertical_horizontal_run_offset(max_font_size: f32) -> (f32, f32) {
    (
        max_font_size * CJK_VERTICAL_HORIZONTAL_RUN_X_OFFSET_EM,
        max_font_size * CJK_VERTICAL_HORIZONTAL_RUN_Y_OFFSET_EM,
    )
}

pub(crate) fn vertical_upright_column_y_offset(
    has_horizontal_run: bool,
    max_font_size: f32,
) -> f32 {
    if has_horizontal_run {
        max_font_size * CJK_VERTICAL_HORIZONTAL_RUN_COLUMN_Y_OFFSET_EM
    } else {
        0.0
    }
}

pub(crate) fn vertical_upright_row_y_offset(
    row: usize,
    row_count: usize,
    has_horizontal_run: bool,
    max_font_size: f32,
) -> f32 {
    if !has_horizontal_run && row + 1 == row_count && row_count > 1 {
        max_font_size * CJK_VERTICAL_PURE_LAST_ROW_Y_OFFSET_EM
    } else {
        0.0
    }
}
