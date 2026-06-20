use std::cell::OnceCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;
use std::time::Instant;

use clip_model::CanvasSize;

use crate::pass::{WHITE_TRANSPARENT, create_rgba8_texture};
use crate::render_profile;
use crate::stream_bounds::CanvasRect;
use crate::stream_tile_silo_pipeline::TileSiloPipeline;
use crate::{
    GpuMaskResourceCache, GpuMaskResourceKey, GpuRasterResourceCache, GpuRasterResourceInfo,
    GpuRasterResourceKey, GpuRenderError,
};

// Submit-only intermediate flushes can batch more passes; retained GPU bytes remain the hard cap.
const MAX_STREAMING_PASSES_PER_SUBMISSION: usize = 48;
const MAX_STREAMING_RETAINED_RESOURCE_BYTES: usize = 256 * 1024 * 1024;

pub(crate) struct StreamingTexturePair {
    textures: [wgpu::Texture; 2],
    views: [wgpu::TextureView; 2],
    size: CanvasSize,
    byte_len: usize,
}

impl StreamingTexturePair {
    pub(crate) fn new(
        device: &wgpu::Device,
        label_a: &'static str,
        label_b: &'static str,
        size: CanvasSize,
        usage: wgpu::TextureUsages,
    ) -> Self {
        let textures = [
            create_rgba8_texture(device, label_a, size, usage),
            create_rgba8_texture(device, label_b, size, usage),
        ];
        let views = [
            textures[0].create_view(&wgpu::TextureViewDescriptor::default()),
            textures[1].create_view(&wgpu::TextureViewDescriptor::default()),
        ];
        Self {
            textures,
            views,
            size,
            byte_len: rgba8_pair_byte_len(size),
        }
    }

    pub(crate) fn texture(&self, index: usize) -> &wgpu::Texture {
        &self.textures[index]
    }

    pub(crate) fn view(&self, index: usize) -> &wgpu::TextureView {
        &self.views[index]
    }

    pub(crate) fn size(&self) -> CanvasSize {
        self.size
    }

    fn byte_len(&self) -> usize {
        self.byte_len
    }
}

pub(crate) struct RenderedStreamingCache {
    pair: Option<StreamingTexturePair>,
    output_index: usize,
    bounds: Option<CanvasRect>,
    texture_origin: (i32, i32),
}

impl RenderedStreamingCache {
    pub(crate) fn new_with_origin(
        pair: StreamingTexturePair,
        output_index: usize,
        bounds: Option<CanvasRect>,
        texture_origin: (i32, i32),
    ) -> Self {
        Self {
            pair: Some(pair),
            output_index,
            bounds,
            texture_origin,
        }
    }

    pub(crate) fn empty() -> Self {
        Self {
            pair: None,
            output_index: 0,
            bounds: None,
            texture_origin: (0, 0),
        }
    }

    pub(crate) fn view(&self) -> &wgpu::TextureView {
        self.pair
            .as_ref()
            .expect("empty streaming cache has no texture view")
            .view(self.output_index)
    }

    pub(crate) fn bounds(&self) -> Option<CanvasRect> {
        self.bounds
    }

    pub(crate) fn texture_origin(&self) -> (i32, i32) {
        self.texture_origin
    }

    fn byte_len(&self) -> usize {
        self.pair
            .as_ref()
            .map(StreamingTexturePair::byte_len)
            .unwrap_or(0)
    }
}

pub(crate) struct StreamingEncoder<'a, E> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    label: &'static str,
    encoder: Option<wgpu::CommandEncoder>,
    render_bounds: Option<CanvasRect>,
    drawn_resources: Vec<GpuRasterResourceInfo>,
    encoded_passes_since_flush: usize,
    retained_resource_bytes: usize,
    has_pending_commands: bool,
    retained_raster_caches: Vec<GpuRasterResourceCache>,
    retained_mask_caches: Vec<GpuMaskResourceCache>,
    retained_raster_cache_by_key: HashMap<GpuRasterResourceKey, GpuRasterResourceCache>,
    retained_mask_cache_by_key: HashMap<GpuMaskResourceKey, GpuMaskResourceCache>,
    retained_lut_textures: Vec<wgpu::Texture>,
    retained_intermediate_caches: Vec<RenderedStreamingCache>,
    retained_texture_pairs: Vec<StreamingTexturePair>,
    tile_silo_pipeline: OnceCell<Rc<TileSiloPipeline>>,
    _error: PhantomData<E>,
}

impl<'a, E> StreamingEncoder<'a, E>
where
    E: From<GpuRenderError>,
{
    pub(crate) fn new(
        device: &'a wgpu::Device,
        queue: &'a wgpu::Queue,
        label: &'static str,
    ) -> Self {
        Self::new_with_render_bounds(device, queue, label, None)
    }

    pub(crate) fn new_with_render_bounds(
        device: &'a wgpu::Device,
        queue: &'a wgpu::Queue,
        label: &'static str,
        render_bounds: Option<CanvasRect>,
    ) -> Self {
        Self {
            device,
            queue,
            label,
            encoder: Some(
                device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) }),
            ),
            render_bounds,
            drawn_resources: Vec::new(),
            encoded_passes_since_flush: 0,
            retained_resource_bytes: 0,
            has_pending_commands: false,
            retained_raster_caches: Vec::new(),
            retained_mask_caches: Vec::new(),
            retained_raster_cache_by_key: HashMap::new(),
            retained_mask_cache_by_key: HashMap::new(),
            retained_lut_textures: Vec::new(),
            retained_intermediate_caches: Vec::new(),
            retained_texture_pairs: Vec::new(),
            tile_silo_pipeline: OnceCell::new(),
            _error: PhantomData,
        }
    }

    pub(crate) fn device(&self) -> &'a wgpu::Device {
        self.device
    }

    pub(crate) fn encoder_mut(&mut self) -> &mut wgpu::CommandEncoder {
        self.encoder
            .as_mut()
            .expect("streaming encoder must exist before finish")
    }

    pub(crate) fn tile_silo_pipeline(&self) -> Rc<TileSiloPipeline> {
        self.tile_silo_pipeline
            .get_or_init(|| Rc::new(TileSiloPipeline::new(self.device)))
            .clone()
    }

    pub(crate) fn render_bounds(&self) -> Option<CanvasRect> {
        self.render_bounds
    }

    pub(crate) fn clip_pass_bounds(&self, bounds: Option<CanvasRect>) -> Option<CanvasRect> {
        match (bounds, self.render_bounds) {
            (Some(bounds), Some(render_bounds)) => bounds.intersection(render_bounds),
            (Some(bounds), None) => Some(bounds),
            (None, _) => None,
        }
    }

    pub(crate) fn clear_rgba8_texture_pair(
        &mut self,
        first: &wgpu::TextureView,
        second: &wgpu::TextureView,
        label: &'static str,
    ) {
        let encode_start = Instant::now();
        let encoder = self.encoder_mut();
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: first,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(WHITE_TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: second,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(WHITE_TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    }),
                ],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
        }
        render_profile::record_gpu_pass_encode(encode_start.elapsed());
        self.has_pending_commands = true;
    }

    pub(crate) fn push_drawn_resource(&mut self, info: GpuRasterResourceInfo) {
        self.drawn_resources.push(info);
    }

    pub(crate) fn retained_raster_cache(
        &self,
        key: GpuRasterResourceKey,
    ) -> Option<GpuRasterResourceCache> {
        self.retained_raster_cache_by_key.get(&key).cloned()
    }

    pub(crate) fn retain_raster_cache(&mut self, cache: GpuRasterResourceCache) {
        let mut bytes = 0usize;
        let mut has_new_resource = false;
        for info in cache.resource_infos() {
            if self.retained_raster_cache_by_key.contains_key(&info.key) {
                continue;
            }
            bytes = bytes.saturating_add(info.byte_len);
            has_new_resource = true;
            self.retained_raster_cache_by_key
                .insert(info.key, cache.clone());
        }
        if !has_new_resource {
            return;
        }
        self.add_retained_bytes(bytes);
        self.retained_raster_caches.push(cache);
    }

    pub(crate) fn retain_optional_mask_cache(&mut self, cache: Option<GpuMaskResourceCache>) {
        if let Some(cache) = cache {
            self.retain_mask_cache(cache);
        }
    }

    pub(crate) fn retained_mask_cache(
        &self,
        key: GpuMaskResourceKey,
    ) -> Option<GpuMaskResourceCache> {
        self.retained_mask_cache_by_key.get(&key).cloned()
    }

    pub(crate) fn retain_mask_cache(&mut self, cache: GpuMaskResourceCache) {
        let mut bytes = 0usize;
        let mut has_new_resource = false;
        for info in cache.resource_infos() {
            if self.retained_mask_cache_by_key.contains_key(&info.key) {
                continue;
            }
            bytes = bytes.saturating_add(info.byte_len);
            has_new_resource = true;
            self.retained_mask_cache_by_key
                .insert(info.key, cache.clone());
        }
        if !has_new_resource {
            return;
        }
        self.add_retained_bytes(bytes);
        self.retained_mask_caches.push(cache);
    }

    pub(crate) fn retain_lut_texture(&mut self, texture: wgpu::Texture) {
        self.add_retained_bytes(256 * 4);
        self.retained_lut_textures.push(texture);
    }

    pub(crate) fn retain_texture(&mut self, texture: wgpu::Texture, byte_len: usize) {
        self.add_retained_bytes(byte_len);
        self.retained_lut_textures.push(texture);
    }

    pub(crate) fn retain_intermediate_cache(&mut self, cache: RenderedStreamingCache) {
        self.add_retained_bytes(cache.byte_len());
        self.retained_intermediate_caches.push(cache);
    }

    pub(crate) fn retain_texture_pair(&mut self, pair: StreamingTexturePair) {
        self.add_retained_bytes(pair.byte_len());
        self.retained_texture_pairs.push(pair);
    }

    pub(crate) fn finish_pass(&mut self) -> Result<(), E> {
        self.has_pending_commands = true;
        self.encoded_passes_since_flush += 1;
        render_profile::record_streaming_pass();
        if should_flush_streaming_batch(
            self.encoded_passes_since_flush,
            self.retained_resource_bytes,
        ) {
            self.flush()?;
        }
        Ok(())
    }

    pub(crate) fn flush(&mut self) -> Result<(), E> {
        if !self.has_pending_commands {
            return Ok(());
        }
        let encoder = self
            .encoder
            .take()
            .expect("streaming encoder must exist before flush");
        let submit_start = Instant::now();
        self.queue.submit([encoder.finish()]);
        render_profile::record_queue_submit(submit_start.elapsed());
        // Queue order plus the final readback poll waits for completion; intermediate flushes only
        // need resources to stay alive until the command buffer is submitted.
        self.clear_retained_resources();
        self.encoder = Some(
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some(self.label),
                }),
        );
        Ok(())
    }

    pub(crate) fn into_drawn_resources(self) -> Vec<GpuRasterResourceInfo> {
        self.drawn_resources
    }

    fn add_retained_bytes(&mut self, bytes: usize) {
        self.retained_resource_bytes = self.retained_resource_bytes.saturating_add(bytes);
    }

    fn clear_retained_resources(&mut self) {
        self.retained_raster_caches.clear();
        self.retained_mask_caches.clear();
        self.retained_raster_cache_by_key.clear();
        self.retained_mask_cache_by_key.clear();
        self.retained_lut_textures.clear();
        self.retained_intermediate_caches.clear();
        self.retained_texture_pairs.clear();
        self.retained_resource_bytes = 0;
        self.encoded_passes_since_flush = 0;
        self.has_pending_commands = false;
    }
}

fn rgba8_pair_byte_len(size: CanvasSize) -> usize {
    usize::try_from(
        u64::from(size.width)
            .saturating_mul(u64::from(size.height))
            .saturating_mul(4)
            .saturating_mul(2),
    )
    .unwrap_or(usize::MAX)
}

fn should_flush_streaming_batch(encoded_passes: usize, retained_resource_bytes: usize) -> bool {
    encoded_passes >= MAX_STREAMING_PASSES_PER_SUBMISSION
        || retained_resource_bytes >= MAX_STREAMING_RETAINED_RESOURCE_BYTES
}

#[cfg(test)]
mod tests {
    use clip_model::CanvasSize;

    use super::{
        MAX_STREAMING_PASSES_PER_SUBMISSION, MAX_STREAMING_RETAINED_RESOURCE_BYTES,
        rgba8_pair_byte_len, should_flush_streaming_batch,
    };

    #[test]
    fn streaming_batch_flushes_at_pass_threshold() {
        assert!(!should_flush_streaming_batch(
            MAX_STREAMING_PASSES_PER_SUBMISSION - 1,
            0
        ));
        assert!(should_flush_streaming_batch(
            MAX_STREAMING_PASSES_PER_SUBMISSION,
            0
        ));
    }

    #[test]
    fn streaming_batch_flushes_at_resource_byte_threshold() {
        assert!(!should_flush_streaming_batch(
            1,
            MAX_STREAMING_RETAINED_RESOURCE_BYTES - 1
        ));
        assert!(should_flush_streaming_batch(
            1,
            MAX_STREAMING_RETAINED_RESOURCE_BYTES
        ));
    }

    #[test]
    fn rgba8_pair_byte_len_counts_both_ping_pong_textures() {
        assert_eq!(rgba8_pair_byte_len(CanvasSize::new(4, 3)), 4 * 3 * 4 * 2);
    }
}
