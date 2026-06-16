use std::fmt;

use clip_graph::{RenderNodeId, RenderNodeKind};
use clip_model::{CanvasSize, LayerId, Rgba8};

#[derive(Debug)]
pub struct SimpleRasterStackGpuResult {
    pub image: Option<clip_file::tiles::RgbaTileImage>,
    pub drawn_resources: Vec<clip_gpu::GpuRasterResourceInfo>,
    pub unsupported: Vec<SimpleRasterStackUnsupported>,
    pub differing_bytes_from_last_drawn: Option<usize>,
}

#[derive(Debug)]
pub struct NormalRasterStackGpuResult {
    pub image: Option<clip_file::tiles::RgbaTileImage>,
    pub source_count: usize,
    pub resource_stats: NormalRasterStackResourceStats,
    pub drawn_resources: Vec<clip_gpu::GpuRasterResourceInfo>,
    pub mask_resources: Vec<clip_gpu::GpuMaskResourceInfo>,
    pub unsupported: Vec<SimpleRasterStackUnsupported>,
}

#[derive(Debug)]
pub struct NormalRasterStackSupportResult {
    pub source_count: usize,
    pub resource_stats: NormalRasterStackResourceStats,
    pub unsupported: Vec<SimpleRasterStackUnsupported>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NormalRasterStackResourceStats {
    pub raster_count: usize,
    pub raster_bytes: u64,
    pub max_raster_layer_id: Option<LayerId>,
    pub max_raster_width: u32,
    pub max_raster_height: u32,
    pub max_raster_bytes: u64,
    pub mask_count: usize,
    pub mask_bytes: u64,
    pub max_mask_layer_id: Option<LayerId>,
    pub max_mask_width: u32,
    pub max_mask_height: u32,
    pub max_mask_bytes: u64,
}

impl NormalRasterStackResourceStats {
    pub(crate) fn add_raster_source(&mut self, source: &clip_file::metadata::RasterLayerSource) {
        let bytes = u64::from(source.pixel_size.width) * u64::from(source.pixel_size.height) * 4;
        self.raster_count += 1;
        self.raster_bytes += bytes;
        if bytes > self.max_raster_bytes {
            self.max_raster_bytes = bytes;
            self.max_raster_layer_id = Some(source.layer.id);
            self.max_raster_width = source.pixel_size.width;
            self.max_raster_height = source.pixel_size.height;
        }
    }

    pub(crate) fn add_mask_source(&mut self, source: &clip_file::metadata::MaskLayerSource) {
        let bytes = u64::from(source.pixel_size.width) * u64::from(source.pixel_size.height);
        self.mask_count += 1;
        self.mask_bytes += bytes;
        if bytes > self.max_mask_bytes {
            self.max_mask_bytes = bytes;
            self.max_mask_layer_id = Some(source.layer_id);
            self.max_mask_width = source.pixel_size.width;
            self.max_mask_height = source.pixel_size.height;
        }
    }
}

#[derive(Debug)]
pub struct NormalRasterStackPixelTraceResult {
    pub source_count: usize,
    pub samples: Vec<NormalRasterStackPixelTraceSample>,
    pub unsupported: Vec<SimpleRasterStackUnsupported>,
}

#[derive(Debug)]
pub struct NormalRasterStackPixelTraceSample {
    pub source_index: usize,
    pub source: String,
    pub before_rgba: Option<Rgba8>,
    pub rgba: Rgba8,
    pub inputs: Vec<NormalRasterStackPixelTraceInput>,
}

#[derive(Debug)]
pub struct NormalRasterStackPixelTraceInput {
    pub role: String,
    pub render_node_id: Option<u32>,
    pub layer_id: Option<u32>,
    pub blend_mode: Option<String>,
    pub opacity: Option<f32>,
    pub rgba: Option<Rgba8>,
    pub mask_alpha: Option<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NativeTileSiloEstimateResult {
    pub canvas: CanvasSize,
    pub tile_size: u32,
    pub canvas_tiles_x: u32,
    pub canvas_tiles_y: u32,
    pub canvas_tile_count: u64,
    pub top_level_source_count: usize,
    pub total_source_event_count: usize,
    pub raster_source_count: usize,
    pub clipped_raster_source_count: usize,
    pub solid_source_count: usize,
    pub lut_filter_count: usize,
    pub clipping_run_count: usize,
    pub container_clipping_run_count: usize,
    pub container_count: usize,
    pub clipped_container_count: usize,
    pub through_group_count: usize,
    pub mask_reference_count: usize,
    pub unique_raster_resource_count: usize,
    pub unique_mask_resource_count: usize,
    pub raster_tile_slot_count: u64,
    pub mask_tile_slot_count: u64,
    pub raster_compressed_tile_slot_count: u64,
    pub raster_empty_tile_slot_count: u64,
    pub mask_compressed_tile_slot_count: u64,
    pub mask_empty_tile_slot_count: u64,
    pub external_compressed_bytes: u64,
    pub raster_tile_event_count: u64,
    pub compressed_raster_tile_event_count: u64,
    pub solid_tile_event_count: u64,
    pub active_canvas_tile_count: usize,
    pub max_raster_events_per_tile: u32,
    pub mean_raster_events_per_active_tile: f64,
    pub active_compressed_canvas_tile_count: usize,
    pub max_compressed_raster_events_per_tile: u32,
    pub mean_compressed_raster_events_per_active_tile: f64,
    pub collapsible_segment_count: usize,
    pub collapsible_source_event_count: usize,
    pub semantic_barrier_count: usize,
    pub unsupported_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimpleRasterStackUnsupported {
    pub render_node_id: RenderNodeId,
    pub layer_id: LayerId,
    pub kind: RenderNodeKind,
    pub reason: SimpleRasterStackUnsupportedReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SimpleRasterStackUnsupportedReason {
    Paper,
    Clipping,
    Composite(u32),
    Opacity(u16),
    OpacityOutOfRange(u16),
    Mask,
    MaskSize { width: u32, height: u32 },
    NonCanvasSizedRaster { width: u32, height: u32 },
    RasterColorType(Option<u32>),
    RequiresAlphaCompositing,
    PaperSemantics,
    PaperColorMissing,
    ContainerSemantics,
    InsideUnsupportedContainer,
    Filter,
    UnsupportedLayerKind(u32),
}

impl fmt::Display for SimpleRasterStackUnsupportedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Paper => f.write_str("paper fill is not in the strict raster stack pass"),
            Self::Clipping => f.write_str("clipping requires clip-base compositing"),
            Self::Composite(composite) => {
                write!(f, "LayerComposite {composite} is not direct copy")
            }
            Self::Opacity(opacity) => write!(f, "LayerOpacity {opacity} requires opacity handling"),
            Self::OpacityOutOfRange(opacity) => {
                write!(
                    f,
                    "LayerOpacity {opacity} is outside the supported 0..256 range"
                )
            }
            Self::Mask => f.write_str("layer mask requires mask sampling"),
            Self::MaskSize { width, height } => {
                write!(f, "mask size {width}x{height} does not match the canvas")
            }
            Self::NonCanvasSizedRaster { width, height } => write!(
                f,
                "raster size {width}x{height} requires placement metadata",
            ),
            Self::RasterColorType(color_type) => {
                write!(f, "raster colour type {color_type:?} is not supported")
            }
            Self::RequiresAlphaCompositing => {
                f.write_str("stacked non-opaque raster requires alpha compositing")
            }
            Self::PaperSemantics => {
                f.write_str("paper layer has unsupported clip, mask, or composite semantics")
            }
            Self::PaperColorMissing => f.write_str("paper layer has no decoded paper colour"),
            Self::ContainerSemantics => {
                f.write_str("container requires folder compositing semantics")
            }
            Self::InsideUnsupportedContainer => {
                f.write_str("node is inside an unsupported container")
            }
            Self::Filter => f.write_str("filter layer is not in the strict raster stack pass"),
            Self::UnsupportedLayerKind(kind) => write!(f, "unsupported layer kind {kind}"),
        }
    }
}

#[derive(Debug)]
pub struct DrawRasterLayerGpuResult {
    pub image: clip_file::tiles::RgbaTileImage,
    pub resource_info: clip_gpu::GpuRasterResourceInfo,
    pub differing_bytes: usize,
}
