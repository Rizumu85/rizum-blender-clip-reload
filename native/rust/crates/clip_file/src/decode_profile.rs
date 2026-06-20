use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DecodeTileKind {
    Raster,
    Mask,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DecodeProfileSnapshot {
    pub selected_raster_tiles: u64,
    pub selected_mask_tiles: u64,
    pub skipped_empty_raster_tiles: u64,
    pub skipped_empty_mask_tiles: u64,
    pub compressed_bytes_read: u64,
    pub zlib_inflate_ms: u64,
    pub tile_swizzle_rgba_ms: u64,
    pub mask_r8_decode_ms: u64,
    pub mask_crop_ms: u64,
    pub atlas_chunk_build_ms: u64,
    pub sparse_atlas_pool_update_ms: u64,
    pub region_patch_render_ms: u64,
}

static ENABLED: OnceLock<bool> = OnceLock::new();

static SELECTED_RASTER_TILES: AtomicU64 = AtomicU64::new(0);
static SELECTED_MASK_TILES: AtomicU64 = AtomicU64::new(0);
static SKIPPED_EMPTY_RASTER_TILES: AtomicU64 = AtomicU64::new(0);
static SKIPPED_EMPTY_MASK_TILES: AtomicU64 = AtomicU64::new(0);
static COMPRESSED_BYTES_READ: AtomicU64 = AtomicU64::new(0);
static ZLIB_INFLATE_MS: AtomicU64 = AtomicU64::new(0);
static TILE_SWIZZLE_RGBA_MS: AtomicU64 = AtomicU64::new(0);
static MASK_R8_DECODE_MS: AtomicU64 = AtomicU64::new(0);
static MASK_CROP_MS: AtomicU64 = AtomicU64::new(0);
static ATLAS_CHUNK_BUILD_MS: AtomicU64 = AtomicU64::new(0);
static SPARSE_ATLAS_POOL_UPDATE_MS: AtomicU64 = AtomicU64::new(0);
static REGION_PATCH_RENDER_MS: AtomicU64 = AtomicU64::new(0);

pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var_os("RIZUM_CLIP_DECODE_PROFILE").is_some())
}

pub fn reset_if_enabled() {
    if !enabled() {
        return;
    }
    for counter in counters() {
        counter.store(0, Ordering::Relaxed);
    }
}

pub fn snapshot_if_enabled() -> Option<DecodeProfileSnapshot> {
    enabled().then(snapshot)
}

pub fn snapshot() -> DecodeProfileSnapshot {
    DecodeProfileSnapshot {
        selected_raster_tiles: SELECTED_RASTER_TILES.load(Ordering::Relaxed),
        selected_mask_tiles: SELECTED_MASK_TILES.load(Ordering::Relaxed),
        skipped_empty_raster_tiles: SKIPPED_EMPTY_RASTER_TILES.load(Ordering::Relaxed),
        skipped_empty_mask_tiles: SKIPPED_EMPTY_MASK_TILES.load(Ordering::Relaxed),
        compressed_bytes_read: COMPRESSED_BYTES_READ.load(Ordering::Relaxed),
        zlib_inflate_ms: ZLIB_INFLATE_MS.load(Ordering::Relaxed),
        tile_swizzle_rgba_ms: TILE_SWIZZLE_RGBA_MS.load(Ordering::Relaxed),
        mask_r8_decode_ms: MASK_R8_DECODE_MS.load(Ordering::Relaxed),
        mask_crop_ms: MASK_CROP_MS.load(Ordering::Relaxed),
        atlas_chunk_build_ms: ATLAS_CHUNK_BUILD_MS.load(Ordering::Relaxed),
        sparse_atlas_pool_update_ms: SPARSE_ATLAS_POOL_UPDATE_MS.load(Ordering::Relaxed),
        region_patch_render_ms: REGION_PATCH_RENDER_MS.load(Ordering::Relaxed),
    }
}

pub(crate) fn record_selected(kind: DecodeTileKind, count: usize) {
    if !enabled() {
        return;
    }
    add(
        match kind {
            DecodeTileKind::Raster => &SELECTED_RASTER_TILES,
            DecodeTileKind::Mask => &SELECTED_MASK_TILES,
        },
        count as u64,
    );
}

pub(crate) fn record_skipped_empty(kind: DecodeTileKind) {
    if !enabled() {
        return;
    }
    add(
        match kind {
            DecodeTileKind::Raster => &SKIPPED_EMPTY_RASTER_TILES,
            DecodeTileKind::Mask => &SKIPPED_EMPTY_MASK_TILES,
        },
        1,
    );
}

pub(crate) fn record_compressed_bytes(bytes: usize) {
    if enabled() {
        add(&COMPRESSED_BYTES_READ, bytes as u64);
    }
}

pub(crate) fn record_zlib_inflate(elapsed: Duration) {
    if enabled() {
        add_duration_ms(&ZLIB_INFLATE_MS, elapsed);
    }
}

pub(crate) fn record_tile_swizzle_rgba(elapsed: Duration) {
    if enabled() {
        add_duration_ms(&TILE_SWIZZLE_RGBA_MS, elapsed);
    }
}

pub(crate) fn record_mask_r8_decode(elapsed: Duration) {
    if enabled() {
        add_duration_ms(&MASK_R8_DECODE_MS, elapsed);
    }
}

pub fn record_mask_crop(elapsed: Duration) {
    if enabled() {
        add_duration_ms(&MASK_CROP_MS, elapsed);
    }
}

pub(crate) fn record_atlas_chunk_build(elapsed: Duration) {
    if enabled() {
        add_duration_ms(&ATLAS_CHUNK_BUILD_MS, elapsed);
    }
}

pub fn record_sparse_atlas_pool_update(elapsed: Duration) {
    if enabled() {
        add_duration_ms(&SPARSE_ATLAS_POOL_UPDATE_MS, elapsed);
    }
}

pub fn record_region_patch_render(elapsed: Duration) {
    if enabled() {
        add_duration_ms(&REGION_PATCH_RENDER_MS, elapsed);
    }
}

fn counters() -> [&'static AtomicU64; 12] {
    [
        &SELECTED_RASTER_TILES,
        &SELECTED_MASK_TILES,
        &SKIPPED_EMPTY_RASTER_TILES,
        &SKIPPED_EMPTY_MASK_TILES,
        &COMPRESSED_BYTES_READ,
        &ZLIB_INFLATE_MS,
        &TILE_SWIZZLE_RGBA_MS,
        &MASK_R8_DECODE_MS,
        &MASK_CROP_MS,
        &ATLAS_CHUNK_BUILD_MS,
        &SPARSE_ATLAS_POOL_UPDATE_MS,
        &REGION_PATCH_RENDER_MS,
    ]
}

fn add(counter: &AtomicU64, value: u64) {
    counter.fetch_add(value, Ordering::Relaxed);
}

fn add_duration_ms(counter: &AtomicU64, elapsed: Duration) {
    add(counter, elapsed.as_millis().try_into().unwrap_or(u64::MAX));
}
