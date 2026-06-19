use clip_model::CanvasSize;

use crate::{
    GpuDeviceConfig, GpuRasterBlendMode, GpuRenderError, GpuRenderer, GpuSparseAtlasFormat,
    GpuSparseAtlasRasterEvent, GpuSparseAtlasTextureKey, GpuSparseAtlasTexturePool,
    GpuSparseAtlasTexturePoolUpdate, GpuSparseAtlasTileRef, GpuSparseAtlasUpdateChunk,
    split_sparse_atlas_raster_event_batches,
};

#[test]
fn sparse_atlas_batch_split_preserves_order_and_accepts_unmasked_events() {
    let first = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 1,
    };
    let second = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 2,
    };
    let mask = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::R8,
        atlas_id: 3,
    };
    let batches = split_sparse_atlas_raster_event_batches(&[
        sparse_event(first, None, 0, 0),
        sparse_event(first, Some(mask), 1, 0),
        sparse_event(first, None, 2, 0),
        sparse_event(second, None, 3, 0),
        sparse_event(first, Some(mask), 4, 0),
    ]);

    assert_eq!(batches.len(), 3);
    assert_eq!(batches[0].events.len(), 3);
    assert_eq!(batches[0].events[1].mask.expect("mask").key, mask);
    assert_eq!(batches[1].events[0].raster.key, second);
    assert_eq!(batches[2].events[0].raster.key, first);
}

#[test]
fn sparse_atlas_batch_split_separates_conflicting_mask_atlases() {
    let raster = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 1,
    };
    let first_mask = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::R8,
        atlas_id: 2,
    };
    let second_mask = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::R8,
        atlas_id: 3,
    };
    let batches = split_sparse_atlas_raster_event_batches(&[
        sparse_event(raster, Some(first_mask), 0, 0),
        sparse_event(raster, Some(second_mask), 1, 0),
    ]);

    assert_eq!(batches.len(), 2);
    assert_eq!(
        batches[0].events[0].mask.expect("first mask").key,
        first_mask
    );
    assert_eq!(
        batches[1].events[0].mask.expect("second mask").key,
        second_mask
    );
}

#[test]
fn sparse_atlas_batch_executor_draws_multiple_atlases_without_losing_prior_pixels() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let mut pool = GpuSparseAtlasTexturePool::default();
    let first = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 11,
    };
    let second = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 12,
    };
    renderer
        .update_sparse_atlas_texture_pool(
            &mut pool,
            &[
                one_pixel_update(first, [255, 0, 0, 255]),
                one_pixel_update(second, [0, 0, 255, 255]),
            ],
        )
        .expect("prepare sparse atlas pool");
    let batches = split_sparse_atlas_raster_event_batches(&[
        sparse_event(first, None, 0, 0),
        sparse_event(second, None, 2, 0),
    ]);

    let output = renderer
        .draw_sparse_atlas_raster_event_batches_to_rgba8(CanvasSize::new(3, 1), &pool, &batches)
        .expect("draw sparse atlas event batches");

    assert_eq!(
        output.pixels,
        vec![255, 0, 0, 255, 255, 255, 255, 0, 0, 0, 255, 255]
    );
}

#[test]
fn sparse_atlas_batch_executor_draws_over_base_accumulator() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let mut pool = GpuSparseAtlasTexturePool::default();
    let raster = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 13,
    };
    renderer
        .update_sparse_atlas_texture_pool(&mut pool, &[one_pixel_update(raster, [255, 0, 0, 255])])
        .expect("prepare sparse atlas pool");
    let batches = split_sparse_atlas_raster_event_batches(&[sparse_event(raster, None, 1, 0)]);
    let base = vec![0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255];

    let output = renderer
        .draw_sparse_atlas_raster_event_batches_over_rgba8(
            CanvasSize::new(3, 1),
            &pool,
            &batches,
            &base,
        )
        .expect("draw sparse atlas event batches over base");

    assert_eq!(
        output.pixels,
        vec![0, 255, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255]
    );
}

#[test]
fn sparse_atlas_batch_executor_returns_base_when_batches_are_empty() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let base = vec![1, 2, 3, 255, 4, 5, 6, 255];

    let output = renderer
        .draw_sparse_atlas_raster_event_batches_over_rgba8(
            CanvasSize::new(2, 1),
            &GpuSparseAtlasTexturePool::default(),
            &[],
            &base,
        )
        .expect("empty sparse atlas event batches keep base");

    assert_eq!(output.pixels, base);
}

#[test]
fn sparse_atlas_batch_executor_rejects_wrong_base_size() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let err = renderer
        .draw_sparse_atlas_raster_event_batches_over_rgba8(
            CanvasSize::new(2, 1),
            &GpuSparseAtlasTexturePool::default(),
            &[],
            &[1, 2, 3, 4],
        )
        .expect_err("short base must fail");

    assert_eq!(
        err,
        GpuRenderError::InputBufferSizeMismatch {
            expected: 8,
            actual: 4,
        }
    );
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

fn sparse_event(
    raster_key: GpuSparseAtlasTextureKey,
    mask_key: Option<GpuSparseAtlasTextureKey>,
    x: i32,
    y: i32,
) -> GpuSparseAtlasRasterEvent {
    GpuSparseAtlasRasterEvent {
        raster: GpuSparseAtlasTileRef {
            key: raster_key,
            atlas_x: 0,
            atlas_y: 0,
            size: CanvasSize::new(1, 1),
        },
        source_offset_x: x,
        source_offset_y: y,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        mask: mask_key.map(|key| GpuSparseAtlasTileRef {
            key,
            atlas_x: 0,
            atlas_y: 0,
            size: CanvasSize::new(1, 1),
        }),
    }
}
