use clip_model::CanvasSize;

use crate::GpuRasterBlendMode;
use crate::blend::blend_kind;

pub const TILE_EVENT_ABI_VERSION: u32 = 2;
const EVENT_HEADER_WORDS: usize = 4;
const RASTER_PAYLOAD_WORDS: usize = 10;
const NO_MASK_ATLAS_COORD: u32 = u32::MAX;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TileEventKind {
    Raster = 1,
    SpecialBlendRaster = 8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TileEventHeader {
    pub(crate) kind: TileEventKind,
    pub(crate) flags: u32,
    pub(crate) payload_offset: u32,
    pub(crate) payload_len: u32,
}

impl TileEventHeader {
    fn words(self) -> [u32; EVENT_HEADER_WORDS] {
        [
            self.kind as u32,
            self.flags,
            self.payload_offset,
            self.payload_len,
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RasterTileEventPayload {
    pub(crate) atlas_origin: (u32, u32),
    pub(crate) source_size: CanvasSize,
    pub(crate) source_offset: (i32, i32),
    pub(crate) opacity: f32,
    pub(crate) blend_mode: GpuRasterBlendMode,
    pub(crate) mask_atlas_origin: Option<(u32, u32)>,
}

impl RasterTileEventPayload {
    fn words(self) -> [u32; RASTER_PAYLOAD_WORDS] {
        [
            self.atlas_origin.0,
            self.atlas_origin.1,
            self.source_size.width,
            self.source_size.height,
            i32_bits(self.source_offset.0),
            i32_bits(self.source_offset.1),
            self.opacity.to_bits(),
            blend_kind(self.blend_mode),
            self.mask_atlas_origin
                .map_or(NO_MASK_ATLAS_COORD, |mask| mask.0),
            self.mask_atlas_origin
                .map_or(NO_MASK_ATLAS_COORD, |mask| mask.1),
        ]
    }

    fn event_kind(self) -> TileEventKind {
        match self.blend_mode {
            GpuRasterBlendMode::AddGlow
            | GpuRasterBlendMode::ColorBurn
            | GpuRasterBlendMode::ColorDodge
            | GpuRasterBlendMode::GlowDodge => TileEventKind::SpecialBlendRaster,
            _ => TileEventKind::Raster,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TileEventProgram {
    abi_version: u32,
    headers: Vec<TileEventHeader>,
    raster_payloads: Vec<RasterTileEventPayload>,
}

impl TileEventProgram {
    pub(crate) fn from_raster_payloads(
        payloads: impl IntoIterator<Item = RasterTileEventPayload>,
    ) -> Self {
        let raster_payloads: Vec<_> = payloads.into_iter().collect();
        let headers = raster_payloads
            .iter()
            .enumerate()
            .map(|(index, payload)| TileEventHeader {
                kind: payload.event_kind(),
                flags: 0,
                payload_offset: u32::try_from(index).unwrap_or(u32::MAX),
                payload_len: 1,
            })
            .collect();
        Self {
            abi_version: TILE_EVENT_ABI_VERSION,
            headers,
            raster_payloads,
        }
    }

    pub(crate) fn header_words(&self) -> Vec<u32> {
        let mut words = Vec::with_capacity(self.headers.len() * EVENT_HEADER_WORDS);
        for header in &self.headers {
            words.extend_from_slice(&header.words());
        }
        words
    }

    pub(crate) fn raster_payload_words(&self) -> Vec<u32> {
        let mut words = Vec::with_capacity(self.raster_payloads.len() * RASTER_PAYLOAD_WORDS);
        for payload in &self.raster_payloads {
            words.extend_from_slice(&payload.words());
        }
        words
    }
}

fn i32_bits(value: i32) -> u32 {
    u32::from_ne_bytes(value.to_ne_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_typed_raster_events_with_shader_words() {
        let program = TileEventProgram::from_raster_payloads([RasterTileEventPayload {
            atlas_origin: (11, 12),
            source_size: CanvasSize::new(31, 32),
            source_offset: (-7, 8),
            opacity: 0.5,
            blend_mode: GpuRasterBlendMode::Multiply,
            mask_atlas_origin: Some((41, 42)),
        }]);

        assert_eq!(program.abi_version, TILE_EVENT_ABI_VERSION);
        assert_eq!(
            program.headers,
            [TileEventHeader {
                kind: TileEventKind::Raster,
                flags: 0,
                payload_offset: 0,
                payload_len: 1,
            }]
        );
        assert_eq!(
            program.header_words(),
            vec![TileEventKind::Raster as u32, 0, 0, 1]
        );
        assert_eq!(
            program.raster_payload_words(),
            vec![
                11,
                12,
                31,
                32,
                i32_bits(-7),
                i32_bits(8),
                0.5f32.to_bits(),
                2,
                41,
                42,
            ]
        );
    }

    #[test]
    fn marks_byte_domain_rasters_as_special_blend_events() {
        let program = TileEventProgram::from_raster_payloads([RasterTileEventPayload {
            atlas_origin: (1, 2),
            source_size: CanvasSize::new(3, 4),
            source_offset: (0, 0),
            opacity: 1.0,
            blend_mode: GpuRasterBlendMode::AddGlow,
            mask_atlas_origin: None,
        }]);

        assert_eq!(
            program.headers,
            [TileEventHeader {
                kind: TileEventKind::SpecialBlendRaster,
                flags: 0,
                payload_offset: 0,
                payload_len: 1,
            }]
        );
        assert_eq!(
            program.header_words(),
            vec![TileEventKind::SpecialBlendRaster as u32, 0, 0, 1]
        );
    }
}
