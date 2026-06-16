use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::process;

use clip_model::{LayerId, Rect, Rgba8};
use clip_runtime::ClipSession;

mod blender_worker;
mod layer_labels;
mod support_json;
mod support_text;

use layer_labels::{layer_label, optional_raw_layer_label};

fn main() {
    let mut args = env::args_os();
    let _program = args.next();
    let Some(path) = args.next() else {
        eprintln!(
            "usage: clip_cli <file.clip> [--plan-only] [--compare-png <ref.png>] [--blender-render-rgba <out.rgba> --blender-render-json <out.json>] [--dump-layer-window <id> <x> <y> <radius>] [--gpu-roundtrip-layer <id>] [--gpu-upload-planned-rasters] [--gpu-draw-layer <id>] [--gpu-simple-stack] [--gpu-support-check] [--gpu-support-json] [--gpu-normal-stack] [--gpu-trace-pixel <x> <y>]"
        );
        process::exit(2);
    };
    let options = parse_options(args.collect());
    if options.blender_render_rgba_path.is_some() != options.blender_render_json_path.is_some() {
        eprintln!("--blender-render-rgba and --blender-render-json must be used together");
        process::exit(2);
    }

    let mut session = match ClipSession::open(&path) {
        Ok(session) => session,
        Err(err) => {
            eprintln!("failed to open {:?}: {err}", path);
            process::exit(1);
        }
    };

    if let (Some(rgba_path), Some(json_path)) = (
        &options.blender_render_rgba_path,
        &options.blender_render_json_path,
    ) {
        if let Err(err) =
            blender_worker::write_blender_render_files(&mut session, rgba_path, json_path)
        {
            eprintln!(
                "failed to render Blender worker files from {:?}: {err}",
                path
            );
            process::exit(1);
        }
        return;
    }

    if options.gpu_support_json {
        let result = match session.check_normal_raster_stack_support() {
            Ok(result) => result,
            Err(err) => {
                eprintln!("failed to check GPU support from {:?}: {err}", path);
                process::exit(1);
            }
        };
        println!(
            "{}",
            support_json::normal_support_report_json(&session, &result)
        );
        return;
    }

    let summary = session.summary();
    println!(
        "clip summary: {}x{} root_layer={} layers={} external_data={}",
        summary.canvas.width,
        summary.canvas.height,
        layer_label(&session, summary.root_layer_id),
        summary.layer_count,
        summary.external_data_count,
    );
    println!("planned nodes:");
    for node in &session.render_plan().nodes {
        println!(
            "  id={} layer={} kind={:?} depth={} clip={} opacity={} composite={} render_mipmap={:?} mask_mipmap={:?} paper_color={:?}",
            node.id.0,
            layer_label(&session, node.layer_id),
            node.kind,
            node.depth,
            node.clip,
            node.opacity.0,
            node.composite,
            node.render_mipmap_id,
            node.mask_mipmap_id,
            node.paper_color,
        );
    }

    if options.plan_only {
        return;
    }

    if !options.gpu_support_check {
        let mut first_pixel = [0u8; 4];
        match session.read_rgba8_region(Rect::new(0, 0, 1, 1), &mut first_pixel) {
            Ok(()) => println!("host first_pixel={first_pixel:?}"),
            Err(err) => println!("host first_pixel_unavailable={err}"),
        }
    }

    if let Some((layer_id, x, y, radius)) = options.dump_layer_window {
        let image = match clip_file::read_raster_layer_rgba(&path, layer_id) {
            Ok(image) => image,
            Err(err) => {
                eprintln!(
                    "failed to read raster layer {} from {:?}: {err}",
                    layer_id.0, path,
                );
                process::exit(1);
            }
        };
        print_layer_window(&image, layer_id, x, y, radius);
    }

    if let Some(reference_path) = &options.compare_png_path {
        let reference = match read_png_rgba8(reference_path) {
            Ok(image) => image,
            Err(err) => {
                eprintln!("failed to read reference PNG {:?}: {err}", reference_path);
                process::exit(1);
            }
        };
        let rendered = match render_full_image(&mut session) {
            Ok(image) => image,
            Err(err) => {
                eprintln!("failed to render full native image from {:?}: {err}", path);
                process::exit(1);
            }
        };
        if rendered.width != reference.width || rendered.height != reference.height {
            eprintln!(
                "image size mismatch: rendered={}x{} reference={}x{}",
                rendered.width, rendered.height, reference.width, reference.height,
            );
            process::exit(1);
        }
        let stats = compare_images(&rendered.pixels, &reference.pixels, rendered.width);
        println!(
            "compare png ref={:?} size={}x{} raw_max={} raw_max_at=({},{},{}) raw_mean={:.6} raw_diff_px={} raw_visible_px={} premul_max={} premul_max_at=({},{},{}) premul_mean={:.6} premul_diff_px={} premul_visible_px={}",
            reference_path,
            rendered.width,
            rendered.height,
            stats.raw_max,
            stats.raw_max_at.x,
            stats.raw_max_at.y,
            channel_name(stats.raw_max_at.channel),
            stats.raw_mean,
            stats.raw_diff_px,
            stats.raw_visible_px,
            stats.premul_max,
            stats.premul_max_at.x,
            stats.premul_max_at.y,
            channel_name(stats.premul_max_at.channel),
            stats.premul_mean,
            stats.premul_diff_px,
            stats.premul_visible_px,
        );
    }

    if let Some(layer_id) = options.gpu_roundtrip_layer_id {
        let image = match session.read_raster_layer_rgba_via_gpu(layer_id) {
            Ok(image) => image,
            Err(err) => {
                eprintln!(
                    "failed to GPU-roundtrip layer {} from {:?}: {err}",
                    layer_id.0, path,
                );
                process::exit(1);
            }
        };
        let stats = image_stats(&image.pixels);
        println!(
            "gpu roundtrip layer={} size={}x{} bytes={} nonzero_alpha={} sums={:?}",
            layer_label(&session, layer_id),
            image.width,
            image.height,
            image.pixels.len(),
            stats.nonzero_alpha,
            stats.sums,
        );
    }

    if options.gpu_upload_planned_rasters {
        let resources = match session.upload_planned_raster_resources_via_gpu() {
            Ok(resources) => resources,
            Err(err) => {
                eprintln!(
                    "failed to upload planned raster resources from {:?}: {err}",
                    path
                );
                process::exit(1);
            }
        };
        println!("gpu planned raster resources count={}", resources.len());
        for resource in resources {
            println!(
                "  node={} layer={} render_mipmap={} size={}x{} bytes={}",
                resource.render_node_id.0,
                layer_label(&session, resource.key.layer_id),
                resource.key.render_mipmap_id,
                resource.size.width,
                resource.size.height,
                resource.byte_len,
            );
        }
    }

    if let Some(layer_id) = options.gpu_draw_layer_id {
        let result = match session.draw_raster_layer_rgba_via_gpu(layer_id) {
            Ok(result) => result,
            Err(err) => {
                eprintln!(
                    "failed to GPU-draw layer {} from {:?}: {err}",
                    layer_id.0, path,
                );
                process::exit(1);
            }
        };
        let stats = image_stats(&result.image.pixels);
        println!(
            "gpu draw layer={} node={} render_mipmap={} size={}x{} bytes={} nonzero_alpha={} sums={:?} differing_bytes={}",
            layer_label(&session, layer_id),
            result.resource_info.render_node_id.0,
            result.resource_info.key.render_mipmap_id,
            result.image.width,
            result.image.height,
            result.image.pixels.len(),
            stats.nonzero_alpha,
            stats.sums,
            result.differing_bytes,
        );
    }

    if options.gpu_simple_stack {
        let result = match session.draw_simple_raster_stack_via_gpu() {
            Ok(result) => result,
            Err(err) => {
                eprintln!(
                    "failed to GPU-draw simple raster stack from {:?}: {err}",
                    path
                );
                process::exit(1);
            }
        };
        let image_stats = result
            .image
            .as_ref()
            .map(|image| image_stats(&image.pixels));
        println!(
            "gpu simple stack drawn={} unsupported={} has_image={} differing_bytes_from_last_drawn={:?}",
            result.drawn_resources.len(),
            result.unsupported.len(),
            result.image.is_some(),
            result.differing_bytes_from_last_drawn,
        );
        for resource in result.drawn_resources {
            println!(
                "  drawn node={} layer={} render_mipmap={} size={}x{} bytes={}",
                resource.render_node_id.0,
                layer_label(&session, resource.key.layer_id),
                resource.key.render_mipmap_id,
                resource.size.width,
                resource.size.height,
                resource.byte_len,
            );
        }
        for unsupported in result.unsupported {
            println!(
                "  unsupported node={} layer={} kind={:?} reason={}",
                unsupported.render_node_id.0,
                layer_label(&session, unsupported.layer_id),
                unsupported.kind,
                unsupported.reason,
            );
        }
        if let (Some(image), Some(stats)) = (&result.image, image_stats) {
            println!(
                "  output size={}x{} bytes={} nonzero_alpha={} sums={:?}",
                image.width,
                image.height,
                image.pixels.len(),
                stats.nonzero_alpha,
                stats.sums,
            );
        }
    }

    if options.gpu_support_check {
        let result = match session.check_normal_raster_stack_support() {
            Ok(result) => result,
            Err(err) => {
                eprintln!("failed to check GPU support from {:?}: {err}", path);
                process::exit(1);
            }
        };
        print!(
            "{}",
            support_text::normal_support_check_text(&session, &result)
        );
    }

    if options.gpu_normal_stack {
        let result = match session.draw_normal_raster_stack_via_gpu() {
            Ok(result) => result,
            Err(err) => {
                eprintln!(
                    "failed to GPU-draw normal raster stack from {:?}: {err}",
                    path
                );
                process::exit(1);
            }
        };
        let image_stats = result
            .image
            .as_ref()
            .map(|image| image_stats(&image.pixels));
        println!(
            "gpu normal stack sources={} raster_resources={} mask_resources={} unsupported={} has_image={}",
            result.source_count,
            result.drawn_resources.len(),
            result.mask_resources.len(),
            result.unsupported.len(),
            result.image.is_some(),
        );
        for resource in result.drawn_resources {
            println!(
                "  drawn node={} layer={} render_mipmap={} size={}x{} bytes={}",
                resource.render_node_id.0,
                layer_label(&session, resource.key.layer_id),
                resource.key.render_mipmap_id,
                resource.size.width,
                resource.size.height,
                resource.byte_len,
            );
        }
        for resource in result.mask_resources {
            println!(
                "  mask node={} layer={} mask_mipmap={} size={}x{} bytes={}",
                resource.render_node_id.0,
                layer_label(&session, resource.key.layer_id),
                resource.key.mask_mipmap_id,
                resource.size.width,
                resource.size.height,
                resource.byte_len,
            );
        }
        for unsupported in result.unsupported {
            println!(
                "  unsupported node={} layer={} kind={:?} reason={}",
                unsupported.render_node_id.0,
                layer_label(&session, unsupported.layer_id),
                unsupported.kind,
                unsupported.reason,
            );
        }
        if let (Some(image), Some(stats)) = (&result.image, image_stats) {
            println!(
                "  output size={}x{} bytes={} nonzero_alpha={} sums={:?}",
                image.width,
                image.height,
                image.pixels.len(),
                stats.nonzero_alpha,
                stats.sums,
            );
        }
    }

    if let Some((x, y)) = options.gpu_trace_pixel {
        let result = match session.trace_normal_raster_stack_pixel_via_gpu(x, y) {
            Ok(result) => result,
            Err(err) => {
                eprintln!("failed to GPU-trace pixel ({x},{y}) from {:?}: {err}", path);
                process::exit(1);
            }
        };
        println!(
            "gpu trace pixel x={} y={} sources={} unsupported={}",
            x,
            y,
            result.source_count,
            result.unsupported.len(),
        );
        for sample in result.samples {
            println!(
                "  prefix={} before={} rgba=[{},{},{},{}] source={}",
                sample.source_index,
                format_optional_rgba(sample.before_rgba),
                sample.rgba.r,
                sample.rgba.g,
                sample.rgba.b,
                sample.rgba.a,
                sample.source,
            );
            for input in sample.inputs {
                println!(
                    "    input role={} node={} layer={} blend={} opacity={} rgba={} mask_alpha={}",
                    input.role,
                    format_optional_u32(input.render_node_id),
                    optional_raw_layer_label(&session, input.layer_id),
                    input.blend_mode.as_deref().unwrap_or("-"),
                    format_optional_f32(input.opacity),
                    format_optional_rgba(input.rgba),
                    format_optional_u8(input.mask_alpha),
                );
            }
        }
        for unsupported in result.unsupported {
            println!(
                "  unsupported node={} layer={} kind={:?} reason={}",
                unsupported.render_node_id.0,
                layer_label(&session, unsupported.layer_id),
                unsupported.kind,
                unsupported.reason,
            );
        }
    }
}

#[derive(Debug, Default)]
struct CliOptions {
    plan_only: bool,
    gpu_roundtrip_layer_id: Option<LayerId>,
    gpu_upload_planned_rasters: bool,
    gpu_draw_layer_id: Option<LayerId>,
    gpu_simple_stack: bool,
    gpu_support_check: bool,
    gpu_support_json: bool,
    gpu_normal_stack: bool,
    gpu_trace_pixel: Option<(u32, u32)>,
    dump_layer_window: Option<(LayerId, u32, u32, u32)>,
    compare_png_path: Option<PathBuf>,
    blender_render_rgba_path: Option<PathBuf>,
    blender_render_json_path: Option<PathBuf>,
}

fn parse_options(args: Vec<OsString>) -> CliOptions {
    let mut options = CliOptions::default();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        if arg == "--gpu-roundtrip-layer" {
            let Some(layer_id) = iter.next() else {
                eprintln!("missing value after --gpu-roundtrip-layer");
                process::exit(2);
            };
            let Some(layer_id) = layer_id
                .to_str()
                .and_then(|value| value.parse::<u32>().ok())
            else {
                eprintln!("invalid layer id for --gpu-roundtrip-layer");
                process::exit(2);
            };
            options.gpu_roundtrip_layer_id = Some(LayerId(layer_id));
        } else if arg == "--gpu-upload-planned-rasters" {
            options.gpu_upload_planned_rasters = true;
        } else if arg == "--gpu-draw-layer" {
            let Some(layer_id) = iter.next() else {
                eprintln!("missing value after --gpu-draw-layer");
                process::exit(2);
            };
            let Some(layer_id) = layer_id
                .to_str()
                .and_then(|value| value.parse::<u32>().ok())
            else {
                eprintln!("invalid layer id for --gpu-draw-layer");
                process::exit(2);
            };
            options.gpu_draw_layer_id = Some(LayerId(layer_id));
        } else if arg == "--gpu-simple-stack" {
            options.gpu_simple_stack = true;
        } else if arg == "--gpu-support-check" {
            options.gpu_support_check = true;
        } else if arg == "--gpu-support-json" {
            options.gpu_support_json = true;
        } else if arg == "--gpu-normal-stack" {
            options.gpu_normal_stack = true;
        } else if arg == "--gpu-trace-pixel" {
            let x = parse_next_u32(&mut iter, "--gpu-trace-pixel x");
            let y = parse_next_u32(&mut iter, "--gpu-trace-pixel y");
            options.gpu_trace_pixel = Some((x, y));
        } else if arg == "--dump-layer-window" {
            let layer_id = parse_next_u32(&mut iter, "--dump-layer-window layer id");
            let x = parse_next_u32(&mut iter, "--dump-layer-window x");
            let y = parse_next_u32(&mut iter, "--dump-layer-window y");
            let radius = parse_next_u32(&mut iter, "--dump-layer-window radius");
            options.dump_layer_window = Some((LayerId(layer_id), x, y, radius));
        } else if arg == "--compare-png" {
            let Some(path) = iter.next() else {
                eprintln!("missing value after --compare-png");
                process::exit(2);
            };
            options.compare_png_path = Some(PathBuf::from(path));
        } else if arg == "--blender-render-rgba" {
            let Some(path) = iter.next() else {
                eprintln!("missing value after --blender-render-rgba");
                process::exit(2);
            };
            options.blender_render_rgba_path = Some(PathBuf::from(path));
        } else if arg == "--blender-render-json" {
            let Some(path) = iter.next() else {
                eprintln!("missing value after --blender-render-json");
                process::exit(2);
            };
            options.blender_render_json_path = Some(PathBuf::from(path));
        } else if arg == "--plan-only" {
            options.plan_only = true;
        } else {
            eprintln!("unknown argument {:?}", arg);
            process::exit(2);
        }
    }
    options
}

fn parse_next_u32(iter: &mut impl Iterator<Item = OsString>, label: &str) -> u32 {
    let Some(value) = iter.next() else {
        eprintln!("missing value for {label}");
        process::exit(2);
    };
    let Some(value) = value.to_str().and_then(|value| value.parse::<u32>().ok()) else {
        eprintln!("invalid integer for {label}");
        process::exit(2);
    };
    value
}

fn format_optional_u32(value: Option<u32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn format_optional_u8(value: Option<u8>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn format_optional_f32(value: Option<f32>) -> String {
    value
        .map(|value| format!("{value:.6}"))
        .unwrap_or_else(|| "-".to_string())
}

fn format_optional_rgba(value: Option<Rgba8>) -> String {
    value
        .map(|rgba| format!("[{},{},{},{}]", rgba.r, rgba.g, rgba.b, rgba.a))
        .unwrap_or_else(|| "-".to_string())
}

#[derive(Debug, Eq, PartialEq)]
struct ImageStats {
    nonzero_alpha: usize,
    sums: [u64; 4],
}

fn image_stats(pixels: &[u8]) -> ImageStats {
    let mut nonzero_alpha = 0usize;
    let mut sums = [0u64; 4];
    for pixel in pixels.chunks_exact(4) {
        if pixel[3] > 0 {
            nonzero_alpha += 1;
        }
        for channel in 0..4 {
            sums[channel] += u64::from(pixel[channel]);
        }
    }
    ImageStats {
        nonzero_alpha,
        sums,
    }
}

fn print_layer_window(
    image: &clip_file::tiles::RgbaTileImage,
    layer_id: LayerId,
    x: u32,
    y: u32,
    radius: u32,
) {
    println!(
        "layer window layer={} size={}x{} center=({}, {}) radius={}",
        layer_id.0, image.width, image.height, x, y, radius,
    );
    if image.width == 0 || image.height == 0 {
        return;
    }
    let min_x = x.saturating_sub(radius);
    let min_y = y.saturating_sub(radius);
    let max_x = x.saturating_add(radius).min(image.width - 1);
    let max_y = y.saturating_add(radius).min(image.height - 1);
    for sample_y in min_y..=max_y {
        let mut row = format!("  y={sample_y}:");
        for sample_x in min_x..=max_x {
            let index = usize::try_from(
                (u64::from(sample_y) * u64::from(image.width) + u64::from(sample_x)) * 4,
            )
            .expect("layer pixel index fits in usize");
            let pixel = &image.pixels[index..index + 4];
            row.push_str(&format!(
                " x={sample_x}[{},{},{},{}]",
                pixel[0], pixel[1], pixel[2], pixel[3]
            ));
        }
        println!("{row}");
    }
}

#[derive(Debug)]
struct RgbaImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

fn render_full_image(session: &mut ClipSession) -> Result<RgbaImage, String> {
    let canvas = session.summary().canvas;
    let byte_len = usize::try_from(u64::from(canvas.width) * u64::from(canvas.height) * 4)
        .map_err(|_| "canvas byte length does not fit in usize".to_string())?;
    let mut pixels = vec![0u8; byte_len];
    session
        .read_rgba8_region(Rect::new(0, 0, canvas.width, canvas.height), &mut pixels)
        .map_err(|err| err.to_string())?;
    Ok(RgbaImage {
        width: canvas.width,
        height: canvas.height,
        pixels,
    })
}

fn read_png_rgba8(path: &PathBuf) -> Result<RgbaImage, String> {
    let file = File::open(path).map_err(|err| err.to_string())?;
    let decoder = png::Decoder::new(BufReader::new(file));
    let mut reader = decoder.read_info().map_err(|err| err.to_string())?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|err| err.to_string())?;
    let bytes = &buffer[..info.buffer_size()];
    if info.bit_depth != png::BitDepth::Eight {
        return Err(format!("unsupported PNG bit depth {:?}", info.bit_depth));
    }

    let pixels = match info.color_type {
        png::ColorType::Rgba => bytes.to_vec(),
        png::ColorType::Rgb => {
            let mut rgba = Vec::with_capacity((bytes.len() / 3) * 4);
            for pixel in bytes.chunks_exact(3) {
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
            rgba
        }
        png::ColorType::GrayscaleAlpha => {
            let mut rgba = Vec::with_capacity((bytes.len() / 2) * 4);
            for pixel in bytes.chunks_exact(2) {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
            rgba
        }
        png::ColorType::Grayscale => {
            let mut rgba = Vec::with_capacity(bytes.len() * 4);
            for gray in bytes {
                rgba.extend_from_slice(&[*gray, *gray, *gray, 255]);
            }
            rgba
        }
        png::ColorType::Indexed => {
            return Err("indexed PNG references are not supported".to_string());
        }
    };

    let expected_len = usize::try_from(u64::from(info.width) * u64::from(info.height) * 4)
        .map_err(|_| "PNG byte length does not fit in usize".to_string())?;
    if pixels.len() != expected_len {
        return Err(format!(
            "decoded PNG byte length mismatch: got {} expected {}",
            pixels.len(),
            expected_len,
        ));
    }

    Ok(RgbaImage {
        width: info.width,
        height: info.height,
        pixels,
    })
}

#[derive(Debug)]
struct CompareStats {
    raw_max: u8,
    raw_max_at: MaxLocation,
    raw_mean: f64,
    raw_diff_px: usize,
    raw_visible_px: usize,
    premul_max: u8,
    premul_max_at: MaxLocation,
    premul_mean: f64,
    premul_diff_px: usize,
    premul_visible_px: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct MaxLocation {
    x: u32,
    y: u32,
    channel: usize,
}

fn compare_images(actual: &[u8], reference: &[u8], width: u32) -> CompareStats {
    assert_eq!(actual.len(), reference.len());
    assert_eq!(actual.len() % 4, 0);

    let mut raw_max = 0u8;
    let mut raw_max_at = MaxLocation {
        x: 0,
        y: 0,
        channel: 0,
    };
    let mut raw_abs_sum = 0u64;
    let mut raw_diff_px = 0usize;
    let mut raw_visible_px = 0usize;
    let mut premul_max = 0u8;
    let mut premul_max_at = MaxLocation {
        x: 0,
        y: 0,
        channel: 0,
    };
    let mut premul_abs_sum = 0u64;
    let mut premul_diff_px = 0usize;
    let mut premul_visible_px = 0usize;

    for (pixel_index, (actual_px, reference_px)) in actual
        .chunks_exact(4)
        .zip(reference.chunks_exact(4))
        .enumerate()
    {
        let mut raw_pixel_max = 0u8;
        let mut premul_pixel_max = 0u8;
        for channel in 0..4 {
            let diff = u8_abs_diff(actual_px[channel], reference_px[channel]);
            raw_pixel_max = raw_pixel_max.max(diff);
            raw_abs_sum += u64::from(diff);
            if diff > raw_max {
                raw_max = diff;
                raw_max_at = max_location(pixel_index, width, channel);
            }

            let actual_value = premul_channel(actual_px, channel);
            let reference_value = premul_channel(reference_px, channel);
            let premul_diff = u8_abs_diff(actual_value, reference_value);
            premul_pixel_max = premul_pixel_max.max(premul_diff);
            premul_abs_sum += u64::from(premul_diff);
            if premul_diff > premul_max {
                premul_max = premul_diff;
                premul_max_at = max_location(pixel_index, width, channel);
            }
        }
        if raw_pixel_max > 0 {
            raw_diff_px += 1;
        }
        if raw_pixel_max > 1 {
            raw_visible_px += 1;
        }
        if premul_pixel_max > 0 {
            premul_diff_px += 1;
        }
        if premul_pixel_max > 1 {
            premul_visible_px += 1;
        }
    }

    let channel_count = actual.len() as f64;
    CompareStats {
        raw_max,
        raw_max_at,
        raw_mean: raw_abs_sum as f64 / channel_count,
        raw_diff_px,
        raw_visible_px,
        premul_max,
        premul_max_at,
        premul_mean: premul_abs_sum as f64 / channel_count,
        premul_diff_px,
        premul_visible_px,
    }
}

fn max_location(pixel_index: usize, width: u32, channel: usize) -> MaxLocation {
    let pixel_index = u32::try_from(pixel_index).expect("image pixel index fits in u32");
    MaxLocation {
        x: pixel_index % width,
        y: pixel_index / width,
        channel,
    }
}

fn channel_name(channel: usize) -> &'static str {
    match channel {
        0 => "r",
        1 => "g",
        2 => "b",
        3 => "a",
        _ => "?",
    }
}

fn premul_channel(pixel: &[u8], channel: usize) -> u8 {
    if channel == 3 {
        pixel[3]
    } else {
        (((u16::from(pixel[channel]) * u16::from(pixel[3])) + 127) / 255) as u8
    }
}

fn u8_abs_diff(lhs: u8, rhs: u8) -> u8 {
    lhs.max(rhs) - lhs.min(rhs)
}
