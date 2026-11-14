use clip_gpu::GpuSparseAtlasScopeEventKind as ScopeKind;
use clip_model::LayerId;

use super::atlas_events::{SparseAtlasRasterEventSkipReason, sparse_atlas_raster_event_plan};
use super::atlas_events_test_support::*;

#[test]
fn simple_container_scope_with_direct_rasters_lowers_to_scope_batch() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("raster", 11, 2, 1, 64, 34),
        ]),
        &[container(
            vec![
                raster(10, 1, clip_gpu::GpuRasterBlendMode::Normal),
                clip_gpu::GpuNormalStackSource::Raster(raster_source(
                    11,
                    2,
                    0.5,
                    clip_gpu::GpuRasterBlendMode::Multiply,
                    None,
                )),
            ],
            0.75,
            clip_gpu::GpuRasterBlendMode::Screen,
        )],
    );

    assert!(plan.skipped_segments.is_empty());
    let batch = &plan.segments[0].batches[0];
    assert_eq!(
        batch.kind,
        clip_gpu::GpuSparseAtlasRasterEventBatchKind::SimpleContainerScope
    );
    assert_eq!(batch.events.len(), 2);
    let scope = batch.scope.expect("container scope payload");
    assert_eq!(scope.kind, ScopeKind::Container);
    assert_eq!(scope.opacity, 0.75);
    assert_eq!(scope.blend_mode, clip_gpu::GpuRasterBlendMode::Screen);
    assert_eq!(scope.local_bounds, clip_model::Rect::new(12, 34, 116, 32));
}

#[test]
fn simple_scope_with_resident_scope_mask_lowers_to_scope_batch() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("mask", 99, 5, 1, 12, 34),
        ]),
        &[masked_container(
            vec![raster(10, 1, clip_gpu::GpuRasterBlendMode::Normal)],
            1.0,
            clip_gpu::GpuRasterBlendMode::Normal,
        )],
    );

    assert!(plan.skipped_segments.is_empty());
    let scope = plan.segments[0].batches[0]
        .scope
        .expect("container scope payload");
    let mask = scope.mask.expect("scope mask tile ref");
    assert_eq!(mask.key.format, clip_gpu::GpuSparseAtlasFormat::R8);
    assert_eq!(mask.key.atlas_id, 3);
    assert_eq!(mask.atlas_x, 80);
    assert_eq!(mask.atlas_y, 32);
    assert_eq!(mask.size, clip_model::CanvasSize::new(64, 32));
}

#[test]
fn simple_scope_with_multi_slot_scope_mask_lowers_to_clipped_scope_batches() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("raster", 10, 1, 1, 76, 34),
            slot("mask", 99, 5, 0, 12, 34),
            slot("mask", 99, 5, 1, 76, 34),
        ]),
        &[masked_container(
            vec![raster(10, 1, clip_gpu::GpuRasterBlendMode::Normal)],
            1.0,
            clip_gpu::GpuRasterBlendMode::Normal,
        )],
    );

    assert!(plan.skipped_segments.is_empty());
    let batches = &plan.segments[0].batches;
    assert_eq!(batches.len(), 2);
    assert_eq!(
        batches[0].scope.expect("first scope").local_bounds,
        clip_model::Rect::new(12, 34, 64, 32)
    );
    assert_eq!(
        batches[1].scope.expect("second scope").local_bounds,
        clip_model::Rect::new(76, 34, 64, 32)
    );
    assert_eq!(batches[0].events[0].source_offset_x, 12);
    assert_eq!(batches[0].events[0].raster.size.width, 64);
    assert_eq!(batches[1].events[0].source_offset_x, 76);
    assert_eq!(batches[1].events[0].raster.size.width, 64);
    assert_eq!(
        batches[0]
            .scope
            .expect("first scope")
            .mask
            .expect("first mask")
            .atlas_x,
        16
    );
    assert_eq!(
        batches[1]
            .scope
            .expect("second scope")
            .mask
            .expect("second mask")
            .atlas_x,
        80
    );
}

#[test]
fn simple_scope_with_partial_multi_slot_scope_mask_is_not_lowered() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("raster", 10, 1, 1, 76, 34),
            slot("mask", 99, 5, 0, 12, 34),
        ]),
        &[masked_container(
            vec![raster(10, 1, clip_gpu::GpuRasterBlendMode::Normal)],
            1.0,
            clip_gpu::GpuRasterBlendMode::Normal,
        )],
    );

    assert!(plan.segments.is_empty());
    assert_eq!(
        plan.skipped_segments[0].reason,
        SparseAtlasRasterEventSkipReason::ScopeMaskNotLowered {
            layer_id: 99,
            resource_id: 5,
        }
    );
}

#[test]
fn simple_scope_with_nested_multi_slot_scope_mask_lowers_ordered_split_events() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("raster", 10, 1, 1, 76, 34),
            slot("mask", 99, 5, 0, 12, 34),
            slot("mask", 99, 5, 1, 76, 34),
        ]),
        &[nested_container(vec![masked_container(
            vec![raster(10, 1, clip_gpu::GpuRasterBlendMode::Normal)],
            0.5,
            clip_gpu::GpuRasterBlendMode::Screen,
        )])],
    );

    assert!(plan.skipped_segments.is_empty());
    let batches = &plan.segments[0].batches;
    assert_eq!(batches.len(), 1);
    let batch = &batches[0];
    assert_eq!(
        batch.scope.expect("outer scope").local_bounds,
        clip_model::Rect::new(12, 34, 128, 32)
    );
    assert_eq!(batch.events.len(), 2);
    assert_eq!(
        tile_event_kinds(batch),
        vec![
            TileEventKind::BeginScope(ScopeKind::Container),
            TileEventKind::Raster,
            TileEventKind::EndScope(ScopeKind::Container),
            TileEventKind::BeginScope(ScopeKind::Container),
            TileEventKind::Raster,
            TileEventKind::EndScope(ScopeKind::Container),
        ]
    );

    let clip_gpu::GpuSparseAtlasTileEvent::BeginScope(first_scope) = batch.tile_events[0] else {
        panic!("expected first nested scope begin");
    };
    assert_eq!(
        first_scope.local_bounds,
        clip_model::Rect::new(12, 34, 64, 32)
    );
    assert_eq!(first_scope.opacity, 0.5);
    assert_eq!(first_scope.blend_mode, clip_gpu::GpuRasterBlendMode::Screen);
    assert_eq!(first_scope.mask.expect("first nested mask").atlas_x, 16);
    let clip_gpu::GpuSparseAtlasTileEvent::Raster(first_raster) = batch.tile_events[1] else {
        panic!("expected first clipped nested raster");
    };
    assert_eq!(first_raster.source_offset_x, 12);
    assert_eq!(first_raster.raster.size.width, 64);

    let clip_gpu::GpuSparseAtlasTileEvent::BeginScope(second_scope) = batch.tile_events[3] else {
        panic!("expected second nested scope begin");
    };
    assert_eq!(
        second_scope.local_bounds,
        clip_model::Rect::new(76, 34, 64, 32)
    );
    assert_eq!(second_scope.mask.expect("second nested mask").atlas_x, 80);
    let clip_gpu::GpuSparseAtlasTileEvent::Raster(second_raster) = batch.tile_events[4] else {
        panic!("expected second clipped nested raster");
    };
    assert_eq!(second_raster.source_offset_x, 76);
    assert_eq!(second_raster.raster.size.width, 64);
}

#[test]
fn simple_scope_with_nested_partial_multi_slot_scope_mask_is_not_lowered() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("raster", 10, 1, 1, 76, 34),
            slot("mask", 99, 5, 0, 12, 34),
        ]),
        &[nested_container(vec![masked_container(
            vec![raster(10, 1, clip_gpu::GpuRasterBlendMode::Normal)],
            1.0,
            clip_gpu::GpuRasterBlendMode::Normal,
        )])],
    );

    assert!(plan.segments.is_empty());
    assert_eq!(
        plan.skipped_segments[0].reason,
        SparseAtlasRasterEventSkipReason::ScopeMaskNotLowered {
            layer_id: 99,
            resource_id: 5,
        }
    );
}

#[test]
fn simple_container_scope_with_point_filter_child_lowers_ordered_tile_events() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(raster_slots(1)),
        &[nested_container(vec![
            raster(10, 1, clip_gpu::GpuRasterBlendMode::Normal),
            lut_filter(None),
        ])],
    );

    assert!(plan.skipped_segments.is_empty());
    let batch = &plan.segments[0].batches[0];
    assert_eq!(batch.events.len(), 1);
    assert_eq!(batch.filters.len(), 1);
    assert_eq!(
        tile_event_kinds(batch),
        vec![TileEventKind::Raster, TileEventKind::PointFilter]
    );
    let clip_gpu::GpuSparseAtlasTileEvent::PointFilter(filter) = &batch.tile_events[1] else {
        panic!("expected point filter tile event");
    };
    assert_eq!(filter.local_bounds, clip_model::Rect::new(12, 34, 64, 32));
}

#[test]
fn simple_container_scope_with_solid_color_child_lowers_ordered_tile_event() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(Vec::new()),
        &[nested_container(vec![solid_color(
            clip_model::Rgba8 {
                r: 10,
                g: 20,
                b: 30,
                a: 255,
            },
            0.5,
        )])],
    );

    assert!(plan.skipped_segments.is_empty());
    let batch = &plan.segments[0].batches[0];
    assert!(batch.events.is_empty());
    assert_eq!(tile_event_kinds(batch), vec![TileEventKind::SolidColor]);
    let clip_gpu::GpuSparseAtlasTileEvent::SolidColor(event) = batch.tile_events[0] else {
        panic!("expected solid color tile event");
    };
    assert_eq!(event.color.r, 10);
    assert_eq!(event.opacity, 0.5);
    assert_eq!(event.local_bounds, clip_model::Rect::new(0, 0, 128, 128));
}

#[test]
fn simple_container_scope_with_masked_point_filter_child_splits_filter_events() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(vec![
            slot("raster", 10, 1, 0, 12, 34),
            slot("raster", 10, 1, 1, 76, 34),
            slot("mask", 10, 9, 0, 12, 34),
            slot("mask", 10, 9, 1, 76, 34),
        ]),
        &[nested_container(vec![
            raster(10, 1, clip_gpu::GpuRasterBlendMode::Normal),
            lut_filter(Some(9)),
        ])],
    );

    assert!(plan.skipped_segments.is_empty());
    let batch = &plan.segments[0].batches[0];
    assert_eq!(batch.events.len(), 2);
    assert_eq!(batch.filters.len(), 2);
    assert_eq!(batch.tile_events.len(), 4);
    assert_eq!(
        batch.filters[0].local_bounds,
        clip_model::Rect::new(12, 34, 64, 32)
    );
    assert_eq!(
        batch.filters[1].local_bounds,
        clip_model::Rect::new(76, 34, 64, 32)
    );
    assert_eq!(
        batch.filters[0].mask.expect("first filter mask").atlas_x,
        16
    );
    assert_eq!(
        batch.filters[1].mask.expect("second filter mask").atlas_x,
        80
    );
}

#[test]
fn simple_container_scope_with_nested_container_child_lowers_ordered_tile_events() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(raster_slots(1)),
        &[nested_container(vec![container(
            vec![raster(10, 1, clip_gpu::GpuRasterBlendMode::Normal)],
            0.5,
            clip_gpu::GpuRasterBlendMode::Screen,
        )])],
    );

    assert!(plan.skipped_segments.is_empty());
    let batch = &plan.segments[0].batches[0];
    assert_eq!(batch.events.len(), 1);
    assert_eq!(
        tile_event_kinds(batch),
        vec![
            TileEventKind::BeginScope(ScopeKind::Container),
            TileEventKind::Raster,
            TileEventKind::EndScope(ScopeKind::Container),
        ]
    );
    let clip_gpu::GpuSparseAtlasTileEvent::BeginScope(scope) = batch.tile_events[0] else {
        panic!("expected nested scope begin");
    };
    assert_eq!(scope.opacity, 0.5);
    assert_eq!(scope.blend_mode, clip_gpu::GpuRasterBlendMode::Screen);
    assert_eq!(scope.local_bounds, clip_model::Rect::new(12, 34, 64, 32));
}

#[test]
fn simple_container_scope_with_too_deep_nested_scope_is_not_lowered() {
    let leaf = raster(10, 1, clip_gpu::GpuRasterBlendMode::Normal);
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(raster_slots(1)),
        &[nested_container(vec![nested_container(vec![
            nested_container(vec![nested_container(vec![nested_container(vec![leaf])])]),
        ])])],
    );

    assert!(plan.segments.is_empty());
    assert_eq!(
        plan.skipped_segments[0].reason,
        SparseAtlasRasterEventSkipReason::ScopeDepthLimitExceeded
    );
}

#[test]
fn simple_container_scope_without_resident_filter_mask_is_not_lowered() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(raster_slots(1)),
        &[nested_container(vec![
            raster(10, 1, clip_gpu::GpuRasterBlendMode::Normal),
            lut_filter(Some(9)),
        ])],
    );

    assert!(plan.segments.is_empty());
    assert_eq!(
        plan.skipped_segments[0].reason,
        SparseAtlasRasterEventSkipReason::FilterMaskNotLowered {
            layer_id: 10,
            resource_id: 9,
        }
    );
}

#[test]
fn simple_scope_without_resident_scope_mask_is_not_lowered() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("SimpleContainerScope")),
        &reload_with_slots(raster_slots(1)),
        &[masked_container(
            vec![raster(10, 1, clip_gpu::GpuRasterBlendMode::Normal)],
            1.0,
            clip_gpu::GpuRasterBlendMode::Normal,
        )],
    );

    assert!(plan.segments.is_empty());
    assert_eq!(
        plan.skipped_segments[0].reason,
        SparseAtlasRasterEventSkipReason::ScopeMaskNotLowered {
            layer_id: 99,
            resource_id: 5,
        }
    );
}

/// One row per scope/clipping permutation that lowers to a single ordered
/// tile-event batch. Each row names the source tree shape, how many resident
/// raster slots it needs (layers `10 + i` with mipmap `1 + i`), the expected
/// `batch.events` count, and the exact ordered tile-event kinds, including the
/// `Container` vs `Through` kind of every scope begin/end.
#[test]
fn scope_permutations_lower_to_expected_ordered_tile_events() {
    use TileEventKind::*;
    use clip_gpu::GpuRasterBlendMode::{Multiply, Normal, Screen};

    struct Case {
        name: &'static str,
        segment_kind: &'static str,
        raster_slot_count: u32,
        source: clip_gpu::GpuNormalStackSource,
        raster_events: usize,
        tile_events: Vec<TileEventKind>,
    }

    let cases = vec![
        Case {
            name: "container > raster clipping run",
            segment_kind: "SimpleContainerScope",
            raster_slot_count: 2,
            source: nested_container(vec![clipping_run_source()]),
            raster_events: 2,
            tile_events: vec![
                BeginClipBase,
                ClipBaseRaster,
                ClippedRaster,
                ResolveClipBase,
            ],
        },
        Case {
            name: "container > nested container > raster clipping run",
            segment_kind: "SimpleContainerScope",
            raster_slot_count: 2,
            source: nested_container(vec![nested_container(vec![clipping_run_source()])]),
            raster_events: 2,
            tile_events: vec![
                BeginScope(ScopeKind::Container),
                BeginClipBase,
                ClipBaseRaster,
                ClippedRaster,
                ResolveClipBase,
                EndScope(ScopeKind::Container),
            ],
        },
        Case {
            name: "through > nested through > raster clipping run",
            segment_kind: "SimpleThroughScope",
            raster_slot_count: 2,
            source: through(vec![through(vec![clipping_run_source()], 1.0)], 1.0),
            raster_events: 2,
            tile_events: vec![
                BeginScope(ScopeKind::Through),
                BeginClipBase,
                ClipBaseRaster,
                ClippedRaster,
                ResolveClipBase,
                EndScope(ScopeKind::Through),
            ],
        },
        Case {
            name: "through > nested container > raster clipping run",
            segment_kind: "SimpleThroughScope",
            raster_slot_count: 2,
            source: through(vec![nested_container(vec![clipping_run_source()])], 1.0),
            raster_events: 2,
            tile_events: vec![
                BeginScope(ScopeKind::Container),
                BeginClipBase,
                ClipBaseRaster,
                ClippedRaster,
                ResolveClipBase,
                EndScope(ScopeKind::Container),
            ],
        },
        Case {
            name: "clipped container sibling > raster",
            segment_kind: "SimpleContainerScope",
            raster_slot_count: 2,
            source: clipped_container_sibling(vec![raster(11, 2, Multiply)], Screen),
            raster_events: 2,
            tile_events: vec![
                BeginClipBase,
                ClipBaseRaster,
                BeginClippedScope,
                Raster,
                EndClippedScope,
                ResolveClipBase,
            ],
        },
        Case {
            name: "clipped container sibling > solid color",
            segment_kind: "SimpleContainerScope",
            raster_slot_count: 1,
            source: clipped_container_sibling(
                vec![solid_color(
                    clip_model::Rgba8 {
                        r: 0,
                        g: 0,
                        b: 255,
                        a: 255,
                    },
                    1.0,
                )],
                Screen,
            ),
            raster_events: 1,
            tile_events: vec![
                BeginClipBase,
                ClipBaseRaster,
                BeginClippedScope,
                SolidColor,
                EndClippedScope,
                ResolveClipBase,
            ],
        },
        Case {
            name: "clipped container sibling > raster, point filter",
            segment_kind: "SimpleContainerScope",
            raster_slot_count: 2,
            source: clipped_container_sibling(
                vec![raster(11, 2, Multiply), lut_filter(None)],
                Screen,
            ),
            raster_events: 2,
            tile_events: vec![
                BeginClipBase,
                ClipBaseRaster,
                BeginClippedScope,
                Raster,
                PointFilter,
                EndClippedScope,
                ResolveClipBase,
            ],
        },
        Case {
            name: "clipped container sibling > nested container > raster",
            segment_kind: "SimpleContainerScope",
            raster_slot_count: 2,
            source: clipped_container_sibling(
                vec![container(vec![raster(11, 2, Multiply)], 0.75, Screen)],
                Normal,
            ),
            raster_events: 2,
            tile_events: vec![
                BeginClipBase,
                ClipBaseRaster,
                BeginClippedScope,
                BeginScope(ScopeKind::Container),
                Raster,
                EndScope(ScopeKind::Container),
                EndClippedScope,
                ResolveClipBase,
            ],
        },
        Case {
            name: "clipped container sibling > child clipping run",
            segment_kind: "SimpleContainerScope",
            raster_slot_count: 3,
            source: clipped_container_sibling(vec![child_clipping_run()], Normal),
            raster_events: 3,
            tile_events: vec![
                BeginClipBase,
                ClipBaseRaster,
                BeginClippedScope,
                BeginClipBase,
                ClipBaseRaster,
                ClippedRaster,
                ResolveClipBase,
                EndClippedScope,
                ResolveClipBase,
            ],
        },
        Case {
            name: "clipped container sibling > through > through > raster",
            segment_kind: "SimpleContainerScope",
            raster_slot_count: 2,
            source: clipped_container_sibling(
                vec![through(
                    vec![through(vec![raster(11, 2, Normal)], 0.75)],
                    0.5,
                )],
                Normal,
            ),
            raster_events: 2,
            tile_events: vec![
                BeginClipBase,
                ClipBaseRaster,
                BeginClippedScope,
                BeginScope(ScopeKind::Through),
                BeginScope(ScopeKind::Through),
                Raster,
                EndScope(ScopeKind::Through),
                EndScope(ScopeKind::Through),
                EndClippedScope,
                ResolveClipBase,
            ],
        },
        Case {
            name: "clipped container sibling > through > child clipping run",
            segment_kind: "SimpleContainerScope",
            raster_slot_count: 3,
            source: clipped_container_sibling(
                vec![through(vec![child_clipping_run()], 0.5)],
                Normal,
            ),
            raster_events: 3,
            tile_events: vec![
                BeginClipBase,
                ClipBaseRaster,
                BeginClippedScope,
                BeginScope(ScopeKind::Through),
                BeginClipBase,
                ClipBaseRaster,
                ClippedRaster,
                ResolveClipBase,
                EndScope(ScopeKind::Through),
                EndClippedScope,
                ResolveClipBase,
            ],
        },
        Case {
            name: "clipped container sibling > through > raster, then point filter",
            segment_kind: "SimpleContainerScope",
            raster_slot_count: 2,
            source: clipped_container_sibling(
                vec![through(vec![raster(11, 2, Normal)], 0.5), lut_filter(None)],
                Normal,
            ),
            raster_events: 2,
            tile_events: vec![
                BeginClipBase,
                ClipBaseRaster,
                BeginClippedScope,
                BeginScope(ScopeKind::Through),
                Raster,
                EndScope(ScopeKind::Through),
                PointFilter,
                EndClippedScope,
                ResolveClipBase,
            ],
        },
    ];

    for case in cases {
        let plan = sparse_atlas_raster_event_plan(
            &diff_with_segment(segment(case.segment_kind)),
            &reload_with_slots(raster_slots(case.raster_slot_count)),
            &[case.source],
        );

        assert!(
            plan.skipped_segments.is_empty(),
            "{}: skipped {:?}",
            case.name,
            plan.skipped_segments
        );
        assert_eq!(
            plan.segments[0].batches.len(),
            1,
            "{}: batch count",
            case.name
        );
        let batch = &plan.segments[0].batches[0];
        assert_eq!(
            batch.events.len(),
            case.raster_events,
            "{}: raster event count",
            case.name
        );
        let expected_filters = case
            .tile_events
            .iter()
            .filter(|kind| **kind == PointFilter)
            .count();
        assert_eq!(
            batch.filters.len(),
            expected_filters,
            "{}: point filter count",
            case.name
        );
        assert_eq!(
            tile_event_kinds(batch),
            case.tile_events,
            "{}: ordered tile event kinds",
            case.name
        );
    }
}

/// Payload-free view of a tile event, so ordered-event tests compare whole
/// sequences in one assertion while still distinguishing scope kinds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TileEventKind {
    Raster,
    SolidColor,
    PointFilter,
    BeginScope(ScopeKind),
    EndScope(ScopeKind),
    BeginClippedScope,
    EndClippedScope,
    BeginClipBase,
    ClipBaseRaster,
    ClippedRaster,
    ResolveClipBase,
}

fn tile_event_kinds(batch: &clip_gpu::GpuSparseAtlasRasterEventBatch) -> Vec<TileEventKind> {
    use clip_gpu::GpuSparseAtlasTileEvent as Event;
    batch
        .tile_events
        .iter()
        .map(|event| match event {
            Event::Raster(_) => TileEventKind::Raster,
            Event::SolidColor(_) => TileEventKind::SolidColor,
            Event::PointFilter(_) => TileEventKind::PointFilter,
            Event::BeginScope(scope) => TileEventKind::BeginScope(scope.kind),
            Event::EndScope(scope) => TileEventKind::EndScope(scope.kind),
            Event::BeginClippedScope(_) => TileEventKind::BeginClippedScope,
            Event::EndClippedScope(_) => TileEventKind::EndClippedScope,
            Event::BeginClipBase(_) => TileEventKind::BeginClipBase,
            Event::ClipBaseRaster(_) => TileEventKind::ClipBaseRaster,
            Event::ClippedRaster(_) => TileEventKind::ClippedRaster,
            Event::ResolveClipBase(_) => TileEventKind::ResolveClipBase,
        })
        .collect()
}

/// Resident raster slots for layers `10 + i` / mipmap `1 + i`, all at canvas
/// `(12, 34)`, matching the layer ids used by the source builders below.
fn raster_slots(count: u32) -> Vec<super::atlas_rerun::SparseAtlasRerunSlot> {
    (0..count)
        .map(|index| slot("raster", 10 + index, 1 + index, index, 12, 34))
        .collect()
}

fn invert_lut() -> Vec<u8> {
    let mut lut = Vec::with_capacity(256 * 4);
    for value in 0u8..=255 {
        lut.extend_from_slice(&[255 - value, 255 - value, 255 - value, 255]);
    }
    lut
}

fn raster(
    layer_id: u32,
    render_mipmap_id: u32,
    blend_mode: clip_gpu::GpuRasterBlendMode,
) -> clip_gpu::GpuNormalStackSource {
    clip_gpu::GpuNormalStackSource::Raster(raster_source(
        layer_id,
        render_mipmap_id,
        1.0,
        blend_mode,
        None,
    ))
}

fn solid_color(color: clip_model::Rgba8, opacity: f32) -> clip_gpu::GpuNormalStackSource {
    clip_gpu::GpuNormalStackSource::SolidColor { color, opacity }
}

fn lut_filter(mask_mipmap_id: Option<u32>) -> clip_gpu::GpuNormalStackSource {
    clip_gpu::GpuNormalStackSource::LutFilter {
        lut_rgba: invert_lut(),
        opacity: 1.0,
        mask_key: mask_mipmap_id.map(|mask_mipmap_id| clip_gpu::GpuMaskResourceKey {
            layer_id: LayerId(10),
            mask_mipmap_id,
        }),
        filter_mode: clip_gpu::GpuLutFilterMode::ToneCurveRgb,
    }
}

fn container(
    children: Vec<clip_gpu::GpuNormalStackSource>,
    opacity: f32,
    blend_mode: clip_gpu::GpuRasterBlendMode,
) -> clip_gpu::GpuNormalStackSource {
    clip_gpu::GpuNormalStackSource::Container {
        children,
        opacity,
        mask_key: None,
        blend_mode,
    }
}

/// Container masked by layer 99 / mask mipmap 5.
fn masked_container(
    children: Vec<clip_gpu::GpuNormalStackSource>,
    opacity: f32,
    blend_mode: clip_gpu::GpuRasterBlendMode,
) -> clip_gpu::GpuNormalStackSource {
    clip_gpu::GpuNormalStackSource::Container {
        children,
        opacity,
        mask_key: Some(clip_gpu::GpuMaskResourceKey {
            layer_id: LayerId(99),
            mask_mipmap_id: 5,
        }),
        blend_mode,
    }
}

fn nested_container(
    children: Vec<clip_gpu::GpuNormalStackSource>,
) -> clip_gpu::GpuNormalStackSource {
    container(children, 1.0, clip_gpu::GpuRasterBlendMode::Normal)
}

fn through(
    children: Vec<clip_gpu::GpuNormalStackSource>,
    opacity: f32,
) -> clip_gpu::GpuNormalStackSource {
    clip_gpu::GpuNormalStackSource::ThroughGroup {
        children,
        opacity,
        mask_key: None,
    }
}

/// Base raster 10/1 with a clipped Multiply raster 11/2.
fn clipping_run_source() -> clip_gpu::GpuNormalStackSource {
    clip_gpu::GpuNormalStackSource::ClippingRun {
        base: raster_source(10, 1, 1.0, clip_gpu::GpuRasterBlendMode::Normal, None),
        clipped: vec![clip_gpu::GpuClippedStackSource::Raster(raster_source(
            11,
            2,
            1.0,
            clip_gpu::GpuRasterBlendMode::Multiply,
            None,
        ))],
    }
}

/// Base raster 11/2 with a clipped Normal raster 12/3, used inside a clipped
/// container sibling whose own base is raster 10/1.
fn child_clipping_run() -> clip_gpu::GpuNormalStackSource {
    clip_gpu::GpuNormalStackSource::ClippingRun {
        base: raster_source(11, 2, 1.0, clip_gpu::GpuRasterBlendMode::Normal, None),
        clipped: vec![clip_gpu::GpuClippedStackSource::Raster(raster_source(
            12,
            3,
            1.0,
            clip_gpu::GpuRasterBlendMode::Normal,
            None,
        ))],
    }
}

/// Outer container whose only child is a clipping run: base raster 10/1 with a
/// clipped container sibling (layer 11, opacity 0.5) holding `children`.
fn clipped_container_sibling(
    children: Vec<clip_gpu::GpuNormalStackSource>,
    blend_mode: clip_gpu::GpuRasterBlendMode,
) -> clip_gpu::GpuNormalStackSource {
    nested_container(vec![clip_gpu::GpuNormalStackSource::ClippingRun {
        base: raster_source(10, 1, 1.0, clip_gpu::GpuRasterBlendMode::Normal, None),
        clipped: vec![clip_gpu::GpuClippedStackSource::Container {
            layer_id: LayerId(11),
            children,
            opacity: 0.5,
            mask_key: None,
            blend_mode,
        }],
    }])
}
