use clip_model::{CanvasSize, Rect};

use crate::{
    GpuDeviceConfig, GpuLutFilterMode, GpuRasterBlendMode, GpuRenderer, GpuSparseAtlasFormat,
    GpuSparseAtlasPointFilterEvent, GpuSparseAtlasRasterEvent, GpuSparseAtlasRasterEventBatch,
    GpuSparseAtlasScopeEvent, GpuSparseAtlasScopeEventKind, GpuSparseAtlasTextureKey,
    GpuSparseAtlasTexturePool, GpuSparseAtlasTexturePoolUpdate, GpuSparseAtlasTileEvent,
    GpuSparseAtlasTileRef, GpuSparseAtlasUpdateChunk,
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
fn sparse_atlas_batch_executor_draws_split_scope_mask_batches() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let mut pool = GpuSparseAtlasTexturePool::default();
    let raster = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 38,
    };
    let mask = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::R8,
        atlas_id: 39,
    };
    renderer
        .update_sparse_atlas_texture_pool(
            &mut pool,
            &[
                two_pixel_update(raster, [255, 0, 0, 255], [255, 0, 0, 255]),
                two_pixel_mask_update(mask, 0, 255),
            ],
        )
        .expect("prepare sparse atlas pool");
    let batches = vec![
        GpuSparseAtlasRasterEventBatch::simple_container_scope(
            vec![sparse_event_atlas_x_and_offset(raster, 0, 0)],
            1.0,
            GpuRasterBlendMode::Normal,
            Rect::new(0, 0, 1, 1),
            Some(GpuSparseAtlasTileRef {
                key: mask,
                atlas_x: 0,
                atlas_y: 0,
                size: CanvasSize::new(1, 1),
            }),
        ),
        GpuSparseAtlasRasterEventBatch::simple_container_scope(
            vec![sparse_event_atlas_x_and_offset(raster, 1, 1)],
            1.0,
            GpuRasterBlendMode::Normal,
            Rect::new(1, 0, 1, 1),
            Some(GpuSparseAtlasTileRef {
                key: mask,
                atlas_x: 1,
                atlas_y: 0,
                size: CanvasSize::new(1, 1),
            }),
        ),
    ];

    let output = renderer
        .draw_sparse_atlas_raster_event_batches_to_rgba8(CanvasSize::new(2, 1), &pool, &batches)
        .expect("draw sparse atlas split scope mask batches");

    assert_eq!(output.pixels, vec![255, 255, 255, 0, 255, 0, 0, 255]);
}

#[test]
fn sparse_atlas_batch_executor_draws_nested_split_scope_masks() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let mut pool = GpuSparseAtlasTexturePool::default();
    let raster = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 40,
    };
    let mask = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::R8,
        atlas_id: 41,
    };
    renderer
        .update_sparse_atlas_texture_pool(
            &mut pool,
            &[
                two_pixel_update(raster, [255, 0, 0, 255], [255, 0, 0, 255]),
                two_pixel_mask_update(mask, 0, 255),
            ],
        )
        .expect("prepare sparse atlas pool");
    let left_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: Some(GpuSparseAtlasTileRef {
            key: mask,
            atlas_x: 0,
            atlas_y: 0,
            size: CanvasSize::new(1, 1),
        }),
    };
    let right_scope = GpuSparseAtlasScopeEvent {
        local_bounds: Rect::new(1, 0, 1, 1),
        mask: Some(GpuSparseAtlasTileRef {
            key: mask,
            atlas_x: 1,
            atlas_y: 0,
            size: CanvasSize::new(1, 1),
        }),
        ..left_scope
    };
    let batch = GpuSparseAtlasRasterEventBatch::simple_container_scope_tile_events(
        vec![
            GpuSparseAtlasTileEvent::BeginScope(left_scope),
            GpuSparseAtlasTileEvent::Raster(sparse_event_atlas_x_and_offset(raster, 0, 0)),
            GpuSparseAtlasTileEvent::EndScope(left_scope),
            GpuSparseAtlasTileEvent::BeginScope(right_scope),
            GpuSparseAtlasTileEvent::Raster(sparse_event_atlas_x_and_offset(raster, 1, 1)),
            GpuSparseAtlasTileEvent::EndScope(right_scope),
        ],
        1.0,
        GpuRasterBlendMode::Normal,
        Rect::new(0, 0, 2, 1),
        None,
    );

    let output = renderer
        .draw_sparse_atlas_raster_event_batches_to_rgba8(CanvasSize::new(2, 1), &pool, &[batch])
        .expect("draw sparse atlas nested split scope mask batch");

    assert_eq!(output.pixels, vec![255, 255, 255, 0, 255, 0, 0, 255]);
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

#[test]
fn sparse_atlas_batch_executor_draws_nested_simple_container_scope() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let mut pool = GpuSparseAtlasTexturePool::default();
    let raster = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 36,
    };
    renderer
        .update_sparse_atlas_texture_pool(&mut pool, &[one_pixel_update(raster, [255, 0, 0, 255])])
        .expect("prepare sparse atlas pool");
    let inner_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let batch = GpuSparseAtlasRasterEventBatch::simple_container_scope_tile_events(
        vec![
            GpuSparseAtlasTileEvent::BeginScope(inner_scope),
            GpuSparseAtlasTileEvent::Raster(sparse_event(raster)),
            GpuSparseAtlasTileEvent::EndScope(inner_scope),
        ],
        1.0,
        GpuRasterBlendMode::Normal,
        Rect::new(0, 0, 1, 1),
        None,
    );

    let output = renderer
        .draw_sparse_atlas_raster_event_batches_to_rgba8(CanvasSize::new(1, 1), &pool, &[batch])
        .expect("draw sparse atlas nested scope batch");

    assert_eq!(output.pixels, vec![255, 0, 0, 255]);
}

#[test]
fn sparse_atlas_batch_executor_draws_nested_scope_clipping_run() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let mut pool = GpuSparseAtlasTexturePool::default();
    let raster = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 37,
    };
    renderer
        .update_sparse_atlas_texture_pool(
            &mut pool,
            &[two_pixel_update(raster, [255, 0, 0, 0], [0, 0, 255, 255])],
        )
        .expect("prepare sparse atlas pool");
    let inner_scope = GpuSparseAtlasScopeEvent {
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
    let batch = GpuSparseAtlasRasterEventBatch::simple_container_scope_tile_events(
        vec![
            GpuSparseAtlasTileEvent::BeginScope(inner_scope),
            GpuSparseAtlasTileEvent::BeginClipBase(clip_scope),
            GpuSparseAtlasTileEvent::ClipBaseRaster(sparse_event_atlas_x(raster, 0)),
            GpuSparseAtlasTileEvent::ClippedRaster(sparse_event_atlas_x(raster, 1)),
            GpuSparseAtlasTileEvent::ResolveClipBase(clip_scope),
            GpuSparseAtlasTileEvent::EndScope(inner_scope),
        ],
        1.0,
        GpuRasterBlendMode::Normal,
        Rect::new(0, 0, 1, 1),
        None,
    );

    let output = renderer
        .draw_sparse_atlas_raster_event_batches_to_rgba8(CanvasSize::new(1, 1), &pool, &[batch])
        .expect("draw sparse atlas nested clipping scope batch");

    assert_eq!(output.pixels, vec![255, 255, 255, 0]);
}

#[test]
fn sparse_atlas_batch_executor_draws_simple_container_scope_clipping_run() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let mut pool = GpuSparseAtlasTexturePool::default();
    let raster = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 35,
    };
    renderer
        .update_sparse_atlas_texture_pool(
            &mut pool,
            &[two_pixel_update(raster, [255, 0, 0, 0], [0, 0, 255, 255])],
        )
        .expect("prepare sparse atlas pool");
    let clip_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let batch = GpuSparseAtlasRasterEventBatch::simple_container_scope_tile_events(
        vec![
            GpuSparseAtlasTileEvent::BeginClipBase(clip_scope),
            GpuSparseAtlasTileEvent::ClipBaseRaster(sparse_event_atlas_x(raster, 0)),
            GpuSparseAtlasTileEvent::ClippedRaster(sparse_event_atlas_x(raster, 1)),
            GpuSparseAtlasTileEvent::ResolveClipBase(clip_scope),
        ],
        1.0,
        GpuRasterBlendMode::Normal,
        Rect::new(0, 0, 1, 1),
        None,
    );

    let output = renderer
        .draw_sparse_atlas_raster_event_batches_to_rgba8(CanvasSize::new(1, 1), &pool, &[batch])
        .expect("draw sparse atlas scope clipping-run batch");

    assert_eq!(output.pixels, vec![255, 255, 255, 0]);
}

#[test]
fn sparse_atlas_batch_executor_draws_clipped_container_sibling() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let mut pool = GpuSparseAtlasTexturePool::default();
    let raster = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 40,
    };
    renderer
        .update_sparse_atlas_texture_pool(
            &mut pool,
            &[two_pixel_update(raster, [255, 0, 0, 255], [0, 0, 255, 255])],
        )
        .expect("prepare sparse atlas pool");
    let clip_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let clipped_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let batch = GpuSparseAtlasRasterEventBatch::simple_container_scope_tile_events(
        vec![
            GpuSparseAtlasTileEvent::BeginClipBase(clip_scope),
            GpuSparseAtlasTileEvent::ClipBaseRaster(sparse_event_atlas_x(raster, 0)),
            GpuSparseAtlasTileEvent::BeginClippedScope(clipped_scope),
            GpuSparseAtlasTileEvent::Raster(sparse_event_atlas_x(raster, 1)),
            GpuSparseAtlasTileEvent::EndClippedScope(clipped_scope),
            GpuSparseAtlasTileEvent::ResolveClipBase(clip_scope),
        ],
        1.0,
        GpuRasterBlendMode::Normal,
        Rect::new(0, 0, 1, 1),
        None,
    );

    let output = renderer
        .draw_sparse_atlas_raster_event_batches_to_rgba8(CanvasSize::new(1, 1), &pool, &[batch])
        .expect("draw sparse atlas clipped container sibling batch");

    assert_eq!(output.pixels, vec![0, 0, 255, 255]);
}

#[test]
fn sparse_atlas_batch_executor_filters_clipped_container_sibling() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let mut pool = GpuSparseAtlasTexturePool::default();
    let raster = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 41,
    };
    renderer
        .update_sparse_atlas_texture_pool(
            &mut pool,
            &[two_pixel_update(raster, [255, 0, 0, 255], [0, 0, 255, 255])],
        )
        .expect("prepare sparse atlas pool");
    let clip_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let clipped_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let batch = GpuSparseAtlasRasterEventBatch::simple_container_scope_tile_events(
        vec![
            GpuSparseAtlasTileEvent::BeginClipBase(clip_scope),
            GpuSparseAtlasTileEvent::ClipBaseRaster(sparse_event_atlas_x(raster, 0)),
            GpuSparseAtlasTileEvent::BeginClippedScope(clipped_scope),
            GpuSparseAtlasTileEvent::Raster(sparse_event_atlas_x(raster, 1)),
            GpuSparseAtlasTileEvent::PointFilter(GpuSparseAtlasPointFilterEvent {
                lut_rgba: invert_lut(),
                opacity: 1.0,
                filter_mode: GpuLutFilterMode::ToneCurveRgb,
                local_bounds: Rect::new(0, 0, 1, 1),
                mask: None,
            }),
            GpuSparseAtlasTileEvent::EndClippedScope(clipped_scope),
            GpuSparseAtlasTileEvent::ResolveClipBase(clip_scope),
        ],
        1.0,
        GpuRasterBlendMode::Normal,
        Rect::new(0, 0, 1, 1),
        None,
    );

    let output = renderer
        .draw_sparse_atlas_raster_event_batches_to_rgba8(CanvasSize::new(1, 1), &pool, &[batch])
        .expect("draw sparse atlas filtered clipped container sibling batch");

    assert_eq!(output.pixels, vec![255, 255, 0, 255]);
}

#[test]
fn sparse_atlas_batch_executor_draws_nested_scope_inside_clipped_container_sibling() {
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
    let clip_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let clipped_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let nested_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let batch = GpuSparseAtlasRasterEventBatch::simple_container_scope_tile_events(
        vec![
            GpuSparseAtlasTileEvent::BeginClipBase(clip_scope),
            GpuSparseAtlasTileEvent::ClipBaseRaster(sparse_event_atlas_x(raster, 0)),
            GpuSparseAtlasTileEvent::BeginClippedScope(clipped_scope),
            GpuSparseAtlasTileEvent::BeginScope(nested_scope),
            GpuSparseAtlasTileEvent::Raster(sparse_event_atlas_x(raster, 1)),
            GpuSparseAtlasTileEvent::EndScope(nested_scope),
            GpuSparseAtlasTileEvent::EndClippedScope(clipped_scope),
            GpuSparseAtlasTileEvent::ResolveClipBase(clip_scope),
        ],
        1.0,
        GpuRasterBlendMode::Normal,
        Rect::new(0, 0, 1, 1),
        None,
    );

    let output = renderer
        .draw_sparse_atlas_raster_event_batches_to_rgba8(CanvasSize::new(1, 1), &pool, &[batch])
        .expect("draw sparse atlas nested clipped container sibling batch");

    assert_eq!(output.pixels, vec![0, 0, 255, 255]);
}

#[test]
fn sparse_atlas_batch_executor_draws_child_clipping_run_inside_clipped_container_sibling() {
    let renderer = GpuRenderer::new(GpuDeviceConfig::default()).expect("create renderer");
    let mut pool = GpuSparseAtlasTexturePool::default();
    let raster = GpuSparseAtlasTextureKey {
        format: GpuSparseAtlasFormat::Rgba8,
        atlas_id: 42,
    };
    renderer
        .update_sparse_atlas_texture_pool(
            &mut pool,
            &[three_pixel_update(
                raster,
                [255, 0, 0, 255],
                [0, 255, 0, 255],
                [0, 0, 255, 255],
            )],
        )
        .expect("prepare sparse atlas pool");
    let outer_clip_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let clipped_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let inner_clip_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let batch = GpuSparseAtlasRasterEventBatch::simple_container_scope_tile_events(
        vec![
            GpuSparseAtlasTileEvent::BeginClipBase(outer_clip_scope),
            GpuSparseAtlasTileEvent::ClipBaseRaster(sparse_event_atlas_x(raster, 0)),
            GpuSparseAtlasTileEvent::BeginClippedScope(clipped_scope),
            GpuSparseAtlasTileEvent::BeginClipBase(inner_clip_scope),
            GpuSparseAtlasTileEvent::ClipBaseRaster(sparse_event_atlas_x(raster, 1)),
            GpuSparseAtlasTileEvent::ClippedRaster(sparse_event_atlas_x(raster, 2)),
            GpuSparseAtlasTileEvent::ResolveClipBase(inner_clip_scope),
            GpuSparseAtlasTileEvent::EndClippedScope(clipped_scope),
            GpuSparseAtlasTileEvent::ResolveClipBase(outer_clip_scope),
        ],
        1.0,
        GpuRasterBlendMode::Normal,
        Rect::new(0, 0, 1, 1),
        None,
    );

    let output = renderer
        .draw_sparse_atlas_raster_event_batches_to_rgba8(CanvasSize::new(1, 1), &pool, &[batch])
        .expect("draw sparse atlas child clipping-run clipped sibling batch");

    assert_eq!(output.pixels, vec![0, 0, 255, 255]);
}

#[test]
fn sparse_atlas_batch_executor_draws_through_child_inside_clipped_container_sibling() {
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
    let clip_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let clipped_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let through_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Through,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let batch = GpuSparseAtlasRasterEventBatch::simple_container_scope_tile_events(
        vec![
            GpuSparseAtlasTileEvent::BeginClipBase(clip_scope),
            GpuSparseAtlasTileEvent::ClipBaseRaster(sparse_event_atlas_x(raster, 0)),
            GpuSparseAtlasTileEvent::BeginClippedScope(clipped_scope),
            GpuSparseAtlasTileEvent::BeginScope(through_scope),
            GpuSparseAtlasTileEvent::Raster(sparse_event_atlas_x(raster, 1)),
            GpuSparseAtlasTileEvent::EndScope(through_scope),
            GpuSparseAtlasTileEvent::EndClippedScope(clipped_scope),
            GpuSparseAtlasTileEvent::ResolveClipBase(clip_scope),
        ],
        1.0,
        GpuRasterBlendMode::Normal,
        Rect::new(0, 0, 1, 1),
        None,
    );

    let output = renderer
        .draw_sparse_atlas_raster_event_batches_to_rgba8(CanvasSize::new(1, 1), &pool, &[batch])
        .expect("draw sparse atlas through-child clipped sibling batch");

    assert_eq!(output.pixels, vec![0, 0, 255, 255]);
}

#[test]
fn sparse_atlas_batch_executor_draws_nested_through_child_inside_clipped_container_sibling() {
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
    let clip_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let clipped_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let nested_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Container,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let through_scope = GpuSparseAtlasScopeEvent {
        kind: GpuSparseAtlasScopeEventKind::Through,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        local_bounds: Rect::new(0, 0, 1, 1),
        mask: None,
    };
    let batch = GpuSparseAtlasRasterEventBatch::simple_container_scope_tile_events(
        vec![
            GpuSparseAtlasTileEvent::BeginClipBase(clip_scope),
            GpuSparseAtlasTileEvent::ClipBaseRaster(sparse_event_atlas_x(raster, 0)),
            GpuSparseAtlasTileEvent::BeginClippedScope(clipped_scope),
            GpuSparseAtlasTileEvent::BeginScope(nested_scope),
            GpuSparseAtlasTileEvent::BeginScope(through_scope),
            GpuSparseAtlasTileEvent::Raster(sparse_event_atlas_x(raster, 1)),
            GpuSparseAtlasTileEvent::EndScope(through_scope),
            GpuSparseAtlasTileEvent::EndScope(nested_scope),
            GpuSparseAtlasTileEvent::EndClippedScope(clipped_scope),
            GpuSparseAtlasTileEvent::ResolveClipBase(clip_scope),
        ],
        1.0,
        GpuRasterBlendMode::Normal,
        Rect::new(0, 0, 1, 1),
        None,
    );

    let output = renderer
        .draw_sparse_atlas_raster_event_batches_to_rgba8(CanvasSize::new(1, 1), &pool, &[batch])
        .expect("draw sparse atlas nested-through clipped sibling batch");

    assert_eq!(output.pixels, vec![0, 0, 255, 255]);
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

fn three_pixel_update(
    key: GpuSparseAtlasTextureKey,
    left: [u8; 4],
    middle: [u8; 4],
    right: [u8; 4],
) -> GpuSparseAtlasTexturePoolUpdate {
    let mut pixels = Vec::with_capacity(12);
    pixels.extend_from_slice(&left);
    pixels.extend_from_slice(&middle);
    pixels.extend_from_slice(&right);
    GpuSparseAtlasTexturePoolUpdate {
        key,
        atlas_size: CanvasSize::new(3, 1),
        chunks: vec![GpuSparseAtlasUpdateChunk {
            atlas_x: 0,
            atlas_y: 0,
            size: CanvasSize::new(3, 1),
            pixels,
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

fn two_pixel_mask_update(
    key: GpuSparseAtlasTextureKey,
    left: u8,
    right: u8,
) -> GpuSparseAtlasTexturePoolUpdate {
    GpuSparseAtlasTexturePoolUpdate {
        key,
        atlas_size: CanvasSize::new(2, 1),
        chunks: vec![GpuSparseAtlasUpdateChunk {
            atlas_x: 0,
            atlas_y: 0,
            size: CanvasSize::new(2, 1),
            pixels: vec![left, right],
        }],
    }
}

fn sparse_event(key: GpuSparseAtlasTextureKey) -> GpuSparseAtlasRasterEvent {
    sparse_event_atlas_x(key, 0)
}

fn sparse_event_atlas_x(key: GpuSparseAtlasTextureKey, atlas_x: u32) -> GpuSparseAtlasRasterEvent {
    sparse_event_atlas_x_and_offset(key, atlas_x, 0)
}

fn sparse_event_atlas_x_and_offset(
    key: GpuSparseAtlasTextureKey,
    atlas_x: u32,
    source_offset_x: i32,
) -> GpuSparseAtlasRasterEvent {
    GpuSparseAtlasRasterEvent {
        raster: GpuSparseAtlasTileRef {
            key,
            atlas_x,
            atlas_y: 0,
            size: CanvasSize::new(1, 1),
        },
        source_offset_x,
        source_offset_y: 0,
        opacity: 1.0,
        blend_mode: GpuRasterBlendMode::Normal,
        mask: None,
    }
}
