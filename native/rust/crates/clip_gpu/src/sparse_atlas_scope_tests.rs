use clip_model::{CanvasSize, Rect};

use crate::{
    GpuDeviceConfig, GpuLutFilterMode, GpuRasterBlendMode, GpuRenderer, GpuSparseAtlasFormat,
    GpuSparseAtlasPointFilterEvent, GpuSparseAtlasRasterEvent, GpuSparseAtlasRasterEventBatch,
    GpuSparseAtlasTextureKey, GpuSparseAtlasTexturePool, GpuSparseAtlasTexturePoolUpdate,
    GpuSparseAtlasTileEvent, GpuSparseAtlasTileRef, GpuSparseAtlasUpdateChunk,
};

#[test]
fn sparse_atlas_batch_executor_draws_simple_container_scope() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let mut pool = GpuSparseAtlasTexturePool::default();
    let raster = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 31,
    };
    renderer
        .update_sparse_atlas_texture_pool(&mut pool, &[one_pixel_update(raster, [255, 0, 0, 255])])
        .expect("prepare sparse atlas pool");
    let batch = GpuSparseAtlasRasterEventBatch::simple_container_scope(
        vec![sparse_event(raster)],
        1.0,
        GpuRasterBlendMode::Normal,
        Rect::new(0, 0, 1, 1),
        None,
    );

    let output = renderer
        .draw_sparse_atlas_raster_event_batches_to_rgba8(CanvasSize::new(1, 1), &pool, &[batch])
        .expect("draw sparse atlas scope batch");

    assert_eq!(output.pixels, vec![255, 0, 0, 255]);
}

#[test]
fn sparse_atlas_batch_executor_applies_simple_container_scope_mask() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let mut pool = GpuSparseAtlasTexturePool::default();
    let raster = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 32,
    };
    let mask = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::R8,
        atlas_id: 33,
    };
    renderer
        .update_sparse_atlas_texture_pool(
            &mut pool,
            &[
                one_pixel_update(raster, [255, 0, 0, 255]),
                one_pixel_mask_update(mask, 0),
            ],
        )
        .expect("prepare sparse atlas pool");
    let batch = GpuSparseAtlasRasterEventBatch::simple_container_scope(
        vec![sparse_event(raster)],
        1.0,
        GpuRasterBlendMode::Normal,
        Rect::new(0, 0, 1, 1),
        Some(GpuSparseAtlasTileRef {
            key: mask,
            atlas_x: 0,
            atlas_y: 0,
            size: CanvasSize::new(1, 1),
        }),
    );

    let output = renderer
        .draw_sparse_atlas_raster_event_batches_to_rgba8(CanvasSize::new(1, 1), &pool, &[batch])
        .expect("draw sparse atlas masked scope batch");

    assert_eq!(output.pixels, vec![255, 255, 255, 0]);
}

#[test]
fn sparse_atlas_batch_executor_applies_simple_container_scope_point_filter() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let mut pool = GpuSparseAtlasTexturePool::default();
    let raster = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 34,
    };
    renderer
        .update_sparse_atlas_texture_pool(&mut pool, &[one_pixel_update(raster, [255, 0, 0, 255])])
        .expect("prepare sparse atlas pool");
    let batch = GpuSparseAtlasRasterEventBatch::simple_container_scope_tile_events(
        vec![
            GpuSparseAtlasTileEvent::Raster(sparse_event(raster)),
            GpuSparseAtlasTileEvent::PointFilter(GpuSparseAtlasPointFilterEvent {
                lut_rgba: invert_lut(),
                opacity: 1.0,
                filter_mode: GpuLutFilterMode::ToneCurveRgb,
                local_bounds: Rect::new(0, 0, 1, 1),
                mask: None,
            }),
        ],
        1.0,
        GpuRasterBlendMode::Normal,
        Rect::new(0, 0, 1, 1),
        None,
    );

    let output = renderer
        .draw_sparse_atlas_raster_event_batches_to_rgba8(CanvasSize::new(1, 1), &pool, &[batch])
        .expect("draw sparse atlas filtered scope batch");

    assert_eq!(output.pixels, vec![0, 255, 255, 255]);
}

fn one_pixel_update(
    key: GpuSparseAtlasTextureKey,
    rgba: [u8; 4],
) -> GpuSparseAtlasTexturePoolUpdate {
    GpuSparseAtlasTexturePoolUpdate {
        key,
        atlas_size: CanvasSize::new(1, 1),
        chunks: vec![GpuSparseAtlasUpdateChunk {
            atlas_x: 0,
            atlas_y: 0,
            size: CanvasSize::new(1, 1),
            pixels: rgba.to_vec(),
        }],
    }
}

fn invert_lut() -> Vec<u8> {
    let mut lut = Vec::with_capacity(256 * 4);
    for value in 0u8..=255 {
        lut.extend_from_slice(&[255 - value, 255 - value, 255 - value, 255]);
    }
    lut
}

fn one_pixel_mask_update(
    key: GpuSparseAtlasTextureKey,
    alpha: u8,
) -> GpuSparseAtlasTexturePoolUpdate {
    GpuSparseAtlasTexturePoolUpdate {
        key,
        atlas_size: CanvasSize::new(1, 1),
        chunks: vec![GpuSparseAtlasUpdateChunk {
            atlas_x: 0,
            atlas_y: 0,
            size: CanvasSize::new(1, 1),
            pixels: vec![alpha],
        }],
    }
}

fn sparse_event(key: GpuSparseAtlasTextureKey) -> GpuSparseAtlasRasterEvent {
    GpuSparseAtlasRasterEvent {
        raster: GpuSparseAtlasTileRef {
            key,
            atlas_x: 0,
            atlas_y: 0,
            size: CanvasSize::new(1, 1),
        },
        source_offset_x: 0,
        source_offset_y: 0,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        mask: None,
    }
}
