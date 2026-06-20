use clip_model::CanvasSize;

use crate::render_profile;
use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::CanvasRect;
use crate::stream_program::RenderSegment;
use crate::stream_tile_silo_plan::{TILE_SIZE, source_bounds, source_local_bounds};
use crate::{GpuClippedStackSource, GpuNormalRasterSource, GpuNormalStackSource};

#[derive(Default)]
struct SegmentDrilldownStats {
    child_source_count: u32,
    nested_container_through_count: u32,
    raster_source_count: u32,
    mask_count: u32,
    source_tile_intersections: u32,
    active_canvas_tile_count: u32,
    max_events_per_dirty_tile: u32,
    event_sources_outside_target_rect: bool,
    tile_counts: Vec<u32>,
    tile_cols: u32,
}

impl SegmentDrilldownStats {
    fn new(target_size: CanvasSize) -> Self {
        let tile_cols = div_ceil_u32(target_size.width, TILE_SIZE);
        let tile_rows = div_ceil_u32(target_size.height, TILE_SIZE);
        let tile_count = usize::try_from(u64::from(tile_cols) * u64::from(tile_rows)).unwrap_or(0);
        Self {
            tile_counts: vec![0; tile_count],
            tile_cols,
            ..Self::default()
        }
    }

    fn count_source_node(&mut self) {
        self.child_source_count = self.child_source_count.saturating_add(1);
    }

    fn count_scope_node(&mut self) {
        self.nested_container_through_count = self.nested_container_through_count.saturating_add(1);
    }

    fn count_mask(&mut self) {
        self.mask_count = self.mask_count.saturating_add(1);
    }

    fn record_raster_local_bounds(&mut self, bounds: CanvasRect) {
        self.raster_source_count = self.raster_source_count.saturating_add(1);
        if self.tile_cols == 0 || self.tile_counts.is_empty() {
            return;
        }
        let tile_x0 = bounds.x / TILE_SIZE;
        let tile_y0 = bounds.y / TILE_SIZE;
        let tile_x1 = bounds.x.saturating_add(bounds.width).saturating_sub(1) / TILE_SIZE;
        let tile_y1 = bounds.y.saturating_add(bounds.height).saturating_sub(1) / TILE_SIZE;
        for tile_y in tile_y0..=tile_y1 {
            for tile_x in tile_x0..=tile_x1 {
                let Some(tile_index) = u64::from(tile_y)
                    .checked_mul(u64::from(self.tile_cols))
                    .and_then(|row| row.checked_add(u64::from(tile_x)))
                    .and_then(|index| usize::try_from(index).ok())
                else {
                    continue;
                };
                let Some(count) = self.tile_counts.get_mut(tile_index) else {
                    continue;
                };
                if *count == 0 {
                    self.active_canvas_tile_count = self.active_canvas_tile_count.saturating_add(1);
                }
                *count = count.saturating_add(1);
                self.source_tile_intersections = self.source_tile_intersections.saturating_add(1);
                self.max_events_per_dirty_tile = self.max_events_per_dirty_tile.max(*count);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn record_profiled_segment<P>(
    provider: &P,
    output_size: CanvasSize,
    segment: &RenderSegment,
    sources: &[GpuNormalStackSource],
    target_origin: (i32, i32),
    target_size: CanvasSize,
    elapsed: std::time::Duration,
    counters: render_profile::RenderProfileSegmentCounters,
    kind: &'static str,
    reason_text: Option<&'static str>,
) where
    P: GpuNormalStackResourceProvider,
{
    if !render_profile::enabled() {
        return;
    }
    let drilldown = segment_drilldown(
        provider,
        output_size,
        target_origin,
        target_size,
        sources.get(segment.source_range.clone()).unwrap_or(&[]),
    );
    let is_legacy = matches!(kind, "LegacySource" | "LegacyFallback");
    render_profile::record_segment(render_profile::RenderProfileSegmentRecord {
        kind,
        source_shape: source_shape_name(sources, segment.source_range.start),
        legacy_reason: reason_text,
        elapsed,
        counters,
        source_start: segment.source_range.start,
        source_end: segment.source_range.end,
        first_layer_id: first_layer_id_for_range(sources, segment.source_range.clone()),
        target_origin,
        target_size,
        expected_passes: segment.cost_hint.expected_passes,
        tile_events: segment.cost_hint.tile_events,
        legacy_sources: segment.cost_hint.legacy_sources,
        raster_source_count: drilldown.raster_source_count,
        compressed_tile_event_count: drilldown.source_tile_intersections,
        active_canvas_tile_count: drilldown.active_canvas_tile_count,
        max_events_per_dirty_tile: drilldown.max_events_per_dirty_tile,
        event_sources_outside_target_rect: drilldown.event_sources_outside_target_rect,
        uses_sparse_resident_atlas: false,
        uses_per_run_atlas: kind_uses_per_run_atlas(kind),
        gpu_batches: segment.cost_hint.expected_passes,
        child_source_count: drilldown.child_source_count,
        nested_container_through_count: drilldown.nested_container_through_count,
        barrier_raster_count: if is_legacy {
            drilldown.raster_source_count
        } else {
            0
        },
        barrier_mask_count: if is_legacy { drilldown.mask_count } else { 0 },
        legacy_subpass_count_estimate: if is_legacy {
            drilldown.child_source_count
        } else {
            0
        },
        legacy_direct_subpass_count: if is_legacy {
            u32::try_from(segment.source_range.len()).unwrap_or(u32::MAX)
        } else {
            0
        },
        largest_legacy_direct_subpass_us: 0,
    });
}

fn segment_drilldown<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    sources: &[GpuNormalStackSource],
) -> SegmentDrilldownStats
where
    P: GpuNormalStackResourceProvider,
{
    let mut stats = SegmentDrilldownStats::new(target_size);
    for source in sources {
        collect_source_drilldown(
            provider,
            output_size,
            target_origin,
            target_size,
            source,
            &mut stats,
        );
    }
    stats
}

fn collect_source_drilldown<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    source: &GpuNormalStackSource,
    stats: &mut SegmentDrilldownStats,
) where
    P: GpuNormalStackResourceProvider,
{
    stats.count_source_node();
    match source {
        GpuNormalStackSource::Raster(raster) => {
            collect_raster_drilldown(
                provider,
                output_size,
                target_origin,
                target_size,
                *raster,
                stats,
            );
        }
        GpuNormalStackSource::ClippingRun { base, clipped } => {
            collect_raster_drilldown(
                provider,
                output_size,
                target_origin,
                target_size,
                *base,
                stats,
            );
            for clipped_source in clipped {
                collect_clipped_source_drilldown(
                    provider,
                    output_size,
                    target_origin,
                    target_size,
                    clipped_source,
                    stats,
                );
            }
        }
        GpuNormalStackSource::ContainerClippingRun {
            children,
            mask_key,
            clipped,
            ..
        } => {
            stats.count_scope_node();
            if mask_key.is_some() {
                stats.count_mask();
            }
            for child in children {
                collect_source_drilldown(
                    provider,
                    output_size,
                    target_origin,
                    target_size,
                    child,
                    stats,
                );
            }
            for clipped_source in clipped {
                collect_clipped_source_drilldown(
                    provider,
                    output_size,
                    target_origin,
                    target_size,
                    clipped_source,
                    stats,
                );
            }
        }
        GpuNormalStackSource::Container {
            children, mask_key, ..
        }
        | GpuNormalStackSource::ThroughGroup {
            children, mask_key, ..
        } => {
            stats.count_scope_node();
            if mask_key.is_some() {
                stats.count_mask();
            }
            for child in children {
                collect_source_drilldown(
                    provider,
                    output_size,
                    target_origin,
                    target_size,
                    child,
                    stats,
                );
            }
        }
        GpuNormalStackSource::SolidColor { .. } => {}
        GpuNormalStackSource::LutFilter { mask_key, .. } => {
            if mask_key.is_some() {
                stats.count_mask();
            }
        }
    }
}

fn collect_clipped_source_drilldown<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    source: &GpuClippedStackSource,
    stats: &mut SegmentDrilldownStats,
) where
    P: GpuNormalStackResourceProvider,
{
    stats.count_source_node();
    match source {
        GpuClippedStackSource::Raster(raster) => {
            collect_raster_drilldown(
                provider,
                output_size,
                target_origin,
                target_size,
                *raster,
                stats,
            );
        }
        GpuClippedStackSource::Container {
            children, mask_key, ..
        } => {
            stats.count_scope_node();
            if mask_key.is_some() {
                stats.count_mask();
            }
            for child in children {
                collect_source_drilldown(
                    provider,
                    output_size,
                    target_origin,
                    target_size,
                    child,
                    stats,
                );
            }
        }
    }
}

fn collect_raster_drilldown<P>(
    provider: &P,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    raster: GpuNormalRasterSource,
    stats: &mut SegmentDrilldownStats,
) where
    P: GpuNormalStackResourceProvider,
{
    if raster.mask_key.is_some() {
        stats.count_mask();
    }
    let Some(size) = provider.raster_resource_size(raster) else {
        stats.raster_source_count = stats.raster_source_count.saturating_add(1);
        return;
    };
    let offset = provider
        .raster_resource_offset(raster)
        .unwrap_or((raster.offset_x, raster.offset_y));
    let Some(bounds) = source_bounds(offset, size, output_size) else {
        stats.raster_source_count = stats.raster_source_count.saturating_add(1);
        return;
    };
    if !rect_within_target(bounds, target_origin, target_size) {
        stats.event_sources_outside_target_rect = true;
    }
    let Some(local_bounds) = source_local_bounds(offset, size, target_origin, target_size) else {
        stats.raster_source_count = stats.raster_source_count.saturating_add(1);
        return;
    };
    stats.record_raster_local_bounds(local_bounds);
}

fn rect_within_target(
    rect: CanvasRect,
    target_origin: (i32, i32),
    target_size: CanvasSize,
) -> bool {
    let target_x0 = i64::from(target_origin.0);
    let target_y0 = i64::from(target_origin.1);
    let target_x1 = target_x0 + i64::from(target_size.width);
    let target_y1 = target_y0 + i64::from(target_size.height);
    let rect_x0 = i64::from(rect.x);
    let rect_y0 = i64::from(rect.y);
    let rect_x1 = rect_x0 + i64::from(rect.width);
    let rect_y1 = rect_y0 + i64::from(rect.height);
    rect_x0 >= target_x0 && rect_y0 >= target_y0 && rect_x1 <= target_x1 && rect_y1 <= target_y1
}

fn kind_uses_per_run_atlas(kind: &str) -> bool {
    matches!(
        kind,
        "RasterClippingRun"
            | "RasterRun"
            | "RasterFilterRun"
            | "PointFilterRun"
            | "SimpleContainerScope"
            | "SimpleThroughScope"
    )
}

fn div_ceil_u32(value: u32, divisor: u32) -> u32 {
    if divisor == 0 {
        return 0;
    }
    value.saturating_add(divisor - 1) / divisor
}

fn source_shape_name(sources: &[GpuNormalStackSource], source_index: usize) -> &'static str {
    match sources.get(source_index) {
        Some(GpuNormalStackSource::Raster(_)) => "Raster",
        Some(GpuNormalStackSource::ClippingRun { .. }) => "ClippingRun",
        Some(GpuNormalStackSource::ContainerClippingRun { .. }) => "ContainerClippingRun",
        Some(GpuNormalStackSource::Container { .. }) => "Container",
        Some(GpuNormalStackSource::ThroughGroup { .. }) => "ThroughGroup",
        Some(GpuNormalStackSource::SolidColor { .. }) => "SolidColor",
        Some(GpuNormalStackSource::LutFilter { .. }) => "LutFilter",
        None => "Unknown",
    }
}

fn first_layer_id_for_range(
    sources: &[GpuNormalStackSource],
    source_range: std::ops::Range<usize>,
) -> Option<u32> {
    sources
        .get(source_range)
        .and_then(|range| range.iter().find_map(first_layer_id_for_source))
}

fn first_layer_id_for_source(source: &GpuNormalStackSource) -> Option<u32> {
    match source {
        GpuNormalStackSource::Raster(raster) => Some(raster.key.layer_id.0),
        GpuNormalStackSource::ClippingRun { base, .. } => Some(base.key.layer_id.0),
        GpuNormalStackSource::ContainerClippingRun { children, .. }
        | GpuNormalStackSource::Container { children, .. }
        | GpuNormalStackSource::ThroughGroup { children, .. } => {
            children.iter().find_map(first_layer_id_for_source)
        }
        GpuNormalStackSource::SolidColor { .. } | GpuNormalStackSource::LutFilter { .. } => None,
    }
}
