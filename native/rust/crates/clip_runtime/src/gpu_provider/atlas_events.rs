use clip_model::CanvasSize;

use super::atlas_rerun::{
    SparseAtlasReloadPlan, SparseAtlasRerunSegment, SparseAtlasRerunSlot, affected_rerun_segments,
};
use crate::reload_diff::{ReloadDiffPlan, ReloadDiffSegment, ReloadDirtySegmentEventRange};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SparseAtlasRasterEventPlan {
    pub segments: Vec<SparseAtlasRasterEventSegment>,
    pub skipped_segments: Vec<SparseAtlasRasterEventSkip>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SparseAtlasRasterEventSegment {
    pub ordinal: u32,
    pub event_ranges: Vec<ReloadDirtySegmentEventRange>,
    pub batches: Vec<clip_gpu::GpuSparseAtlasRasterEventBatch>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SparseAtlasRasterEventSkip {
    pub ordinal: u32,
    pub reason: SparseAtlasRasterEventSkipReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SparseAtlasRasterEventSkipReason {
    SegmentManifestMissing,
    NonRasterRun,
    EmptyRasterSlots,
    SourceSpanOutOfRange,
    RasterSourceMissing {
        layer_id: u32,
        resource_id: u32,
    },
    MaskSlotMissing {
        layer_id: u32,
        resource_id: u32,
        canvas_x: u32,
        canvas_y: u32,
    },
    MixedSparseAtlasKeys,
    CanvasCoordinateOutOfRange,
}

pub(crate) fn sparse_atlas_raster_event_plan(
    diff: &ReloadDiffPlan,
    reload: &SparseAtlasReloadPlan,
    sources: &[clip_gpu::GpuNormalStackSource],
) -> SparseAtlasRasterEventPlan {
    sparse_atlas_raster_event_plan_for_segments(diff, &reload.rerunnable_segments, sources)
}

pub(crate) fn sparse_atlas_raster_affected_event_plan(
    diff: &ReloadDiffPlan,
    reload: &SparseAtlasReloadPlan,
    sources: &[clip_gpu::GpuNormalStackSource],
) -> SparseAtlasRasterEventPlan {
    let affected_segments = affected_rerun_segments(diff, &reload.cache);
    sparse_atlas_raster_event_plan_for_segments(diff, &affected_segments, sources)
}

fn sparse_atlas_raster_event_plan_for_segments(
    diff: &ReloadDiffPlan,
    rerunnable_segments: &[SparseAtlasRerunSegment],
    sources: &[clip_gpu::GpuNormalStackSource],
) -> SparseAtlasRasterEventPlan {
    let mut segments = Vec::new();
    let mut skipped_segments = Vec::new();
    for rerun_segment in rerunnable_segments {
        match lower_raster_run_segment(diff, rerun_segment, sources) {
            Ok(segment) => segments.push(segment),
            Err(reason) => skipped_segments.push(SparseAtlasRasterEventSkip {
                ordinal: rerun_segment.ordinal,
                reason,
            }),
        }
    }
    SparseAtlasRasterEventPlan {
        segments,
        skipped_segments,
    }
}

impl From<SparseAtlasRasterEventPlan> for crate::GpuSparseAtlasRasterEventPlan {
    fn from(value: SparseAtlasRasterEventPlan) -> Self {
        Self {
            segments: value.segments.into_iter().map(Into::into).collect(),
            skipped_segments: value.skipped_segments.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<SparseAtlasRasterEventSegment> for crate::GpuSparseAtlasRasterEventSegment {
    fn from(value: SparseAtlasRasterEventSegment) -> Self {
        Self {
            ordinal: value.ordinal,
            event_ranges: value
                .event_ranges
                .into_iter()
                .map(|range| crate::GpuSparseAtlasEventRange {
                    start: range.start,
                    end: range.end,
                })
                .collect(),
            batches: value.batches,
        }
    }
}

impl From<SparseAtlasRasterEventSkip> for crate::GpuSparseAtlasRasterEventSkip {
    fn from(value: SparseAtlasRasterEventSkip) -> Self {
        Self {
            ordinal: value.ordinal,
            reason: value.reason.into(),
        }
    }
}

impl From<SparseAtlasRasterEventSkipReason> for crate::GpuSparseAtlasRasterEventSkipReason {
    fn from(value: SparseAtlasRasterEventSkipReason) -> Self {
        match value {
            SparseAtlasRasterEventSkipReason::SegmentManifestMissing => {
                Self::SegmentManifestMissing
            }
            SparseAtlasRasterEventSkipReason::NonRasterRun => Self::NonRasterRun,
            SparseAtlasRasterEventSkipReason::EmptyRasterSlots => Self::EmptyRasterSlots,
            SparseAtlasRasterEventSkipReason::SourceSpanOutOfRange => Self::SourceSpanOutOfRange,
            SparseAtlasRasterEventSkipReason::RasterSourceMissing {
                layer_id,
                resource_id,
            } => Self::RasterSourceMissing {
                layer_id,
                resource_id,
            },
            SparseAtlasRasterEventSkipReason::MaskSlotMissing {
                layer_id,
                resource_id,
                canvas_x,
                canvas_y,
            } => Self::MaskSlotMissing {
                layer_id,
                resource_id,
                canvas_x,
                canvas_y,
            },
            SparseAtlasRasterEventSkipReason::MixedSparseAtlasKeys => Self::MixedSparseAtlasKeys,
            SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange => {
                Self::CanvasCoordinateOutOfRange
            }
        }
    }
}

fn lower_raster_run_segment(
    diff: &ReloadDiffPlan,
    rerun_segment: &SparseAtlasRerunSegment,
    sources: &[clip_gpu::GpuNormalStackSource],
) -> Result<SparseAtlasRasterEventSegment, SparseAtlasRasterEventSkipReason> {
    let segment = diff
        .manifest
        .segments
        .iter()
        .find(|segment| segment.ordinal == rerun_segment.ordinal)
        .ok_or(SparseAtlasRasterEventSkipReason::SegmentManifestMissing)?;
    if segment.kind == "RasterClippingRun" {
        return lower_raster_clipping_run_segment(segment, rerun_segment, sources);
    }
    if segment.kind != "RasterRun" {
        return Err(SparseAtlasRasterEventSkipReason::NonRasterRun);
    }

    let source_span = segment_source_span(segment, sources)?;
    let mut raster_slots = rerun_segment
        .resident_slots
        .iter()
        .filter(|slot| slot.kind == "raster")
        .collect::<Vec<_>>();
    if raster_slots.is_empty() {
        return Err(SparseAtlasRasterEventSkipReason::EmptyRasterSlots);
    }
    raster_slots.sort_by_key(|slot| {
        (
            slot.event_start,
            slot.event_end,
            slot.canvas_y,
            slot.canvas_x,
            slot.tile_y,
            slot.tile_x,
        )
    });

    let mut events = Vec::with_capacity(raster_slots.len());
    for slot in raster_slots {
        let raster = raster_source_for_slot(source_span, slot).ok_or(
            SparseAtlasRasterEventSkipReason::RasterSourceMissing {
                layer_id: slot.layer_id,
                resource_id: slot.resource_id,
            },
        )?;
        let mask = mask_tile_ref_for_raster(rerun_segment, raster, slot)?;
        events.push(clip_gpu::GpuSparseAtlasRasterEvent {
            raster: tile_ref_for_slot(slot),
            source_offset_x: i32::try_from(slot.canvas_x)
                .map_err(|_| SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?,
            source_offset_y: i32::try_from(slot.canvas_y)
                .map_err(|_| SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?,
            opacity: raster.opacity,
            blend_mode: raster.blend_mode,
            mask,
        });
    }

    Ok(SparseAtlasRasterEventSegment {
        ordinal: rerun_segment.ordinal,
        event_ranges: rerun_segment.event_ranges.clone(),
        batches: clip_gpu::split_sparse_atlas_raster_event_batches(&events),
    })
}

fn lower_raster_clipping_run_segment(
    segment: &ReloadDiffSegment,
    rerun_segment: &SparseAtlasRerunSegment,
    sources: &[clip_gpu::GpuNormalStackSource],
) -> Result<SparseAtlasRasterEventSegment, SparseAtlasRasterEventSkipReason> {
    let source_span = segment_source_span(segment, sources)?;
    let [clip_gpu::GpuNormalStackSource::ClippingRun { base, clipped }] = source_span else {
        return Err(SparseAtlasRasterEventSkipReason::SourceSpanOutOfRange);
    };

    let mut events = Vec::new();
    push_raster_events_for_source(rerun_segment, *base, &mut events)?;
    let base_event_count = u32::try_from(events.len())
        .map_err(|_| SparseAtlasRasterEventSkipReason::SourceSpanOutOfRange)?;
    for clipped_source in clipped {
        let clip_gpu::GpuClippedStackSource::Raster(raster) = clipped_source else {
            return Err(SparseAtlasRasterEventSkipReason::NonRasterRun);
        };
        push_raster_events_for_source(rerun_segment, *raster, &mut events)?;
    }
    if events.is_empty() {
        return Err(SparseAtlasRasterEventSkipReason::EmptyRasterSlots);
    }
    if !events_share_executor_atlases(&events) {
        return Err(SparseAtlasRasterEventSkipReason::MixedSparseAtlasKeys);
    }

    Ok(SparseAtlasRasterEventSegment {
        ordinal: rerun_segment.ordinal,
        event_ranges: rerun_segment.event_ranges.clone(),
        batches: vec![
            clip_gpu::GpuSparseAtlasRasterEventBatch::raster_clipping_run(
                events,
                base_event_count,
                base.blend_mode,
            ),
        ],
    })
}

fn segment_source_span<'a>(
    segment: &ReloadDiffSegment,
    sources: &'a [clip_gpu::GpuNormalStackSource],
) -> Result<&'a [clip_gpu::GpuNormalStackSource], SparseAtlasRasterEventSkipReason> {
    let start = usize::try_from(segment.source_start)
        .map_err(|_| SparseAtlasRasterEventSkipReason::SourceSpanOutOfRange)?;
    let end = usize::try_from(segment.source_end)
        .map_err(|_| SparseAtlasRasterEventSkipReason::SourceSpanOutOfRange)?;
    sources
        .get(start..end)
        .ok_or(SparseAtlasRasterEventSkipReason::SourceSpanOutOfRange)
}

fn raster_source_for_slot(
    sources: &[clip_gpu::GpuNormalStackSource],
    slot: &SparseAtlasRerunSlot,
) -> Option<clip_gpu::GpuNormalRasterSource> {
    sources.iter().find_map(|source| {
        let clip_gpu::GpuNormalStackSource::Raster(raster) = source else {
            return None;
        };
        (raster.key.layer_id.0 == slot.layer_id && raster.key.render_mipmap_id == slot.resource_id)
            .then_some(*raster)
    })
}

fn push_raster_events_for_source(
    segment: &SparseAtlasRerunSegment,
    raster: clip_gpu::GpuNormalRasterSource,
    events: &mut Vec<clip_gpu::GpuSparseAtlasRasterEvent>,
) -> Result<(), SparseAtlasRasterEventSkipReason> {
    let mut raster_slots = segment
        .resident_slots
        .iter()
        .filter(|slot| {
            slot.kind == "raster"
                && slot.layer_id == raster.key.layer_id.0
                && slot.resource_id == raster.key.render_mipmap_id
        })
        .collect::<Vec<_>>();
    raster_slots.sort_by_key(|slot| {
        (
            slot.event_start,
            slot.event_end,
            slot.canvas_y,
            slot.canvas_x,
            slot.tile_y,
            slot.tile_x,
        )
    });

    for slot in raster_slots {
        let mask = mask_tile_ref_for_raster(segment, raster, slot)?;
        events.push(clip_gpu::GpuSparseAtlasRasterEvent {
            raster: tile_ref_for_slot(slot),
            source_offset_x: i32::try_from(slot.canvas_x)
                .map_err(|_| SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?,
            source_offset_y: i32::try_from(slot.canvas_y)
                .map_err(|_| SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange)?,
            opacity: raster.opacity,
            blend_mode: raster.blend_mode,
            mask,
        });
    }
    Ok(())
}

fn events_share_executor_atlases(events: &[clip_gpu::GpuSparseAtlasRasterEvent]) -> bool {
    let Some(first) = events.first() else {
        return true;
    };
    let raster_key = first.raster.key;
    let mut mask_key = None;
    for event in events {
        if event.raster.key != raster_key {
            return false;
        }
        let Some(next_mask_key) = event.mask.map(|mask| mask.key) else {
            continue;
        };
        if let Some(current_mask_key) = mask_key {
            if current_mask_key != next_mask_key {
                return false;
            }
        } else {
            mask_key = Some(next_mask_key);
        }
    }
    true
}

fn mask_tile_ref_for_raster(
    segment: &SparseAtlasRerunSegment,
    raster: clip_gpu::GpuNormalRasterSource,
    raster_slot: &SparseAtlasRerunSlot,
) -> Result<Option<clip_gpu::GpuSparseAtlasTileRef>, SparseAtlasRasterEventSkipReason> {
    let Some(mask_key) = raster.mask_key else {
        return Ok(None);
    };
    let mask_slot = segment.resident_slots.iter().find(|slot| {
        slot.kind == "mask"
            && slot.layer_id == mask_key.layer_id.0
            && slot.resource_id == mask_key.mask_mipmap_id
            && slot.canvas_x == raster_slot.canvas_x
            && slot.canvas_y == raster_slot.canvas_y
            && slot.slot.width == raster_slot.slot.width
            && slot.slot.height == raster_slot.slot.height
    });
    mask_slot.map(tile_ref_for_slot).map(Some).ok_or(
        SparseAtlasRasterEventSkipReason::MaskSlotMissing {
            layer_id: mask_key.layer_id.0,
            resource_id: mask_key.mask_mipmap_id,
            canvas_x: raster_slot.canvas_x,
            canvas_y: raster_slot.canvas_y,
        },
    )
}

fn tile_ref_for_slot(slot: &SparseAtlasRerunSlot) -> clip_gpu::GpuSparseAtlasTileRef {
    clip_gpu::GpuSparseAtlasTileRef {
        key: clip_gpu::GpuSparseAtlasTextureKey {
            format: slot.format,
            atlas_id: slot.slot.atlas_id,
        },
        atlas_x: slot.slot.x,
        atlas_y: slot.slot.y,
        size: CanvasSize::new(slot.slot.width, slot.slot.height),
    }
}
