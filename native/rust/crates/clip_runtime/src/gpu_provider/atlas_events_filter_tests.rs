use clip_model::LayerId;

use super::atlas_events::{SparseAtlasRasterEventSkipReason, sparse_atlas_raster_event_plan};
use super::atlas_events_test_support::*;

#[test]
fn point_filter_run_lowers_to_filter_batch() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("PointFilterRun")),
        &reload_with_slots(Vec::new()),
        &[point_filter_source(None)],
    );

    assert!(plan.skipped_segments.is_empty());
    assert_eq!(plan.segments.len(), 1);
    let batch = &plan.segments[0].batches[0];
    assert_eq!(
        batch.kind,
        clip_gpu::GpuSparseAtlasRasterEventBatchKind::PointFilterRun
    );
    assert!(batch.events.is_empty());
    assert_eq!(batch.filters.len(), 1);
    assert_eq!(batch.filters[0].local_bounds.width, 128);
    assert_eq!(batch.filters[0].local_bounds.height, 128);
    assert_eq!(batch.filters[0].lut_rgba.len(), 256 * 4);
}

#[test]
fn masked_point_filter_run_is_explicitly_skipped() {
    let plan = sparse_atlas_raster_event_plan(
        &diff_with_segment(segment("PointFilterRun")),
        &reload_with_slots(Vec::new()),
        &[point_filter_source(Some(9))],
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

fn point_filter_source(mask_mipmap_id: Option<u32>) -> clip_gpu::GpuNormalStackSource {
    clip_gpu::GpuNormalStackSource::LutFilter {
        lut_rgba: identity_lut(),
        opacity: 1.0,
        mask_key: mask_mipmap_id.map(|mask_mipmap_id| clip_gpu::GpuMaskResourceKey {
            layer_id: LayerId(10),
            mask_mipmap_id,
        }),
        filter_mode: clip_gpu::GpuLutFilterMode::ToneCurveRgb,
    }
}

fn identity_lut() -> Vec<u8> {
    let mut lut = Vec::with_capacity(256 * 4);
    for value in 0..=255u8 {
        lut.extend_from_slice(&[value, value, value, 255]);
    }
    lut
}
