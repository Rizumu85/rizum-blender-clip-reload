use clip_graph::RenderNodeKind;
use clip_model::CanvasSize;

use crate::RuntimeError;
use crate::reload_diff::{
    RELOAD_TILE_SIZE, ReloadDiffSegment, ReloadDiffSegmentResource, ReloadDiffSegmentTileRef,
    ReloadDiffSource, ReloadDiffTile, ReloadPatchRect,
};

pub(crate) fn raster_source_manifest(
    container: &clip_file::container::ClipContainer,
    canvas: CanvasSize,
    source: &clip_file::metadata::RasterLayerSource,
) -> Result<ReloadDiffSource, RuntimeError> {
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
    let body = container
        .external_data_body(&source.external_id)
        .ok_or_else(|| clip_file::ClipFileError::MissingExternalData(source.external_id.clone()))?;
    let tile_cols = tile_cols(source.pixel_size.width)?;
    let expected_tiles = tile_count(source.pixel_size)?;
    let inspection = clip_file::external::inspect_external_tile_blocks_with_compressed_tiles(
        body,
        per_tile_len,
        expected_tiles,
        tile_cols,
    )?;
    let tiles = compressed_tiles_to_manifest(
        &inspection.compressed_tiles,
        source.offset_x,
        source.offset_y,
        source.pixel_size,
        canvas,
    )?;
    Ok(ReloadDiffSource {
        kind: "raster".to_string(),
        layer_id: source.layer.id.0,
        resource_id: source.render_mipmap_id,
        external_id: source.external_id.clone(),
        offset_x: source.offset_x,
        offset_y: source.offset_y,
        width: source.pixel_size.width,
        height: source.pixel_size.height,
        color_type: source.color_type,
        empty_fill: None,
        signature: raster_source_signature(source),
        tiles,
    })
}

pub(crate) fn mask_source_manifest(
    container: &clip_file::container::ClipContainer,
    canvas: CanvasSize,
    source: &clip_file::metadata::MaskLayerSource,
) -> Result<ReloadDiffSource, RuntimeError> {
    let body = container
        .external_data_body(&source.external_id)
        .ok_or_else(|| clip_file::ClipFileError::MissingExternalData(source.external_id.clone()))?;
    let tile_cols = tile_cols(source.pixel_size.width)?;
    let expected_tiles = tile_count(source.pixel_size)?;
    let inspection = clip_file::external::inspect_external_tile_blocks_with_compressed_tiles(
        body,
        clip_file::tiles::MASK_TILE_BYTES,
        expected_tiles,
        tile_cols,
    )?;
    let tiles = compressed_tiles_to_manifest(
        &inspection.compressed_tiles,
        source.offset_x,
        source.offset_y,
        source.pixel_size,
        canvas,
    )?;
    Ok(ReloadDiffSource {
        kind: "mask".to_string(),
        layer_id: source.layer_id.0,
        resource_id: source.mask_mipmap_id,
        external_id: source.external_id.clone(),
        offset_x: source.offset_x,
        offset_y: source.offset_y,
        width: source.pixel_size.width,
        height: source.pixel_size.height,
        color_type: None,
        empty_fill: Some(source.empty_fill),
        signature: mask_source_signature(source),
        tiles,
    })
}

fn compressed_tiles_to_manifest(
    tiles: &[clip_file::external::ExternalCompressedTile],
    offset_x: i32,
    offset_y: i32,
    source_size: CanvasSize,
    canvas: CanvasSize,
) -> Result<Vec<ReloadDiffTile>, RuntimeError> {
    let mut manifest_tiles = Vec::with_capacity(tiles.len());
    for tile in tiles {
        let tile_x = u32::try_from(tile.tile_x).map_err(|_| RuntimeError::InvalidTileSize)?;
        let tile_y = u32::try_from(tile.tile_y).map_err(|_| RuntimeError::InvalidTileSize)?;
        let Some(rect) = tile_canvas_rect(offset_x, offset_y, source_size, canvas, tile_x, tile_y)
        else {
            continue;
        };
        manifest_tiles.push(ReloadDiffTile {
            tile_x,
            tile_y,
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            compressed_bytes: u32::try_from(tile.compressed_bytes)
                .map_err(|_| RuntimeError::InvalidTileSize)?,
            compressed_hash: tile.compressed_hash,
        });
    }
    Ok(manifest_tiles)
}

fn tile_canvas_rect(
    offset_x: i32,
    offset_y: i32,
    source_size: CanvasSize,
    canvas: CanvasSize,
    tile_x: u32,
    tile_y: u32,
) -> Option<ReloadPatchRect> {
    let source_x0 = i64::from(tile_x) * i64::from(RELOAD_TILE_SIZE);
    let source_y0 = i64::from(tile_y) * i64::from(RELOAD_TILE_SIZE);
    let source_x1 = (source_x0 + i64::from(RELOAD_TILE_SIZE)).min(i64::from(source_size.width));
    let source_y1 = (source_y0 + i64::from(RELOAD_TILE_SIZE)).min(i64::from(source_size.height));
    let canvas_x0 = (i64::from(offset_x) + source_x0).max(0);
    let canvas_y0 = (i64::from(offset_y) + source_y0).max(0);
    let canvas_x1 = (i64::from(offset_x) + source_x1).min(i64::from(canvas.width));
    let canvas_y1 = (i64::from(offset_y) + source_y1).min(i64::from(canvas.height));
    if canvas_x1 <= canvas_x0 || canvas_y1 <= canvas_y0 {
        return None;
    }
    Some(ReloadPatchRect {
        x: u32::try_from(canvas_x0).ok()?,
        y: u32::try_from(canvas_y0).ok()?,
        width: u32::try_from(canvas_x1 - canvas_x0).ok()?,
        height: u32::try_from(canvas_y1 - canvas_y0).ok()?,
    })
}

fn tile_cols(width: u32) -> Result<usize, RuntimeError> {
    if width == 0 {
        return Err(RuntimeError::InvalidTileSize);
    }
    usize::try_from(width.div_ceil(RELOAD_TILE_SIZE)).map_err(|_| RuntimeError::InvalidTileSize)
}

fn tile_count(size: CanvasSize) -> Result<usize, RuntimeError> {
    let cols = u64::from(size.width.div_ceil(RELOAD_TILE_SIZE));
    let rows = u64::from(size.height.div_ceil(RELOAD_TILE_SIZE));
    usize::try_from(cols * rows).map_err(|_| RuntimeError::InvalidTileSize)
}

pub(crate) fn render_node_kind_name(kind: RenderNodeKind) -> &'static str {
    match kind {
        RenderNodeKind::Container => "Container",
        RenderNodeKind::Paper => "Paper",
        RenderNodeKind::Raster => "Raster",
        RenderNodeKind::Filter => "Filter",
        RenderNodeKind::Unsupported(_) => "Unsupported",
    }
}

pub(crate) fn node_signature(node: &clip_graph::RenderNode) -> u64 {
    let mut hash = Hash64::new();
    hash.u32(node.layer_id.0);
    hash.str(render_node_kind_name(node.kind));
    if let RenderNodeKind::Unsupported(raw) = node.kind {
        hash.u32(raw);
    }
    hash.u16(node.depth);
    hash.bool(node.clip);
    hash.u16(node.opacity.0);
    hash.u32(node.composite);
    hash.opt_u32(node.render_mipmap_id);
    hash.opt_u32(node.mask_mipmap_id);
    if let Some(color) = node.paper_color {
        hash.bytes(&[color.r, color.g, color.b, color.a]);
    }
    hash.finish()
}

pub(crate) fn render_segment_manifest(
    segment: &clip_gpu::RenderProgramSegmentInfo,
    sources: &[ReloadDiffSource],
) -> ReloadDiffSegment {
    let barrier_reason = segment
        .barrier_reason
        .map(|reason| reason.as_str().to_string());
    let resources = segment
        .resources
        .iter()
        .map(|resource| ReloadDiffSegmentResource {
            kind: resource.kind.as_str().to_string(),
            layer_id: resource.layer_id,
            resource_id: resource.resource_id,
        })
        .collect::<Vec<_>>();
    let work_list = render_segment_tile_work_list(segment, sources);
    ReloadDiffSegment {
        ordinal: segment.ordinal,
        depth: segment.depth,
        source_start: segment.source_start,
        source_end: segment.source_end,
        kind: segment.kind.to_string(),
        barrier_reason,
        expected_passes: segment.expected_passes,
        tile_events: segment.tile_events,
        legacy_sources: segment.legacy_sources,
        resources,
        tile_work_list_source_count: work_list.source_count,
        tile_work_list_tile_count: work_list.tile_count,
        tile_work_list_signature: work_list.signature,
        tile_work_list: work_list.tiles,
        signature: render_segment_signature(segment),
    }
}

struct SegmentTileWorkList {
    source_count: u32,
    tile_count: u32,
    signature: u64,
    tiles: Vec<ReloadDiffSegmentTileRef>,
}

fn render_segment_tile_work_list(
    segment: &clip_gpu::RenderProgramSegmentInfo,
    sources: &[ReloadDiffSource],
) -> SegmentTileWorkList {
    let mut hash = Hash64::new();
    hash.u32(clip_gpu::TILE_EVENT_ABI_VERSION);
    hash.u32(segment.ordinal);
    hash.u16(segment.depth);
    let mut tiles = Vec::new();
    let mut source_count = 0u32;
    let mut tile_count = 0u32;
    for resource in &segment.resources {
        hash.str(resource.kind.as_str());
        hash.u32(resource.layer_id);
        hash.u32(resource.resource_id);
        if let Some(source) = sources.iter().find(|source| {
            source.kind == resource.kind.as_str()
                && source.layer_id == resource.layer_id
                && source.resource_id == resource.resource_id
        }) {
            source_count = source_count.saturating_add(1);
            hash.u32(usize_to_u32(source.tiles.len()));
            for tile in &source.tiles {
                hash.u32(tile.tile_x);
                hash.u32(tile.tile_y);
                hash.u32(tile.x);
                hash.u32(tile.y);
                hash.u32(tile.width);
                hash.u32(tile.height);
                tiles.push(ReloadDiffSegmentTileRef {
                    kind: source.kind.clone(),
                    layer_id: source.layer_id,
                    resource_id: source.resource_id,
                    tile_x: tile.tile_x,
                    tile_y: tile.tile_y,
                });
            }
            tile_count = tile_count.saturating_add(usize_to_u32(source.tiles.len()));
        } else {
            hash.u32(0);
        }
    }
    SegmentTileWorkList {
        source_count,
        tile_count,
        signature: hash.finish(),
        tiles,
    }
}

fn render_segment_signature(segment: &clip_gpu::RenderProgramSegmentInfo) -> u64 {
    let mut hash = Hash64::new();
    hash.u32(clip_gpu::TILE_EVENT_ABI_VERSION);
    hash.u32(segment.ordinal);
    hash.u16(segment.depth);
    hash.u32(segment.source_start);
    hash.u32(segment.source_end);
    hash.str(segment.kind);
    hash.opt_str(segment.barrier_reason.map(|reason| reason.as_str()));
    hash.u32(segment.expected_passes);
    hash.u32(segment.tile_events);
    hash.u32(segment.legacy_sources);
    hash.finish()
}

fn usize_to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn raster_source_signature(source: &clip_file::metadata::RasterLayerSource) -> u64 {
    let mut hash = Hash64::new();
    hash.str("raster");
    hash.u32(source.layer.id.0);
    hash.u32(source.render_mipmap_id);
    hash.u32(source.offscreen_id);
    hash.str(&source.external_id);
    hash.u32(source.pixel_size.width);
    hash.u32(source.pixel_size.height);
    hash.opt_u32(source.color_type);
    hash.i32(source.offset_x);
    hash.i32(source.offset_y);
    hash.finish()
}

fn mask_source_signature(source: &clip_file::metadata::MaskLayerSource) -> u64 {
    let mut hash = Hash64::new();
    hash.str("mask");
    hash.u32(source.layer_id.0);
    hash.u32(source.mask_mipmap_id);
    hash.u32(source.offscreen_id);
    hash.str(&source.external_id);
    hash.u32(source.pixel_size.width);
    hash.u32(source.pixel_size.height);
    hash.u8(source.empty_fill);
    hash.i32(source.offset_x);
    hash.i32(source.offset_y);
    hash.finish()
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

    fn u16(&mut self, value: u16) {
        self.bytes(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.bytes(&value.to_le_bytes());
    }

    fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    fn opt_u32(&mut self, value: Option<u32>) {
        match value {
            Some(value) => {
                self.u8(1);
                self.u32(value);
            }
            None => self.u8(0),
        }
    }

    fn opt_str(&mut self, value: Option<&str>) {
        match value {
            Some(value) => {
                self.u8(1);
                self.str(value);
            }
            None => self.u8(0),
        }
    }
}
