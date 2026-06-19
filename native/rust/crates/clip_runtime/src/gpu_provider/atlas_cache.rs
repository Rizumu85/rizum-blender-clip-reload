use std::collections::{HashMap, HashSet};

use clip_model::CanvasSize;

use crate::GpuSparseAtlasCacheStats;
use crate::reload_diff::{ReloadDiffManifest, ReloadDiffSource, ReloadDiffTile};

const DEFAULT_ATLAS_TILE_SIZE: u32 = clip_file::tiles::TILE_SIZE as u32;
const DEFAULT_ATLAS_TILES_PER_SIDE: u32 = 32;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum SparseAtlasResourceKind {
    Raster,
    Mask,
}

impl SparseAtlasResourceKind {
    fn from_reload_kind(kind: &str) -> Option<Self> {
        match kind {
            "raster" => Some(Self::Raster),
            "mask" => Some(Self::Mask),
            _ => None,
        }
    }

    pub(super) fn as_reload_kind(self) -> &'static str {
        match self {
            Self::Raster => "raster",
            Self::Mask => "mask",
        }
    }

    fn bytes_per_pixel(self) -> u64 {
        match self {
            Self::Raster => 4,
            Self::Mask => 1,
        }
    }

    pub(super) fn atlas_format(self) -> clip_gpu::GpuSparseAtlasFormat {
        match self {
            Self::Raster => clip_gpu::GpuSparseAtlasFormat::Rgba8,
            Self::Mask => clip_gpu::GpuSparseAtlasFormat::R8,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct SparseAtlasTileId {
    pub kind: SparseAtlasResourceKind,
    pub layer_id: u32,
    pub resource_id: u32,
    pub external_id: String,
    pub source_signature: u64,
    pub tile_x: u32,
    pub tile_y: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SparseAtlasFingerprint {
    pub tile: SparseAtlasTileId,
    pub source_x: u32,
    pub source_y: u32,
    pub width: u32,
    pub height: u32,
    pub compressed_bytes: u32,
    pub compressed_hash: u64,
}

impl SparseAtlasFingerprint {
    pub(crate) fn from_reload_source_tile(
        source: &ReloadDiffSource,
        tile: &ReloadDiffTile,
    ) -> Option<Self> {
        Some(Self {
            tile: SparseAtlasTileId {
                kind: SparseAtlasResourceKind::from_reload_kind(&source.kind)?,
                layer_id: source.layer_id,
                resource_id: source.resource_id,
                external_id: source.external_id.clone(),
                source_signature: source.signature,
                tile_x: tile.tile_x,
                tile_y: tile.tile_y,
            },
            source_x: tile.tile_x.saturating_mul(DEFAULT_ATLAS_TILE_SIZE),
            source_y: tile.tile_y.saturating_mul(DEFAULT_ATLAS_TILE_SIZE),
            width: tile.width,
            height: tile.height,
            compressed_bytes: tile.compressed_bytes,
            compressed_hash: tile.compressed_hash,
        })
    }

    fn upload_byte_len(&self) -> u64 {
        u64::from(self.width)
            .saturating_mul(u64::from(self.height))
            .saturating_mul(self.tile.kind.bytes_per_pixel())
    }

    fn same_payload(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && self.compressed_bytes == other.compressed_bytes
            && self.compressed_hash == other.compressed_hash
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SparseAtlasSlot {
    pub index: u32,
    pub atlas_id: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SparseAtlasUpdateAction {
    Reuse,
    UploadInserted,
    UploadChanged,
}

impl SparseAtlasUpdateAction {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Reuse => "reuse",
            Self::UploadInserted => "upload_inserted",
            Self::UploadChanged => "upload_changed",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SparseAtlasTileUpdate {
    pub fingerprint: SparseAtlasFingerprint,
    pub slot: SparseAtlasSlot,
    pub action: SparseAtlasUpdateAction,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SparseAtlasUpdatePlan {
    pub generation: u64,
    pub atlas_size: CanvasSize,
    pub updates: Vec<SparseAtlasTileUpdate>,
    pub stats: SparseAtlasCacheStats,
}

impl Default for SparseAtlasUpdatePlan {
    fn default() -> Self {
        Self {
            generation: 0,
            atlas_size: CanvasSize::new(0, 0),
            updates: Vec::new(),
            stats: SparseAtlasCacheStats::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SparseAtlasCacheStats {
    pub cached_tiles: usize,
    pub cached_bytes: u64,
    pub atlas_count: u32,
    pub reused_tiles: usize,
    pub inserted_tiles: usize,
    pub changed_tiles: usize,
    pub evicted_tiles: usize,
    pub upload_bytes: u64,
}

#[derive(Debug)]
pub(crate) struct SparseAtlasCache {
    tile_size: u32,
    atlas_tiles_per_side: u32,
    generation: u64,
    next_slot_index: u32,
    free_slots: Vec<u32>,
    entries: HashMap<SparseAtlasTileId, SparseAtlasEntry>,
}

#[derive(Clone, Debug)]
struct SparseAtlasEntry {
    fingerprint: SparseAtlasFingerprint,
    slot: SparseAtlasSlot,
    last_used_generation: u64,
}

impl Default for SparseAtlasCache {
    fn default() -> Self {
        Self::new(DEFAULT_ATLAS_TILE_SIZE, DEFAULT_ATLAS_TILES_PER_SIDE)
    }
}

impl SparseAtlasCache {
    pub(crate) fn new(tile_size: u32, atlas_tiles_per_side: u32) -> Self {
        assert!(tile_size > 0, "tile_size must be positive");
        assert!(
            atlas_tiles_per_side > 0,
            "atlas_tiles_per_side must be positive"
        );
        Self {
            tile_size,
            atlas_tiles_per_side,
            generation: 0,
            next_slot_index: 0,
            free_slots: Vec::new(),
            entries: HashMap::new(),
        }
    }

    pub(crate) fn plan_reload_manifest(
        &mut self,
        manifest: &ReloadDiffManifest,
    ) -> SparseAtlasUpdatePlan {
        let fingerprints = manifest_tile_work_list_fingerprints(manifest);
        if fingerprints.is_empty() {
            return self.plan_generation(manifest.sources.iter().flat_map(|source| {
                source.tiles.iter().filter_map(|tile| {
                    SparseAtlasFingerprint::from_reload_source_tile(source, tile)
                })
            }));
        }
        self.plan_generation(fingerprints)
    }

    pub(crate) fn plan_generation<I>(&mut self, fingerprints: I) -> SparseAtlasUpdatePlan
    where
        I: IntoIterator<Item = SparseAtlasFingerprint>,
    {
        self.generation = self.generation.saturating_add(1);
        let generation = self.generation;
        let mut updates = Vec::new();
        let mut seen = HashSet::new();
        let mut stats = SparseAtlasCacheStats::default();

        for fingerprint in fingerprints {
            if !seen.insert(fingerprint.tile.clone()) {
                continue;
            }

            let byte_len = fingerprint.upload_byte_len();
            let update = if let Some(entry) = self.entries.get_mut(&fingerprint.tile) {
                entry.last_used_generation = generation;
                let action = if entry.fingerprint.same_payload(&fingerprint) {
                    stats.reused_tiles += 1;
                    SparseAtlasUpdateAction::Reuse
                } else {
                    stats.changed_tiles += 1;
                    stats.upload_bytes = stats.upload_bytes.saturating_add(byte_len);
                    entry.fingerprint = fingerprint.clone();
                    entry.slot.width = fingerprint.width;
                    entry.slot.height = fingerprint.height;
                    SparseAtlasUpdateAction::UploadChanged
                };
                SparseAtlasTileUpdate {
                    fingerprint,
                    slot: entry.slot,
                    action,
                }
            } else {
                let slot = self.allocate_slot(fingerprint.width, fingerprint.height);
                self.entries.insert(
                    fingerprint.tile.clone(),
                    SparseAtlasEntry {
                        fingerprint: fingerprint.clone(),
                        slot,
                        last_used_generation: generation,
                    },
                );
                stats.inserted_tiles += 1;
                stats.upload_bytes = stats.upload_bytes.saturating_add(byte_len);
                SparseAtlasTileUpdate {
                    fingerprint,
                    slot,
                    action: SparseAtlasUpdateAction::UploadInserted,
                }
            };
            updates.push(update);
        }

        stats.evicted_tiles = self.reclaim_unused(generation);
        self.fill_resident_stats(&mut stats);
        SparseAtlasUpdatePlan {
            generation,
            atlas_size: self.atlas_size(),
            updates,
            stats,
        }
    }

    #[cfg(test)]
    pub(crate) fn stats(&self) -> SparseAtlasCacheStats {
        let mut stats = SparseAtlasCacheStats::default();
        self.fill_resident_stats(&mut stats);
        stats
    }

    fn allocate_slot(&mut self, width: u32, height: u32) -> SparseAtlasSlot {
        let index = self.free_slots.pop().unwrap_or_else(|| {
            let index = self.next_slot_index;
            self.next_slot_index = self.next_slot_index.saturating_add(1);
            index
        });
        self.slot_for_index(index, width, height)
    }

    fn atlas_size(&self) -> CanvasSize {
        CanvasSize::new(
            self.tile_size.saturating_mul(self.atlas_tiles_per_side),
            self.tile_size.saturating_mul(self.atlas_tiles_per_side),
        )
    }

    fn slot_for_index(&self, index: u32, width: u32, height: u32) -> SparseAtlasSlot {
        let slots_per_atlas = self
            .atlas_tiles_per_side
            .saturating_mul(self.atlas_tiles_per_side);
        let atlas_id = index / slots_per_atlas;
        let local = index % slots_per_atlas;
        let tile_x = local % self.atlas_tiles_per_side;
        let tile_y = local / self.atlas_tiles_per_side;
        SparseAtlasSlot {
            index,
            atlas_id,
            x: tile_x.saturating_mul(self.tile_size),
            y: tile_y.saturating_mul(self.tile_size),
            width,
            height,
        }
    }

    fn reclaim_unused(&mut self, generation: u64) -> usize {
        let mut stale = Vec::new();
        for (id, entry) in &self.entries {
            if entry.last_used_generation != generation {
                stale.push(id.clone());
            }
        }
        let evicted = stale.len();
        for id in stale {
            if let Some(entry) = self.entries.remove(&id) {
                self.free_slots.push(entry.slot.index);
            }
        }
        evicted
    }

    fn fill_resident_stats(&self, stats: &mut SparseAtlasCacheStats) {
        stats.cached_tiles = self.entries.len();
        stats.cached_bytes = self
            .entries
            .values()
            .map(|entry| entry.fingerprint.upload_byte_len())
            .sum();
        stats.atlas_count = atlas_count_for_slots(self.next_slot_index, self.atlas_tiles_per_side);
    }
}

impl From<SparseAtlasCacheStats> for GpuSparseAtlasCacheStats {
    fn from(value: SparseAtlasCacheStats) -> Self {
        Self {
            cached_tiles: value.cached_tiles,
            cached_bytes: value.cached_bytes,
            atlas_count: value.atlas_count,
            reused_tiles: value.reused_tiles,
            inserted_tiles: value.inserted_tiles,
            changed_tiles: value.changed_tiles,
            evicted_tiles: value.evicted_tiles,
            upload_bytes: value.upload_bytes,
        }
    }
}

fn manifest_tile_work_list_fingerprints(
    manifest: &ReloadDiffManifest,
) -> Vec<SparseAtlasFingerprint> {
    let mut source_tiles = HashMap::new();
    for source in &manifest.sources {
        for tile in &source.tiles {
            source_tiles.insert(
                (
                    source.kind.as_str(),
                    source.layer_id,
                    source.resource_id,
                    tile.tile_x,
                    tile.tile_y,
                ),
                (source, tile),
            );
        }
    }

    let mut fingerprints = Vec::new();
    let mut seen = HashSet::new();
    for tile_ref in manifest
        .segments
        .iter()
        .flat_map(|segment| segment.tile_work_list.iter())
    {
        let key = (
            tile_ref.kind.as_str(),
            tile_ref.layer_id,
            tile_ref.resource_id,
            tile_ref.tile_x,
            tile_ref.tile_y,
        );
        if !seen.insert((
            tile_ref.kind.as_str(),
            tile_ref.layer_id,
            tile_ref.resource_id,
            tile_ref.tile_x,
            tile_ref.tile_y,
        )) {
            continue;
        }
        let Some((source, tile)) = source_tiles.get(&key) else {
            continue;
        };
        if let Some(fingerprint) = SparseAtlasFingerprint::from_reload_source_tile(source, tile) {
            fingerprints.push(fingerprint);
        }
    }
    fingerprints
}

fn atlas_count_for_slots(next_slot_index: u32, atlas_tiles_per_side: u32) -> u32 {
    if next_slot_index == 0 {
        return 0;
    }
    let slots_per_atlas = atlas_tiles_per_side.saturating_mul(atlas_tiles_per_side);
    next_slot_index.div_ceil(slots_per_atlas)
}
