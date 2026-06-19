use clip_model::CanvasSize;

use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::{CanvasRect, union_optional};
use crate::stream_context::StreamingExecutionContext;
use crate::stream_tile_event::TileEventPayload;
use crate::stream_tile_mask_atlas_plan::MaskAtlasPlan;
use crate::stream_tile_scope_children::{ScopeProgramKind, append_scope_child_events};
use crate::stream_tile_silo_plan::PreparedSiloSource;
use crate::stream_utils::local_pass_bounds;
use crate::{GpuMaskAtlasSource, GpuNormalStackSource};

#[derive(Clone)]
pub(crate) struct ScopeProgramInputs<'a> {
    pub(crate) payloads: Vec<TileEventPayload>,
    pub(crate) event_bounds: Vec<CanvasRect>,
    pub(crate) lut_rows: Vec<&'a [u8]>,
    pub(crate) scope_mask_sources: Vec<GpuMaskAtlasSource>,
    pub(crate) mask_atlas_size: CanvasSize,
    pub(crate) final_dirty_bounds: Option<CanvasRect>,
}

pub(crate) fn build_scope_event_program_inputs<'a, P>(
    context: &StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    target_size: CanvasSize,
    kind: ScopeProgramKind,
    children: &'a [GpuNormalStackSource],
    prepared: Vec<PreparedSiloSource>,
    base_mask_atlas_size: CanvasSize,
    max_mask_atlas_size: u32,
    initial_dirty_bounds: Option<CanvasRect>,
) -> Result<Option<ScopeProgramInputs<'a>>, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let mut child_payloads = Vec::new();
    let mut child_bounds = Vec::new();
    let mut lut_rows = Vec::new();
    let mut scope_dirty = None;
    let mut mask_plan = MaskAtlasPlan::new(base_mask_atlas_size, max_mask_atlas_size);
    if !append_scope_child_events(
        context,
        target_origin,
        kind.container_depth_remaining(),
        kind.through_depth_remaining(),
        kind.clipping_run_policy(),
        children,
        &prepared,
        &mut child_payloads,
        &mut child_bounds,
        &mut lut_rows,
        &mut mask_plan,
        &mut scope_dirty,
    )? {
        return Ok(None);
    }

    let Some(scope_bounds) = context.state.clip_pass_bounds(scope_dirty) else {
        return Ok(None);
    };
    let local_scope_bounds = local_pass_bounds(scope_bounds, target_origin);
    let Some(mask_atlas_origin) = mask_plan.append_mask(
        context.provider,
        kind.mask_key(),
        clip_model::Rect::new(
            scope_bounds.x,
            scope_bounds.y,
            scope_bounds.width,
            scope_bounds.height,
        ),
    ) else {
        return Ok(None);
    };
    let (begin_scope, end_scope) = kind.payloads(local_scope_bounds, mask_atlas_origin);
    let mut payloads = Vec::with_capacity(child_payloads.len() + 2);
    let mut event_bounds = Vec::with_capacity(child_bounds.len() + 2);
    payloads.push(begin_scope);
    event_bounds.push(local_scope_bounds);
    payloads.extend(child_payloads);
    event_bounds.extend(child_bounds);
    payloads.push(end_scope);
    event_bounds.push(local_scope_bounds);

    if !event_bounds_fit_target(&event_bounds, target_size) {
        return Ok(None);
    }
    let mask_atlas_size = mask_plan.size();
    Ok(Some(ScopeProgramInputs {
        payloads,
        event_bounds,
        lut_rows,
        scope_mask_sources: mask_plan.into_sources(),
        mask_atlas_size,
        final_dirty_bounds: union_optional(initial_dirty_bounds, Some(scope_bounds)),
    }))
}

fn event_bounds_fit_target(bounds: &[CanvasRect], target_size: CanvasSize) -> bool {
    bounds.iter().all(|bounds| {
        bounds.width > 0
            && bounds.height > 0
            && bounds.x.saturating_add(bounds.width) <= target_size.width
            && bounds.y.saturating_add(bounds.height) <= target_size.height
    })
}
