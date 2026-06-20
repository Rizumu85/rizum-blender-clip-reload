use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const TOP_SEGMENT_LIMIT: usize = 16;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RenderProfileSnapshot {
    pub source_selection_ms: u64,
    pub gpu_device_init_ms: u64,
    pub render_program_planning_ms: u64,
    pub event_payload_build_ms: u64,
    pub gpu_pass_encode_ms: u64,
    pub queue_submit_ms: u64,
    pub queue_poll_ms: u64,
    pub gpu_execution_wait_proxy_ms: u64,
    pub readback_copy_ms: u64,
    pub readback_cpu_copy_ms: u64,
    pub patch_payload_extraction_ms: u64,
    pub checkpoint_reconstruction_ms: u64,
    pub sparse_atlas_update_ms: u64,
    pub legacy_barrier_segment_count: u64,
    pub legacy_barrier_segment_ms: u64,
    pub legacy_fallback_segment_count: u64,
    pub legacy_fallback_segment_ms: u64,
    pub tile_local_segment_count: u64,
    pub tile_local_segment_ms: u64,
    pub streaming_pass_count: u64,
    pub queue_submit_count: u64,
    pub readback_count: u64,
    pub checkpoint_cache_hits: u64,
    pub checkpoint_cache_misses: u64,
    pub checkpoint_cache_stores: u64,
    pub checkpoint_cache_evictions: u64,
    pub checkpoint_cache_skipped_over_budget: u64,
    pub top_segments: Vec<RenderProfileSegment>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderProfileSegment {
    pub ordinal: u64,
    pub kind: &'static str,
    pub source_shape: &'static str,
    pub barrier_reason: Option<&'static str>,
    pub elapsed_us: u64,
    pub elapsed_ms: u64,
    pub source_start: u32,
    pub source_end: u32,
    pub first_layer_id: Option<u32>,
    pub target_origin_x: i32,
    pub target_origin_y: i32,
    pub target_width: u32,
    pub target_height: u32,
    pub expected_passes: u32,
    pub tile_events: u32,
    pub legacy_sources: u32,
}

static ENABLED: OnceLock<bool> = OnceLock::new();

static SOURCE_SELECTION_MS: AtomicU64 = AtomicU64::new(0);
static GPU_DEVICE_INIT_MS: AtomicU64 = AtomicU64::new(0);
static RENDER_PROGRAM_PLANNING_MS: AtomicU64 = AtomicU64::new(0);
static EVENT_PAYLOAD_BUILD_MS: AtomicU64 = AtomicU64::new(0);
static GPU_PASS_ENCODE_MS: AtomicU64 = AtomicU64::new(0);
static QUEUE_SUBMIT_MS: AtomicU64 = AtomicU64::new(0);
static QUEUE_POLL_MS: AtomicU64 = AtomicU64::new(0);
static GPU_EXECUTION_WAIT_PROXY_MS: AtomicU64 = AtomicU64::new(0);
static READBACK_COPY_MS: AtomicU64 = AtomicU64::new(0);
static READBACK_CPU_COPY_MS: AtomicU64 = AtomicU64::new(0);
static PATCH_PAYLOAD_EXTRACTION_MS: AtomicU64 = AtomicU64::new(0);
static CHECKPOINT_RECONSTRUCTION_MS: AtomicU64 = AtomicU64::new(0);
static SPARSE_ATLAS_UPDATE_MS: AtomicU64 = AtomicU64::new(0);
static LEGACY_BARRIER_SEGMENT_COUNT: AtomicU64 = AtomicU64::new(0);
static LEGACY_BARRIER_SEGMENT_MS: AtomicU64 = AtomicU64::new(0);
static LEGACY_FALLBACK_SEGMENT_COUNT: AtomicU64 = AtomicU64::new(0);
static LEGACY_FALLBACK_SEGMENT_MS: AtomicU64 = AtomicU64::new(0);
static TILE_LOCAL_SEGMENT_COUNT: AtomicU64 = AtomicU64::new(0);
static TILE_LOCAL_SEGMENT_MS: AtomicU64 = AtomicU64::new(0);
static STREAMING_PASS_COUNT: AtomicU64 = AtomicU64::new(0);
static QUEUE_SUBMIT_COUNT: AtomicU64 = AtomicU64::new(0);
static READBACK_COUNT: AtomicU64 = AtomicU64::new(0);
static CHECKPOINT_CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static CHECKPOINT_CACHE_MISSES: AtomicU64 = AtomicU64::new(0);
static CHECKPOINT_CACHE_STORES: AtomicU64 = AtomicU64::new(0);
static CHECKPOINT_CACHE_EVICTIONS: AtomicU64 = AtomicU64::new(0);
static CHECKPOINT_CACHE_SKIPPED_OVER_BUDGET: AtomicU64 = AtomicU64::new(0);
static SEGMENT_ORDINAL: AtomicU64 = AtomicU64::new(0);
static TOP_SEGMENTS: OnceLock<Mutex<Vec<RenderProfileSegment>>> = OnceLock::new();

pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var_os("RIZUM_CLIP_RENDER_PROFILE").is_some())
}

pub fn reset_if_enabled() {
    if !enabled() {
        return;
    }
    for counter in counters() {
        counter.store(0, Ordering::Relaxed);
    }
    SEGMENT_ORDINAL.store(0, Ordering::Relaxed);
    top_segments()
        .lock()
        .expect("render profile mutex poisoned")
        .clear();
}

pub fn snapshot_if_enabled() -> Option<RenderProfileSnapshot> {
    enabled().then(snapshot)
}

pub fn snapshot() -> RenderProfileSnapshot {
    RenderProfileSnapshot {
        source_selection_ms: SOURCE_SELECTION_MS.load(Ordering::Relaxed),
        gpu_device_init_ms: GPU_DEVICE_INIT_MS.load(Ordering::Relaxed),
        render_program_planning_ms: RENDER_PROGRAM_PLANNING_MS.load(Ordering::Relaxed),
        event_payload_build_ms: EVENT_PAYLOAD_BUILD_MS.load(Ordering::Relaxed),
        gpu_pass_encode_ms: GPU_PASS_ENCODE_MS.load(Ordering::Relaxed),
        queue_submit_ms: QUEUE_SUBMIT_MS.load(Ordering::Relaxed),
        queue_poll_ms: QUEUE_POLL_MS.load(Ordering::Relaxed),
        gpu_execution_wait_proxy_ms: GPU_EXECUTION_WAIT_PROXY_MS.load(Ordering::Relaxed),
        readback_copy_ms: READBACK_COPY_MS.load(Ordering::Relaxed),
        readback_cpu_copy_ms: READBACK_CPU_COPY_MS.load(Ordering::Relaxed),
        patch_payload_extraction_ms: PATCH_PAYLOAD_EXTRACTION_MS.load(Ordering::Relaxed),
        checkpoint_reconstruction_ms: CHECKPOINT_RECONSTRUCTION_MS.load(Ordering::Relaxed),
        sparse_atlas_update_ms: SPARSE_ATLAS_UPDATE_MS.load(Ordering::Relaxed),
        legacy_barrier_segment_count: LEGACY_BARRIER_SEGMENT_COUNT.load(Ordering::Relaxed),
        legacy_barrier_segment_ms: LEGACY_BARRIER_SEGMENT_MS.load(Ordering::Relaxed),
        legacy_fallback_segment_count: LEGACY_FALLBACK_SEGMENT_COUNT.load(Ordering::Relaxed),
        legacy_fallback_segment_ms: LEGACY_FALLBACK_SEGMENT_MS.load(Ordering::Relaxed),
        tile_local_segment_count: TILE_LOCAL_SEGMENT_COUNT.load(Ordering::Relaxed),
        tile_local_segment_ms: TILE_LOCAL_SEGMENT_MS.load(Ordering::Relaxed),
        streaming_pass_count: STREAMING_PASS_COUNT.load(Ordering::Relaxed),
        queue_submit_count: QUEUE_SUBMIT_COUNT.load(Ordering::Relaxed),
        readback_count: READBACK_COUNT.load(Ordering::Relaxed),
        checkpoint_cache_hits: CHECKPOINT_CACHE_HITS.load(Ordering::Relaxed),
        checkpoint_cache_misses: CHECKPOINT_CACHE_MISSES.load(Ordering::Relaxed),
        checkpoint_cache_stores: CHECKPOINT_CACHE_STORES.load(Ordering::Relaxed),
        checkpoint_cache_evictions: CHECKPOINT_CACHE_EVICTIONS.load(Ordering::Relaxed),
        checkpoint_cache_skipped_over_budget: CHECKPOINT_CACHE_SKIPPED_OVER_BUDGET
            .load(Ordering::Relaxed),
        top_segments: top_segments_snapshot(),
    }
}

pub fn record_source_selection(elapsed: Duration) {
    add_duration_ms(&SOURCE_SELECTION_MS, elapsed);
}

pub fn record_gpu_device_init(elapsed: Duration) {
    add_duration_ms(&GPU_DEVICE_INIT_MS, elapsed);
}

pub(crate) fn record_render_program_planning(elapsed: Duration) {
    add_duration_ms(&RENDER_PROGRAM_PLANNING_MS, elapsed);
}

pub(crate) fn record_event_payload_build(elapsed: Duration) {
    add_duration_ms(&EVENT_PAYLOAD_BUILD_MS, elapsed);
}

pub(crate) fn record_gpu_pass_encode(elapsed: Duration) {
    add_duration_ms(&GPU_PASS_ENCODE_MS, elapsed);
}

pub(crate) fn record_queue_submit(elapsed: Duration) {
    if !enabled() {
        return;
    }
    add_duration_ms(&QUEUE_SUBMIT_MS, elapsed);
    add(&QUEUE_SUBMIT_COUNT, 1);
}

pub(crate) fn record_queue_poll(elapsed: Duration) {
    if !enabled() {
        return;
    }
    add_duration_ms(&QUEUE_POLL_MS, elapsed);
    add_duration_ms(&GPU_EXECUTION_WAIT_PROXY_MS, elapsed);
}

pub(crate) fn record_readback_copy(elapsed: Duration) {
    add_duration_ms(&READBACK_COPY_MS, elapsed);
}

pub(crate) fn record_readback_cpu_copy(elapsed: Duration) {
    add_duration_ms(&READBACK_CPU_COPY_MS, elapsed);
}

pub(crate) fn record_readback() {
    if enabled() {
        add(&READBACK_COUNT, 1);
    }
}

pub fn record_patch_payload_extraction(elapsed: Duration) {
    add_duration_ms(&PATCH_PAYLOAD_EXTRACTION_MS, elapsed);
}

pub fn record_checkpoint_reconstruction(elapsed: Duration) {
    add_duration_ms(&CHECKPOINT_RECONSTRUCTION_MS, elapsed);
}

pub fn record_sparse_atlas_update(elapsed: Duration) {
    add_duration_ms(&SPARSE_ATLAS_UPDATE_MS, elapsed);
}

pub(crate) fn record_legacy_barrier_segment(elapsed: Duration) {
    if !enabled() {
        return;
    }
    add(&LEGACY_BARRIER_SEGMENT_COUNT, 1);
    add_duration_ms(&LEGACY_BARRIER_SEGMENT_MS, elapsed);
}

pub(crate) fn record_legacy_fallback_segment(elapsed: Duration) {
    if !enabled() {
        return;
    }
    add(&LEGACY_FALLBACK_SEGMENT_COUNT, 1);
    add_duration_ms(&LEGACY_FALLBACK_SEGMENT_MS, elapsed);
}

pub(crate) fn record_tile_local_segment(elapsed: Duration) {
    if !enabled() {
        return;
    }
    add(&TILE_LOCAL_SEGMENT_COUNT, 1);
    add_duration_ms(&TILE_LOCAL_SEGMENT_MS, elapsed);
}

pub(crate) struct RenderProfileSegmentRecord {
    pub kind: &'static str,
    pub source_shape: &'static str,
    pub legacy_reason: Option<&'static str>,
    pub elapsed: Duration,
    pub source_start: usize,
    pub source_end: usize,
    pub first_layer_id: Option<u32>,
    pub target_origin: (i32, i32),
    pub target_size: clip_model::CanvasSize,
    pub expected_passes: u32,
    pub tile_events: u32,
    pub legacy_sources: u32,
}

pub(crate) fn record_segment(record: RenderProfileSegmentRecord) {
    if !enabled() {
        return;
    }
    let segment = RenderProfileSegment {
        ordinal: SEGMENT_ORDINAL.fetch_add(1, Ordering::Relaxed),
        kind: record.kind,
        source_shape: record.source_shape,
        barrier_reason: record.legacy_reason,
        elapsed_us: record.elapsed.as_micros().try_into().unwrap_or(u64::MAX),
        elapsed_ms: record.elapsed.as_millis().try_into().unwrap_or(u64::MAX),
        source_start: usize_to_u32(record.source_start),
        source_end: usize_to_u32(record.source_end),
        first_layer_id: record.first_layer_id,
        target_origin_x: record.target_origin.0,
        target_origin_y: record.target_origin.1,
        target_width: record.target_size.width,
        target_height: record.target_size.height,
        expected_passes: record.expected_passes,
        tile_events: record.tile_events,
        legacy_sources: record.legacy_sources,
    };
    record_top_segment(top_segments(), segment);
}

pub(crate) fn record_streaming_pass() {
    if enabled() {
        add(&STREAMING_PASS_COUNT, 1);
    }
}

pub fn record_checkpoint_cache_hit() {
    if enabled() {
        add(&CHECKPOINT_CACHE_HITS, 1);
    }
}

pub fn record_checkpoint_cache_miss() {
    if enabled() {
        add(&CHECKPOINT_CACHE_MISSES, 1);
    }
}

pub fn record_checkpoint_cache_store() {
    if enabled() {
        add(&CHECKPOINT_CACHE_STORES, 1);
    }
}

pub fn record_checkpoint_cache_eviction(count: usize) {
    if enabled() {
        add(&CHECKPOINT_CACHE_EVICTIONS, count as u64);
    }
}

pub fn record_checkpoint_cache_skipped_over_budget() {
    if enabled() {
        add(&CHECKPOINT_CACHE_SKIPPED_OVER_BUDGET, 1);
    }
}

fn counters() -> [&'static AtomicU64; 27] {
    [
        &SOURCE_SELECTION_MS,
        &GPU_DEVICE_INIT_MS,
        &RENDER_PROGRAM_PLANNING_MS,
        &EVENT_PAYLOAD_BUILD_MS,
        &GPU_PASS_ENCODE_MS,
        &QUEUE_SUBMIT_MS,
        &QUEUE_POLL_MS,
        &GPU_EXECUTION_WAIT_PROXY_MS,
        &READBACK_COPY_MS,
        &READBACK_CPU_COPY_MS,
        &PATCH_PAYLOAD_EXTRACTION_MS,
        &CHECKPOINT_RECONSTRUCTION_MS,
        &SPARSE_ATLAS_UPDATE_MS,
        &LEGACY_BARRIER_SEGMENT_COUNT,
        &LEGACY_BARRIER_SEGMENT_MS,
        &LEGACY_FALLBACK_SEGMENT_COUNT,
        &LEGACY_FALLBACK_SEGMENT_MS,
        &TILE_LOCAL_SEGMENT_COUNT,
        &TILE_LOCAL_SEGMENT_MS,
        &STREAMING_PASS_COUNT,
        &QUEUE_SUBMIT_COUNT,
        &READBACK_COUNT,
        &CHECKPOINT_CACHE_HITS,
        &CHECKPOINT_CACHE_MISSES,
        &CHECKPOINT_CACHE_STORES,
        &CHECKPOINT_CACHE_EVICTIONS,
        &CHECKPOINT_CACHE_SKIPPED_OVER_BUDGET,
    ]
}

fn add(counter: &AtomicU64, value: u64) {
    counter.fetch_add(value, Ordering::Relaxed);
}

fn add_duration_ms(counter: &AtomicU64, elapsed: Duration) {
    if enabled() {
        add(counter, elapsed.as_millis().try_into().unwrap_or(u64::MAX));
    }
}

fn top_segments() -> &'static Mutex<Vec<RenderProfileSegment>> {
    TOP_SEGMENTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn top_segments_snapshot() -> Vec<RenderProfileSegment> {
    if !enabled() {
        return Vec::new();
    }
    top_segments()
        .lock()
        .expect("render profile mutex poisoned")
        .clone()
}

fn record_top_segment(storage: &Mutex<Vec<RenderProfileSegment>>, segment: RenderProfileSegment) {
    let mut segments = storage.lock().expect("render profile mutex poisoned");
    insert_top_segment(&mut segments, segment, TOP_SEGMENT_LIMIT);
}

fn insert_top_segment(
    segments: &mut Vec<RenderProfileSegment>,
    segment: RenderProfileSegment,
    limit: usize,
) {
    segments.push(segment);
    segments.sort_by(|left, right| {
        right
            .elapsed_us
            .cmp(&left.elapsed_us)
            .then_with(|| left.ordinal.cmp(&right.ordinal))
    });
    segments.truncate(limit);
}

fn usize_to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::{RenderProfileSegment, insert_top_segment};

    #[test]
    fn insert_top_segment_keeps_slowest_in_deterministic_order() {
        let mut segments = Vec::new();
        for (ordinal, elapsed_us) in [(0, 10), (1, 30), (2, 20), (3, 30)] {
            insert_top_segment(
                &mut segments,
                RenderProfileSegment {
                    ordinal,
                    kind: "RasterRun",
                    source_shape: "Raster",
                    barrier_reason: None,
                    elapsed_us,
                    elapsed_ms: 0,
                    source_start: ordinal as u32,
                    source_end: ordinal as u32 + 1,
                    first_layer_id: Some(ordinal as u32),
                    target_origin_x: 0,
                    target_origin_y: 0,
                    target_width: 1,
                    target_height: 1,
                    expected_passes: 1,
                    tile_events: 1,
                    legacy_sources: 0,
                },
                3,
            );
        }

        assert_eq!(
            segments
                .iter()
                .map(|segment| segment.ordinal)
                .collect::<Vec<_>>(),
            vec![1, 3, 2]
        );
    }
}
