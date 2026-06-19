use crate::reload_diff::ReloadDirtySegmentEventRange;

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
    FilterMaskNotLowered {
        layer_id: u32,
        resource_id: u32,
    },
    ScopeMaskNotLowered {
        layer_id: u32,
        resource_id: u32,
    },
    InvalidPointFilter,
    MixedSparseAtlasKeys,
    CanvasCoordinateOutOfRange,
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
            SparseAtlasRasterEventSkipReason::FilterMaskNotLowered {
                layer_id,
                resource_id,
            } => Self::FilterMaskNotLowered {
                layer_id,
                resource_id,
            },
            SparseAtlasRasterEventSkipReason::ScopeMaskNotLowered {
                layer_id,
                resource_id,
            } => Self::ScopeMaskNotLowered {
                layer_id,
                resource_id,
            },
            SparseAtlasRasterEventSkipReason::InvalidPointFilter => Self::InvalidPointFilter,
            SparseAtlasRasterEventSkipReason::MixedSparseAtlasKeys => Self::MixedSparseAtlasKeys,
            SparseAtlasRasterEventSkipReason::CanvasCoordinateOutOfRange => {
                Self::CanvasCoordinateOutOfRange
            }
        }
    }
}
