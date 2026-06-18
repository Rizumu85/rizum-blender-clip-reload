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
