#![forbid(unsafe_code)]

pub mod container;
pub mod external;
pub mod metadata;
pub mod tiles;

use std::error::Error;
use std::fmt;
use std::path::Path;

use clip_model::{CanvasSize, LayerId};
use container::{ChunkKind, ClipContainer};
use external::decode_external_tile_blob;
use metadata::{read_mask_layer_source_from_sqlite, read_raster_layer_source_from_sqlite};
use tiles::{
    AlphaTileImage, RgbaTileImage, alpha_tile_blob_len, decode_alpha_tiles, decode_gray_rgba_tiles,
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
    let body = container
        .external_data_body(&source.external_id)
        .ok_or_else(|| ClipFileError::MissingExternalData(source.external_id.clone()))?;
    let expected_len = alpha_tile_blob_len(source.pixel_size.width, source.pixel_size.height)?;
    let blob = decode_external_tile_blob(body, source.empty_fill, Some(expected_len))?;
    let alpha = decode_alpha_tiles(
        &blob.bytes,
        source.pixel_size.width,
        source.pixel_size.height,
    )?;
    place_alpha_on_canvas(
        alpha,
        canvas_size,
        source.offset_x,
        source.offset_y,
        source.empty_fill,
    )
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

fn place_alpha_on_canvas(
    alpha: AlphaTileImage,
    canvas_size: CanvasSize,
    offset_x: i32,
    offset_y: i32,
    empty_fill: u8,
) -> Result<AlphaTileImage, ClipFileError> {
    if alpha.width == canvas_size.width
        && alpha.height == canvas_size.height
        && offset_x == 0
        && offset_y == 0
    {
        return Ok(alpha);
    }

    if offset_x == 0
        && offset_y == 0
        && alpha.width >= canvas_size.width
        && alpha.height >= canvas_size.height
    {
        return crop_alpha_top_left(alpha, canvas_size);
    }

    let canvas_width =
        usize::try_from(canvas_size.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let canvas_height =
        usize::try_from(canvas_size.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let alpha_width = usize::try_from(alpha.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let alpha_height =
        usize::try_from(alpha.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let mut pixels = vec![
        empty_fill;
        canvas_width
            .checked_mul(canvas_height)
            .ok_or(ClipFileError::TileSizeOverflow)?
    ];

    let src_x0 =
        usize::try_from((-offset_x).max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let src_y0 =
        usize::try_from((-offset_y).max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let dst_x0 = usize::try_from(offset_x.max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let dst_y0 = usize::try_from(offset_y.max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let paste_w = alpha_width
        .saturating_sub(src_x0)
        .min(canvas_width.saturating_sub(dst_x0));
    let paste_h = alpha_height
        .saturating_sub(src_y0)
        .min(canvas_height.saturating_sub(dst_y0));

    for row in 0..paste_h {
        let src_start = (src_y0 + row) * alpha_width + src_x0;
        let src_end = src_start + paste_w;
        let dst_start = (dst_y0 + row) * canvas_width + dst_x0;
        let dst_end = dst_start + paste_w;
        pixels[dst_start..dst_end].copy_from_slice(&alpha.pixels[src_start..src_end]);
    }

    Ok(AlphaTileImage {
        width: canvas_size.width,
        height: canvas_size.height,
        pixels,
    })
}

fn place_rgba_on_canvas(
    rgba: RgbaTileImage,
    canvas_size: CanvasSize,
    offset_x: i32,
    offset_y: i32,
) -> Result<RgbaTileImage, ClipFileError> {
    if rgba.width == canvas_size.width
        && rgba.height == canvas_size.height
        && offset_x == 0
        && offset_y == 0
    {
        return Ok(rgba);
    }

    if offset_x == 0
        && offset_y == 0
        && rgba.width >= canvas_size.width
        && rgba.height >= canvas_size.height
    {
        return crop_rgba_top_left(rgba, canvas_size);
    }

    let canvas_width =
        usize::try_from(canvas_size.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let canvas_height =
        usize::try_from(canvas_size.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let rgba_width = usize::try_from(rgba.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let rgba_height = usize::try_from(rgba.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let pixel_count = canvas_width
        .checked_mul(canvas_height)
        .ok_or(ClipFileError::TileSizeOverflow)?;
    let mut pixels = vec![
        0u8;
        pixel_count
            .checked_mul(4)
            .ok_or(ClipFileError::TileSizeOverflow)?
    ];
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[0] = 255;
        pixel[1] = 255;
        pixel[2] = 255;
    }

    let src_x0 =
        usize::try_from((-offset_x).max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let src_y0 =
        usize::try_from((-offset_y).max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let dst_x0 = usize::try_from(offset_x.max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let dst_y0 = usize::try_from(offset_y.max(0)).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let paste_w = rgba_width
        .saturating_sub(src_x0)
        .min(canvas_width.saturating_sub(dst_x0));
    let paste_h = rgba_height
        .saturating_sub(src_y0)
        .min(canvas_height.saturating_sub(dst_y0));

    for row in 0..paste_h {
        let src_start = ((src_y0 + row) * rgba_width + src_x0) * 4;
        let src_end = src_start + paste_w * 4;
        let dst_start = ((dst_y0 + row) * canvas_width + dst_x0) * 4;
        let dst_end = dst_start + paste_w * 4;
        pixels[dst_start..dst_end].copy_from_slice(&rgba.pixels[src_start..src_end]);
    }

    Ok(RgbaTileImage {
        width: canvas_size.width,
        height: canvas_size.height,
        pixels,
    })
}

fn crop_alpha_top_left(
    alpha: AlphaTileImage,
    canvas_size: CanvasSize,
) -> Result<AlphaTileImage, ClipFileError> {
    let canvas_width =
        usize::try_from(canvas_size.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let canvas_height =
        usize::try_from(canvas_size.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let alpha_width = usize::try_from(alpha.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let mut pixels = vec![
        0u8;
        canvas_width
            .checked_mul(canvas_height)
            .ok_or(ClipFileError::TileSizeOverflow)?
    ];
    for row in 0..canvas_height {
        let src_start = row * alpha_width;
        let src_end = src_start + canvas_width;
        let dst_start = row * canvas_width;
        let dst_end = dst_start + canvas_width;
        pixels[dst_start..dst_end].copy_from_slice(&alpha.pixels[src_start..src_end]);
    }
    Ok(AlphaTileImage {
        width: canvas_size.width,
        height: canvas_size.height,
        pixels,
    })
}

fn crop_rgba_top_left(
    rgba: RgbaTileImage,
    canvas_size: CanvasSize,
) -> Result<RgbaTileImage, ClipFileError> {
    let canvas_width =
        usize::try_from(canvas_size.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let canvas_height =
        usize::try_from(canvas_size.height).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let rgba_width = usize::try_from(rgba.width).map_err(|_| ClipFileError::TileSizeOverflow)?;
    let pixel_count = canvas_width
        .checked_mul(canvas_height)
        .ok_or(ClipFileError::TileSizeOverflow)?;
    let mut pixels = vec![
        0u8;
        pixel_count
            .checked_mul(4)
            .ok_or(ClipFileError::TileSizeOverflow)?
    ];
    for row in 0..canvas_height {
        let src_start = row * rgba_width * 4;
        let src_end = src_start + canvas_width * 4;
        let dst_start = row * canvas_width * 4;
        let dst_end = dst_start + canvas_width * 4;
        pixels[dst_start..dst_end].copy_from_slice(&rgba.pixels[src_start..src_end]);
    }
    Ok(RgbaTileImage {
        width: canvas_size.width,
        height: canvas_size.height,
        pixels,
    })
}

#[derive(Debug)]
pub enum ClipFileError {
    Io(std::io::Error),
    InvalidMagic,
    TruncatedChunkHeader {
        offset: usize,
    },
    TruncatedChunkBody {
        offset: usize,
        kind: ChunkKind,
        declared_len: usize,
        available: usize,
    },
    ChunkOffsetOverflow {
        offset: usize,
    },
    ChunkTooLarge {
        offset: usize,
        kind: ChunkKind,
    },
    MissingSqliteChunk,
    InvalidExternalDataHeader,
    InvalidExternalDataBlock(&'static str),
    UnsupportedExternalDataBlock(String),
    Sqlite(rusqlite::Error),
    Zlib(std::io::Error),
    InvalidMetadata(&'static str),
    MissingLayer(LayerId),
    LayerIsNotRaster {
        layer_id: LayerId,
        layer_type: u32,
    },
    LayerHasNoMask {
        layer_id: LayerId,
    },
    LayerHasNoFilterInfo {
        layer_id: LayerId,
    },
    MalformedFilterLayerInfo {
        layer_id: LayerId,
    },
    UnsupportedLayerColorType {
        layer_id: LayerId,
        color_type: Option<u32>,
    },
    MissingExternalData(String),
    UnexpectedDecompressedSize {
        expected: usize,
        actual: usize,
    },
    InvalidTileBlobSize {
        actual: usize,
        per_tile: usize,
    },
    UnexpectedTileCount {
        expected: usize,
        actual: usize,
    },
    TileSizeOverflow,
}

impl fmt::Display for ClipFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::InvalidMagic => f.write_str("not a CLIP file: missing CSFCHUNK magic"),
            Self::TruncatedChunkHeader { offset } => {
                write!(f, "truncated chunk header at byte offset {offset}")
            }
            Self::TruncatedChunkBody {
                offset,
                kind,
                declared_len,
                available,
            } => write!(
                f,
                "truncated {kind} chunk body at byte offset {offset}: declared {declared_len} bytes, only {available} available",
            ),
            Self::ChunkOffsetOverflow { offset } => {
                write!(f, "chunk offset overflow near byte offset {offset}")
            }
            Self::ChunkTooLarge { offset, kind } => {
                write!(
                    f,
                    "{kind} chunk at byte offset {offset} is too large for this platform"
                )
            }
            Self::MissingSqliteChunk => f.write_str("CLIP file has no CHNKSQLi chunk"),
            Self::InvalidExternalDataHeader => f.write_str("invalid CHNKExta external id header"),
            Self::InvalidExternalDataBlock(reason) => {
                write!(f, "invalid CHNKExta block: {reason}")
            }
            Self::UnsupportedExternalDataBlock(name) => {
                write!(f, "unsupported CHNKExta block {name}")
            }
            Self::Sqlite(err) => write!(f, "SQLite metadata error: {err}"),
            Self::Zlib(err) => write!(f, "zlib decode error: {err}"),
            Self::InvalidMetadata(field) => write!(f, "invalid metadata field {field}"),
            Self::MissingLayer(layer_id) => write!(f, "missing layer {}", layer_id.0),
            Self::LayerIsNotRaster {
                layer_id,
                layer_type,
            } => write!(
                f,
                "layer {} is not a raster layer, LayerType={layer_type}",
                layer_id.0,
            ),
            Self::LayerHasNoMask { layer_id } => {
                write!(f, "layer {} has no layer mask mipmap", layer_id.0)
            }
            Self::LayerHasNoFilterInfo { layer_id } => {
                write!(f, "layer {} has no filter layer info", layer_id.0)
            }
            Self::MalformedFilterLayerInfo { layer_id } => {
                write!(f, "layer {} has malformed filter layer info", layer_id.0)
            }
            Self::UnsupportedLayerColorType {
                layer_id,
                color_type,
            } => write!(
                f,
                "layer {} has unsupported color type {:?}",
                layer_id.0, color_type,
            ),
            Self::MissingExternalData(external_id) => {
                write!(f, "missing external data body {external_id}")
            }
            Self::UnexpectedDecompressedSize { expected, actual } => write!(
                f,
                "unexpected decompressed size: expected {expected} bytes, got {actual}",
            ),
            Self::InvalidTileBlobSize { actual, per_tile } => write!(
                f,
                "invalid tile blob size: {actual} bytes is not a multiple of {per_tile}",
            ),
            Self::UnexpectedTileCount { expected, actual } => {
                write!(
                    f,
                    "unexpected tile count: expected {expected}, got {actual}"
                )
            }
            Self::TileSizeOverflow => f.write_str("tile size calculation overflow"),
        }
    }
}

impl Error for ClipFileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Sqlite(err) => Some(err),
            Self::Zlib(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ClipFileError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<rusqlite::Error> for ClipFileError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

impl From<std::string::FromUtf16Error> for ClipFileError {
    fn from(_value: std::string::FromUtf16Error) -> Self {
        Self::InvalidExternalDataBlock("invalid utf16")
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::container::ClipContainer;
    use crate::external::decode_external_tile_blob;
    use crate::metadata::{
        read_filter_layer_source_from_sqlite, read_layer_graph_records_from_sqlite,
        read_raster_layer_source_from_sqlite,
    };
    use crate::tiles::{decode_rgba_tiles, rgba_tile_blob_len};

    use super::*;

    #[test]
    fn reads_test_clipping_summary() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
        let summary = read_summary(path).expect("read Test_Clipping.clip summary");

        assert_eq!(summary.canvas, CanvasSize::new(512, 512));
        assert_eq!(summary.root_layer_id, LayerId(2));
        assert_eq!(summary.layer_count, 4);
        assert_eq!(summary.external_data_count, 7);
    }

    #[test]
    fn reads_test_clipping_layer_graph_records() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
        let container = ClipContainer::open(path).expect("open Test_Clipping.clip container");
        let records =
            read_layer_graph_records_from_sqlite(container.sqlite_bytes()).expect("read layers");

        assert_eq!(records.len(), 4);
        assert_eq!(records[0].id, LayerId(2));
        assert_eq!(records[0].kind, clip_model::LayerKind::Folder);
        assert_eq!(records[0].first_child_layer_id, Some(LayerId(4)));
        assert_eq!(records[0].next_layer_id, None);

        assert_eq!(records[1].id, LayerId(4));
        assert_eq!(records[1].kind, clip_model::LayerKind::Paper);
        assert_eq!(records[1].next_layer_id, Some(LayerId(10)));
        assert_eq!(
            records[1].paper_color,
            Some(clip_model::Rgba8 {
                r: 226,
                g: 226,
                b: 226,
                a: 255,
            }),
        );

        assert_eq!(records[2].id, LayerId(10));
        assert_eq!(records[2].kind, clip_model::LayerKind::Raster);
        assert_eq!(records[2].next_layer_id, Some(LayerId(11)));
        assert_eq!(records[2].render_mipmap_id, Some(15));

        assert_eq!(records[3].id, LayerId(11));
        assert_eq!(records[3].kind, clip_model::LayerKind::Raster);
        assert_eq!(records[3].next_layer_id, None);
        assert_eq!(records[3].render_mipmap_id, Some(16));
    }

    #[test]
    fn decodes_test_clipping_layer_10_rgba_tiles() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
        let container = ClipContainer::open(path).expect("open Test_Clipping.clip container");
        let summary = metadata::read_summary_from_sqlite(
            container.sqlite_bytes(),
            container.external_data().len(),
        )
        .expect("read summary");
        let source = read_raster_layer_source_from_sqlite(
            container.sqlite_bytes(),
            LayerId(10),
            summary.canvas,
        )
        .expect("read raster layer source");

        assert_eq!(source.layer.kind, clip_model::LayerKind::Raster);
        assert!(source.layer.visibility.is_visible());
        assert_eq!(source.render_mipmap_id, 15);
        assert_eq!(source.offscreen_id, 62);
        assert_eq!(
            source.external_id,
            "extrnlid7A4545CCDE9D4E579B1230B4DB88B130",
        );
        assert_eq!(source.pixel_size, CanvasSize::new(512, 512));
        assert_eq!(source.color_type, Some(0));
        assert_eq!(source.offset_x, 0);
        assert_eq!(source.offset_y, 0);

        let body = container
            .external_data_body(&source.external_id)
            .ok_or_else(|| ClipFileError::MissingExternalData(source.external_id.clone()))
            .expect("external data body");
        let expected_len =
            rgba_tile_blob_len(source.pixel_size.width, source.pixel_size.height).unwrap();
        let blob = decode_external_tile_blob(body, 0, Some(expected_len))
            .expect("decode external tile blob");
        assert_eq!(blob.external_id, source.external_id);
        assert_eq!(blob.bytes.len(), 1_310_720);

        let image = decode_rgba_tiles(
            &blob.bytes,
            source.pixel_size.width,
            source.pixel_size.height,
        )
        .expect("decode rgba tiles");
        assert_eq!(image.width, 512);
        assert_eq!(image.height, 512);

        assert_eq!(pixel_at(&image.pixels, 512, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel_at(&image.pixels, 512, 100, 100), [0, 0, 0, 0]);

        let mut alpha_count = 0usize;
        let mut sums = [0u64; 4];
        for pixel in image.pixels.chunks_exact(4) {
            if pixel[3] > 0 {
                alpha_count += 1;
            }
            for channel in 0..4 {
                sums[channel] += u64::from(pixel[channel]);
            }
        }
        assert_eq!(alpha_count, 37_151);
        assert_eq!(sums, [8_507_579, 5_832_707, 5_832_707, 8_933_976]);
    }

    #[test]
    fn reads_ref_terra_render_offsets() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Ref_Terra404_Live2D.clip");
        let container = ClipContainer::open(path).expect("open Ref_Terra404_Live2D.clip");
        let summary = metadata::read_summary_from_sqlite(
            container.sqlite_bytes(),
            container.external_data().len(),
        )
        .expect("read summary");

        let layer_4 = read_raster_layer_source_from_sqlite(
            container.sqlite_bytes(),
            LayerId(4),
            summary.canvas,
        )
        .expect("read raster layer 4 source");
        let layer_5 = read_raster_layer_source_from_sqlite(
            container.sqlite_bytes(),
            LayerId(5),
            summary.canvas,
        )
        .expect("read raster layer 5 source");

        assert_eq!(layer_4.offset_x, -768);
        assert_eq!(layer_4.offset_y, 0);
        assert_eq!(layer_4.color_type, Some(0));
        assert_eq!(layer_5.offset_x, -1280);
        assert_eq!(layer_5.offset_y, 0);
        assert_eq!(layer_5.color_type, Some(0));
    }

    #[test]
    fn public_api_decodes_test_clipping_layer_11() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Clipping.clip");
        let image = read_raster_layer_rgba(path, LayerId(11)).expect("decode layer 11");

        assert_eq!(image.width, 512);
        assert_eq!(image.height, 512);
        assert_eq!(pixel_at(&image.pixels, 512, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel_at(&image.pixels, 512, 100, 100), [80, 70, 229, 255],);
        assert_eq!(pixel_at(&image.pixels, 512, 300, 300), [80, 70, 229, 255],);

        let alpha_count = image
            .pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[3] > 0)
            .count();
        assert_eq!(alpha_count, 196_553);
    }

    #[test]
    fn public_api_decodes_test_mask_layer_5_mask() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Mask.clip");
        let mask = read_layer_mask_alpha(path, LayerId(5)).expect("decode layer 5 mask");

        assert_eq!(mask.width, 512);
        assert_eq!(mask.height, 512);
        assert_eq!(mask.pixels[0], 255);
        assert_eq!(mask.pixels[100 * 512 + 100], 255);
        assert_eq!(mask.pixels[256 * 512 + 256], 255);

        let nonzero = mask.pixels.iter().filter(|value| **value > 0).count();
        let sum: u64 = mask.pixels.iter().map(|value| u64::from(*value)).sum();
        assert_eq!(nonzero, 227_309);
        assert_eq!(sum, 57_888_516);
    }

    #[test]
    fn public_api_decodes_test_grayscale_layer_5() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_ Grayscale.clip");
        let image = read_raster_layer_rgba(path, LayerId(5)).expect("decode grayscale layer 5");

        assert_eq!(image.width, 1024);
        assert_eq!(image.height, 1024);
        assert_eq!(pixel_at(&image.pixels, 1024, 0, 0), [129, 129, 129, 38]);
        assert_eq!(pixel_at(&image.pixels, 1024, 100, 100), [129, 129, 129, 34],);
        assert_eq!(pixel_at(&image.pixels, 1024, 300, 300), [129, 129, 129, 26],);
        assert_eq!(pixel_at(&image.pixels, 1024, 512, 512), [129, 129, 129, 18],);
        let (nonzero, sums) = rgba_stats(&image.pixels);
        assert_eq!(nonzero, 701_073);
        assert_eq!(sums, [93_813_265, 93_813_265, 93_813_265, 70_251_599]);
    }

    #[test]
    fn public_api_decodes_test_monochrome_layer_7() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Monochrome.clip");
        let image = read_raster_layer_rgba(path, LayerId(7)).expect("decode monochrome layer 7");

        assert_eq!(image.width, 1024);
        assert_eq!(image.height, 1024);
        assert_eq!(pixel_at(&image.pixels, 1024, 0, 0), [255, 255, 255, 255],);
        assert_eq!(
            pixel_at(&image.pixels, 1024, 100, 100),
            [255, 255, 255, 255],
        );
        assert_eq!(
            pixel_at(&image.pixels, 1024, 300, 300),
            [255, 255, 255, 255],
        );
        assert_eq!(pixel_at(&image.pixels, 1024, 512, 512), [0, 0, 0, 0]);
        let (nonzero, sums) = rgba_stats(&image.pixels);
        assert_eq!(nonzero, 422_394);
        assert_eq!(sums, [104_312_850, 104_312_850, 104_312_850, 107_710_470],);
    }

    #[test]
    fn reads_test_tone_curve_filter_payload() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_ToneCurve.clip");
        let container = ClipContainer::open(path).expect("open Test_ToneCurve.clip container");
        let filter = read_filter_layer_source_from_sqlite(container.sqlite_bytes(), LayerId(6))
            .expect("read tone curve filter source");

        assert_eq!(filter.layer_id, LayerId(6));
        assert_eq!(filter.filter_type, 3);
        assert_eq!(filter.payload.len(), 4160);
    }

    #[test]
    fn reads_test_gradiation_filter_payload() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../img/Test_Gradiation.clip");
        let container = ClipContainer::open(path).expect("open Test_Gradiation.clip container");
        let filter = read_filter_layer_source_from_sqlite(container.sqlite_bytes(), LayerId(6))
            .expect("read gradient map filter source");

        assert_eq!(filter.layer_id, LayerId(6));
        assert_eq!(filter.filter_type, 9);
        assert_eq!(filter.payload.len(), 280);
    }

    fn pixel_at(pixels: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
        let offset = (y * width + x) * 4;
        pixels[offset..offset + 4].try_into().unwrap()
    }

    fn rgba_stats(pixels: &[u8]) -> (usize, [u64; 4]) {
        let mut nonzero = 0usize;
        let mut sums = [0u64; 4];
        for pixel in pixels.chunks_exact(4) {
            if pixel[3] > 0 {
                nonzero += 1;
            }
            for channel in 0..4 {
                sums[channel] += u64::from(pixel[channel]);
            }
        }
        (nonzero, sums)
    }
}
