use clip_model::{CanvasSize, Rect};

use crate::{
    GpuDeviceConfig, GpuRasterBlendMode, GpuRenderer, GpuSparseAtlasFormat,
    GpuSparseAtlasRasterEvent, GpuSparseAtlasRasterEventBatch, GpuSparseAtlasScopeEvent,
    GpuSparseAtlasScopeEventKind, GpuSparseAtlasTextureKey, GpuSparseAtlasTexturePool,
    GpuSparseAtlasTexturePoolUpdate, GpuSparseAtlasTileEvent, GpuSparseAtlasTileRef,
    GpuSparseAtlasUpdateChunk,
};

#[test]
fn sparse_atlas_batch_executor_draws_through_container_clipping_run() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let mut pool = GpuSparseAtlasTexturePool::default();
    let raster = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 42,
    };
    renderer
        .update_sparse_atlas_texture_pool(
            &mut pool,
            &[two_pixel_update(raster, [255, 0, 0, 255], [0, 0, 255, 255])],
        )
        .expect("prepare sparse atlas pool");
    let container_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let clip_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let batch = GpuSparseAtlasRasterEventBatch::simple_through_scope_tile_events(
        vec![
            GpuSparseAtlasTileEvent::BeginScope(container_scope),
            GpuSparseAtlasTileEvent::BeginClipBase(clip_scope),
            GpuSparseAtlasTileEvent::ClipBaseRaster(sparse_event_atlas_x(raster, 0)),
            GpuSparseAtlasTileEvent::ClippedRaster(sparse_event_atlas_x(raster, 1)),
            GpuSparseAtlasTileEvent::ResolveClipBase(clip_scope),
            GpuSparseAtlasTileEvent::EndScope(container_scope),
        ],
        1.0,
        Rect::new(0, 0, 1, 1),
        None,
    );
    let base = vec![0, 255, 0, 255];

    let output = renderer
        .draw_sparse_atlas_raster_event_batches_over_rgba8(
            CanvasSize::new(1, 1),
            &pool,
            &[batch],
            &base,
        )
        .expect("draw sparse atlas through container clipping batch");

    assert_eq!(output.pixels, vec![0, 0, 255, 255]);
}

fn two_pixel_update(
    key: GpuSparseAtlasTextureKey,
    left: [u8; 4],
    right: [u8; 4],
) -> GpuSparseAtlasTexturePoolUpdate {
    let mut pixels = Vec::with_capacity(8);
    pixels.extend_from_slice(&left);
    pixels.extend_from_slice(&right);
    GpuSparseAtlasTexturePoolUpdate {
        key,
        atlas_size: CanvasSize::new(2, 1),
        chunks: vec![GpuSparseAtlasUpdateChunk {
            atlas_x: 0,
            atlas_y: 0,
            size: CanvasSize::new(2, 1),
            pixels,
        }],
    }
}

fn sparse_event_atlas_x(key: GpuSparseAtlasTextureKey, atlas_x: u32) -> GpuSparseAtlasRasterEvent {
    GpuSparseAtlasRasterEvent {
        raster: GpuSparseAtlasTileRef {
            key,
            atlas_x,
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
