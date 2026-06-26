use std::collections::HashMap;

use skia_safe::{Color, Font, FontHinting, FontMgr, Paint, Typeface, font};

use super::TextCharStyle;

pub(super) const SKIA_SYNTHETIC_ITALIC_SKEW: f32 = -0.17;
pub(super) const SKIA_FITTED_SYNTHETIC_ITALIC_SKEW: f32 = -0.18;

pub(super) struct FontResolver {
    db: fontdb::Database,
    font_mgr: FontMgr,
    cache: HashMap<FontRequest, Option<ResolvedFont>>,
}

#[derive(Clone)]
pub(super) struct ResolvedFont {
    pub(super) typeface: Typeface,
    pub(super) metrics: TextFontMetrics,
    pub(super) synthetic_italic: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct TextFontMetrics {
    pub(super) underline_thickness: Option<f32>,
    pub(super) underline_position: Option<f32>,
    pub(super) strikethrough_thickness: Option<f32>,
    pub(super) strikethrough_position: Option<f32>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct FontRequest {
    name: Option<String>,
    fallback: Option<String>,
    bold: bool,
    italic: bool,
}

impl FontResolver {
    pub(super) fn new() -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        Self {
            db,
            font_mgr: FontMgr::new(),
            cache: HashMap::new(),
        }
    }

    pub(super) fn resolve(&mut self, style: &TextCharStyle) -> Option<ResolvedFont> {
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

pub(super) fn skia_font(resolved: &ResolvedFont, style: &TextCharStyle) -> Font {
    let mut font = Font::from_typeface(resolved.typeface.clone(), style.font_size_px);
    font.set_subpixel(true);
    font.set_edging(font::Edging::AntiAlias);
    font.set_hinting(FontHinting::None);
    if resolved.synthetic_italic {
        font.set_skew_x(SKIA_SYNTHETIC_ITALIC_SKEW);
    }
    font
}

pub(super) fn horizontal_glyph_font(
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

pub(super) fn horizontal_synthetic_italic_skew(
    style: &TextCharStyle,
    logical_style: &TextCharStyle,
) -> f32 {
    if text_style_is_quad_fitted(style, logical_style) {
        SKIA_FITTED_SYNTHETIC_ITALIC_SKEW
    } else {
        SKIA_SYNTHETIC_ITALIC_SKEW
    }
}

pub(super) fn text_style_is_quad_fitted(
    style: &TextCharStyle,
    logical_style: &TextCharStyle,
) -> bool {
    (style.font_size_px - logical_style.font_size_px).abs() > f32::EPSILON
}

pub(super) fn vertical_horizontal_run_font(resolved: &ResolvedFont, style: &TextCharStyle) -> Font {
    let mut font = skia_font(resolved, style);
    font.set_baseline_snap(false);
    font
}

pub(super) fn vertical_upright_font(
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

pub(super) fn text_paint(color: clip_model::Rgba8) -> Paint {
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_color(Color::from_argb(color.a, color.r, color.g, color.b));
    paint
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

pub(super) fn font_name_candidates(name: &str) -> Vec<String> {
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
