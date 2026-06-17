use std::collections::HashMap;

use clip_model::{CanvasSize, Rect};

use crate::{GpuTextureCacheStats, RuntimeError};

const DEFAULT_GPU_TEXTURE_CACHE_BYTES: usize = 768 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct PersistentGpuTextureCache {
    rasters: HashMap<RasterTextureCacheKey, clip_gpu::GpuRasterResourceCache>,
    masks: HashMap<MaskTextureCacheKey, clip_gpu::GpuMaskResourceCache>,
    cached_bytes: usize,
    max_bytes: usize,
    frame: GpuTextureCacheStats,
}

impl PersistentGpuTextureCache {
    pub(crate) fn new() -> Self {
        Self {
            rasters: HashMap::new(),
            masks: HashMap::new(),
            cached_bytes: 0,
            max_bytes: DEFAULT_GPU_TEXTURE_CACHE_BYTES,
            frame: GpuTextureCacheStats::default(),
        }
    }

    pub(crate) fn begin_frame(&mut self) {
        self.frame = GpuTextureCacheStats::default();
    }

    pub(crate) fn frame_stats(&self) -> GpuTextureCacheStats {
        GpuTextureCacheStats {
            cached_rasters: self.rasters.len(),
            cached_masks: self.masks.len(),
            cached_bytes: self.cached_bytes,
            ..self.frame
        }
    }

    pub(crate) fn raster_cache(
        &mut self,
        key: &RasterTextureCacheKey,
    ) -> Option<clip_gpu::GpuRasterResourceCache> {
        match self.rasters.get(key).cloned() {
            Some(cache) => {
                self.frame.raster_hits += 1;
                Some(cache)
            }
            None => {
                self.frame.raster_misses += 1;
                None
            }
        }
    }

    pub(crate) fn insert_raster(
        &mut self,
        key: RasterTextureCacheKey,
        cache: clip_gpu::GpuRasterResourceCache,
    ) {
        let byte_len = raster_cache_byte_len(&cache);
        if byte_len > self.max_bytes {
            return;
        }
        self.ensure_room(byte_len);
        if let Some(old) = self.rasters.insert(key, cache) {
            self.cached_bytes = self
                .cached_bytes
                .saturating_sub(raster_cache_byte_len(&old));
        }
        self.cached_bytes = self.cached_bytes.saturating_add(byte_len);
    }

    pub(crate) fn mask_cache(
        &mut self,
        key: &MaskTextureCacheKey,
    ) -> Option<clip_gpu::GpuMaskResourceCache> {
        match self.masks.get(key).cloned() {
            Some(cache) => {
                self.frame.mask_hits += 1;
                Some(cache)
            }
            None => {
                self.frame.mask_misses += 1;
                None
            }
        }
    }

    pub(crate) fn insert_mask(
        &mut self,
        key: MaskTextureCacheKey,
        cache: clip_gpu::GpuMaskResourceCache,
    ) {
        let byte_len = mask_cache_byte_len(&cache);
        if byte_len > self.max_bytes {
            return;
        }
        self.ensure_room(byte_len);
        if let Some(old) = self.masks.insert(key, cache) {
            self.cached_bytes = self.cached_bytes.saturating_sub(mask_cache_byte_len(&old));
        }
        self.cached_bytes = self.cached_bytes.saturating_add(byte_len);
    }

    fn ensure_room(&mut self, byte_len: usize) {
        if self.cached_bytes.saturating_add(byte_len) <= self.max_bytes {
            return;
        }
        self.rasters.clear();
        self.masks.clear();
        self.cached_bytes = 0;
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct RasterTextureCacheKey {
    layer_id: u32,
    render_mipmap_id: u32,
    external_id: String,
    source_width: u32,
    source_height: u32,
    source_offset_x: i32,
    source_offset_y: i32,
    color_type: Option<u32>,
    decode_rect: CacheRect,
    upload_offset_x: i32,
    upload_offset_y: i32,
    compressed_fingerprint: u64,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct MaskTextureCacheKey {
    layer_id: u32,
    mask_mipmap_id: u32,
    external_id: String,
    source_width: u32,
    source_height: u32,
    source_offset_x: i32,
    source_offset_y: i32,
    empty_fill: u8,
    decode_rect: CacheRect,
    upload_offset_x: i32,
    upload_offset_y: i32,
    fill_value: u8,
    compressed_fingerprint: u64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct CacheRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl From<Rect> for CacheRect {
    fn from(value: Rect) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
        }
    }
}

pub(super) fn raster_texture_cache_key(
    container: &clip_file::container::ClipContainer,
    source: &clip_file::metadata::RasterLayerSource,
    visible: crate::source_crop::RasterSourceDecodeRegion,
) -> Result<RasterTextureCacheKey, RuntimeError> {
    let color_type = source.color_type.unwrap_or(0);
    let per_tile_len = match color_type {
        0 => clip_file::tiles::RGBA_TILE_BYTES,
        1 => clip_file::tiles::GRAY_RGBA_TILE_BYTES,
        2 => clip_file::tiles::MONO_RGBA_TILE_BYTES,
        _ => {
            return Err(clip_file::ClipFileError::UnsupportedLayerColorType {
                layer_id: source.layer.id,
                color_type: source.color_type,
            }
            .into());
        }
    };
    Ok(RasterTextureCacheKey {
        layer_id: source.layer.id.0,
        render_mipmap_id: source.render_mipmap_id,
        external_id: source.external_id.clone(),
        source_width: source.pixel_size.width,
        source_height: source.pixel_size.height,
        source_offset_x: source.offset_x,
        source_offset_y: source.offset_y,
        color_type: source.color_type,
        decode_rect: visible.source_rect.into(),
        upload_offset_x: visible.offset_x,
        upload_offset_y: visible.offset_y,
        compressed_fingerprint: compressed_source_fingerprint(
            container,
            &source.external_id,
            source.pixel_size,
            visible.source_rect,
            per_tile_len,
        )?,
    })
}

pub(super) fn mask_texture_cache_key(
    container: &clip_file::container::ClipContainer,
    source: &clip_file::metadata::MaskLayerSource,
    payload: &super::MaskUploadPayload,
) -> Result<MaskTextureCacheKey, RuntimeError> {
    let decode_rect = Rect::new(
        payload.upload_origin_x,
        payload.upload_origin_y,
        payload.image.width,
        payload.image.height,
    );
    Ok(MaskTextureCacheKey {
        layer_id: source.layer_id.0,
        mask_mipmap_id: source.mask_mipmap_id,
        external_id: source.external_id.clone(),
        source_width: source.pixel_size.width,
        source_height: source.pixel_size.height,
        source_offset_x: source.offset_x,
        source_offset_y: source.offset_y,
        empty_fill: source.empty_fill,
        decode_rect: decode_rect.into(),
        upload_offset_x: payload.origin_x,
        upload_offset_y: payload.origin_y,
        fill_value: payload.fill_value,
        compressed_fingerprint: compressed_source_fingerprint(
            container,
            &source.external_id,
            source.pixel_size,
            decode_rect,
            clip_file::tiles::MASK_TILE_BYTES,
        )?,
    })
}

fn compressed_source_fingerprint(
    container: &clip_file::container::ClipContainer,
    external_id: &str,
    source_size: CanvasSize,
    source_rect: Rect,
    per_tile_len: usize,
) -> Result<u64, RuntimeError> {
    let body = container
        .external_data_body(external_id)
        .ok_or_else(|| clip_file::ClipFileError::MissingExternalData(external_id.to_owned()))?;
    let tile_cols = tile_cols(source_size.width)?;
    let expected_tiles = tile_count(source_size)?;
    let inspection = clip_file::external::inspect_external_tile_blocks_with_compressed_tiles(
        body,
        per_tile_len,
        expected_tiles,
        tile_cols,
    )?;
    let mut hash = Hash64::new();
    hash.str(external_id);
    hash.rect(source_rect);
    for tile in inspection
        .compressed_tiles
        .iter()
        .filter(|tile| tile_intersects_rect(**tile, source_size, source_rect))
    {
        hash.usize(tile.tile_x);
        hash.usize(tile.tile_y);
        hash.usize(tile.compressed_bytes);
        hash.u64(tile.compressed_hash);
    }
    Ok(hash.finish())
}

fn tile_intersects_rect(
    tile: clip_file::external::ExternalCompressedTile,
    source_size: CanvasSize,
    rect: Rect,
) -> bool {
    let tile_x0 = (tile.tile_x as u64).saturating_mul(clip_file::tiles::TILE_SIZE as u64);
    let tile_y0 = (tile.tile_y as u64).saturating_mul(clip_file::tiles::TILE_SIZE as u64);
    let tile_x1 = (tile_x0 + clip_file::tiles::TILE_SIZE as u64).min(u64::from(source_size.width));
    let tile_y1 = (tile_y0 + clip_file::tiles::TILE_SIZE as u64).min(u64::from(source_size.height));
    let rect_x0 = u64::from(rect.x);
    let rect_y0 = u64::from(rect.y);
    let rect_x1 = rect_x0 + u64::from(rect.width);
    let rect_y1 = rect_y0 + u64::from(rect.height);
    tile_x1 > rect_x0 && rect_x1 > tile_x0 && tile_y1 > rect_y0 && rect_y1 > tile_y0
}

fn tile_cols(width: u32) -> Result<usize, RuntimeError> {
    if width == 0 {
        return Err(RuntimeError::InvalidTileSize);
    }
    usize::try_from(width.div_ceil(clip_file::tiles::TILE_SIZE as u32))
        .map_err(|_| RuntimeError::InvalidTileSize)
}

fn tile_count(size: CanvasSize) -> Result<usize, RuntimeError> {
    let cols = u64::from(size.width.div_ceil(clip_file::tiles::TILE_SIZE as u32));
    let rows = u64::from(size.height.div_ceil(clip_file::tiles::TILE_SIZE as u32));
    usize::try_from(cols * rows).map_err(|_| RuntimeError::InvalidTileSize)
}

fn raster_cache_byte_len(cache: &clip_gpu::GpuRasterResourceCache) -> usize {
    cache.resource_infos().map(|info| info.byte_len).sum()
}

fn mask_cache_byte_len(cache: &clip_gpu::GpuMaskResourceCache) -> usize {
    cache.resource_infos().map(|info| info.byte_len).sum()
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

    fn str(&mut self, value: &str) {
        self.bytes(value.as_bytes());
        self.u8(0xff);
    }

    fn u8(&mut self, value: u8) {
        self.bytes(&[value]);
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }

    fn usize(&mut self, value: usize) {
        self.u64(value as u64);
    }

    fn rect(&mut self, value: Rect) {
        self.u32(value.x);
        self.u32(value.y);
        self.u32(value.width);
        self.u32(value.height);
    }
}
