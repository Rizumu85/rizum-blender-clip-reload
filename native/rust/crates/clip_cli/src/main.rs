use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::process;

use clip_model::Rect;
use clip_runtime::ClipSession;

mod blender_server;
mod blender_worker;
mod layer_labels;
mod layer_window;
mod options;
mod pixel_trace_text;
mod support_json;
mod support_text;
mod tile_silo_text;

use layer_labels::layer_label;
use options::parse_options;
use pixel_trace_text::print_pixel_trace_result;

fn main() {
    let mut args = env::args_os();
    let _program = args.next();
    let Some(path) = args.next() else {
        eprintln!(
            "usage: clip_cli <file.clip> [--plan-only] [--compare-png <ref.png>] [--blender-render-rgba <out.rgba> --blender-render-json <out.json> [--blender-reload-old-json <manifest.json>]] [--dump-layer-window <id> <x> <y> <radius>] [--dump-layer-rgba <id> <out.rgba>] [--gpu-roundtrip-layer <id>] [--gpu-upload-planned-rasters] [--gpu-draw-layer <id>] [--gpu-simple-stack] [--gpu-support-check] [--gpu-support-json] [--gpu-normal-stack] [--gpu-trace-pixel <x> <y>] [--gpu-trace-layer-pixel <layer> <x> <y>] [--tile-silo-estimate] [--tile-size <px>] | clip_cli --blender-render-server"
        );
        process::exit(2);
    };
    if path == OsString::from("--blender-render-server") {
        process::exit(blender_server::run_blender_render_server());
    }
    let path = PathBuf::from(path);
    let options = match parse_options(args.collect()) {
        Ok(options) => options,
        Err(err) => {
            eprintln!("{err}");
            process::exit(2);
        }
    };
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
        let previous_manifest = match &options.blender_reload_old_json_path {
            Some(path) => match read_reload_manifest(path) {
                Ok(manifest) => Some(manifest),
                Err(err) => {
                    eprintln!("failed to read Blender reload manifest {:?}: {err}", path);
                    process::exit(1);
                }
            },
            None => None,
        };
        if let Err(err) = blender_worker::write_blender_render_files(
            &mut session,
            rgba_path,
            json_path,
            previous_manifest.as_ref(),
        ) {
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

    if options.tile_silo_estimate {
        let result = match session.estimate_tile_silo_plan(options.tile_size) {
            Ok(result) => result,
            Err(err) => {
                eprintln!("failed to estimate tile-silo plan from {:?}: {err}", path);
                process::exit(1);
            }
        };
        print!(
            "{}",
            tile_silo_text::tile_silo_estimate_text(&session, &result)
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
        match layer_window::dump_layer_window(&path, layer_id, x, y, radius) {
            Ok(()) => {}
            Err(err) => {
                eprintln!("{err}");
                process::exit(1);
            }
        }
    }

    if let Some((layer_id, out_path)) = &options.dump_layer_rgba {
        match layer_window::dump_layer_rgba(&path, *layer_id, out_path) {
            Ok(()) => {}
            Err(err) => {
                eprintln!("{err}");
                process::exit(1);
            }
        }
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
        print_pixel_trace_result(&session, &format!("gpu trace pixel x={x} y={y}"), result);
    }

    if let Some((layer_id, x, y)) = options.gpu_trace_layer_pixel {
        let result = match session.trace_layer_stack_pixel_via_gpu(layer_id, x, y) {
            Ok(result) => result,
            Err(err) => {
                eprintln!(
                    "failed to GPU-trace layer {} pixel ({x},{y}) from {:?}: {err}",
                    layer_label(&session, layer_id),
                    path
                );
                process::exit(1);
            }
        };
        print_pixel_trace_result(
            &session,
            &format!(
                "gpu trace layer={} x={x} y={y}",
                layer_label(&session, layer_id)
            ),
            result,
        );
    }
}

fn read_reload_manifest(path: &PathBuf) -> Result<clip_runtime::ReloadDiffManifest, String> {
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    serde_json::from_slice(&bytes).map_err(|err| err.to_string())
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
