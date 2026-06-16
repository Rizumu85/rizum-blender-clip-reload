use clip_model::LayerId;
use clip_runtime::ClipSession;
use clip_runtime::NormalRasterStackSupportResult;
use serde_json::json;

pub(crate) fn normal_support_report_json(
    session: &ClipSession,
    result: &NormalRasterStackSupportResult,
) -> String {
    let summary = session.summary();
    let stats = result.resource_stats;
    let unsupported = result
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
    serde_json::to_string_pretty(&json!({
        "canvas": {
            "width": summary.canvas.width,
            "height": summary.canvas.height,
        },
        "root_layer_id": summary.root_layer_id.0,
        "layer_count": summary.layer_count,
        "external_data_count": summary.external_data_count,
        "support": {
            "source_count": result.source_count,
            "unsupported_count": result.unsupported.len(),
            "unsupported": unsupported,
        },
        "resources": {
            "raster_count": stats.raster_count,
            "raster_bytes": stats.raster_bytes,
            "max_raster_layer_id": stats.max_raster_layer_id.map(|layer_id| layer_id.0),
            "max_raster_layer_name": layer_name(session, stats.max_raster_layer_id),
            "max_raster_width": stats.max_raster_width,
            "max_raster_height": stats.max_raster_height,
            "max_raster_bytes": stats.max_raster_bytes,
            "mask_count": stats.mask_count,
            "mask_bytes": stats.mask_bytes,
            "max_mask_layer_id": stats.max_mask_layer_id.map(|layer_id| layer_id.0),
            "max_mask_layer_name": layer_name(session, stats.max_mask_layer_id),
            "max_mask_width": stats.max_mask_width,
            "max_mask_height": stats.max_mask_height,
            "max_mask_bytes": stats.max_mask_bytes,
        },
    }))
    .expect("support report JSON is serializable")
}

fn layer_name(session: &ClipSession, layer_id: Option<LayerId>) -> Option<&str> {
    layer_id.and_then(|layer_id| session.layer_name(layer_id))
}

#[cfg(test)]
mod tests {
    use clip_graph::{RenderNodeId, RenderNodeKind};
    use clip_model::LayerId;
    use clip_runtime::{NormalRasterStackResourceStats, NormalRasterStackSupportResult};
    use clip_runtime::{SimpleRasterStackUnsupported, SimpleRasterStackUnsupportedReason};

    use super::normal_support_report_json;

    #[test]
    fn writes_support_report_as_json_with_layer_names() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../img/Test_Clipping.clip");
        let session = clip_runtime::ClipSession::open(path).expect("open Test_Clipping.clip");
        let result = NormalRasterStackSupportResult {
            source_count: 6,
            resource_stats: NormalRasterStackResourceStats {
                raster_count: 3,
                raster_bytes: 2048,
                max_raster_layer_id: Some(LayerId(10)),
                max_raster_width: 32,
                max_raster_height: 16,
                max_raster_bytes: 2048,
                mask_count: 1,
                mask_bytes: 512,
                max_mask_layer_id: Some(LayerId(11)),
                max_mask_width: 32,
                max_mask_height: 16,
                max_mask_bytes: 512,
            },
            unsupported: vec![SimpleRasterStackUnsupported {
                render_node_id: RenderNodeId(2),
                layer_id: LayerId(10),
                kind: RenderNodeKind::Raster,
                reason: SimpleRasterStackUnsupportedReason::Composite(999),
            }],
        };

        let report = normal_support_report_json(&session, &result);
        let parsed: serde_json::Value =
            serde_json::from_str(&report).expect("support report should be valid JSON");

        assert_eq!(parsed["canvas"]["width"], 512);
        assert_eq!(parsed["root_layer_id"], 2);
        assert_eq!(parsed["support"]["source_count"], 6);
        assert_eq!(parsed["support"]["unsupported_count"], 1);
        assert_eq!(parsed["support"]["unsupported"][0]["layer_id"], 10);
        assert_eq!(parsed["support"]["unsupported"][0]["layer_name"], "Layer 1");
        assert_eq!(parsed["resources"]["raster_count"], 3);
        assert_eq!(parsed["resources"]["max_raster_layer_id"], 10);
        assert_eq!(parsed["resources"]["max_raster_layer_name"], "Layer 1");
        assert_eq!(parsed["resources"]["max_mask_layer_id"], 11);
        assert_eq!(parsed["resources"]["max_mask_layer_name"], "Layer 2");
    }
}
