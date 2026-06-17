use serde::{Deserialize, Serialize};

use crate::{ClipSession, RuntimeError};

mod reload_diff_manifest;
mod reload_diff_plan;
#[cfg(test)]
mod reload_diff_tests;

use reload_diff_manifest::{
    mask_source_manifest, node_signature, raster_source_manifest, render_node_kind_name,
};
use reload_diff_plan::{full_plan, plan_reload_diff};

pub(crate) const MANIFEST_ABI: u32 = 1;
pub(crate) const RELOAD_TILE_SIZE: u32 = clip_file::tiles::TILE_SIZE as u32;
pub(crate) const FULL_DIRTY_AREA_RATIO: f64 = 0.5;
pub(crate) const MAX_PATCH_RECTS: usize = 256;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReloadDiffManifest {
    pub abi: u32,
    pub tile_size: u32,
    pub width: u32,
    pub height: u32,
    pub root_layer_id: u32,
    pub nodes: Vec<ReloadDiffNode>,
    pub sources: Vec<ReloadDiffSource>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReloadDiffNode {
    pub layer_id: u32,
    pub kind: String,
    pub depth: u16,
    pub clip: bool,
    pub opacity: u16,
    pub composite: u32,
    pub render_mipmap_id: Option<u32>,
    pub mask_mipmap_id: Option<u32>,
    pub paper_color: Option<[u8; 4]>,
    pub signature: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReloadDiffSource {
    pub kind: String,
    pub layer_id: u32,
    pub resource_id: u32,
    pub external_id: String,
    pub offset_x: i32,
    pub offset_y: i32,
    pub width: u32,
    pub height: u32,
    pub color_type: Option<u32>,
    pub empty_fill: Option<u8>,
    pub signature: u64,
    pub tiles: Vec<ReloadDiffTile>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReloadDiffTile {
    pub tile_x: u32,
    pub tile_y: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub compressed_bytes: u32,
    pub compressed_hash: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReloadDiffPlan {
    pub manifest: ReloadDiffManifest,
    pub mode: ReloadDiffMode,
    pub reason: String,
    pub dirty_rects: Vec<ReloadPatchRect>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReloadDiffMode {
    Full,
    Patch,
    NoChange,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReloadPatchRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl ClipSession {
    pub fn reload_diff_manifest(&self) -> Result<ReloadDiffManifest, RuntimeError> {
        let mut nodes = Vec::with_capacity(self.render_plan.nodes.len());
        for node in &self.render_plan.nodes {
            nodes.push(ReloadDiffNode {
                layer_id: node.layer_id.0,
                kind: render_node_kind_name(node.kind).to_string(),
                depth: node.depth,
                clip: node.clip,
                opacity: node.opacity.0,
                composite: node.composite,
                render_mipmap_id: node.render_mipmap_id,
                mask_mipmap_id: node.mask_mipmap_id,
                paper_color: node
                    .paper_color
                    .map(|color| [color.r, color.g, color.b, color.a]),
                signature: node_signature(node),
            });
        }

        let mut sources = Vec::new();
        for source in self.raster_sources.values() {
            sources.push(raster_source_manifest(
                &self.container,
                self.summary.canvas,
                source,
            )?);
        }
        for source in self.mask_sources.values() {
            sources.push(mask_source_manifest(
                &self.container,
                self.summary.canvas,
                source,
            )?);
        }
        sources.sort_by_key(|source| {
            (
                source.kind.clone(),
                source.layer_id,
                source.resource_id,
                source.external_id.clone(),
            )
        });

        Ok(ReloadDiffManifest {
            abi: MANIFEST_ABI,
            tile_size: RELOAD_TILE_SIZE,
            width: self.summary.canvas.width,
            height: self.summary.canvas.height,
            root_layer_id: self.summary.root_layer_id.0,
            nodes,
            sources,
        })
    }

    pub fn plan_reload_diff(
        &self,
        previous: Option<&ReloadDiffManifest>,
    ) -> Result<ReloadDiffPlan, RuntimeError> {
        let manifest = self.reload_diff_manifest()?;
        let Some(previous) = previous else {
            return Ok(full_plan(manifest, "no previous reload manifest"));
        };
        Ok(plan_reload_diff(previous, manifest))
    }
}
