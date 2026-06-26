use clip_model::{CanvasSize, Rect, Rgba8};
use skia_safe::{Canvas, Color, surfaces};

use crate::RuntimeError;

mod arc;
mod decoration;
mod font;
mod horizontal;
mod shaped;
mod vertical;

#[cfg(test)]
use arc::arc_outer_radius;
use arc::{render_arc_entry_surface, text_layout_is_arc};
#[cfg(test)]
use decoration::{
    DecorationThicknessQuantize, decoration_line_end_inset, decoration_line_paint,
    decoration_thickness, decoration_y, plan_text_decoration_commands,
};
use font::{FontResolver, skia_font, text_paint};
#[cfg(test)]
use font::{
    ResolvedFont, SKIA_FITTED_SYNTHETIC_ITALIC_SKEW, SKIA_SYNTHETIC_ITALIC_SKEW, TextFontMetrics,
    font_name_candidates, horizontal_synthetic_italic_skew, vertical_horizontal_run_font,
    vertical_upright_font,
};
#[cfg(test)]
use horizontal::{
    HorizontalTextRunPlan, glyph_run_end, horizontal_glyph_baseline_y,
    plan_horizontal_text_line_commands,
};
use horizontal::{build_horizontal_text_plan, draw_horizontal_text_plan};
#[cfg(test)]
use skia_safe::{FontHinting, FontMgr, font as skia_font_mod};
#[cfg(test)]
use vertical::{
    CJK_VERTICAL_ITEM_ADVANCE_EM, CJK_VERTICAL_MIDPOINT_Y_EM,
    CJK_VERTICAL_MIXED_MIDPOINT_OFFSET_EM, CJK_VERTICAL_MIXED_RIGHT_COLUMN_X_EM,
    CJK_VERTICAL_PURE_ITEM_ADVANCE_EM, CJK_VERTICAL_PURE_MIDPOINT_Y_EM,
    CJK_VERTICAL_PURE_RIGHT_COLUMN_X_EM, VerticalTextItemKind, cjk_vertical_horizontal_run_offset,
    cjk_vertical_item_advance_em, cjk_vertical_midpoint_y_em, cjk_vertical_right_column_x_em,
    vertical_column_row_positions, vertical_text_box, vertical_text_uses_upright_layout,
    vertical_upright_column_step, vertical_upright_column_y_offset, vertical_upright_item_columns,
    vertical_upright_row_y_offset,
};
use vertical::{render_vertical_entry_surface, text_layout_is_vertical};
fn shaped_text_probe_enabled() -> bool {
    std::env::var_os("RIZUM_CLIP_SHAPED_TEXT").is_some()
}

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
    let styles = text_char_styles(entry, resolution_dpi);
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
    let plan = build_horizontal_text_plan(entry, layout, chars, styles, logical_styles, fonts);
    draw_horizontal_text_plan(canvas, &plan, fonts)?;
    Ok(())
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
        assert_eq!(font.edging(), skia_font_mod::Edging::AntiAlias);
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
    fn horizontal_text_plan_owns_line_origins_and_run_ranges() {
        let entry = clip_file::metadata::TextLayerEntry {
            text: "A\nBC".to_owned(),
            attributes: clip_file::metadata::TextLayerAttributes {
                default_font: Some("Arial".to_owned()),
                fallback_font: None,
                fonts: Vec::new(),
                layout_flags: None,
                path_mode: None,
                path_angle_a_degrees: None,
                path_angle_b_degrees: None,
                path_center: None,
                font_size_100: Some(1000),
                color: None,
                bbox: None,
                quad_verts_100: None,
                box_size: None,
                align: None,
                underline_spans: Vec::new(),
                strikethrough_spans: Vec::new(),
                runs: vec![clip_file::metadata::TextLayerRun {
                    start: 3,
                    length: 1,
                    style_flags: 2,
                    field_defaults_flags: 0,
                    color: Rgba8 {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 255,
                    },
                    font_scale: 0,
                    font: None,
                }],
            },
        };
        let chars = entry.text.chars().collect::<Vec<_>>();
        let styles = text_char_styles(&entry, 72);
        let logical_styles = styles.clone();
        let mut fonts = FontResolver::new();

        let plan = build_horizontal_text_plan(
            &entry,
            &TextRasterLayout {
                size: CanvasSize::new(200, 200),
                offset_x: 0,
                offset_y: 0,
            },
            chars,
            styles,
            logical_styles,
            &mut fonts,
        );

        assert_eq!(plan.lines.len(), 2);
        assert_eq!(plan.lines[0].origin, (0.0, 0.0));
        assert_eq!(plan.lines[1].origin, (0.0, 12.0));
        assert_eq!(
            plan.lines[1].runs,
            vec![
                HorizontalTextRunPlan { start: 2, end: 3 },
                HorizontalTextRunPlan { start: 3, end: 4 },
            ]
        );
    }

    #[test]
    fn horizontal_text_line_plans_glyphs_and_decorations_before_drawing() {
        let entry = clip_file::metadata::TextLayerEntry {
            text: "AB".to_owned(),
            attributes: clip_file::metadata::TextLayerAttributes {
                default_font: Some("Arial".to_owned()),
                fallback_font: None,
                fonts: Vec::new(),
                layout_flags: None,
                path_mode: None,
                path_angle_a_degrees: None,
                path_angle_b_degrees: None,
                path_center: None,
                font_size_100: Some(1000),
                color: None,
                bbox: None,
                quad_verts_100: None,
                box_size: None,
                align: None,
                underline_spans: vec![clip_file::metadata::TextLayerSpan {
                    start: 0,
                    length: 2,
                }],
                strikethrough_spans: Vec::new(),
                runs: Vec::new(),
            },
        };
        let chars = entry.text.chars().collect::<Vec<_>>();
        let styles = text_char_styles(&entry, 72);
        let logical_styles = styles.clone();
        let mut fonts = FontResolver::new();
        let plan = build_horizontal_text_plan(
            &entry,
            &TextRasterLayout {
                size: CanvasSize::new(200, 200),
                offset_x: 0,
                offset_y: 0,
            },
            chars,
            styles,
            logical_styles,
            &mut fonts,
        );

        let commands =
            plan_horizontal_text_line_commands(&plan, &plan.lines[0], &mut fonts).unwrap();

        assert_eq!(commands.glyphs.len(), 1);
        assert_eq!(commands.decorations.len(), 1);
        assert_eq!(commands.glyphs[0].x, plan.lines[0].origin.0);
        assert!(commands.decorations[0].x1 > commands.decorations[0].x0);
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
        assert_eq!(y.round() as i32, 2);
    }

    #[test]
    fn mixed_cjk_vertical_paragraph_origin_shifts_up() {
        assert_eq!(vertical_upright_column_y_offset(false, 50.0), 0.0);
        assert_eq!(
            vertical_upright_column_y_offset(true, 50.0).round() as i32,
            -5
        );
    }

    #[test]
    fn pure_cjk_vertical_last_row_uses_bottom_alignment_offset() {
        assert_eq!(vertical_upright_row_y_offset(0, 4, false, 50.0), 0.0);
        assert_eq!(vertical_upright_row_y_offset(3, 4, true, 50.0), 0.0);
        assert_eq!(
            vertical_upright_row_y_offset(3, 4, false, 50.0).round() as i32,
            3
        );
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
        assert!(
            (cjk_vertical_item_advance_em(true) - CJK_VERTICAL_PURE_ITEM_ADVANCE_EM).abs() < 0.001
        );
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

        let commands = plan_text_decoration_commands(
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

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].x0, 2.0);
        assert_eq!(commands[0].x1, 20.0);
        assert_eq!(commands[0].y, 9.0);
        assert_eq!(commands[0].thickness, 2.0);
        assert!(commands[0].inset_ends);
    }

    #[test]
    fn strikethrough_position_uses_fitted_size_after_layout_fit() {
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

        let commands = plan_text_decoration_commands(
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

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].x0, 2.0);
        assert_eq!(commands[0].x1, 20.0);
        assert!((commands[0].y - 13.2).abs() < 0.001);
        assert_eq!(commands[0].thickness, 1.0);
        assert!(commands[0].inset_ends);
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
