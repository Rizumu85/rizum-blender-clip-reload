use std::collections::HashSet;

use clip_gpu::{GpuClippedStackSource, GpuMaskResourceKey, GpuNormalStackSource};
use clip_model::CanvasSize;

use crate::results::NativeTileSiloEstimateResult;
use crate::stack_plan::GpuRenderStackSelection;
use crate::tile_silo_occupancy;
use crate::tile_silo_options::tile_silo_options;
use crate::{ClipSession, RuntimeError, source_crop};

impl ClipSession {
    pub fn estimate_tile_silo_plan(
        &self,
        tile_size: u32,
    ) -> Result<NativeTileSiloEstimateResult, RuntimeError> {
        if tile_size == 0 {
            return Err(RuntimeError::InvalidTileSize);
        }

        let selection = self.select_gpu_normal_render_stack(tile_silo_options())?;
        let GpuRenderStackSelection {
            sources,
            resource_plan: _,
            unsupported,
        } = selection;
        let mut estimate =
            TileSiloEstimateBuilder::new(self, tile_size, sources.len(), unsupported.len())?;
        estimate.walk_sources(&sources, &mut SegmentState::default())?;
        Ok(estimate.finish())
    }
}

struct TileSiloEstimateBuilder<'a> {
    session: &'a ClipSession,
    result: NativeTileSiloEstimateResult,
    raster_events_by_tile: Vec<u32>,
    compressed_raster_events_by_tile: Vec<u32>,
    seen_rasters: HashSet<clip_gpu::GpuRasterResourceKey>,
    seen_masks: HashSet<GpuMaskResourceKey>,
}

impl<'a> TileSiloEstimateBuilder<'a> {
    fn new(
        session: &'a ClipSession,
        tile_size: u32,
        top_level_source_count: usize,
        unsupported_count: usize,
    ) -> Result<Self, RuntimeError> {
        let canvas = session.summary.canvas;
        let canvas_tiles_x = canvas.width.div_ceil(tile_size);
        let canvas_tiles_y = canvas.height.div_ceil(tile_size);
        let canvas_tile_count = u64::from(canvas_tiles_x) * u64::from(canvas_tiles_y);
        let tile_slots = usize::try_from(canvas_tile_count)
            .map_err(|_| clip_file::ClipFileError::TileSizeOverflow)?;

        Ok(Self {
            session,
            result: NativeTileSiloEstimateResult {
                canvas,
                tile_size,
                canvas_tiles_x,
                canvas_tiles_y,
                canvas_tile_count,
                top_level_source_count,
                total_source_event_count: 0,
                raster_source_count: 0,
                clipped_raster_source_count: 0,
                solid_source_count: 0,
                lut_filter_count: 0,
                clipping_run_count: 0,
                container_clipping_run_count: 0,
                container_count: 0,
                clipped_container_count: 0,
                through_group_count: 0,
                mask_reference_count: 0,
                unique_raster_resource_count: 0,
                unique_mask_resource_count: 0,
                raster_tile_slot_count: 0,
                mask_tile_slot_count: 0,
                raster_compressed_tile_slot_count: 0,
                raster_empty_tile_slot_count: 0,
                mask_compressed_tile_slot_count: 0,
                mask_empty_tile_slot_count: 0,
                external_compressed_bytes: 0,
                raster_tile_event_count: 0,
                compressed_raster_tile_event_count: 0,
                solid_tile_event_count: 0,
                active_canvas_tile_count: 0,
                max_raster_events_per_tile: 0,
                mean_raster_events_per_active_tile: 0.0,
                active_compressed_canvas_tile_count: 0,
                max_compressed_raster_events_per_tile: 0,
                mean_compressed_raster_events_per_active_tile: 0.0,
                collapsible_segment_count: 0,
                collapsible_source_event_count: 0,
                semantic_barrier_count: 0,
                unsupported_count,
            },
            raster_events_by_tile: vec![0; tile_slots],
            compressed_raster_events_by_tile: vec![0; tile_slots],
            seen_rasters: HashSet::new(),
            seen_masks: HashSet::new(),
        })
    }

    fn walk_sources(
        &mut self,
        sources: &[GpuNormalStackSource],
        segment: &mut SegmentState,
    ) -> Result<(), RuntimeError> {
        for source in sources {
            self.walk_source(source, segment)?;
        }
        segment.close();
        Ok(())
    }

    fn walk_source(
        &mut self,
        source: &GpuNormalStackSource,
        segment: &mut SegmentState,
    ) -> Result<(), RuntimeError> {
        self.result.total_source_event_count += 1;
        match source {
            GpuNormalStackSource::Raster(source) => {
                self.add_raster(*source, false)?;
                self.add_mask(source.mask_key)?;
                self.add_collapsible_events(segment, 1);
            }
            GpuNormalStackSource::ClippingRun { base, clipped } => {
                self.result.clipping_run_count += 1;
                self.add_raster(*base, false)?;
                self.add_mask(base.mask_key)?;

                let mut clipped_raster_events = 0;
                let has_clipped_container = clipped
                    .iter()
                    .any(|source| matches!(source, GpuClippedStackSource::Container { .. }));
                for clipped_source in clipped {
                    match clipped_source {
                        GpuClippedStackSource::Raster(raster) => {
                            self.result.total_source_event_count += 1;
                            self.add_raster(*raster, true)?;
                            self.add_mask(raster.mask_key)?;
                            clipped_raster_events += 1;
                        }
                        GpuClippedStackSource::Container {
                            children, mask_key, ..
                        } => {
                            self.result.total_source_event_count += 1;
                            self.result.clipped_container_count += 1;
                            self.add_mask(*mask_key)?;
                            self.result.semantic_barrier_count += 1;
                            segment.close();
                            self.walk_sources(children, &mut SegmentState::default())?;
                        }
                    }
                }

                if has_clipped_container {
                    self.result.semantic_barrier_count += 1;
                    segment.close();
                } else {
                    self.add_collapsible_events(segment, 1 + clipped_raster_events);
                }
            }
            GpuNormalStackSource::ContainerClippingRun {
                children,
                mask_key,
                clipped,
                ..
            } => {
                self.result.container_clipping_run_count += 1;
                self.result.semantic_barrier_count += 1;
                self.add_mask(*mask_key)?;
                segment.close();
                self.walk_sources(children, &mut SegmentState::default())?;
                self.walk_clipped_sources_as_barriers(clipped)?;
            }
            GpuNormalStackSource::Container {
                children, mask_key, ..
            } => {
                self.result.container_count += 1;
                self.result.semantic_barrier_count += 1;
                self.add_mask(*mask_key)?;
                segment.close();
                self.walk_sources(children, &mut SegmentState::default())?;
            }
            GpuNormalStackSource::ThroughGroup {
                children, mask_key, ..
            } => {
                self.result.through_group_count += 1;
                self.result.semantic_barrier_count += 1;
                self.add_mask(*mask_key)?;
                segment.close();
                self.walk_sources(children, &mut SegmentState::default())?;
            }
            GpuNormalStackSource::SolidColor { opacity, .. } => {
                self.result.solid_source_count += 1;
                if *opacity > 0.0 {
                    self.result.solid_tile_event_count += self.result.canvas_tile_count;
                    self.add_collapsible_events(segment, 1);
                }
            }
            GpuNormalStackSource::LutFilter { mask_key, .. } => {
                self.result.lut_filter_count += 1;
                self.result.semantic_barrier_count += 1;
                self.add_mask(*mask_key)?;
                segment.close();
            }
        }
        Ok(())
    }

    fn walk_clipped_sources_as_barriers(
        &mut self,
        clipped: &[GpuClippedStackSource],
    ) -> Result<(), RuntimeError> {
        for clipped_source in clipped {
            match clipped_source {
                GpuClippedStackSource::Raster(raster) => {
                    self.result.total_source_event_count += 1;
                    self.add_raster(*raster, true)?;
                    self.add_mask(raster.mask_key)?;
                }
                GpuClippedStackSource::Container {
                    children, mask_key, ..
                } => {
                    self.result.total_source_event_count += 1;
                    self.result.clipped_container_count += 1;
                    self.result.semantic_barrier_count += 1;
                    self.add_mask(*mask_key)?;
                    self.walk_sources(children, &mut SegmentState::default())?;
                }
            }
        }
        Ok(())
    }

    fn add_raster(
        &mut self,
        source: clip_gpu::GpuNormalRasterSource,
        clipped: bool,
    ) -> Result<(), RuntimeError> {
        self.result.raster_source_count += 1;
        if clipped {
            self.result.clipped_raster_source_count += 1;
        }

        let metadata = self
            .session
            .raster_sources
            .get(&source.key.layer_id)
            .ok_or(clip_file::ClipFileError::InvalidMetadata(
                "missing planned raster source",
            ))?;
        let tile_count = self.source_tile_count(
            metadata.pixel_size,
            metadata.offset_x,
            metadata.offset_y,
            true,
        )?;
        self.result.raster_tile_event_count += tile_count;
        let block_inspection = tile_silo_occupancy::inspect_raster_blocks(self.session, metadata)?;
        tile_silo_occupancy::add_compressed_raster_tile_events(
            &mut self.compressed_raster_events_by_tile,
            self.result.canvas,
            self.result.canvas_tiles_x,
            self.result.tile_size,
            metadata,
            &block_inspection.compressed_tiles,
        )?;
        if self.seen_rasters.insert(source.key) {
            self.result.unique_raster_resource_count += 1;
            self.result.raster_tile_slot_count += tile_count;
            self.add_raster_block_stats(block_inspection.stats);
        }
        Ok(())
    }

    fn add_mask(&mut self, key: Option<GpuMaskResourceKey>) -> Result<(), RuntimeError> {
        let Some(key) = key else {
            return Ok(());
        };
        self.result.mask_reference_count += 1;
        if !self.seen_masks.insert(key) {
            return Ok(());
        }

        let metadata = self.session.mask_sources.get(&key.layer_id).ok_or(
            clip_file::ClipFileError::LayerHasNoMask {
                layer_id: key.layer_id,
            },
        )?;
        self.result.unique_mask_resource_count += 1;
        self.result.mask_tile_slot_count += self.source_tile_count(
            metadata.pixel_size,
            metadata.offset_x,
            metadata.offset_y,
            false,
        )?;
        let stats = tile_silo_occupancy::inspect_mask_block_stats(self.session, metadata)?;
        self.result.mask_compressed_tile_slot_count += stats.compressed_block_count as u64;
        self.result.mask_empty_tile_slot_count += stats.empty_block_count as u64;
        self.result.external_compressed_bytes += stats.compressed_bytes;
        Ok(())
    }

    fn add_raster_block_stats(&mut self, stats: clip_file::external::ExternalTileBlockStats) {
        self.result.raster_compressed_tile_slot_count += stats.compressed_block_count as u64;
        self.result.raster_empty_tile_slot_count += stats.empty_block_count as u64;
        self.result.external_compressed_bytes += stats.compressed_bytes;
    }

    fn source_tile_count(
        &mut self,
        source_size: CanvasSize,
        offset_x: i32,
        offset_y: i32,
        count_raster_events: bool,
    ) -> Result<u64, RuntimeError> {
        let Some(visible) = source_crop::visible_raster_source_decode_region(
            source_size,
            offset_x,
            offset_y,
            self.result.canvas,
        )?
        else {
            return Ok(0);
        };

        let tile_x0 = visible.offset_x as u32 / self.result.tile_size;
        let tile_y0 = visible.offset_y as u32 / self.result.tile_size;
        let tile_x1 =
            (visible.offset_x as u32 + visible.source_rect.width - 1) / self.result.tile_size;
        let tile_y1 =
            (visible.offset_y as u32 + visible.source_rect.height - 1) / self.result.tile_size;
        let tile_count = u64::from(tile_x1 - tile_x0 + 1) * u64::from(tile_y1 - tile_y0 + 1);

        if count_raster_events {
            for y in tile_y0..=tile_y1 {
                for x in tile_x0..=tile_x1 {
                    let index = usize::try_from(
                        u64::from(y) * u64::from(self.result.canvas_tiles_x) + u64::from(x),
                    )
                    .map_err(|_| clip_file::ClipFileError::TileSizeOverflow)?;
                    self.raster_events_by_tile[index] += 1;
                }
            }
        }

        Ok(tile_count)
    }

    fn add_collapsible_events(&mut self, segment: &mut SegmentState, count: usize) {
        if count == 0 {
            return;
        }
        if !segment.open {
            segment.open = true;
            self.result.collapsible_segment_count += 1;
        }
        self.result.collapsible_source_event_count += count;
    }

    fn finish(mut self) -> NativeTileSiloEstimateResult {
        let mut active_event_total = 0u64;
        for events in self.raster_events_by_tile {
            if events == 0 {
                continue;
            }
            self.result.active_canvas_tile_count += 1;
            self.result.max_raster_events_per_tile =
                self.result.max_raster_events_per_tile.max(events);
            active_event_total += u64::from(events);
        }
        if self.result.active_canvas_tile_count > 0 {
            self.result.mean_raster_events_per_active_tile =
                active_event_total as f64 / self.result.active_canvas_tile_count as f64;
        }

        let mut active_compressed_event_total = 0u64;
        for events in self.compressed_raster_events_by_tile {
            if events == 0 {
                continue;
            }
            self.result.active_compressed_canvas_tile_count += 1;
            self.result.max_compressed_raster_events_per_tile = self
                .result
                .max_compressed_raster_events_per_tile
                .max(events);
            active_compressed_event_total += u64::from(events);
        }
        self.result.compressed_raster_tile_event_count = active_compressed_event_total;
        if self.result.active_compressed_canvas_tile_count > 0 {
            self.result.mean_compressed_raster_events_per_active_tile =
                active_compressed_event_total as f64
                    / self.result.active_compressed_canvas_tile_count as f64;
        }
        self.result
    }
}

#[derive(Default)]
struct SegmentState {
    open: bool,
}

impl SegmentState {
    fn close(&mut self) {
        self.open = false;
    }
}

#[cfg(test)]
#[path = "tile_silo_estimate_tests.rs"]
mod tests;
