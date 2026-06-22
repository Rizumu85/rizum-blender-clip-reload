use clip_model::{CanvasSize, LayerId, LayerKind, LayerOpacity, LayerVisibility, Rgba8};

pub(crate) const LAYER_TYPE_RASTER: u32 = 1;
pub(crate) const LAYER_TYPE_RASTER_MASKED: u32 = 3;
pub(crate) const LAYER_TYPE_LAYER_FOLDER: u32 = 0;
pub(crate) const LAYER_TYPE_GROUP: u32 = 2;
pub(crate) const LAYER_TYPE_FOLDER: u32 = 256;
pub(crate) const LAYER_TYPE_PAPER: u32 = 1584;
pub(crate) const LAYER_TYPE_FILTER: u32 = 4098;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayerRecord {
    pub id: LayerId,
    pub kind: LayerKind,
    pub visibility: LayerVisibility,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanvasRecord {
    pub id: u32,
    pub size: CanvasSize,
    pub root_layer_id: LayerId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RasterLayerSource {
    pub layer: LayerRecord,
    pub render_mipmap_id: u32,
    pub offscreen_id: u32,
    pub external_id: String,
    pub pixel_size: CanvasSize,
    pub color_type: Option<u32>,
    pub offset_x: i32,
    pub offset_y: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaskLayerSource {
    pub layer_id: LayerId,
    pub mask_mipmap_id: u32,
    pub offscreen_id: u32,
    pub external_id: String,
    pub pixel_size: CanvasSize,
    pub empty_fill: u8,
    pub offset_x: i32,
    pub offset_y: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterLayerSource {
    pub layer_id: LayerId,
    pub filter_type: u32,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextLayerSource {
    pub layer: LayerRecord,
    pub entries: Vec<TextLayerEntry>,
    pub resolution_dpi: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextLayerEntry {
    pub text: String,
    pub attributes: TextLayerAttributes,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextLayerAttributes {
    pub default_font: Option<String>,
    pub fallback_font: Option<String>,
    pub fonts: Vec<TextLayerFontMapping>,
    pub font_size_100: Option<i32>,
    pub color: Option<Rgba8>,
    pub bbox: Option<TextLayerRect>,
    pub quad_verts_100: Option<[i32; 8]>,
    pub box_size: Option<(i32, i32)>,
    pub align: Option<u8>,
    pub underline_spans: Vec<TextLayerSpan>,
    pub strikethrough_spans: Vec<TextLayerSpan>,
    pub runs: Vec<TextLayerRun>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextLayerFontMapping {
    pub display_name: String,
    pub font_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextLayerRun {
    pub start: i32,
    pub length: u32,
    pub style_flags: u8,
    pub field_defaults_flags: u8,
    pub color: Rgba8,
    pub font_scale: i32,
    pub font: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextLayerSpan {
    pub start: i32,
    pub length: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextLayerRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayerGraphRecord {
    pub id: LayerId,
    pub name: String,
    pub kind: LayerKind,
    pub visibility: LayerVisibility,
    pub clip: bool,
    pub opacity: LayerOpacity,
    pub composite: u32,
    pub next_layer_id: Option<LayerId>,
    pub first_child_layer_id: Option<LayerId>,
    pub render_mipmap_id: Option<u32>,
    pub mask_mipmap_id: Option<u32>,
    pub paper_color: Option<Rgba8>,
}

pub(crate) fn layer_kind(layer_type: u32) -> LayerKind {
    match layer_type {
        LAYER_TYPE_RASTER => LayerKind::Raster,
        LAYER_TYPE_RASTER_MASKED => LayerKind::MaskedRaster,
        LAYER_TYPE_LAYER_FOLDER => LayerKind::Folder,
        LAYER_TYPE_GROUP => LayerKind::Group,
        LAYER_TYPE_FOLDER => LayerKind::Folder,
        LAYER_TYPE_PAPER => LayerKind::Paper,
        LAYER_TYPE_FILTER => LayerKind::Filter,
        other => LayerKind::Unsupported(other),
    }
}
