use std::fs;
use std::path::Path;

use clip_model::Rect;
use clip_runtime::ClipSession;
use serde_json::json;

pub(crate) fn write_blender_render_files(
    session: &mut ClipSession,
    rgba_path: &Path,
    json_path: &Path,
) -> Result<(), String> {
    let canvas = session.summary().canvas;
    let summary = session.summary();
    let root_layer_id = summary.root_layer_id.0;
    let layer_count = summary.layer_count;
    let external_data_count = summary.external_data_count;
    let support = session
        .check_normal_raster_stack_support()
        .map_err(|err| err.to_string())?;
    let stats = support.resource_stats;

    let byte_len = usize::try_from(u64::from(canvas.width) * u64::from(canvas.height) * 4)
        .map_err(|_| "canvas byte length does not fit in usize".to_string())?;
    let mut pixels = vec![0u8; byte_len];
    session
        .read_rgba8_region(Rect::new(0, 0, canvas.width, canvas.height), &mut pixels)
        .map_err(|err| err.to_string())?;
    fs::write(rgba_path, &pixels).map_err(|err| err.to_string())?;

    let unsupported = support
        .unsupported
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
    let report = if support.unsupported.is_empty() {
        format!(
            "Full native support for {} source(s).",
            support.source_count
        )
    } else {
        format!(
            "{} unsupported node(s) across {} source(s).",
            support.unsupported.len(),
            support.source_count
        )
    };
    let metadata = json!({
        "width": canvas.width,
        "height": canvas.height,
        "root_layer_id": root_layer_id,
        "layer_count": layer_count,
        "external_data_count": external_data_count,
        "renderer_version": env!("CARGO_PKG_VERSION"),
        "support": {
            "source_count": support.source_count,
            "unsupported_count": support.unsupported.len(),
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
    });
    let metadata = serde_json::to_vec_pretty(&metadata).map_err(|err| err.to_string())?;
    fs::write(json_path, metadata).map_err(|err| err.to_string())?;
    Ok(())
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

        write_blender_render_files(&mut session, &rgba_path, &json_path)
            .expect("write worker files");

        let rgba = std::fs::read(&rgba_path).expect("read rgba");
        assert_eq!(rgba.len(), 512 * 512 * 4);
        let metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&json_path).expect("read json"))
                .expect("parse json");
        assert_eq!(metadata["width"], 512);
        assert_eq!(metadata["height"], 512);
        assert_eq!(metadata["support"]["unsupported_count"], 0);

        let _ = std::fs::remove_file(rgba_path);
        let _ = std::fs::remove_file(json_path);
        let _ = std::fs::remove_dir(temp);
    }
}
