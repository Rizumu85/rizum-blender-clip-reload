use super::RuntimeGpuRenderer;
use crate::gpu_provider::{
    GpuResourcePlan, RuntimeGpuResourceProvider, cache::PersistentGpuTextureCache,
};
use crate::{ClipSession, GpuTextureCacheStats, RuntimeError};

#[derive(Debug, Default)]
pub(crate) struct SegmentCheckpointCache {
    entry: Option<SegmentCheckpointCacheEntry>,
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
    fn get(&self, key: &SegmentCheckpointKey) -> Option<SegmentCheckpoint> {
        let entry = self.entry.as_ref()?;
        (entry.key == *key).then(|| SegmentCheckpoint {
            pixels: entry.pixels.clone(),
            texture_cache_stats: GpuTextureCacheStats::default(),
            drawn_resources: entry.drawn_resources.clone(),
            mask_resources: entry.mask_resources.clone(),
        })
    }

    fn store(&mut self, key: SegmentCheckpointKey, checkpoint: &SegmentCheckpoint) {
        self.entry = Some(SegmentCheckpointCacheEntry {
            key,
            pixels: checkpoint.pixels.clone(),
            drawn_resources: checkpoint.drawn_resources.clone(),
            mask_resources: checkpoint.mask_resources.clone(),
        });
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
        if let Some(checkpoint) = self.segment_checkpoint_cache.borrow().get(&key) {
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
    use super::{SegmentCheckpointKey, initial_transparent_rgba8};

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
