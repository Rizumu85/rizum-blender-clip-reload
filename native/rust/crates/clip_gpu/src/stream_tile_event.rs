use clip_model::CanvasSize;

use crate::blend::blend_kind;
use crate::stream_bounds::CanvasRect;
use crate::{GpuLutFilterMode, GpuRasterBlendMode};

pub const TILE_EVENT_ABI_VERSION: u32 = 4;
const EVENT_HEADER_WORDS: usize = 4;
const RASTER_PAYLOAD_WORDS: usize = 10;
const POINT_FILTER_PAYLOAD_WORDS: usize = 10;
const SCOPE_PAYLOAD_WORDS: usize = 8;
const NO_MASK_ATLAS_COORD: u32 = u32::MAX;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TileEventKind {
    Raster = 1,
    BeginContainer = 5,
    EndContainer = 6,
    PointFilter = 7,
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PointFilterTileEventPayload {
    pub(crate) lut_row: u32,
    pub(crate) opacity: f32,
    pub(crate) filter_mode: GpuLutFilterMode,
    pub(crate) local_bounds: CanvasRect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScopeTileEventPayload {
    pub(crate) opacity: f32,
    pub(crate) blend_mode: GpuRasterBlendMode,
    pub(crate) local_bounds: CanvasRect,
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

impl PointFilterTileEventPayload {
    fn words(self) -> [u32; POINT_FILTER_PAYLOAD_WORDS] {
        let (mode, hue, saturation, luminosity) = match self.filter_mode {
            GpuLutFilterMode::ToneCurveRgb => (0, 0.0, 0.0, 0.0),
            GpuLutFilterMode::GradientMapLum => (1, 0.0, 0.0, 0.0),
            GpuLutFilterMode::ThresholdLum => (2, 0.0, 0.0, 0.0),
            GpuLutFilterMode::Hsl(params) => (
                3,
                params.hue_turns,
                params.saturation_delta,
                params.luminosity_delta,
            ),
        };
        [
            self.lut_row,
            self.opacity.to_bits(),
            mode,
            hue.to_bits(),
            saturation.to_bits(),
            luminosity.to_bits(),
            self.local_bounds.x,
            self.local_bounds.y,
            self.local_bounds.width,
            self.local_bounds.height,
        ]
    }
}

impl ScopeTileEventPayload {
    fn words(self) -> [u32; SCOPE_PAYLOAD_WORDS] {
        [
            self.opacity.to_bits(),
            blend_kind(self.blend_mode),
            self.local_bounds.x,
            self.local_bounds.y,
            self.local_bounds.width,
            self.local_bounds.height,
            0,
            0,
        ]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TileEventPayload {
    Raster(RasterTileEventPayload),
    BeginContainer(ScopeTileEventPayload),
    EndContainer(ScopeTileEventPayload),
    PointFilter(PointFilterTileEventPayload),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TileEventProgram {
    abi_version: u32,
    headers: Vec<TileEventHeader>,
    raster_payloads: Vec<RasterTileEventPayload>,
    filter_payloads: Vec<PointFilterTileEventPayload>,
    scope_payloads: Vec<ScopeTileEventPayload>,
}

impl TileEventProgram {
    pub(crate) fn from_raster_payloads(
        payloads: impl IntoIterator<Item = RasterTileEventPayload>,
    ) -> Self {
        Self::from_payloads(payloads.into_iter().map(TileEventPayload::Raster))
    }

    pub(crate) fn from_payloads(payloads: impl IntoIterator<Item = TileEventPayload>) -> Self {
        let mut headers = Vec::new();
        let mut raster_payloads = Vec::new();
        let mut filter_payloads = Vec::new();
        let mut scope_payloads = Vec::new();
        for payload in payloads {
            match payload {
                TileEventPayload::Raster(payload) => {
                    headers.push(TileEventHeader {
                        kind: payload.event_kind(),
                        flags: 0,
                        payload_offset: u32::try_from(raster_payloads.len()).unwrap_or(u32::MAX),
                        payload_len: 1,
                    });
                    raster_payloads.push(payload);
                }
                TileEventPayload::BeginContainer(payload) => {
                    headers.push(TileEventHeader {
                        kind: TileEventKind::BeginContainer,
                        flags: 0,
                        payload_offset: u32::try_from(scope_payloads.len()).unwrap_or(u32::MAX),
                        payload_len: 1,
                    });
                    scope_payloads.push(payload);
                }
                TileEventPayload::EndContainer(payload) => {
                    headers.push(TileEventHeader {
                        kind: TileEventKind::EndContainer,
                        flags: 0,
                        payload_offset: u32::try_from(scope_payloads.len()).unwrap_or(u32::MAX),
                        payload_len: 1,
                    });
                    scope_payloads.push(payload);
                }
                TileEventPayload::PointFilter(payload) => {
                    headers.push(TileEventHeader {
                        kind: TileEventKind::PointFilter,
                        flags: 0,
                        payload_offset: u32::try_from(filter_payloads.len()).unwrap_or(u32::MAX),
                        payload_len: 1,
                    });
                    filter_payloads.push(payload);
                }
            }
        }
        Self {
            abi_version: TILE_EVENT_ABI_VERSION,
            headers,
            raster_payloads,
            filter_payloads,
            scope_payloads,
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

    pub(crate) fn filter_payload_words(&self) -> Vec<u32> {
        let mut words = Vec::with_capacity(self.filter_payloads.len() * POINT_FILTER_PAYLOAD_WORDS);
        for payload in &self.filter_payloads {
            words.extend_from_slice(&payload.words());
        }
        words
    }

    pub(crate) fn scope_payload_words(&self) -> Vec<u32> {
        let mut words = Vec::with_capacity(self.scope_payloads.len() * SCOPE_PAYLOAD_WORDS);
        for payload in &self.scope_payloads {
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

    #[test]
    fn builds_typed_point_filter_events() {
        let program = TileEventProgram::from_payloads([TileEventPayload::PointFilter(
            PointFilterTileEventPayload {
                lut_row: 3,
                opacity: 0.25,
                filter_mode: GpuLutFilterMode::ThresholdLum,
                local_bounds: CanvasRect {
                    x: 1,
                    y: 2,
                    width: 31,
                    height: 32,
                },
            },
        )]);

        assert_eq!(
            program.header_words(),
            vec![TileEventKind::PointFilter as u32, 0, 0, 1]
        );
        assert_eq!(
            program.filter_payload_words(),
            vec![
                3,
                0.25f32.to_bits(),
                2,
                0.0f32.to_bits(),
                0.0f32.to_bits(),
                0.0f32.to_bits(),
                1,
                2,
                31,
                32,
            ]
        );
    }

    #[test]
    fn builds_typed_container_scope_events() {
        let scope = ScopeTileEventPayload {
            opacity: 1.0,
            blend_mode: GpuRasterBlendMode::Normal,
            local_bounds: CanvasRect {
                x: 4,
                y: 5,
                width: 6,
                height: 7,
            },
        };
        let program = TileEventProgram::from_payloads([
            TileEventPayload::BeginContainer(scope),
            TileEventPayload::EndContainer(scope),
        ]);

        assert_eq!(
            program.header_words(),
            vec![
                TileEventKind::BeginContainer as u32,
                0,
                0,
                1,
                TileEventKind::EndContainer as u32,
                0,
                1,
                1,
            ]
        );
        assert_eq!(
            program.scope_payload_words(),
            vec![
                1.0f32.to_bits(),
                0,
                4,
                5,
                6,
                7,
                0,
                0,
                1.0f32.to_bits(),
                0,
                4,
                5,
                6,
                7,
                0,
                0,
            ]
        );
    }
}
