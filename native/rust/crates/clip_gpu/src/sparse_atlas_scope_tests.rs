use clip_model::{CanvasSize, Rect};

use crate::{
    GpuDeviceConfig, GpuRasterBlendMode, GpuRenderer, GpuSparseAtlasFormat,
    GpuSparseAtlasRasterEvent, GpuSparseAtlasRasterEventBatch, GpuSparseAtlasTextureKey,
    GpuSparseAtlasTexturePool, GpuSparseAtlasTexturePoolUpdate, GpuSparseAtlasTileRef,
    GpuSparseAtlasUpdateChunk,
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
