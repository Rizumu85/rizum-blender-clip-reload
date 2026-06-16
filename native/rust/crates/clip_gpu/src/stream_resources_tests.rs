use std::collections::HashMap;

use clip_model::{CanvasSize, LayerId, Rgba8};

use crate::stream_bounds::CanvasRect;
use crate::stream_resources::{
    KnownSourceActivity, known_clipped_sibling_activity, known_clipping_run_activity,
    known_raster_source_bounds, known_stack_activity, preserving_pass_bounds_for_change,
};
use crate::{
    GpuClippedStackSource, GpuMaskResourceCache, GpuMaskResourceKey, GpuNormalRasterSource,
    GpuNormalStackSource, GpuRasterBlendMode, GpuRasterResourceCache, GpuRasterResourceKey,
    GpuRenderError, GpuRenderer,
};

struct SizeProvider {
    sizes: HashMap<GpuRasterResourceKey, CanvasSize>,
    offsets: HashMap<GpuRasterResourceKey, (i32, i32)>,
}

impl SizeProvider {
    fn new(sizes: &[(GpuRasterResourceKey, CanvasSize)]) -> Self {
        Self {
            sizes: sizes.iter().copied().collect(),
            offsets: HashMap::new(),
        }
    }

    fn with_offsets(mut self, offsets: &[(GpuRasterResourceKey, (i32, i32))]) -> Self {
        self.offsets = offsets.iter().copied().collect();
        self
    }
}

impl crate::stream::GpuNormalStackResourceProvider for SizeProvider {
    type Error = GpuRenderError;

    fn raster_resource(
        &mut self,
        _renderer: &GpuRenderer,
        _source: GpuNormalRasterSource,
    ) -> Result<GpuRasterResourceCache, Self::Error> {
        unreachable!("activity checks must not request raster resources")
    }

    fn raster_resource_size(&self, source: GpuNormalRasterSource) -> Option<CanvasSize> {
        self.sizes.get(&source.key).copied()
    }

    fn raster_resource_offset(&self, source: GpuNormalRasterSource) -> Option<(i32, i32)> {
        self.offsets.get(&source.key).copied()
    }

    fn mask_resource(
        &mut self,
        _renderer: &GpuRenderer,
        _key: GpuMaskResourceKey,
    ) -> Result<GpuMaskResourceCache, Self::Error> {
        unreachable!("activity checks must not request mask resources")
    }
}

#[test]
fn known_raster_bounds_use_provider_offset() {
    let key = raster_key(1);
    let provider =
        SizeProvider::new(&[(key, CanvasSize::new(4, 5))]).with_offsets(&[(key, (6, 7))]);

    assert_eq!(
        known_raster_source_bounds(&provider, raster_source(key, 1, 1), CanvasSize::new(20, 20)),
        Some(Some(CanvasRect {
            x: 6,
            y: 7,
            width: 4,
            height: 5,
        }))
    );
}

#[test]
fn off_canvas_container_stack_is_known_empty() {
    let key = raster_key(1);
    let provider = SizeProvider::new(&[(key, CanvasSize::new(8, 8))]);
    let sources = vec![GpuNormalStackSource::Container {
        children: vec![GpuNormalStackSource::Raster(raster_source(key, 40, 40))],
        opacity: 1.0,
        mask_key: None,
        blend_mode: GpuRasterBlendMode::Normal,
    }];

    assert_eq!(
        known_stack_activity(&provider, &sources, CanvasSize::new(32, 32)),
        KnownSourceActivity::Empty
    );
}

#[test]
fn unknown_raster_size_keeps_stack_activity_unknown() {
    let provider = SizeProvider::new(&[]);
    let sources = vec![GpuNormalStackSource::Raster(raster_source(
        raster_key(1),
        40,
        40,
    ))];

    assert_eq!(
        known_stack_activity(&provider, &sources, CanvasSize::new(32, 32)),
        KnownSourceActivity::Unknown
    );
}

#[test]
fn visible_solid_marks_stack_as_writing() {
    let provider = SizeProvider::new(&[]);
    let sources = vec![GpuNormalStackSource::SolidColor {
        color: Rgba8 {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        opacity: 1.0,
    }];

    assert_eq!(
        known_stack_activity(&provider, &sources, CanvasSize::new(32, 32)),
        KnownSourceActivity::Writes
    );
}

#[test]
fn off_canvas_clipping_base_is_known_empty() {
    let key = raster_key(1);
    let provider = SizeProvider::new(&[(key, CanvasSize::new(8, 8))]);

    assert_eq!(
        known_clipping_run_activity(
            &provider,
            raster_source(key, -16, -16),
            CanvasSize::new(8, 8),
        ),
        KnownSourceActivity::Empty
    );
}

#[test]
fn zero_opacity_clipped_sibling_is_known_empty() {
    let base_key = raster_key(1);
    let clipped_key = raster_key(2);
    let provider = SizeProvider::new(&[
        (base_key, CanvasSize::new(8, 8)),
        (clipped_key, CanvasSize::new(8, 8)),
    ]);

    assert_eq!(
        known_clipped_sibling_activity(
            &provider,
            raster_source(base_key, 0, 0),
            &[GpuClippedStackSource::Raster(raster_source_with_opacity(
                clipped_key,
                0,
                0,
                0.0,
            ))],
            CanvasSize::new(8, 8),
        ),
        KnownSourceActivity::Empty
    );
}

#[test]
fn non_overlapping_clipped_sibling_is_known_empty() {
    let base_key = raster_key(1);
    let clipped_key = raster_key(2);
    let provider = SizeProvider::new(&[
        (base_key, CanvasSize::new(4, 4)),
        (clipped_key, CanvasSize::new(4, 4)),
    ]);

    assert_eq!(
        known_clipped_sibling_activity(
            &provider,
            raster_source(base_key, 0, 0),
            &[GpuClippedStackSource::Raster(raster_source(
                clipped_key,
                8,
                8,
            ))],
            CanvasSize::new(16, 16),
        ),
        KnownSourceActivity::Empty
    );
}

#[test]
fn intersecting_clipped_sibling_is_known_writing() {
    let base_key = raster_key(1);
    let clipped_key = raster_key(2);
    let provider = SizeProvider::new(&[
        (base_key, CanvasSize::new(4, 4)),
        (clipped_key, CanvasSize::new(4, 4)),
    ]);

    assert_eq!(
        known_clipped_sibling_activity(
            &provider,
            raster_source(base_key, 0, 0),
            &[GpuClippedStackSource::Raster(raster_source(
                clipped_key,
                2,
                2,
            ))],
            CanvasSize::new(16, 16),
        ),
        KnownSourceActivity::Writes
    );
}

#[test]
fn unknown_clipped_sibling_size_stays_unknown() {
    let base_key = raster_key(1);
    let clipped_key = raster_key(2);
    let provider = SizeProvider::new(&[(base_key, CanvasSize::new(4, 4))]);

    assert_eq!(
        known_clipped_sibling_activity(
            &provider,
            raster_source(base_key, 0, 0),
            &[GpuClippedStackSource::Raster(raster_source(
                clipped_key,
                2,
                2,
            ))],
            CanvasSize::new(16, 16),
        ),
        KnownSourceActivity::Unknown
    );
}

#[test]
fn preserving_pass_bounds_do_not_expand_dirty_area() {
    let dirty = CanvasRect {
        x: 10,
        y: 10,
        width: 12,
        height: 12,
    };
    let larger_source = CanvasRect {
        x: 0,
        y: 0,
        width: 40,
        height: 40,
    };

    assert_eq!(
        preserving_pass_bounds_for_change(Some(dirty), Some(larger_source)),
        Some(dirty)
    );
}

#[test]
fn preserving_pass_bounds_skip_non_overlapping_source() {
    let dirty = CanvasRect {
        x: 10,
        y: 10,
        width: 12,
        height: 12,
    };
    let outside_source = CanvasRect {
        x: 30,
        y: 30,
        width: 4,
        height: 4,
    };

    assert_eq!(
        preserving_pass_bounds_for_change(Some(dirty), Some(outside_source)),
        None
    );
}

fn raster_source(key: GpuRasterResourceKey, offset_x: i32, offset_y: i32) -> GpuNormalRasterSource {
    raster_source_with_opacity(key, offset_x, offset_y, 1.0)
}

fn raster_source_with_opacity(
    key: GpuRasterResourceKey,
    offset_x: i32,
    offset_y: i32,
    opacity: f32,
) -> GpuNormalRasterSource {
    GpuNormalRasterSource {
        key,
        opacity,
        mask_key: None,
        offset_x,
        offset_y,
        blend_mode: GpuRasterBlendMode::Normal,
    }
}

fn raster_key(id: u32) -> GpuRasterResourceKey {
    GpuRasterResourceKey {
        layer_id: LayerId(id),
        render_mipmap_id: id,
    }
}
