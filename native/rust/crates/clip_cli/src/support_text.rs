use clip_runtime::{ClipSession, NormalRasterStackSupportResult};

use crate::layer_labels::optional_layer_label;

pub(crate) fn normal_support_check_text(
    session: &ClipSession,
    result: &NormalRasterStackSupportResult,
) -> String {
    let stats = result.resource_stats;
    let mut lines = vec![
        format!(
            "gpu support check sources={} unsupported={}",
            result.source_count,
            result.unsupported.len(),
        ),
        format!(
            "  resource stats rasters={} raster_bytes={} max_raster_layer={} max_raster={}x{} max_raster_bytes={} masks={} mask_bytes={} max_mask_layer={} max_mask={}x{} max_mask_bytes={}",
            stats.raster_count,
            stats.raster_bytes,
            optional_layer_label(session, stats.max_raster_layer_id),
            stats.max_raster_width,
            stats.max_raster_height,
            stats.max_raster_bytes,
            stats.mask_count,
            stats.mask_bytes,
            optional_layer_label(session, stats.max_mask_layer_id),
            stats.max_mask_width,
            stats.max_mask_height,
            stats.max_mask_bytes,
        ),
    ];
    lines.extend(result.unsupported.iter().map(|unsupported| {
        format!(
            "  unsupported node={} layer={} kind={:?} reason={}",
            unsupported.render_node_id.0,
            optional_layer_label(session, Some(unsupported.layer_id)),
            unsupported.kind,
            unsupported.reason,
        )
    }));
    lines.push(String::new());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use clip_graph::{RenderNodeId, RenderNodeKind};
    use clip_model::LayerId;
    use clip_runtime::{NormalRasterStackResourceStats, NormalRasterStackSupportResult};
    use clip_runtime::{SimpleRasterStackUnsupported, SimpleRasterStackUnsupportedReason};

    use super::normal_support_check_text;

    #[test]
    fn writes_support_check_text_with_layer_names() {
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

        let report = normal_support_check_text(&session, &result);

        assert!(report.contains("gpu support check sources=6 unsupported=1"));
        assert!(report.contains("max_raster_layer=10 [Layer 1]"));
        assert!(report.contains("max_mask_layer=11 [Layer 2]"));
        assert!(report.contains(
            "unsupported node=2 layer=10 [Layer 1] kind=Raster reason=LayerComposite 999 is not direct copy"
        ));
    }
}
