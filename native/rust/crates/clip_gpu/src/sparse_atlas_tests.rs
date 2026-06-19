use clip_model::CanvasSize;

use crate::{
    GpuDeviceConfig, GpuRenderError, GpuRenderer, GpuSparseAtlasFormat, GpuSparseAtlasUpdateChunk,
};
use crate::{
    GpuRasterBlendMode, GpuSparseAtlasRasterEvent, GpuSparseAtlasTextureKey,
    GpuSparseAtlasTexturePool, GpuSparseAtlasTexturePoolUpdate, GpuSparseAtlasTileRef,
};

#[test]
fn sparse_rgba_atlas_updates_regions_in_place() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let atlas = renderer
        .create_sparse_atlas_texture(CanvasSize::new(2, 2), GpuSparseAtlasFormat::Rgba8)
        .expect("create sparse atlas");

    renderer
        .update_sparse_atlas_texture(
            &atlas,
            &[
                GpuSparseAtlasUpdateChunk {
                    atlas_x: 0,
                    atlas_y: 0,
                    size: CanvasSize::new(2, 2),
                    pixels: vec![1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255],
                },
                GpuSparseAtlasUpdateChunk {
                    atlas_x: 1,
                    atlas_y: 1,
                    size: CanvasSize::new(1, 1),
                    pixels: vec![100, 101, 102, 255],
                },
            ],
        )
        .expect("update sparse atlas");

    let pixels = renderer
        .read_sparse_rgba_atlas_texture(&atlas)
        .expect("read sparse atlas");

    assert_eq!(
        pixels,
        vec![1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 100, 101, 102, 255,]
    );
}

#[test]
fn sparse_atlas_rejects_short_chunk_payload() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let atlas = renderer
        .create_sparse_atlas_texture(CanvasSize::new(2, 2), GpuSparseAtlasFormat::Rgba8)
        .expect("create sparse atlas");

    let err = renderer
        .update_sparse_atlas_texture(
            &atlas,
            &[GpuSparseAtlasUpdateChunk {
                atlas_x: 0,
                atlas_y: 0,
                size: CanvasSize::new(1, 1),
                pixels: vec![1, 2, 3],
            }],
        )
        .expect_err("short chunk must fail");

    assert_eq!(
        err,
        GpuRenderError::InputBufferSizeMismatch {
            expected: 4,
            actual: 3
        }
    );
}

#[test]
fn sparse_atlas_rejects_out_of_bounds_chunk() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let atlas = renderer
        .create_sparse_atlas_texture(CanvasSize::new(2, 2), GpuSparseAtlasFormat::R8)
        .expect("create sparse atlas");

    let err = renderer
        .update_sparse_atlas_texture(
            &atlas,
            &[GpuSparseAtlasUpdateChunk {
                atlas_x: 1,
                atlas_y: 1,
                size: CanvasSize::new(2, 1),
                pixels: vec![1, 2],
            }],
        )
        .expect_err("out of bounds chunk must fail");

    assert_eq!(
        err,
        GpuRenderError::UploadRegionOutOfBounds {
            texture_size: CanvasSize::new(2, 2),
            origin_x: 1,
            origin_y: 1,
            upload_size: CanvasSize::new(2, 1),
        }
    );
}

#[test]
fn sparse_atlas_pool_reuses_existing_texture() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let mut pool = GpuSparseAtlasTexturePool::default();
    let key = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 3,
    };

    let first = renderer
        .update_sparse_atlas_texture_pool(
            &mut pool,
            &[GpuSparseAtlasTexturePoolUpdate {
                key,
                atlas_size: CanvasSize::new(1, 1),
                chunks: vec![GpuSparseAtlasUpdateChunk {
                    atlas_x: 0,
                    atlas_y: 0,
                    size: CanvasSize::new(1, 1),
                    pixels: vec![1, 2, 3, 255],
                }],
            }],
        )
        .expect("first pool update");
    let second = renderer
        .update_sparse_atlas_texture_pool(
            &mut pool,
            &[GpuSparseAtlasTexturePoolUpdate {
                key,
                atlas_size: CanvasSize::new(1, 1),
                chunks: vec![GpuSparseAtlasUpdateChunk {
                    atlas_x: 0,
                    atlas_y: 0,
                    size: CanvasSize::new(1, 1),
                    pixels: vec![9, 8, 7, 255],
                }],
            }],
        )
        .expect("second pool update");

    assert_eq!(first.created_atlases, 1);
    assert_eq!(second.created_atlases, 0);
    assert_eq!(pool.len(), 1);
    let pixels = renderer
        .read_sparse_rgba_atlas_texture(pool.texture(key).expect("pooled texture"))
        .expect("read pooled texture");
    assert_eq!(pixels, vec![9, 8, 7, 255]);
}

#[test]
fn sparse_atlas_pool_rejects_size_change_for_existing_key() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let mut pool = GpuSparseAtlasTexturePool::default();
    let key = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 4,
    };
    renderer
        .update_sparse_atlas_texture_pool(
            &mut pool,
            &[GpuSparseAtlasTexturePoolUpdate {
                key,
                atlas_size: CanvasSize::new(1, 1),
                chunks: vec![GpuSparseAtlasUpdateChunk {
                    atlas_x: 0,
                    atlas_y: 0,
                    size: CanvasSize::new(1, 1),
                    pixels: vec![1, 2, 3, 255],
                }],
            }],
        )
        .expect("first pool update");

    let err = renderer
        .update_sparse_atlas_texture_pool(
            &mut pool,
            &[GpuSparseAtlasTexturePoolUpdate {
                key,
                atlas_size: CanvasSize::new(2, 1),
                chunks: Vec::new(),
            }],
        )
        .expect_err("size change must fail");

    assert_eq!(
        err,
        GpuRenderError::SparseAtlasSizeMismatch {
            expected: CanvasSize::new(1, 1),
            actual: CanvasSize::new(2, 1),
        }
    );
}

#[test]
fn sparse_atlas_raster_executor_draws_from_resident_pool_texture() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let mut pool = GpuSparseAtlasTexturePool::default();
    let atlas_key = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 7,
    };
    renderer
        .update_sparse_atlas_texture_pool(
            &mut pool,
            &[GpuSparseAtlasTexturePoolUpdate {
                key: atlas_key,
                atlas_size: CanvasSize::new(4, 2),
                chunks: vec![
                    GpuSparseAtlasUpdateChunk {
                        atlas_x: 0,
                        atlas_y: 0,
                        size: CanvasSize::new(2, 2),
                        pixels: [255, 0, 0, 255].repeat(4),
                    },
                    GpuSparseAtlasUpdateChunk {
                        atlas_x: 2,
                        atlas_y: 0,
                        size: CanvasSize::new(2, 2),
                        pixels: [0, 0, 255, 255].repeat(4),
                    },
                ],
            }],
        )
        .expect("prepare sparse atlas pool");

    let output = renderer
        .draw_sparse_atlas_raster_events_to_rgba8(
            CanvasSize::new(5, 4),
            &pool,
            &[
                GpuSparseAtlasRasterEvent {
                    raster: GpuSparseAtlasTileRef {
                        key: atlas_key,
                        atlas_x: 0,
                        atlas_y: 0,
                        size: CanvasSize::new(2, 2),
                    },
                    source_offset_x: 1,
                    source_offset_y: 1,
                    opacity: 1.0,
                    blend_mode: GpuRasterBlendMode::Normal,
                    mask: None,
                },
                GpuSparseAtlasRasterEvent {
                    raster: GpuSparseAtlasTileRef {
                        key: atlas_key,
                        atlas_x: 2,
                        atlas_y: 0,
                        size: CanvasSize::new(2, 2),
                    },
                    source_offset_x: 2,
                    source_offset_y: 1,
                    opacity: 1.0,
                    blend_mode: GpuRasterBlendMode::Normal,
                    mask: None,
                },
            ],
        )
        .expect("draw sparse atlas events");

    let mut expected = [255, 255, 255, 0].repeat(20);
    for y in 1..=2 {
        for x in 1..=2 {
            let offset = ((y * 5 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[255, 0, 0, 255]);
        }
        for x in 2..=3 {
            let offset = ((y * 5 + x) * 4) as usize;
            expected[offset..offset + 4].copy_from_slice(&[0, 0, 255, 255]);
        }
    }
    assert_eq!(output.pixels, expected);
}

#[test]
fn sparse_atlas_raster_executor_rejects_missing_pool_texture() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let atlas_key = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 8,
    };
    let err = renderer
        .draw_sparse_atlas_raster_events_to_rgba8(
            CanvasSize::new(2, 2),
            &GpuSparseAtlasTexturePool::default(),
            &[GpuSparseAtlasRasterEvent {
                raster: GpuSparseAtlasTileRef {
                    key: atlas_key,
                    atlas_x: 0,
                    atlas_y: 0,
                    size: CanvasSize::new(1, 1),
                },
                source_offset_x: 0,
                source_offset_y: 0,
                opacity: 1.0,
                blend_mode: GpuRasterBlendMode::Normal,
                mask: None,
            }],
        )
        .expect_err("missing atlas must fail");

    assert_eq!(
        err,
        GpuRenderError::MissingSparseAtlasTexture { key: atlas_key }
    );
}

#[test]
fn sparse_atlas_raster_executor_rejects_mixed_raster_atlases() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let mut pool = GpuSparseAtlasTexturePool::default();
    let first = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 9,
    };
    let second = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 10,
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
    let event = |key| GpuSparseAtlasRasterEvent {
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
    };

    let err = renderer
        .draw_sparse_atlas_raster_events_to_rgba8(
            CanvasSize::new(2, 2),
            &pool,
            &[event(first), event(second)],
        )
        .expect_err("mixed atlas keys must fail");

    assert_eq!(err, GpuRenderError::SparseAtlasMixedTextureKeys);
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
