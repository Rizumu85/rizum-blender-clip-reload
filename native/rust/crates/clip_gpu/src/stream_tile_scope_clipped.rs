use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::{CanvasRect, union_optional};
use crate::stream_context::StreamingExecutionContext;
use crate::stream_tile_event::{ScopeTileEventPayload, TileEventPayload};
use crate::stream_tile_filter_silo::raster_payload;
use crate::stream_tile_mask_atlas_plan::MaskAtlasPlan;
use crate::stream_tile_silo_plan::PreparedSiloSource;
use crate::stream_utils::local_pass_bounds;
use crate::{GpuClippedStackSource, GpuNormalStackSource};

#[allow(clippy::too_many_arguments)]
pub(crate) fn append_clipped_sibling_events<P>(
    context: &StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    clipped: &[GpuClippedStackSource],
    prepared: &[PreparedSiloSource],
    payloads: &mut Vec<TileEventPayload>,
    event_bounds: &mut Vec<CanvasRect>,
    mask_plan: &mut MaskAtlasPlan,
) -> Result<bool, P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    for clipped_source in clipped {
        match clipped_source {
            GpuClippedStackSource::Raster(raster) => {
                for prepared_source in prepared.iter().filter(|item| item.source == *raster) {
                    payloads.push(TileEventPayload::ClippedRaster(raster_payload(
                        prepared_source,
                    )));
                    event_bounds.push(prepared_source.local_bounds);
                }
            }
            GpuClippedStackSource::Container {
                children,
                opacity,
                mask_key,
                blend_mode,
                ..
            } => {
                if *opacity <= 0.0 || children.is_empty() {
                    return Ok(false);
                }
                let mut child_payloads = Vec::new();
                let mut child_bounds = Vec::new();
                let mut child_dirty = None;
                for child in children {
                    let GpuNormalStackSource::Raster(raster) = child else {
                        return Ok(false);
                    };
                    for prepared_source in prepared.iter().filter(|item| item.source == *raster) {
                        child_payloads
                            .push(TileEventPayload::Raster(raster_payload(prepared_source)));
                        child_bounds.push(prepared_source.local_bounds);
                        child_dirty = union_optional(child_dirty, Some(prepared_source.bounds));
                    }
                }
                let Some(scope_bounds) = context.state.clip_pass_bounds(child_dirty) else {
                    continue;
                };
                let local_scope_bounds = local_pass_bounds(scope_bounds, target_origin);
                let Some(mask_atlas_origin) = mask_plan.append_mask(
                    context.provider,
                    *mask_key,
                    clip_model::Rect::new(
                        scope_bounds.x,
                        scope_bounds.y,
                        scope_bounds.width,
                        scope_bounds.height,
                    ),
                ) else {
                    return Ok(false);
                };
                let scope = ScopeTileEventPayload {
                    opacity: *opacity,
                    blend_mode: *blend_mode,
                    local_bounds: local_scope_bounds,
                    mask_atlas_origin,
                };
                payloads.push(TileEventPayload::BeginClippedContainer(scope));
                event_bounds.push(local_scope_bounds);
                payloads.extend(child_payloads);
                event_bounds.extend(child_bounds);
                payloads.push(TileEventPayload::EndClippedContainer(scope));
                event_bounds.push(local_scope_bounds);
            }
        }
    }
    Ok(true)
}
