use std::path::PathBuf;
use std::time::Instant;

use clip_model::Rect;
use clip_runtime::ClipSession;

use crate::blender_worker;
use crate::compare_png;
use crate::layer_labels::layer_label;
use crate::layer_window;
use crate::options::CliOptions;
use crate::performance_plan_json;
use crate::pixel_trace_text::print_pixel_trace_result;
use crate::reload_manifest::read_reload_manifest;
use crate::support_json;
use crate::support_text;
use crate::tile_silo_text;

pub fn run_file_command(path: PathBuf, options: CliOptions) -> i32 {
    if options.blender_render_rgba_path.is_some() != options.blender_render_json_path.is_some() {
        eprintln!("--blender-render-rgba and --blender-render-json must be used together");
        return 2;
    }

    let mut session = match ClipSession::open(&path) {
        Ok(session) => session,
        Err(err) => {
            eprintln!("failed to open {:?}: {err}", path);
            return 1;
        }
    };
    clip_file::decode_profile::reset_if_enabled();
    clip_runtime::render_profile::reset_if_enabled();

    if let (Some(rgba_path), Some(json_path)) = (
        &options.blender_render_rgba_path,
        &options.blender_render_json_path,
    ) {
        let previous_manifest = match &options.blender_reload_old_json_path {
            Some(path) => match read_reload_manifest(path) {
                Ok(manifest) => Some(manifest),
                Err(err) => {
                    eprintln!("failed to read Blender reload manifest {:?}: {err}", path);
                    return 1;
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
            return 1;
        }
        return 0;
    }

    if options.gpu_support_json {
        let result = match session.check_normal_raster_stack_support() {
            Ok(result) => result,
            Err(err) => {
                eprintln!("failed to check GPU support from {:?}: {err}", path);
                return 1;
            }
        };
        println!(
            "{}",
            support_json::normal_support_report_json(&session, &result)
        );
        return 0;
    }

    if options.tile_silo_estimate {
        let result = match session.estimate_tile_silo_plan(options.tile_size) {
            Ok(result) => result,
            Err(err) => {
                eprintln!("failed to estimate tile-silo plan from {:?}: {err}", path);
                return 1;
            }
        };
        print!(
            "{}",
            tile_silo_text::tile_silo_estimate_text(&session, &result)
        );
        return 0;
    }

    if options.performance_plan_json {
        let result = match session.performance_plan(options.tile_size) {
            Ok(result) => result,
            Err(err) => {
                eprintln!("failed to build performance plan from {:?}: {err}", path);
                return 1;
            }
        };
        println!(
            "{}",
            performance_plan_json::performance_plan_json(&session, &result)
        );
        return 0;
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
        return 0;
    }

    let profiling_active =
        clip_file::decode_profile::enabled() || clip_runtime::render_profile::enabled();
    if !options.gpu_support_check && !profiling_active {
        let mut first_pixel = [0u8; 4];
        match session.read_rgba8_region(Rect::new(0, 0, 1, 1), &mut first_pixel) {
            Ok(()) => println!("host first_pixel={first_pixel:?}"),
            Err(err) => println!("host first_pixel_unavailable={err}"),
        }
    }

    if let Some((layer_id, x, y, radius)) = options.dump_layer_window {
        if let Err(err) = layer_window::dump_layer_window(&path, layer_id, x, y, radius) {
            eprintln!("{err}");
            return 1;
        }
    }

    if let Some((layer_id, out_path)) = &options.dump_layer_rgba {
        if let Err(err) = layer_window::dump_layer_rgba(&path, *layer_id, out_path) {
            eprintln!("{err}");
            return 1;
        }
    }

    if let Some(reference_path) = &options.compare_png_path {
        let reference = match compare_png::read_png_rgba8(reference_path) {
            Ok(image) => image,
            Err(err) => {
                eprintln!("failed to read reference PNG {:?}: {err}", reference_path);
                return 1;
            }
        };
        let render_start = Instant::now();
        let rendered = match compare_png::render_full_image(&mut session) {
            Ok(image) => image,
            Err(err) => {
                eprintln!("failed to render full native image from {:?}: {err}", path);
                return 1;
            }
        };
        let worker_total_ms = render_start
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX);
        match compare_png::compare_png_report(reference_path, &rendered, &reference) {
            Ok(report) => println!("{report}"),
            Err(err) => {
                eprintln!("{err}");
                return 1;
            }
        }
        print_decode_profile(worker_total_ms);
        print_render_profile(worker_total_ms);
    }

    if let Some(layer_id) = options.gpu_roundtrip_layer_id {
        let image = match session.read_raster_layer_rgba_via_gpu(layer_id) {
            Ok(image) => image,
            Err(err) => {
                eprintln!(
                    "failed to GPU-roundtrip layer {} from {:?}: {err}",
                    layer_id.0, path,
                );
                return 1;
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
                return 1;
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
                return 1;
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
                return 1;
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
                return 1;
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
                return 1;
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
                return 1;
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
                return 1;
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

    0
}

fn print_render_profile(worker_total_ms: u64) {
    let Some(profile) = clip_runtime::render_profile::snapshot_if_enabled() else {
        return;
    };
    println!(
        "render_profile source_selection_ms={} gpu_device_init_ms={} render_program_planning_ms={} event_payload_build_ms={} gpu_pass_encode_ms={} queue_submit_ms={} queue_poll_ms={} queue_submit_poll_ms={} gpu_execution_time_ms=unavailable gpu_execution_time_source=cpu_wait_proxy gpu_execution_wait_proxy_ms={} readback_copy_ms={} readback_cpu_copy_ms={} patch_payload_extraction_ms={} checkpoint_reconstruction_ms={} sparse_atlas_update_ms={} legacy_barrier_segment_count={} legacy_barrier_segment_ms={} legacy_fallback_segment_count={} legacy_fallback_segment_ms={} tile_local_segment_count={} tile_local_segment_ms={} streaming_pass_count={} queue_submit_count={} readback_count={} checkpoint_cache_hits={} checkpoint_cache_misses={} checkpoint_cache_stores={} checkpoint_cache_evictions={} checkpoint_cache_skipped_over_budget={} worker_total_ms={}",
        profile.source_selection_ms,
        profile.gpu_device_init_ms,
        profile.render_program_planning_ms,
        profile.event_payload_build_ms,
        profile.gpu_pass_encode_ms,
        profile.queue_submit_ms,
        profile.queue_poll_ms,
        profile
            .queue_submit_ms
            .saturating_add(profile.queue_poll_ms),
        profile.gpu_execution_wait_proxy_ms,
        profile.readback_copy_ms,
        profile.readback_cpu_copy_ms,
        profile.patch_payload_extraction_ms,
        profile.checkpoint_reconstruction_ms,
        profile.sparse_atlas_update_ms,
        profile.legacy_barrier_segment_count,
        profile.legacy_barrier_segment_ms,
        profile.legacy_fallback_segment_count,
        profile.legacy_fallback_segment_ms,
        profile.tile_local_segment_count,
        profile.tile_local_segment_ms,
        profile.streaming_pass_count,
        profile.queue_submit_count,
        profile.readback_count,
        profile.checkpoint_cache_hits,
        profile.checkpoint_cache_misses,
        profile.checkpoint_cache_stores,
        profile.checkpoint_cache_evictions,
        profile.checkpoint_cache_skipped_over_budget,
        worker_total_ms,
    );
    for (rank, segment) in profile.top_segments.iter().enumerate() {
        println!(
            "render_profile_top_segment rank={} ordinal={} kind={} source_shape={} barrier_reason={} elapsed_us={} elapsed_ms={} source_range={}..{} first_layer_id={} target_origin={},{} target_size={}x{} expected_passes={} tile_events={} legacy_sources={}",
            rank + 1,
            segment.ordinal,
            segment.kind,
            segment.source_shape,
            segment.barrier_reason.unwrap_or("none"),
            segment.elapsed_us,
            segment.elapsed_ms,
            segment.source_start,
            segment.source_end,
            segment
                .first_layer_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "none".to_string()),
            segment.target_origin_x,
            segment.target_origin_y,
            segment.target_width,
            segment.target_height,
            segment.expected_passes,
            segment.tile_events,
            segment.legacy_sources,
        );
    }
}

fn print_decode_profile(worker_total_ms: u64) {
    let Some(profile) = clip_file::decode_profile::snapshot_if_enabled() else {
        return;
    };
    println!(
        "decode_profile selected_raster_tiles={} selected_mask_tiles={} skipped_empty_raster_tiles={} skipped_empty_mask_tiles={} compressed_bytes_read={} zlib_inflate_ms={} tile_swizzle_rgba_ms={} mask_r8_decode_ms={} mask_crop_ms={} atlas_chunk_build_ms={} sparse_atlas_pool_update_ms={} region_patch_render_ms={} worker_total_ms={}",
        profile.selected_raster_tiles,
        profile.selected_mask_tiles,
        profile.skipped_empty_raster_tiles,
        profile.skipped_empty_mask_tiles,
        profile.compressed_bytes_read,
        profile.zlib_inflate_ms,
        profile.tile_swizzle_rgba_ms,
        profile.mask_r8_decode_ms,
        profile.mask_crop_ms,
        profile.atlas_chunk_build_ms,
        profile.sparse_atlas_pool_update_ms,
        profile.region_patch_render_ms,
        worker_total_ms,
    );
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
