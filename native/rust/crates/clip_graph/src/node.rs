use clip_model::{LayerId, LayerKind, LayerOpacity, LayerVisibility, Rgba8};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RenderNodeId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderNodeKind {
    Container,
    Paper,
    Raster,
    Filter,
    Text,
    Unsupported(u32),
}

impl RenderNodeKind {
    pub fn from_layer_kind(kind: LayerKind) -> Self {
        match kind {
            LayerKind::Folder | LayerKind::Group => Self::Container,
            LayerKind::Paper => Self::Paper,
            LayerKind::Raster | LayerKind::MaskedRaster => Self::Raster,
            LayerKind::Filter => Self::Filter,
            LayerKind::Text => Self::Text,
            LayerKind::Unsupported(raw) => Self::Unsupported(raw),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayerGraphInput {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderNode {
    pub id: RenderNodeId,
    pub layer_id: LayerId,
    pub layer_name: String,
    pub kind: RenderNodeKind,
    pub depth: u16,
    pub clip: bool,
    pub opacity: LayerOpacity,
    pub composite: u32,
    pub render_mipmap_id: Option<u32>,
    pub mask_mipmap_id: Option<u32>,
    pub paper_color: Option<Rgba8>,
}

impl RenderNode {
    pub fn from_layer_input(id: RenderNodeId, layer: LayerGraphInput, depth: u16) -> Self {
        Self {
            id,
            layer_id: layer.id,
            layer_name: layer.name,
            kind: RenderNodeKind::from_layer_kind(layer.kind),
            depth,
            clip: layer.clip,
            opacity: layer.opacity,
            composite: layer.composite,
            render_mipmap_id: layer.render_mipmap_id,
            mask_mipmap_id: layer.mask_mipmap_id,
            paper_color: layer.paper_color,
        }
    }
}
