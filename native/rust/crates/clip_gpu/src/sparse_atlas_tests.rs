use clip_model::CanvasSize;

use crate::{
    GpuDeviceConfig, GpuRenderError, GpuRenderer, GpuSparseAtlasFormat, GpuSparseAtlasUpdateChunk,
};
use crate::{GpuSparseAtlasTextureKey, GpuSparseAtlasTexturePool, GpuSparseAtlasTexturePoolUpdate};

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
