use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;

use clip_model::{CanvasSize, LayerId};

use crate::{LayerGraphInput, RenderNode, RenderNodeId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderPlan {
    pub canvas: CanvasSize,
    pub root_layer_id: LayerId,
    pub nodes: Vec<RenderNode>,
}

impl RenderPlan {
    pub fn build(
        canvas: CanvasSize,
        root_layer_id: LayerId,
        layers: &[LayerGraphInput],
    ) -> Result<Self, RenderPlanError> {
        build_render_plan(canvas, root_layer_id, layers)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RenderPlanError {
    DuplicateLayerId(LayerId),
    MissingLayer {
        layer_id: LayerId,
    },
    MissingLinkedLayer {
        from_layer_id: LayerId,
        target_layer_id: LayerId,
    },
    LayerGraphCycle {
        layer_id: LayerId,
    },
    DepthOverflow {
        layer_id: LayerId,
    },
    NodeCountOverflow,
}

impl fmt::Display for RenderPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateLayerId(layer_id) => {
                write!(f, "duplicate layer id {}", layer_id.0)
            }
            Self::MissingLayer { layer_id } => {
                write!(f, "missing root layer {}", layer_id.0)
            }
            Self::MissingLinkedLayer {
                from_layer_id,
                target_layer_id,
            } => write!(
                f,
                "layer {} references missing linked layer {}",
                from_layer_id.0, target_layer_id.0,
            ),
            Self::LayerGraphCycle { layer_id } => {
                write!(f, "layer graph contains a cycle at layer {}", layer_id.0)
            }
            Self::DepthOverflow { layer_id } => {
                write!(f, "layer graph depth overflows at layer {}", layer_id.0)
            }
            Self::NodeCountOverflow => f.write_str("render node count overflow"),
        }
    }
}

impl Error for RenderPlanError {}

pub fn build_render_plan(
    canvas: CanvasSize,
    root_layer_id: LayerId,
    layers: &[LayerGraphInput],
) -> Result<RenderPlan, RenderPlanError> {
    let mut planner = Planner::new(layers)?;
    planner.traverse_root(root_layer_id)?;
    Ok(RenderPlan {
        canvas,
        root_layer_id,
        nodes: planner.nodes,
    })
}

struct Planner {
    layers: HashMap<LayerId, LayerGraphInput>,
    visited: HashSet<LayerId>,
    nodes: Vec<RenderNode>,
}

impl Planner {
    fn new(layers: &[LayerGraphInput]) -> Result<Self, RenderPlanError> {
        let mut by_id = HashMap::with_capacity(layers.len());
        for layer in layers {
            if by_id.insert(layer.id, layer.clone()).is_some() {
                return Err(RenderPlanError::DuplicateLayerId(layer.id));
            }
        }
        Ok(Self {
            layers: by_id,
            visited: HashSet::new(),
            nodes: Vec::new(),
        })
    }

    fn traverse_root(&mut self, root_layer_id: LayerId) -> Result<(), RenderPlanError> {
        let root =
            self.layers
                .get(&root_layer_id)
                .cloned()
                .ok_or(RenderPlanError::MissingLayer {
                    layer_id: root_layer_id,
                })?;
        self.traverse_layer(root, 0)
    }

    fn traverse_chain(
        &mut self,
        first_layer_id: LayerId,
        depth: u16,
        parent_layer_id: LayerId,
    ) -> Result<(), RenderPlanError> {
        let mut current = first_layer_id;
        let mut from = parent_layer_id;
        loop {
            let layer =
                self.layers
                    .get(&current)
                    .cloned()
                    .ok_or(RenderPlanError::MissingLinkedLayer {
                        from_layer_id: from,
                        target_layer_id: current,
                    })?;
            let next_layer_id = layer.next_layer_id;
            self.traverse_layer(layer, depth)?;
            let Some(next_layer_id) = next_layer_id else {
                return Ok(());
            };
            from = current;
            current = next_layer_id;
        }
    }

    fn traverse_layer(
        &mut self,
        layer: LayerGraphInput,
        depth: u16,
    ) -> Result<(), RenderPlanError> {
        if !self.visited.insert(layer.id) {
            return Err(RenderPlanError::LayerGraphCycle { layer_id: layer.id });
        }

        if !layer.visibility.is_visible() {
            return Ok(());
        }

        let node_id = RenderNodeId(
            u32::try_from(self.nodes.len()).map_err(|_| RenderPlanError::NodeCountOverflow)?,
        );
        let first_child_layer_id = layer.first_child_layer_id;
        let layer_id = layer.id;
        self.nodes
            .push(RenderNode::from_layer_input(node_id, layer, depth));

        if let Some(first_child_layer_id) = first_child_layer_id {
            let child_depth = depth
                .checked_add(1)
                .ok_or(RenderPlanError::DepthOverflow { layer_id })?;
            self.traverse_chain(first_child_layer_id, child_depth, layer_id)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use clip_model::{CanvasSize, LayerId, LayerKind, LayerOpacity, LayerVisibility};

    use crate::{LayerGraphInput, RenderNodeKind, build_render_plan};

    #[test]
    fn preserves_visible_sibling_and_child_order() {
        let layers: Vec<LayerGraphInput> = vec![
            layer(LayerId(2), LayerKind::Folder).with_child(LayerId(4)),
            layer(LayerId(4), LayerKind::Paper).with_next(LayerId(10)),
            layer(LayerId(10), LayerKind::Raster).with_next(LayerId(11)),
            layer(LayerId(11), LayerKind::Raster),
        ]
        .into_iter()
        .map(Into::into)
        .collect();

        let plan = build_render_plan(CanvasSize::new(512, 512), LayerId(2), &layers).unwrap();

        assert_eq!(plan.nodes.len(), 4);
        assert_eq!(plan.nodes[0].layer_id, LayerId(2));
        assert_eq!(plan.nodes[0].kind, RenderNodeKind::Container);
        assert_eq!(plan.nodes[0].depth, 0);
        assert_eq!(plan.nodes[1].layer_id, LayerId(4));
        assert_eq!(plan.nodes[1].kind, RenderNodeKind::Paper);
        assert_eq!(plan.nodes[1].depth, 1);
        assert_eq!(plan.nodes[2].layer_id, LayerId(10));
        assert_eq!(plan.nodes[2].kind, RenderNodeKind::Raster);
        assert_eq!(plan.nodes[3].layer_id, LayerId(11));
        assert_eq!(plan.nodes[3].kind, RenderNodeKind::Raster);
    }

    #[test]
    fn hidden_container_hides_subtree_but_not_next_sibling() {
        let layers: Vec<LayerGraphInput> = vec![
            layer(LayerId(2), LayerKind::Folder).with_child(LayerId(4)),
            layer(LayerId(4), LayerKind::Folder)
                .hidden()
                .with_child(LayerId(5))
                .with_next(LayerId(10)),
            layer(LayerId(5), LayerKind::Raster),
            layer(LayerId(10), LayerKind::Raster),
        ]
        .into_iter()
        .map(Into::into)
        .collect();

        let plan = build_render_plan(CanvasSize::new(512, 512), LayerId(2), &layers).unwrap();

        let ids: Vec<_> = plan.nodes.iter().map(|node| node.layer_id).collect();
        assert_eq!(ids, vec![LayerId(2), LayerId(10)]);
    }

    fn layer(id: LayerId, kind: LayerKind) -> TestLayer {
        TestLayer(LayerGraphInput {
            id,
            name: String::new(),
            kind,
            visibility: LayerVisibility(1),
            clip: false,
            opacity: LayerOpacity::MAX,
            composite: 0,
            next_layer_id: None,
            first_child_layer_id: None,
            render_mipmap_id: None,
            mask_mipmap_id: None,
            paper_color: None,
        })
    }

    struct TestLayer(LayerGraphInput);

    impl TestLayer {
        fn with_next(mut self, layer_id: LayerId) -> Self {
            self.0.next_layer_id = Some(layer_id);
            self
        }

        fn with_child(mut self, layer_id: LayerId) -> Self {
            self.0.first_child_layer_id = Some(layer_id);
            self
        }

        fn hidden(mut self) -> Self {
            self.0.visibility = LayerVisibility(0);
            self
        }
    }

    impl From<TestLayer> for LayerGraphInput {
        fn from(value: TestLayer) -> Self {
            value.0
        }
    }
}
