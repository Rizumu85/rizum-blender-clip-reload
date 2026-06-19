use clip_model::CanvasSize;

use crate::stream_bounds::CanvasRect;
use crate::stream_tile_event::{
    PointFilterTileEventPayload, RasterTileEventPayload, ScopeTileEventPayload,
    SolidColorTileEventPayload, TILE_EVENT_ABI_VERSION, TileEventHeader, TileEventKind,
    TileEventPayload, TileEventProgram,
};
use crate::{GpuLutFilterMode, GpuRasterBlendMode};

fn i32_bits(value: i32) -> u32 {
    u32::from_ne_bytes(value.to_ne_bytes())
}

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
            mask_atlas_origin: Some((41, 42)),
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
            41,
            42,
        ]
    );
}

#[test]
fn builds_typed_solid_color_events() {
    let program = TileEventProgram::from_payloads([TileEventPayload::SolidColor(
        SolidColorTileEventPayload {
            color: clip_model::Rgba8 {
                r: 10,
                g: 20,
                b: 30,
                a: 40,
            },
            opacity: 0.75,
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
        vec![TileEventKind::SolidColor as u32, 0, 0, 1]
    );
    assert_eq!(
        program.filter_payload_words(),
        vec![10, 20, 30, 40, 0.75f32.to_bits(), 0, 1, 2, 31, 32, 0, 0,]
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
        mask_atlas_origin: Some((8, 9)),
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
            8,
            9,
            1.0f32.to_bits(),
            0,
            4,
            5,
            6,
            7,
            8,
            9,
        ]
    );
}

#[test]
fn builds_typed_clipped_container_scope_events() {
    let scope = ScopeTileEventPayload {
        opacity: 0.75,
        blend_mode: GpuRasterBlendMode::Multiply,
        local_bounds: CanvasRect {
            x: 4,
            y: 5,
            width: 6,
            height: 7,
        },
        mask_atlas_origin: None,
    };
    let program = TileEventProgram::from_payloads([
        TileEventPayload::BeginClippedContainer(scope),
        TileEventPayload::EndClippedContainer(scope),
    ]);

    assert_eq!(
        program.header_words(),
        vec![
            TileEventKind::BeginContainer as u32,
            2,
            0,
            1,
            TileEventKind::EndContainer as u32,
            2,
            1,
            1,
        ]
    );
    assert_eq!(program.scope_payload_words().len(), 16);
}

#[test]
fn builds_typed_through_scope_events() {
    let scope = ScopeTileEventPayload {
        opacity: 0.5,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: CanvasRect {
            x: 4,
            y: 5,
            width: 6,
            height: 7,
        },
        mask_atlas_origin: None,
    };
    let program = TileEventProgram::from_payloads([
        TileEventPayload::BeginThrough(scope),
        TileEventPayload::EndThrough(scope),
    ]);

    assert_eq!(
        program.header_words(),
        vec![
            TileEventKind::BeginThrough as u32,
            0,
            0,
            1,
            TileEventKind::EndThrough as u32,
            0,
            1,
            1,
        ]
    );
    assert_eq!(program.scope_payload_words().len(), 16);
}

#[test]
fn builds_typed_clipping_scope_events() {
    let raster = RasterTileEventPayload {
        atlas_origin: (1, 2),
        source_size: CanvasSize::new(3, 4),
        source_offset: (0, 0),
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Multiply,
        mask_atlas_origin: None,
    };
    let scope = ScopeTileEventPayload {
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Multiply,
        local_bounds: CanvasRect {
            x: 0,
            y: 0,
            width: 3,
            height: 4,
        },
        mask_atlas_origin: None,
    };
    let program = TileEventProgram::from_payloads([
        TileEventPayload::BeginClipBase(scope),
        TileEventPayload::ClipBaseRaster(raster),
        TileEventPayload::ClippedRaster(raster),
        TileEventPayload::ResolveClipBase(scope),
    ]);

    assert_eq!(
        program.header_words(),
        vec![
            TileEventKind::BeginClipBase as u32,
            0,
            0,
            1,
            TileEventKind::Raster as u32,
            1,
            0,
            1,
            TileEventKind::ClippedRaster as u32,
            0,
            1,
            1,
            TileEventKind::ResolveClipBase as u32,
            0,
            1,
            1,
        ]
    );
    assert_eq!(program.raster_payload_words().len(), 20);
    assert_eq!(program.scope_payload_words().len(), 16);
}
