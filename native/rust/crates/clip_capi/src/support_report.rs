use clip_runtime::{ClipSession, SimpleRasterStackUnsupported};

pub(crate) fn support_report(
    session: Option<&ClipSession>,
    source_count: usize,
    unsupported: &[SimpleRasterStackUnsupported],
) -> String {
    if unsupported.is_empty() {
        return format!("Full native support for {source_count} source(s).");
    }
    let mut report = format!("{} unsupported node(s).", unsupported.len());
    for item in unsupported {
        let layer_label = support_layer_label(session, item);
        report.push_str(&format!(
            "\n- {layer_label} node {} {:?}: {}",
            item.render_node_id.0, item.kind, item.reason,
        ));
    }
    report
}

fn support_layer_label(
    session: Option<&ClipSession>,
    item: &SimpleRasterStackUnsupported,
) -> String {
    let name = session
        .and_then(|session| session.layer_name(item.layer_id))
        .unwrap_or("")
        .trim();
    if name.is_empty() {
        return format!("layer {}", item.layer_id.0);
    }
    let name = name
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>();
    format!("layer {} [{}]", item.layer_id.0, name)
}

#[cfg(test)]
mod tests {
    use clip_graph::{RenderNodeId, RenderNodeKind};
    use clip_model::LayerId;
    use clip_runtime::{ClipSession, SimpleRasterStackUnsupportedReason};

    use super::*;

    #[test]
    fn support_report_lists_all_unsupported_details() {
        let unsupported: Vec<_> = (0..10)
            .map(|index| SimpleRasterStackUnsupported {
                render_node_id: RenderNodeId(index + 4),
                layer_id: LayerId(index + 9),
                kind: RenderNodeKind::Filter,
                reason: SimpleRasterStackUnsupportedReason::Filter,
            })
            .collect();

        let report = support_report(None, 12, &unsupported);

        assert!(report.starts_with("10 unsupported node(s)."));
        assert!(report.contains(
            "- layer 9 node 4 Filter: filter layer is not in the strict raster stack pass"
        ));
        assert!(report.contains(
            "- layer 16 node 11 Filter: filter layer is not in the strict raster stack pass"
        ));
        assert!(report.contains(
            "- layer 17 node 12 Filter: filter layer is not in the strict raster stack pass"
        ));
        assert!(report.contains(
            "- layer 18 node 13 Filter: filter layer is not in the strict raster stack pass"
        ));
        assert!(!report.contains("more unsupported node(s)"));
    }

    #[test]
    fn support_report_includes_layer_names_when_session_has_them() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../img/Test_Clipping.clip");
        let session = ClipSession::open(path).expect("open Test_Clipping.clip");
        let unsupported = vec![SimpleRasterStackUnsupported {
            render_node_id: RenderNodeId(2),
            layer_id: LayerId(10),
            kind: RenderNodeKind::Raster,
            reason: SimpleRasterStackUnsupportedReason::Composite(999),
        }];

        let report = support_report(Some(&session), 4, &unsupported);

        assert!(
            report.contains(
                "- layer 10 [Layer 1] node 2 Raster: LayerComposite 999 is not direct copy"
            )
        );
    }
}
