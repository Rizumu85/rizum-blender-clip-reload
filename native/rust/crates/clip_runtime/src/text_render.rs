use std::collections::HashMap;

use ab_glyph::{Font, FontArc, FontVec, PxScale, ScaleFont, point};
use clip_model::{CanvasSize, Rect, Rgba8};

use crate::RuntimeError;

const SYNTHETIC_ITALIC_SUPERSAMPLE: f32 = 3.0;

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
    let mut pixels = vec![0; len];
    let mut fonts = FontResolver::new();
    for entry in &source.entries {
        render_entry_region(
            &mut pixels,
            region,
            source.resolution_dpi,
            layout,
            entry,
            &mut fonts,
        )?;
    }
    Ok(clip_file::tiles::RgbaTileImage {
        width: region.width,
        height: region.height,
        pixels,
    })
}

fn render_entry_region(
    pixels: &mut [u8],
    region: Rect,
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
    let lines = text_lines(&chars);
    fit_single_line_to_quad_width(entry, &chars, &lines, &mut styles, fonts);
    let default_font_size = styles
        .iter()
        .map(|style| style.font_size_px)
        .fold(1.0f32, f32::max);
    let line_height = (default_font_size * 1.2).max(1.0);
    let mut y = 0.0f32;
    for (start, end) in lines {
        let line_width = measure_line_width(&chars[start..end], &styles[start..end], fonts);
        let x = match entry.attributes.align {
            Some(1) => layout.size.width as f32 - line_width,
            Some(2) => (layout.size.width as f32 - line_width) * 0.5,
            _ => 0.0,
        }
        .max(0.0);
        draw_line(
            pixels,
            region,
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
    chars
        .iter()
        .zip(styles.iter())
        .map(|(ch, style)| {
            let Some(resolved) = fonts.resolve(style) else {
                return style.font_size_px * 0.55;
            };
            let scaled = resolved.font.as_scaled(PxScale::from(style.font_size_px));
            scaled.h_advance(resolved.font.glyph_id(*ch))
        })
        .sum()
}

fn draw_line(
    pixels: &mut [u8],
    region: Rect,
    origin: (f32, f32),
    chars: &[char],
    styles: &[TextCharStyle],
    decoration_styles: &[TextCharStyle],
    fonts: &mut FontResolver,
) -> Result<(), RuntimeError> {
    let mut x = origin.0;
    let mut char_positions = Vec::with_capacity(chars.len());
    let mut char_metrics = Vec::with_capacity(chars.len());
    for (ch, style) in chars.iter().zip(styles.iter()) {
        let Some(resolved) = fonts.resolve(style) else {
            let next_x = x + style.font_size_px * 0.55;
            char_positions.push((x, next_x));
            char_metrics.push(None);
            x = next_x;
            continue;
        };
        let font = &resolved.font;
        let scale = PxScale::from(style.font_size_px);
        let scaled = font.as_scaled(scale);
        let glyph_id = font.glyph_id(*ch);
        let baseline_y = origin.1 + scaled.ascent();
        if resolved.synthetic_italic {
            draw_synthetic_italic_glyph(
                pixels,
                region,
                font,
                glyph_id,
                scale,
                x,
                baseline_y,
                style.color,
            );
        } else {
            let glyph = glyph_id.with_scale_and_position(scale, point(x, baseline_y));
            if let Some(outlined) = font.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                outlined.draw(|local_x, local_y, coverage| {
                    let source_x = bounds.min.x.floor() as i32 + local_x as i32;
                    let source_y = bounds.min.y.floor() as i32 + local_y as i32;
                    blend_pixel_at_source(
                        pixels,
                        region,
                        source_x,
                        source_y,
                        style.color,
                        coverage,
                    );
                });
            }
        }
        let next_x = x + scaled.h_advance(glyph_id);
        char_positions.push((x, next_x));
        char_metrics.push(Some(resolved.metrics));
        x = next_x;
    }
    draw_text_decorations(
        pixels,
        region,
        origin,
        styles,
        decoration_styles,
        &char_positions,
        &char_metrics,
    );
    Ok(())
}

fn draw_synthetic_italic_glyph(
    pixels: &mut [u8],
    region: Rect,
    font: &FontArc,
    glyph_id: ab_glyph::GlyphId,
    scale: PxScale,
    x: f32,
    baseline_y: f32,
    color: Rgba8,
) {
    let high_scale = PxScale::from(scale.x * SYNTHETIC_ITALIC_SUPERSAMPLE);
    let high_glyph = glyph_id.with_scale_and_position(
        high_scale,
        point(
            x * SYNTHETIC_ITALIC_SUPERSAMPLE,
            baseline_y * SYNTHETIC_ITALIC_SUPERSAMPLE,
        ),
    );
    let Some(outlined) = font.outline_glyph(high_glyph) else {
        return;
    };
    let bounds = outlined.px_bounds();
    let mut coverage_by_pixel: HashMap<(i32, i32), f32> = HashMap::new();
    let sample_area = SYNTHETIC_ITALIC_SUPERSAMPLE * SYNTHETIC_ITALIC_SUPERSAMPLE;
    outlined.draw(|local_x, local_y, coverage| {
        let source_x = (bounds.min.x.floor() + local_x as f32) / SYNTHETIC_ITALIC_SUPERSAMPLE;
        let source_y = (bounds.min.y.floor() + local_y as f32) / SYNTHETIC_ITALIC_SUPERSAMPLE;
        let shifted_x = source_x + synthetic_italic_shift(baseline_y, source_y);
        let source_x = shifted_x.floor() as i32;
        let source_y = source_y.floor() as i32;
        if source_x < region.x as i32
            || source_y < region.y as i32
            || source_x >= region.x.saturating_add(region.width) as i32
            || source_y >= region.y.saturating_add(region.height) as i32
        {
            return;
        }
        *coverage_by_pixel.entry((source_x, source_y)).or_insert(0.0) += coverage / sample_area;
    });
    for ((source_x, source_y), coverage) in coverage_by_pixel {
        blend_pixel_at_source(pixels, region, source_x, source_y, color, coverage.min(1.0));
    }
}

fn synthetic_italic_shift(baseline_y: f32, source_y: f32) -> f32 {
    (baseline_y - source_y) * 0.23
}

fn draw_text_decorations(
    pixels: &mut [u8],
    region: Rect,
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
        if styles[start].underline {
            let y = decoration_y(origin.1, styles[start].font_size_px, 0.82, None, None);
            let thickness = decoration_thickness(
                decoration_styles[start].font_size_px,
                metrics.and_then(|metrics| metrics.underline_thickness),
                24.0,
            );
            draw_decoration_rect(pixels, region, x0, x1, y, thickness, styles[start].color);
        }
        if styles[start].strikethrough {
            let strikethrough_position = metrics.and_then(|metrics| metrics.strikethrough_position);
            // CSP follows unusually high strikeout metrics for display fonts, but
            // ordinary fonts match the legacy fallback better in the focused samples.
            let metric_position = strikethrough_position.filter(|position| *position > 0.45);
            let y = decoration_y(
                origin.1,
                styles[start].font_size_px,
                0.52,
                metric_position,
                metrics.and_then(|metrics| metrics.strikethrough_thickness),
            );
            let thickness = decoration_thickness(
                decoration_styles[start].font_size_px,
                metrics.and_then(|metrics| metrics.strikethrough_thickness),
                24.0,
            );
            draw_decoration_rect(pixels, region, x0, x1, y, thickness, styles[start].color);
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

fn decoration_thickness(font_size_px: f32, metric: Option<f32>, fallback_divisor: f32) -> i32 {
    metric
        .map(|ratio| ratio * font_size_px)
        .unwrap_or(font_size_px / fallback_divisor)
        .round()
        .clamp(1.0, 16.0) as i32
}

fn draw_decoration_rect(
    pixels: &mut [u8],
    region: Rect,
    x0: f32,
    x1: f32,
    y: f32,
    thickness: i32,
    color: Rgba8,
) {
    let left = x0.floor() as i32;
    let right = x1.ceil() as i32;
    let top = y.round() as i32;
    for source_y in top..top + thickness.max(1) {
        for source_x in left..right {
            blend_pixel_at_source(pixels, region, source_x, source_y, color, 1.0);
        }
    }
}

fn blend_pixel_at_source(
    pixels: &mut [u8],
    region: Rect,
    source_x: i32,
    source_y: i32,
    color: Rgba8,
    coverage: f32,
) {
    if coverage <= 0.0
        || source_x < region.x as i32
        || source_y < region.y as i32
        || source_x >= region.x.saturating_add(region.width) as i32
        || source_y >= region.y.saturating_add(region.height) as i32
    {
        return;
    }
    let target_x = (source_x - region.x as i32) as u32;
    let target_y = (source_y - region.y as i32) as u32;
    blend_pixel(
        pixels,
        region.width,
        target_x,
        target_y,
        color,
        coverage_alpha(coverage),
    );
}

fn coverage_alpha(coverage: f32) -> u8 {
    (coverage * 255.0).round().clamp(0.0, 255.0) as u8
}

fn blend_pixel(pixels: &mut [u8], width: u32, x: u32, y: u32, color: Rgba8, coverage_alpha: u8) {
    let Ok(index) = usize::try_from((u64::from(y) * u64::from(width) + u64::from(x)) * 4) else {
        return;
    };
    let Some(dst) = pixels.get_mut(index..index + 4) else {
        return;
    };
    let src_a = u32::from(coverage_alpha) * u32::from(color.a) / 255;
    let inv_a = 255 - src_a;
    dst[0] = ((u32::from(color.r) * src_a + u32::from(dst[0]) * inv_a + 127) / 255) as u8;
    dst[1] = ((u32::from(color.g) * src_a + u32::from(dst[1]) * inv_a + 127) / 255) as u8;
    dst[2] = ((u32::from(color.b) * src_a + u32::from(dst[2]) * inv_a + 127) / 255) as u8;
    dst[3] = (src_a + u32::from(dst[3]) * inv_a / 255).min(255) as u8;
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
    cache: HashMap<FontRequest, Option<ResolvedFont>>,
}

#[derive(Clone)]
struct ResolvedFont {
    font: FontArc,
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
            font = FontVec::try_from_vec_and_index(data.to_vec(), face_index)
                .ok()
                .map(FontArc::from);
        });
        font.map(|font| ResolvedFont {
            font,
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
    fn text_char_styles_expand_decoration_spans() {
        let entry = clip_file::metadata::TextLayerEntry {
            text: "Test".to_owned(),
            attributes: clip_file::metadata::TextLayerAttributes {
                default_font: Some("Arial".to_owned()),
                fallback_font: None,
                fonts: Vec::new(),
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
    fn decorations_use_logical_thickness_after_layout_fit() {
        let mut pixels = vec![0; 40 * 40 * 4];
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
            &mut pixels,
            Rect::new(0, 0, 40, 40),
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

        let dark_rows = (0..40)
            .filter(|&y| {
                (0..40).any(|x| {
                    let index = (y * 40 + x) * 4;
                    pixels[index + 3] != 0
                })
            })
            .count();
        assert_eq!(dark_rows, 2);
    }

    #[test]
    fn high_strikethrough_metric_position_overrides_fallback_y() {
        let fallback_y = decoration_y(0.0, 100.0, 0.52, None, Some(0.1));
        let metric_y = decoration_y(0.0, 100.0, 0.52, Some(0.512), Some(0.102));

        assert_eq!(fallback_y.round() as i32, 52);
        assert_eq!(metric_y.round() as i32, 44);
    }
}
