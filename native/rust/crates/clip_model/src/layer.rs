#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LayerId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayerKind {
    Raster,
    MaskedRaster,
    Folder,
    Group,
    Paper,
    Filter,
    Unsupported(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    HardLight,
    SoftLight,
    Add,
    AddGlow,
    Subtract,
    Difference,
    Lighten,
    Darken,
    ColorDodge,
    ColorBurn,
    Through,
    Unknown(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LayerOpacity(pub u16);

impl LayerOpacity {
    pub const MAX: Self = Self(256);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LayerVisibility(pub u32);

impl LayerVisibility {
    pub fn is_visible(self) -> bool {
        (self.0 & 1) != 0
    }
}
