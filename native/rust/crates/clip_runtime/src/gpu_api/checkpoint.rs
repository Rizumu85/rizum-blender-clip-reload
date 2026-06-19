use std::collections::VecDeque;

use super::RuntimeGpuRenderer;
use crate::gpu_provider::{
    GpuResourcePlan, RuntimeGpuResourceProvider, cache::PersistentGpuTextureCache,
};
use crate::{ClipSession, GpuTextureCacheStats, RuntimeError};

const DEFAULT_CHECKPOINT_MAX_ENTRIES: usize = 2;
const DEFAULT_CHECKPOINT_BUDGET_BYTES: usize = 512 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct SegmentCheckpointCache {
    entries: VecDeque<SegmentCheckpointCacheEntry>,
    max_entries: usize,
    budget_bytes: usize,
    cached_bytes: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct SegmentCheckpoint {
    pub(crate) pixels: Vec<u8>,
    pub(crate) texture_cache_stats: GpuTextureCacheStats,
    pub(crate) drawn_resources: Vec<clip_gpu::GpuRasterResourceInfo>,
    pub(crate) mask_resources: Vec<clip_gpu::GpuMaskResourceInfo>,
}

#[derive(Clone, Debug)]
struct SegmentCheckpointCacheEntry {
    key: SegmentCheckpointKey,
    pixels: Vec<u8>,
    drawn_resources: Vec<clip_gpu::GpuRasterResourceInfo>,
    mask_resources: Vec<clip_gpu::GpuMaskResourceInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SegmentCheckpointKey {
    width: u32,
    height: u32,
    root_layer_id: u32,
    source_start: u32,
    prefix_signature: u64,
}

impl SegmentCheckpointCache {
    fn new(max_entries: usize, budget_bytes: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            max_entries,
            budget_bytes,
            cached_bytes: 0,
        }
    }

    fn get(&mut self, key: &SegmentCheckpointKey) -> Option<SegmentCheckpoint> {
        let index = self.entries.iter().position(|entry| entry.key == *key)?;
        let entry = self.entries.remove(index)?;
        let checkpoint = SegmentCheckpoint {
            pixels: entry.pixels.clone(),
            texture_cache_stats: GpuTextureCacheStats::default(),
            drawn_resources: entry.drawn_resources.clone(),
            mask_resources: entry.mask_resources.clone(),
        };
        self.entries.push_back(entry);
        Some(checkpoint)
    }

    fn store(&mut self, key: SegmentCheckpointKey, checkpoint: &SegmentCheckpoint) {
        let entry = SegmentCheckpointCacheEntry {
            key,
            pixels: checkpoint.pixels.clone(),
            drawn_resources: checkpoint.drawn_resources.clone(),
            mask_resources: checkpoint.mask_resources.clone(),
        };
        let byte_len = entry.byte_len();
        if byte_len > self.budget_bytes || self.max_entries == 0 {
            return;
        }

        if let Some(index) = self
            .entries
            .iter()
            .position(|existing| existing.key == entry.key)
        {
            if let Some(existing) = self.entries.remove(index) {
                self.cached_bytes = self.cached_bytes.saturating_sub(existing.byte_len());
            }
        }
        while self.cached_bytes.saturating_add(byte_len) > self.budget_bytes
            || self.entries.len() >= self.max_entries
        {
            let Some(evicted) = self.entries.pop_front() else {
                break;
            };
            self.cached_bytes = self.cached_bytes.saturating_sub(evicted.byte_len());
        }
        self.cached_bytes = self.cached_bytes.saturating_add(byte_len);
        self.entries.push_back(entry);
    }
}

impl Default for SegmentCheckpointCache {
    fn default() -> Self {
        Self::new(
            DEFAULT_CHECKPOINT_MAX_ENTRIES,
            DEFAULT_CHECKPOINT_BUDGET_BYTES,
        )
    }
}

impl SegmentCheckpointCacheEntry {
    fn byte_len(&self) -> usize {
        self.pixels.len()
    }
}

impl RuntimeGpuRenderer {
    pub(crate) fn prefix_checkpoint_rgba8(
        &self,
        session: &ClipSession,
        plan: &crate::ReloadDiffPlan,
        source_start: u32,
        sources: &[clip_gpu::GpuNormalStackSource],
        resource_plan: GpuResourcePlan,
    ) -> Result<SegmentCheckpoint, RuntimeError> {
        let key = SegmentCheckpointKey::from_reload_plan(plan, source_start);
        if let Some(checkpoint) = self.segment_checkpoint_cache.borrow_mut().get(&key) {
            return Ok(checkpoint);
        }

        let checkpoint = self.render_prefix_checkpoint_rgba8(session, sources, resource_plan)?;
        self.segment_checkpoint_cache
            .borrow_mut()
            .store(key, &checkpoint);
        Ok(checkpoint)
    }

    fn render_prefix_checkpoint_rgba8(
        &self,
        session: &ClipSession,
        sources: &[clip_gpu::GpuNormalStackSource],
        resource_plan: GpuResourcePlan,
    ) -> Result<SegmentCheckpoint, RuntimeError> {
        if sources.is_empty() {
            return initial_transparent_rgba8(session.summary.canvas).map(|pixels| {
                SegmentCheckpoint {
                    pixels,
                    texture_cache_stats: GpuTextureCacheStats::default(),
                    drawn_resources: Vec::new(),
                    mask_resources: Vec::new(),
                }
            });
        }

        let mut texture_cache = self.texture_cache.borrow_mut();
        let mut provider = match texture_cache.as_mut() {
            Some(cache) => {
                cache.begin_frame();
                RuntimeGpuResourceProvider::with_texture_cache(
                    &session.container,
                    session.summary.canvas,
                    resource_plan,
                    cache,
                )?
            }
            None => RuntimeGpuResourceProvider::new(
                &session.container,
                session.summary.canvas,
                resource_plan,
            )?,
        };
        let output = self.renderer.draw_normal_stack_with_provider_to_rgba8(
            session.summary.canvas,
            sources,
            &mut provider,
        )?;
        let mask_resources = std::mem::take(&mut provider.mask_resources);
        drop(provider);
        let texture_cache_stats = texture_cache
            .as_ref()
            .map(PersistentGpuTextureCache::frame_stats)
            .unwrap_or_default();
        Ok(SegmentCheckpoint {
            pixels: output.pixels,
            texture_cache_stats,
            drawn_resources: output.drawn_resources,
            mask_resources,
        })
    }
}

impl SegmentCheckpointKey {
    fn from_reload_plan(plan: &crate::ReloadDiffPlan, source_start: u32) -> Self {
        let manifest = &plan.manifest;
        Self {
            width: manifest.width,
            height: manifest.height,
            root_layer_id: manifest.root_layer_id,
            source_start,
            prefix_signature: prefix_signature(manifest, source_start),
        }
    }
}

pub(crate) fn initial_transparent_rgba8(
    size: clip_model::CanvasSize,
) -> Result<Vec<u8>, RuntimeError> {
    let len = usize::try_from(
        u64::from(size.width)
            .checked_mul(u64::from(size.height))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(clip_gpu::GpuRenderError::TextureSizeOverflow)?,
    )
    .map_err(|_| clip_gpu::GpuRenderError::TextureSizeOverflow)?;
    let mut pixels = vec![0u8; len];
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[255, 255, 255, 0]);
    }
    Ok(pixels)
}

fn prefix_signature(manifest: &crate::ReloadDiffManifest, source_start: u32) -> u64 {
    let mut hash = Hash64::new();
    hash.u32(manifest.abi);
    hash.u32(manifest.tile_size);
    hash.u32(manifest.tile_event_abi_version);
    hash.u32(manifest.width);
    hash.u32(manifest.height);
    hash.u32(manifest.root_layer_id);
    hash.u32(source_start);
    hash.u32(usize_to_u32(manifest.nodes.len()));
    for node in &manifest.nodes {
        hash.u64(node.signature);
    }
    for segment in manifest
        .segments
        .iter()
        .filter(|segment| segment.source_end <= source_start)
    {
        hash.u32(segment.ordinal);
        hash.u32(segment.source_start);
        hash.u32(segment.source_end);
        hash.u64(segment.signature);
        hash.u64(segment.tile_work_list_signature);
    }
    hash.finish()
}

fn usize_to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

struct Hash64(u64);

impl Hash64 {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn finish(self) -> u64 {
        self.0
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SegmentCheckpoint, SegmentCheckpointCache, SegmentCheckpointKey, initial_transparent_rgba8,
    };
    use crate::GpuTextureCacheStats;

    #[test]
    fn initial_transparent_base_uses_white_rgb_zero_alpha() {
        let pixels =
            initial_transparent_rgba8(clip_model::CanvasSize::new(2, 1)).expect("base pixels");

        assert_eq!(pixels, vec![255, 255, 255, 0, 255, 255, 255, 0]);
    }

    #[test]
    fn checkpoint_key_changes_when_prefix_segment_changes() {
        let mut plan = patch_plan_with_segments();
        let first = SegmentCheckpointKey::from_reload_plan(&plan, 2);

        plan.manifest.segments[0].signature ^= 1;
        let changed_prefix = SegmentCheckpointKey::from_reload_plan(&plan, 2);
        assert_ne!(first, changed_prefix);
    }

    #[test]
    fn checkpoint_key_ignores_suffix_tile_changes() {
        let mut plan = patch_plan_with_segments();
        let first = SegmentCheckpointKey::from_reload_plan(&plan, 1);

        plan.manifest.segments[1].tile_work_list_signature ^= 1;
        let changed_suffix = SegmentCheckpointKey::from_reload_plan(&plan, 1);
        assert_eq!(first, changed_suffix);
    }

    #[test]
    fn checkpoint_cache_keeps_multiple_budgeted_entries() {
        let plan = patch_plan_with_segments();
        let key_a = SegmentCheckpointKey::from_reload_plan(&plan, 1);
        let key_b = SegmentCheckpointKey::from_reload_plan(&plan, 2);
        let mut cache = SegmentCheckpointCache::new(2, 8);

        cache.store(key_a.clone(), &checkpoint_with_len(4));
        cache.store(key_b.clone(), &checkpoint_with_len(4));

        assert_eq!(cache.get(&key_a).expect("key_a cached").pixels.len(), 4);
        assert_eq!(cache.get(&key_b).expect("key_b cached").pixels.len(), 4);
    }

    #[test]
    fn checkpoint_cache_evicts_lru_entry_by_count() {
        let plan = patch_plan_with_segments();
        let key_a = SegmentCheckpointKey::from_reload_plan(&plan, 1);
        let key_b = SegmentCheckpointKey::from_reload_plan(&plan, 2);
        let key_c = SegmentCheckpointKey {
            source_start: 3,
            ..key_b.clone()
        };
        let mut cache = SegmentCheckpointCache::new(2, 12);

        cache.store(key_a.clone(), &checkpoint_with_len(4));
        cache.store(key_b.clone(), &checkpoint_with_len(4));
        assert!(cache.get(&key_a).is_some());
        cache.store(key_c.clone(), &checkpoint_with_len(4));

        assert!(cache.get(&key_a).is_some());
        assert!(cache.get(&key_b).is_none());
        assert!(cache.get(&key_c).is_some());
    }

    #[test]
    fn checkpoint_cache_skips_over_budget_entry() {
        let plan = patch_plan_with_segments();
        let key = SegmentCheckpointKey::from_reload_plan(&plan, 1);
        let mut cache = SegmentCheckpointCache::new(2, 3);

        cache.store(key.clone(), &checkpoint_with_len(4));

        assert!(cache.get(&key).is_none());
    }

    fn checkpoint_with_len(len: usize) -> SegmentCheckpoint {
        SegmentCheckpoint {
            pixels: vec![7; len],
            texture_cache_stats: GpuTextureCacheStats::default(),
            drawn_resources: Vec::new(),
            mask_resources: Vec::new(),
        }
    }

    fn patch_plan_with_segments() -> crate::ReloadDiffPlan {
        crate::ReloadDiffPlan {
            manifest: crate::ReloadDiffManifest {
                abi: 4,
                tile_size: 256,
                tile_event_abi_version: clip_gpu::TILE_EVENT_ABI_VERSION,
                width: 2,
                height: 1,
                root_layer_id: 1,
                nodes: vec![crate::ReloadDiffNode {
                    layer_id: 1,
                    kind: "Raster".to_string(),
                    depth: 0,
                    clip: false,
                    opacity: 255,
                    composite: 0,
                    render_mipmap_id: Some(1),
                    mask_mipmap_id: None,
                    paper_color: None,
                    signature: 11,
                }],
                sources: Vec::new(),
                segments: vec![segment(1, 0, 1, 101, 201), segment(2, 1, 2, 102, 202)],
            },
            mode: crate::ReloadDiffMode::Patch,
            reason: "test".to_string(),
            dirty_rects: Vec::new(),
            dirty_segments: Vec::new(),
        }
    }

    fn segment(
        ordinal: u32,
        source_start: u32,
        source_end: u32,
        signature: u64,
        tile_work_list_signature: u64,
    ) -> crate::ReloadDiffSegment {
        crate::ReloadDiffSegment {
            ordinal,
            depth: 0,
            source_start,
            source_end,
            kind: "RasterRun".to_string(),
            barrier_reason: None,
            expected_passes: 1,
            tile_events: 1,
            legacy_sources: 0,
            resources: Vec::new(),
            tile_work_list_source_count: 0,
            tile_work_list_tile_count: 0,
            tile_work_list_signature,
            tile_work_list: Vec::new(),
            signature,
        }
    }
}
