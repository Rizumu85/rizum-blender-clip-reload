use std::fs;
use std::path::Path;

use clip_runtime::{
    ClipSession, ReloadDiffManifest, ReloadDiffMode, ReloadPatchRect, RuntimeError,
    RuntimeGpuRenderer,
};
use serde_json::json;

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
    let summary = session.summary();
    let root_layer_id = summary.root_layer_id.0;
    let layer_count = summary.layer_count;
    let external_data_count = summary.external_data_count;
    let reload_plan = session
        .plan_reload_diff(previous_manifest)
        .map_err(|err| err.to_string())?;
    if reload_plan.mode == ReloadDiffMode::NoChange {
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
            reload_plan_metadata(&reload_plan, 0),
        );
        let metadata = serde_json::to_vec_pretty(&metadata).map_err(|err| err.to_string())?;
        fs::write(json_path, metadata).map_err(|err| err.to_string())?;
        return Ok(());
    }

    let render = match renderer {
        Some(renderer) => renderer.draw_normal_raster_stack(session),
        None => session.draw_normal_raster_stack_via_gpu(),
    }
    .map_err(|err| err.to_string())?;
    if !render.unsupported.is_empty() {
        return Err(RuntimeError::UnsupportedRenderPlan {
            unsupported: render.unsupported,
        }
        .to_string());
    }
    let source_count = render.source_count;
    let stats = render.resource_stats;
    let unsupported_items = render.unsupported;
    let image = render
        .image
        .ok_or(RuntimeError::EmptyRenderPlan)
        .map_err(|err| err.to_string())?;
    let (rgba_payload, payload_bytes) = match reload_plan.mode {
        ReloadDiffMode::Full => (image.pixels.clone(), image.pixels.len()),
        ReloadDiffMode::Patch => {
            let payload = patch_pixels(&image.pixels, image.width, &reload_plan.dirty_rects)?;
            let len = payload.len();
            (payload, len)
        }
        ReloadDiffMode::NoChange => unreachable!("no-change reload returns before rendering"),
    };
    fs::write(rgba_path, &rgba_payload).map_err(|err| err.to_string())?;

    let metadata = base_metadata(
        session,
        root_layer_id,
        layer_count,
        external_data_count,
        Some((&unsupported_items, source_count)),
        payload_bytes,
        Some(stats),
        reload_plan_metadata(&reload_plan, payload_bytes),
    );
    let metadata = serde_json::to_vec_pretty(&metadata).map_err(|err| err.to_string())?;
    fs::write(json_path, metadata).map_err(|err| err.to_string())?;
    Ok(())
}

fn base_metadata(
    session: &ClipSession,
    root_layer_id: u32,
    layer_count: usize,
    external_data_count: usize,
    support: Option<(&[clip_runtime::SimpleRasterStackUnsupported], usize)>,
    payload_bytes: usize,
    stats: Option<clip_runtime::NormalRasterStackResourceStats>,
    reload_diff: serde_json::Value,
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
    json!({
        "width": canvas.width,
        "height": canvas.height,
        "root_layer_id": root_layer_id,
        "layer_count": layer_count,
        "external_data_count": external_data_count,
        "renderer_version": env!("CARGO_PKG_VERSION"),
        "payload_bytes": payload_bytes,
        "reload_diff": reload_diff,
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
        "rects": plan.dirty_rects,
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
    use super::write_blender_render_files;

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
        assert_eq!(
            metadata["reload_diff"]["rects"].as_array().unwrap().len(),
            0
        );

        let _ = std::fs::remove_file(rgba_path);
        let _ = std::fs::remove_file(json_path);
        let _ = std::fs::remove_dir(temp);
    }
}
