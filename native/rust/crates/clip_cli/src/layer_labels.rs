use clip_model::LayerId;
use clip_runtime::ClipSession;

pub(crate) fn layer_label(session: &ClipSession, layer_id: LayerId) -> String {
    let name = session.layer_name(layer_id).unwrap_or("").trim();
    if name.is_empty() {
        return layer_id.0.to_string();
    }
    let name = name
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>();
    format!("{} [{}]", layer_id.0, name)
}

pub(crate) fn optional_layer_label(session: &ClipSession, layer_id: Option<LayerId>) -> String {
    layer_id.map_or_else(
        || "-".to_string(),
        |layer_id| layer_label(session, layer_id),
    )
}

pub(crate) fn optional_raw_layer_label(session: &ClipSession, layer_id: Option<u32>) -> String {
    optional_layer_label(session, layer_id.map(LayerId))
}

#[cfg(test)]
mod tests {
    use clip_model::LayerId;

    use super::{layer_label, optional_layer_label};

    #[test]
    fn layer_label_includes_layer_name_when_available() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../img/Test_Clipping.clip");
        let session = clip_runtime::ClipSession::open(path).expect("open Test_Clipping.clip");

        assert_eq!(layer_label(&session, LayerId(10)), "10 [Layer 1]");
        assert_eq!(
            optional_layer_label(&session, Some(LayerId(11))),
            "11 [Layer 2]"
        );
        assert_eq!(optional_layer_label(&session, None), "-");
    }
}
