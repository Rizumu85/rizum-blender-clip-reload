use crate::GpuNormalStackSource;
use crate::stream::{GpuNormalStackResourceProvider, encode_source_with_provider};
use crate::stream_bounds::CanvasRect;
use crate::stream_context::StreamingExecutionContext;
use crate::stream_state::StreamingTexturePair;

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_legacy_segment<P>(
    context: &mut StreamingExecutionContext<'_, '_, P>,
    target_origin: (i32, i32),
    sources: &[GpuNormalStackSource],
    source_range: std::ops::Range<usize>,
    texture_pair: &StreamingTexturePair,
    previous_index: &mut usize,
    next_index: &mut usize,
    fallback_texture: Option<&wgpu::Texture>,
    dirty_bounds: &mut Option<CanvasRect>,
) -> Result<(), P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    for source_index in source_range {
        let effective_fallback_texture = match fallback_texture {
            Some(texture) => texture,
            None => texture_pair.texture(*previous_index),
        };
        let did_write = encode_source_with_provider(
            context,
            target_origin,
            &sources[source_index],
            texture_pair.texture(*previous_index),
            effective_fallback_texture,
            texture_pair.view(*previous_index),
            texture_pair.view(*next_index),
            dirty_bounds,
        )?;
        if did_write {
            std::mem::swap(previous_index, next_index);
        }
    }

    Ok(())
}
