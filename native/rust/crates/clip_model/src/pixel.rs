#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    pub const TRANSPARENT_WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 0,
    };
}
