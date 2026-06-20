use std::fs;
use std::path::Path;
use std::time::Instant;

use clip_runtime::{
    ClipSession, ReloadDiffManifest, ReloadDiffMode, ReloadPatchRect, RuntimeError,
    RuntimeGpuRenderer,
};
use serde_json::json;

use crate::blender_worker_render_profile::{render_profile_metadata, tile_cache_diagnostics};
use crate::blender_worker_sparse::sparse_atlas_metadata;
use crate::blender_worker_task_graph::{
    render_task_graph_for_full_render, render_task_graph_for_patch, render_task_graph_no_change,
};

pub(crate) fn write_blender_render_files(
    session: &mut ClipSession,
    rgba_path: &Path,
    json_path: &Path,
    previous_manifest: Option<&ReloadDiffManifest>,
) -> Result<(), String> {
    write_blender_render_files_with_renderer(session, rgba_path, json_path, previous_manifest, None)
}

pub(crate) fn write_blender_render_files_with_renderer(
    session: &mut ClipSession,
    rgba_path: &Path,
    json_path: &Path,
    previous_manifest: Option<&ReloadDiffManifest>,
    renderer: Option<&RuntimeGpuRenderer>,
) -> Result<(), String> {
    clip_file::decode_profile::reset_if_enabled();
    clip_runtime::render_profile::reset_if_enabled();
    let worker_start = Instant::now();
    let summary = session.summary();
    let root_layer_id = summary.root_layer_id.0;
    let layer_count = summary.layer_count;
    let external_data_count = summary.external_data_count;
    let reload_plan = session
        .plan_reload_diff(previous_manifest)
        .map_err(|err| err.to_string())?;
    if reload_plan.mode == ReloadDiffMode::NoChange {
        let sparse_atlas = renderer
            .map(|renderer| renderer.plan_sparse_atlas_reload(&reload_plan))
            .unwrap_or_default();
        let support = session
            .check_normal_raster_stack_support()
            .map_err(|err| err.to_string())?;
        fs::write(rgba_path, []).map_err(|err| err.to_string())?;
        let metadata = base_metadata(
            session,
            root_layer_id,
            layer_count,
            external_data_count,
            Some((&support.unsupported, support.source_count)),
            0,
            Some(support.resource_stats),
            clip_runtime::GpuTextureCacheStats::default(),
            &sparse_atlas,
            reload_plan_metadata(&reload_plan, 0, None, None),
            render_task_graph_no_change(),
            tile_cache_diagnostics(
                &reload_plan,
                &sparse_atlas,
                renderer
                    .map(RuntimeGpuRenderer::checkpoint_cache_stats)
                    .unwrap_or_default(),
            ),
            worker_start
                .elapsed()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
        );
        let metadata = serde_json::to_vec_pretty(&metadata).map_err(|err| err.to_string())?;
        fs::write(json_path, metadata).map_err(|err| err.to_string())?;
        return Ok(());
    }

    if reload_plan.mode == ReloadDiffMode::Patch
        && let Some(renderer) = renderer
    {
        let mut sparse_reconstructed_ms = None;
        let mut region_fallback_ms = None;
        let sparse_initial_start = Instant::now();
        let sparse_patch = renderer
            .draw_sparse_atlas_initial_segment_patches(session, &reload_plan)
            .map_err(|err| err.to_string())?;
        let sparse_initial_ms = elapsed_ms(sparse_initial_start);
        let (render, sparse_atlas, patch_renderer, patch_renderer_fallback_reason) =
            if let Some((render, sparse_atlas)) = sparse_patch {
                (render, sparse_atlas, "sparse_atlas_initial_segments", None)
            } else {
                let sparse_reconstructed_start = Instant::now();
                let sparse_patch = renderer
                    .draw_sparse_atlas_reconstructed_segment_patches(session, &reload_plan)
                    .map_err(|err| err.to_string())?;
                let reconstructed_ms = elapsed_ms(sparse_reconstructed_start);
                sparse_reconstructed_ms = Some(reconstructed_ms);
                if let Some((render, sparse_atlas)) = sparse_patch {
                    (
                        render,
                        sparse_atlas,
                        "sparse_atlas_reconstructed_segments",
                        None,
                    )
                } else {
                    let region_start = Instant::now();
                    let (render, sparse_atlas) = match renderer
                        .draw_normal_raster_stack_patches_with_region_resident_sparse_atlas(
                            session,
                            &reload_plan,
                        )
                        .map_err(|err| err.to_string())?
                    {
                        Some((render, sparse_atlas)) => (render, sparse_atlas),
                        None => {
                            let sparse_atlas = renderer.plan_sparse_atlas_reload(&reload_plan);
                            let render = renderer
                                .draw_normal_raster_stack_patches(session, &reload_plan.dirty_rects)
                                .map_err(|err| err.to_string())?;
                            (render, sparse_atlas)
                        }
                    };
                    let region_ms = elapsed_ms(region_start);
                    region_fallback_ms = Some(region_ms);
                    (
                        render,
                        sparse_atlas,
                        "region",
                        Some(
                            "sparse_atlas_initial_segments and \
                             sparse_atlas_reconstructed_segments returned no executable patch",
                        ),
                    )
                }
            };
        let render_task_graph = render_task_graph_for_patch(
            &reload_plan,
            patch_renderer,
            patch_renderer_fallback_reason,
            sparse_initial_ms,
            sparse_reconstructed_ms,
            region_fallback_ms,
        );
        let source_count = render.source_count;
        let stats = render.resource_stats;
        let texture_cache_stats = render.texture_cache_stats;
        let unsupported_items = render.unsupported;
        let payload_bytes = render.payload.len();
        fs::write(rgba_path, &render.payload).map_err(|err| err.to_string())?;

        let metadata = base_metadata(
            session,
            root_layer_id,
            layer_count,
            external_data_count,
            Some((&unsupported_items, source_count)),
            payload_bytes,
            Some(stats),
            texture_cache_stats,
            &sparse_atlas,
            reload_plan_metadata(
                &reload_plan,
                payload_bytes,
                Some(patch_renderer),
                patch_renderer_fallback_reason,
            ),
            render_task_graph,
            tile_cache_diagnostics(
                &reload_plan,
                &sparse_atlas,
                renderer.checkpoint_cache_stats(),
            ),
            worker_start
                .elapsed()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
        );
        let metadata = serde_json::to_vec_pretty(&metadata).map_err(|err| err.to_string())?;
        fs::write(json_path, metadata).map_err(|err| err.to_string())?;
        return Ok(());
    }

    let render_start = Instant::now();
    let render = match renderer {
        Some(renderer) => renderer.draw_normal_raster_stack(session),
        None => session.draw_normal_raster_stack_via_gpu(),
    }
    .map_err(|err| err.to_string())?;
    let render_ms = elapsed_ms(render_start);
    let source_count = render.source_count;
    let stats = render.resource_stats;
    let texture_cache_stats = render.texture_cache_stats;
    let unsupported_items = render.unsupported;
    let image = render
        .image
        .ok_or(RuntimeError::EmptyRenderPlan)
        .map_err(|err| err.to_string())?;
    let (rgba_payload, payload_bytes) = match reload_plan.mode {
        ReloadDiffMode::Full => (image.pixels.clone(), image.pixels.len()),
        ReloadDiffMode::Patch => {
            let extraction_start = Instant::now();
            let payload = patch_pixels(&image.pixels, image.width, &reload_plan.dirty_rects)?;
            clip_runtime::render_profile::record_patch_payload_extraction(
                extraction_start.elapsed(),
            );
            let len = payload.len();
            (payload, len)
        }
        ReloadDiffMode::NoChange => unreachable!("no-change reload returns before rendering"),
    };
    let sparse_atlas = renderer
        .map(|renderer| renderer.plan_sparse_atlas_reload(&reload_plan))
        .unwrap_or_default();
    let patch_renderer =
        (reload_plan.mode == ReloadDiffMode::Patch).then_some("full_render_patch_extract");
    let patch_renderer_fallback_reason = patch_renderer.map(
        |_| "persistent RuntimeGpuRenderer unavailable; rendered full image then extracted patch",
    );
    fs::write(rgba_path, &rgba_payload).map_err(|err| err.to_string())?;

    let metadata = base_metadata(
        session,
        root_layer_id,
        layer_count,
        external_data_count,
        Some((&unsupported_items, source_count)),
        payload_bytes,
        Some(stats),
        texture_cache_stats,
        &sparse_atlas,
        reload_plan_metadata(
            &reload_plan,
            payload_bytes,
            patch_renderer,
            patch_renderer_fallback_reason,
        ),
        render_task_graph_for_full_render(&reload_plan, render_ms, patch_renderer),
        tile_cache_diagnostics(
            &reload_plan,
            &sparse_atlas,
            renderer
                .map(RuntimeGpuRenderer::checkpoint_cache_stats)
                .unwrap_or_default(),
        ),
        worker_start
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX),
    );
    let metadata = serde_json::to_vec_pretty(&metadata).map_err(|err| err.to_string())?;
    fs::write(json_path, metadata).map_err(|err| err.to_string())?;
    Ok(())
}

fn elapsed_ms(start: Instant) -> u64 {
    start.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

fn base_metadata(
    session: &ClipSession,
    root_layer_id: u32,
    layer_count: usize,
    external_data_count: usize,
    support: Option<(&[clip_runtime::SimpleRasterStackUnsupported], usize)>,
    payload_bytes: usize,
    stats: Option<clip_runtime::NormalRasterStackResourceStats>,
    texture_cache: clip_runtime::GpuTextureCacheStats,
    sparse_atlas: &clip_runtime::GpuSparseAtlasReloadPlan,
    reload_diff: serde_json::Value,
    render_task_graph: Option<serde_json::Value>,
    tile_cache_diagnostics: Option<serde_json::Value>,
    worker_total_ms: u64,
) -> serde_json::Value {
    let canvas = session.summary().canvas;
    let (unsupported_items, source_count) = support.unwrap_or((&[], 0));
    let unsupported = unsupported_items
        .iter()
        .map(|item| {
            json!({
                "node_id": item.render_node_id.0,
                "layer_id": item.layer_id.0,
                "layer_name": session.layer_name(item.layer_id),
                "kind": format!("{:?}", item.kind),
                "reason": item.reason.to_string(),
            })
        })
        .collect::<Vec<_>>();
    let report = if unsupported_items.is_empty() {
        format!("Full native support for {} source(s).", source_count)
    } else {
        format!(
            "{} unsupported node(s) across {} source(s).",
            unsupported_items.len(),
            source_count
        )
    };
    let stats = stats.unwrap_or_default();
    let decode_profile = clip_file::decode_profile::snapshot_if_enabled().map(|profile| {
        json!({
            "selected_raster_tiles": profile.selected_raster_tiles,
            "selected_mask_tiles": profile.selected_mask_tiles,
            "skipped_empty_raster_tiles": profile.skipped_empty_raster_tiles,
            "skipped_empty_mask_tiles": profile.skipped_empty_mask_tiles,
            "compressed_bytes_read": profile.compressed_bytes_read,
            "zlib_inflate_ms": profile.zlib_inflate_ms,
            "tile_swizzle_rgba_ms": profile.tile_swizzle_rgba_ms,
            "mask_r8_decode_ms": profile.mask_r8_decode_ms,
            "mask_crop_ms": profile.mask_crop_ms,
            "atlas_chunk_build_ms": profile.atlas_chunk_build_ms,
            "sparse_atlas_pool_update_ms": profile.sparse_atlas_pool_update_ms,
            "region_patch_render_ms": profile.region_patch_render_ms,
            "worker_total_ms": worker_total_ms,
        })
    });
    json!({
        "width": canvas.width,
        "height": canvas.height,
        "root_layer_id": root_layer_id,
        "layer_count": layer_count,
        "external_data_count": external_data_count,
        "renderer_version": env!("CARGO_PKG_VERSION"),
        "payload_bytes": payload_bytes,
        "reload_diff": reload_diff,
        "decode_profile": decode_profile,
        "render_profile": render_profile_metadata(worker_total_ms),
        "render_task_graph": render_task_graph,
        "tile_cache_diagnostics": tile_cache_diagnostics,
        "texture_cache": {
            "raster_hits": texture_cache.raster_hits,
            "raster_misses": texture_cache.raster_misses,
            "mask_hits": texture_cache.mask_hits,
            "mask_misses": texture_cache.mask_misses,
            "cached_rasters": texture_cache.cached_rasters,
            "cached_masks": texture_cache.cached_masks,
            "cached_bytes": texture_cache.cached_bytes,
        },
        "sparse_atlas_cache": sparse_atlas_metadata(sparse_atlas),
        "support": {
            "source_count": source_count,
            "unsupported_count": unsupported_items.len(),
            "report": report,
            "unsupported": unsupported,
        },
        "resources": {
            "raster_count": stats.raster_count,
            "raster_bytes": stats.raster_bytes,
            "max_raster_layer_id": stats.max_raster_layer_id.map(|layer_id| layer_id.0),
            "max_raster_width": stats.max_raster_width,
            "max_raster_height": stats.max_raster_height,
            "max_raster_bytes": stats.max_raster_bytes,
            "mask_count": stats.mask_count,
            "mask_bytes": stats.mask_bytes,
            "max_mask_layer_id": stats.max_mask_layer_id.map(|layer_id| layer_id.0),
            "max_mask_width": stats.max_mask_width,
            "max_mask_height": stats.max_mask_height,
            "max_mask_bytes": stats.max_mask_bytes,
        },
    })
}

fn reload_plan_metadata(
    plan: &clip_runtime::ReloadDiffPlan,
    payload_bytes: usize,
    patch_renderer: Option<&str>,
    patch_renderer_fallback_reason: Option<&str>,
) -> serde_json::Value {
    let mode = match plan.mode {
        ReloadDiffMode::Full => "full",
        ReloadDiffMode::Patch => "patch",
        ReloadDiffMode::NoChange => "no_change",
    };
    json!({
        "mode": mode,
        "reason": plan.reason,
        "payload_bytes": payload_bytes,
        "patch_renderer": patch_renderer,
        "patch_renderer_fallback_reason": patch_renderer_fallback_reason,
        "rects": plan.dirty_rects,
        "dirty_segments": plan.dirty_segments.iter().map(|segment| {
            json!({
                "ordinal": segment.ordinal,
                "dirty_tile_count": segment.dirty_tile_count,
                "dirty_resource_count": segment.dirty_resource_count,
                "dirty_event_ranges": segment.dirty_event_ranges.iter().map(|range| {
                    json!({
                        "start": range.start,
                        "end": range.end,
                    })
                }).collect::<Vec<_>>(),
            })
        }).collect::<Vec<_>>(),
        "manifest": plan.manifest,
    })
}

fn patch_pixels(
    pixels: &[u8],
    image_width: u32,
    rects: &[ReloadPatchRect],
) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let stride = usize::try_from(u64::from(image_width) * 4)
        .map_err(|_| "image stride does not fit in usize".to_string())?;
    for rect in rects {
        let row_bytes = usize::try_from(u64::from(rect.width) * 4)
            .map_err(|_| "patch row size does not fit in usize".to_string())?;
        let x_bytes = usize::try_from(u64::from(rect.x) * 4)
            .map_err(|_| "patch x offset does not fit in usize".to_string())?;
        for row in rect.y..rect.y + rect.height {
            let row_start = usize::try_from(row)
                .map_err(|_| "patch row offset does not fit in usize".to_string())?
                .checked_mul(stride)
                .ok_or_else(|| "patch row offset overflow".to_string())?;
            let start = row_start
                .checked_add(x_bytes)
                .ok_or_else(|| "patch byte offset overflow".to_string())?;
            let end = start
                .checked_add(row_bytes)
                .ok_or_else(|| "patch byte length overflow".to_string())?;
            let row_pixels = pixels
                .get(start..end)
                .ok_or_else(|| "patch rect exceeds rendered image".to_string())?;
            output.extend_from_slice(row_pixels);
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{write_blender_render_files, write_blender_render_files_with_renderer};

    #[test]
    fn writes_blender_worker_rgba_and_json_files() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../img/Test_Clipping.clip");
        let mut session = clip_runtime::ClipSession::open(path).expect("open Test_Clipping.clip");
        let temp =
            std::env::temp_dir().join(format!("clip_blender_worker_test_{}", std::process::id()));
        std::fs::create_dir_all(&temp).expect("create temp dir");
        let rgba_path = temp.join("render.rgba");
        let json_path = temp.join("render.json");

        write_blender_render_files(&mut session, &rgba_path, &json_path, None)
            .expect("write worker files");

        let rgba = std::fs::read(&rgba_path).expect("read rgba");
        assert_eq!(rgba.len(), 512 * 512 * 4);
        let metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&json_path).expect("read json"))
                .expect("parse json");
        assert_eq!(metadata["width"], 512);
        assert_eq!(metadata["height"], 512);
        assert_eq!(metadata["support"]["unsupported_count"], 0);
        assert_eq!(metadata["reload_diff"]["mode"], "full");
        assert!(metadata["reload_diff"]["patch_renderer"].is_null());
        assert!(metadata["reload_diff"]["patch_renderer_fallback_reason"].is_null());
        assert!(metadata["reload_diff"]["manifest"].is_object());

        let _ = std::fs::remove_file(rgba_path);
        let _ = std::fs::remove_file(json_path);
        let _ = std::fs::remove_dir(temp);
    }

    #[test]
    fn writes_no_change_worker_output_from_previous_manifest() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../img/Test_Clipping.clip");
        let mut session = clip_runtime::ClipSession::open(&path).expect("open Test_Clipping.clip");
        let manifest = session
            .reload_diff_manifest()
            .expect("build reload diff manifest");
        let temp = std::env::temp_dir().join(format!(
            "clip_blender_worker_diff_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&temp).expect("create temp dir");
        let rgba_path = temp.join("render.rgba");
        let json_path = temp.join("render.json");

        write_blender_render_files(&mut session, &rgba_path, &json_path, Some(&manifest))
            .expect("write worker files");

        assert!(std::fs::read(&rgba_path).expect("read rgba").is_empty());
        let metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&json_path).expect("read json"))
                .expect("parse json");
        assert_eq!(metadata["reload_diff"]["mode"], "no_change");
        assert!(metadata["reload_diff"]["patch_renderer"].is_null());
        assert!(metadata["reload_diff"]["patch_renderer_fallback_reason"].is_null());
        assert_eq!(
            metadata["reload_diff"]["rects"].as_array().unwrap().len(),
            0
        );

        let _ = std::fs::remove_file(rgba_path);
        let _ = std::fs::remove_file(json_path);
        let _ = std::fs::remove_dir(temp);
    }

    #[test]
    fn persistent_worker_reports_sparse_atlas_cache_reuse() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../img/Test_Clipping.clip");
        let mut session = clip_runtime::ClipSession::open(&path).expect("open Test_Clipping.clip");
        let manifest = session
            .reload_diff_manifest()
            .expect("build reload diff manifest");
        let renderer =
            clip_runtime::RuntimeGpuRenderer::new_with_texture_cache().expect("create renderer");
        let temp = std::env::temp_dir().join(format!(
            "clip_blender_worker_sparse_atlas_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&temp).expect("create temp dir");
        let rgba_path = temp.join("render.rgba");
        let json_path = temp.join("render.json");

        write_blender_render_files_with_renderer(
            &mut session,
            &rgba_path,
            &json_path,
            None,
            Some(&renderer),
        )
        .expect("write first worker files");
        let first: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&json_path).expect("read first json"))
                .expect("parse first json");
        assert!(
            first["sparse_atlas_cache"]["inserted_tiles"]
                .as_u64()
                .unwrap()
                > 0
        );

        write_blender_render_files_with_renderer(
            &mut session,
            &rgba_path,
            &json_path,
            Some(&manifest),
            Some(&renderer),
        )
        .expect("write second worker files");
        let second: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&json_path).expect("read second json"))
                .expect("parse second json");
        assert_eq!(second["reload_diff"]["mode"], "no_change");
        assert!(
            second["sparse_atlas_cache"]["reused_tiles"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert_eq!(
            second["sparse_atlas_cache"]["rerunnable_segments"]
                .as_array()
                .unwrap()
                .len(),
            0
        );

        let _ = std::fs::remove_file(rgba_path);
        let _ = std::fs::remove_file(json_path);
        let _ = std::fs::remove_dir(temp);
    }

    #[test]
    fn persistent_worker_reports_sparse_atlas_rerunnable_patch_segments() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../img/Test_Clipping.clip");
        let mut session = clip_runtime::ClipSession::open(&path).expect("open Test_Clipping.clip");
        let mut previous_manifest = session
            .reload_diff_manifest()
            .expect("build reload diff manifest");
        let first_tile = previous_manifest
            .sources
            .iter_mut()
            .find_map(|source| source.tiles.first_mut())
            .expect("manifest has compressed tiles");
        first_tile.compressed_hash ^= 1;
        let renderer =
            clip_runtime::RuntimeGpuRenderer::new_with_texture_cache().expect("create renderer");
        let temp = std::env::temp_dir().join(format!(
            "clip_blender_worker_sparse_atlas_patch_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&temp).expect("create temp dir");
        let rgba_path = temp.join("render.rgba");
        let json_path = temp.join("render.json");

        write_blender_render_files_with_renderer(
            &mut session,
            &rgba_path,
            &json_path,
            Some(&previous_manifest),
            Some(&renderer),
        )
        .expect("write worker patch files");
        let metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&json_path).expect("read json"))
                .expect("parse json");

        assert_eq!(metadata["reload_diff"]["mode"], "patch");
        assert_eq!(metadata["reload_diff"]["patch_renderer"], "region");
        assert_eq!(
            metadata["reload_diff"]["patch_renderer_fallback_reason"],
            "sparse_atlas_initial_segments and sparse_atlas_reconstructed_segments returned no executable patch"
        );
        assert!(
            metadata["sparse_atlas_cache"]["inserted_tiles"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert!(
            !metadata["sparse_atlas_cache"]["rerunnable_segments"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            !metadata["sparse_atlas_cache"]["rerunnable_segments"][0]["resident_slots"]
                .as_array()
                .unwrap()
                .is_empty()
        );

        let _ = std::fs::remove_file(rgba_path);
        let _ = std::fs::remove_file(json_path);
        let _ = std::fs::remove_dir(temp);
    }
}
