use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
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
