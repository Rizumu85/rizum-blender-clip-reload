use std::path::Path;

use clip_model::{CanvasSize, LayerId, Rect};

use crate::ClipFileError;
use crate::atlas_chunks::{RgbaAtlasChunkCollector, RgbaAtlasChunkTarget, RgbaAtlasTileChunk};
use crate::container::ClipContainer;
use crate::external::{decode_external_tile_blob, visit_external_non_empty_tile_block_selection};
use crate::metadata::{
    self, read_mask_layer_source_from_sqlite, read_raster_layer_source_from_sqlite,
};
use crate::placement::{place_alpha_on_canvas, place_rgba_on_canvas};
use crate::tile_region::{
    AlphaTileRegionWriter, TileBlockRef, TileRegionWriter, tile_block_selection_for_region,
};
use crate::tiles::{
    AlphaTileImage, GRAY_RGBA_TILE_BYTES, MASK_TILE_BYTES, MONO_RGBA_TILE_BYTES, RGBA_TILE_BYTES,
    RgbaTileImage, alpha_tile_blob_len, decode_alpha_tiles, decode_gray_rgba_tiles,
    decode_mono_rgba_tiles, decode_rgba_tiles, gray_rgba_tile_blob_len, mono_rgba_tile_blob_len,
    rgba_tile_blob_len,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipFileSummary {
    pub canvas: CanvasSize,
    pub root_layer_id: LayerId,
    pub layer_count: usize,
    pub external_data_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlacedRgbaTileImage {
    pub image: RgbaTileImage,
    pub offset_x: i32,
    pub offset_y: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RasterLayerSourceInfo {
    pub render_mipmap_id: u32,
    pub pixel_size: CanvasSize,
    pub offset_x: i32,
    pub offset_y: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RasterAtlasTileChunk {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub fn read_summary(path: impl AsRef<Path>) -> Result<ClipFileSummary, ClipFileError> {
    let container = ClipContainer::open(path)?;
    metadata::read_summary_from_sqlite(container.sqlite_bytes(), container.external_data().len())
}

pub fn read_raster_layer_rgba(
    path: impl AsRef<Path>,
    layer_id: LayerId,
) -> Result<RgbaTileImage, ClipFileError> {
    let container = ClipContainer::open(path)?;
    let summary = metadata::read_summary_from_sqlite(
        container.sqlite_bytes(),
        container.external_data().len(),
    )?;
    let placed =
        read_raster_layer_source_rgba_from_container(&container, summary.canvas, layer_id)?;
    place_rgba_on_canvas(
        placed.image,
        summary.canvas,
        placed.offset_x,
        placed.offset_y,
    )
}

pub fn read_raster_layer_source_rgba(
    path: impl AsRef<Path>,
    layer_id: LayerId,
) -> Result<PlacedRgbaTileImage, ClipFileError> {
    let container = ClipContainer::open(path)?;
    let summary = metadata::read_summary_from_sqlite(
        container.sqlite_bytes(),
        container.external_data().len(),
    )?;
    read_raster_layer_source_rgba_from_container(&container, summary.canvas, layer_id)
}

pub fn read_raster_layer_source_info(
    path: impl AsRef<Path>,
    layer_id: LayerId,
) -> Result<RasterLayerSourceInfo, ClipFileError> {
    let container = ClipContainer::open(path)?;
    let summary = metadata::read_summary_from_sqlite(
        container.sqlite_bytes(),
        container.external_data().len(),
    )?;
    read_raster_layer_source_info_from_container(&container, summary.canvas, layer_id)
}

pub fn read_layer_mask_alpha(
    path: impl AsRef<Path>,
    layer_id: LayerId,
) -> Result<AlphaTileImage, ClipFileError> {
    let container = ClipContainer::open(path)?;
    let summary = metadata::read_summary_from_sqlite(
        container.sqlite_bytes(),
        container.external_data().len(),
    )?;
    read_layer_mask_alpha_from_container(&container, summary.canvas, layer_id)
}

pub fn read_raster_layer_source_info_from_container(
    container: &ClipContainer,
    canvas_size: CanvasSize,
    layer_id: LayerId,
) -> Result<RasterLayerSourceInfo, ClipFileError> {
    let source =
        read_raster_layer_source_from_sqlite(container.sqlite_bytes(), layer_id, canvas_size)?;
    Ok(RasterLayerSourceInfo {
        render_mipmap_id: source.render_mipmap_id,
        pixel_size: source.pixel_size,
        offset_x: source.offset_x,
        offset_y: source.offset_y,
    })
}

pub fn read_raster_layer_source_rgba_from_container(
    container: &ClipContainer,
    canvas_size: CanvasSize,
    layer_id: LayerId,
) -> Result<PlacedRgbaTileImage, ClipFileError> {
    let source =
        read_raster_layer_source_from_sqlite(container.sqlite_bytes(), layer_id, canvas_size)?;
    read_resolved_raster_layer_source_rgba_from_container(container, &source)
}

pub fn read_resolved_raster_layer_source_rgba_from_container(
    container: &ClipContainer,
    source: &metadata::RasterLayerSource,
) -> Result<PlacedRgbaTileImage, ClipFileError> {
    let image = decode_raster_source_rgba(container, source, source.layer.id)?;
    Ok(PlacedRgbaTileImage {
        image,
        offset_x: source.offset_x,
        offset_y: source.offset_y,
    })
}

pub fn read_resolved_raster_layer_source_rgba_region_from_container(
    container: &ClipContainer,
    source: &metadata::RasterLayerSource,
    source_region: Rect,
) -> Result<RgbaTileImage, ClipFileError> {
    decode_raster_source_rgba_region(container, source, source.layer.id, source_region)
}

pub fn read_resolved_raster_layer_source_rgba_region_atlas_chunks_from_container(
    container: &ClipContainer,
    source: &metadata::RasterLayerSource,
    source_region: Rect,
    atlas_size: CanvasSize,
    atlas_origin_x: u32,
    atlas_origin_y: u32,
) -> Result<Vec<RasterAtlasTileChunk>, ClipFileError> {
    read_raster_source_rgba_region_atlas_chunks(
        container,
        source,
        source.layer.id,
        source_region,
        RgbaAtlasChunkTarget {
            size: atlas_size,
            origin_x: atlas_origin_x,
            origin_y: atlas_origin_y,
        },
    )
}

pub fn read_layer_mask_alpha_from_container(
    container: &ClipContainer,
    canvas_size: CanvasSize,
    layer_id: LayerId,
) -> Result<AlphaTileImage, ClipFileError> {
    let source =
        read_mask_layer_source_from_sqlite(container.sqlite_bytes(), layer_id, canvas_size)?;
    read_resolved_layer_mask_alpha_from_container(container, canvas_size, &source)
}

pub fn read_resolved_layer_mask_alpha_from_container(
    container: &ClipContainer,
    canvas_size: CanvasSize,
    source: &metadata::MaskLayerSource,
) -> Result<AlphaTileImage, ClipFileError> {
    let alpha = decode_mask_source_alpha(container, source)?;
    place_alpha_on_canvas(
        alpha,
        canvas_size,
        source.offset_x,
        source.offset_y,
        source.empty_fill,
    )
}

pub fn read_resolved_layer_mask_alpha_region_from_container(
    container: &ClipContainer,
    source: &metadata::MaskLayerSource,
    source_region: Rect,
) -> Result<AlphaTileImage, ClipFileError> {
    decode_mask_source_alpha_region(container, source, source_region)
}

fn decode_mask_source_alpha(
    container: &ClipContainer,
    source: &metadata::MaskLayerSource,
) -> Result<AlphaTileImage, ClipFileError> {
    let body = container
        .external_data_body(&source.external_id)
        .ok_or_else(|| ClipFileError::MissingExternalData(source.external_id.clone()))?;
    let expected_len = alpha_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?;
    let blob = decode_external_tile_blob(body, source.empty_fill, Some(expected_len))?;
    decode_alpha_tiles(
        &blob.bytes,
        source.pixel_size.width,
        source.pixel_size.height,
    )
}

fn decode_mask_source_alpha_region(
    container: &ClipContainer,
    source: &metadata::MaskLayerSource,
    source_region: Rect,
) -> Result<AlphaTileImage, ClipFileError> {
    let body = container
        .external_data_body(&source.external_id)
        .ok_or_else(|| ClipFileError::MissingExternalData(source.external_id.clone()))?;
    let expected_len = alpha_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?;
    let tile_selection = tile_block_selection_for_region(
        source.pixel_size.width,
        source.pixel_size.height,
        source_region,
    )?;
    let expected_tile_count = expected_len / MASK_TILE_BYTES;
    let mut writer = AlphaTileRegionWriter::new_with_fill(
        source.pixel_size.width,
        source.pixel_size.height,
        source_region,
        source.empty_fill,
    )?;
    visit_external_non_empty_tile_block_selection(
        body,
        MASK_TILE_BYTES,
        expected_tile_count,
        tile_selection,
        |tile_index, bytes| writer.write_alpha_block(TileBlockRef { tile_index, bytes }),
    )?;
    writer.finish()
}

fn decode_raster_source_rgba(
    container: &ClipContainer,
    source: &metadata::RasterLayerSource,
    layer_id: LayerId,
) -> Result<RgbaTileImage, ClipFileError> {
    let body = container
        .external_data_body(&source.external_id)
        .ok_or_else(|| ClipFileError::MissingExternalData(source.external_id.clone()))?;
    let color_type = source.color_type.unwrap_or(0);
    let expected_len = match color_type {
        0 => rgba_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?,
        1 => gray_rgba_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?,
        2 => mono_rgba_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?,
        _ => {
            return Err(ClipFileError::UnsupportedLayerColorType {
                layer_id,
                color_type: source.color_type,
            });
        }
    };
    let blob = decode_external_tile_blob(body, 0, Some(expected_len))?;
    match color_type {
        0 => decode_rgba_tiles(
            &blob.bytes,
            source.pixel_size.width,
            source.pixel_size.height,
        ),
        1 => decode_gray_rgba_tiles(
            &blob.bytes,
            source.pixel_size.width,
            source.pixel_size.height,
        ),
        2 => decode_mono_rgba_tiles(
            &blob.bytes,
            source.pixel_size.width,
            source.pixel_size.height,
        ),
        _ => unreachable!("unsupported raster color type should have returned earlier"),
    }
}

fn decode_raster_source_rgba_region(
    container: &ClipContainer,
    source: &metadata::RasterLayerSource,
    layer_id: LayerId,
    source_region: Rect,
) -> Result<RgbaTileImage, ClipFileError> {
    let body = container
        .external_data_body(&source.external_id)
        .ok_or_else(|| ClipFileError::MissingExternalData(source.external_id.clone()))?;
    let color_type = source.color_type.unwrap_or(0);
    let (expected_len, per_tile_len) = match color_type {
        0 => (
            rgba_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?,
            RGBA_TILE_BYTES,
        ),
        1 => (
            gray_rgba_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?,
            GRAY_RGBA_TILE_BYTES,
        ),
        2 => (
            mono_rgba_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?,
            MONO_RGBA_TILE_BYTES,
        ),
        _ => {
            return Err(ClipFileError::UnsupportedLayerColorType {
                layer_id,
                color_type: source.color_type,
            });
        }
    };
    let tile_selection = tile_block_selection_for_region(
        source.pixel_size.width,
        source.pixel_size.height,
        source_region,
    )?;
    let expected_tile_count = expected_len / per_tile_len;
    let mut writer = TileRegionWriter::new(
        source.pixel_size.width,
        source.pixel_size.height,
        source_region,
    )?;
    visit_external_non_empty_tile_block_selection(
        body,
        per_tile_len,
        expected_tile_count,
        tile_selection,
        |tile_index, bytes| {
            let block = TileBlockRef { tile_index, bytes };
            match color_type {
                0 => writer.write_rgba_block(block),
                1 => writer.write_gray_block(block),
                2 => writer.write_mono_block(block),
                _ => unreachable!("unsupported raster color type should have returned earlier"),
            }
        },
    )?;
    writer.finish()
}

fn read_raster_source_rgba_region_atlas_chunks(
    container: &ClipContainer,
    source: &metadata::RasterLayerSource,
    layer_id: LayerId,
    source_region: Rect,
    target: RgbaAtlasChunkTarget,
) -> Result<Vec<RasterAtlasTileChunk>, ClipFileError> {
    let body = container
        .external_data_body(&source.external_id)
        .ok_or_else(|| ClipFileError::MissingExternalData(source.external_id.clone()))?;
    let color_type = source.color_type.unwrap_or(0);
    let (expected_len, per_tile_len) = match color_type {
        0 => (
            rgba_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?,
            RGBA_TILE_BYTES,
        ),
        1 => (
            gray_rgba_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?,
            GRAY_RGBA_TILE_BYTES,
        ),
        2 => (
            mono_rgba_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?,
            MONO_RGBA_TILE_BYTES,
        ),
        _ => {
            return Err(ClipFileError::UnsupportedLayerColorType {
                layer_id,
                color_type: source.color_type,
            });
        }
    };
    let expected_tile_count = expected_len / per_tile_len;
    let mut collector = RgbaAtlasChunkCollector::new(
        source.pixel_size.width,
        source.pixel_size.height,
        source_region,
        target,
    )?;
    let tile_selection = collector.selection()?;
    visit_external_non_empty_tile_block_selection(
        body,
        per_tile_len,
        expected_tile_count,
        tile_selection,
        |tile_index, bytes| {
            let block = TileBlockRef { tile_index, bytes };
            match color_type {
                0 => collector.write_rgba_block(block),
                1 => collector.write_gray_block(block),
                2 => collector.write_mono_block(block),
                _ => unreachable!("unsupported raster color type should have returned earlier"),
            }
        },
    )?;
    Ok(collector
        .finish()
        .into_iter()
        .map(raster_atlas_tile_chunk)
        .collect())
}

fn raster_atlas_tile_chunk(chunk: RgbaAtlasTileChunk) -> RasterAtlasTileChunk {
    RasterAtlasTileChunk {
        x: chunk.x,
        y: chunk.y,
        width: chunk.width,
        height: chunk.height,
        pixels: chunk.pixels,
    }
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
