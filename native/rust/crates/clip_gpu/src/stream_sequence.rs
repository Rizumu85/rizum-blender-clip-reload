use clip_model::CanvasSize;

use crate::pass::NormalStackPipelines;
use crate::stream::{GpuNormalStackResourceProvider, encode_source_with_provider};
use crate::stream_bounds::CanvasRect;
use crate::stream_state::{StreamingEncoder, StreamingTexturePair};
use crate::stream_tile_silo::{encode_raster_silo_run_with_provider, raster_silo_run_len};
use crate::{GpuNormalStackSource, GpuRenderer};

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_source_sequence_with_provider<P>(
    renderer: &GpuRenderer,
    provider: &mut P,
    state: &mut StreamingEncoder<'_, P::Error>,
    output_size: CanvasSize,
    target_origin: (i32, i32),
    sources: &[GpuNormalStackSource],
    texture_pair: &StreamingTexturePair,
    mut previous_index: usize,
    mut next_index: usize,
    fallback_texture: Option<&wgpu::Texture>,
    pipelines: &NormalStackPipelines,
    dirty_bounds: &mut Option<CanvasRect>,
) -> Result<(usize, usize), P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let mut source_index = 0usize;
    while source_index < sources.len() {
        let run_len = raster_silo_run_len(
            provider,
            output_size,
            target_origin,
            texture_pair.size(),
            &sources[source_index..],
        );
        if run_len >= 2 {
            let wrote_silo = encode_raster_silo_run_with_provider(
                renderer,
                provider,
                state,
                output_size,
                target_origin,
                texture_pair.size(),
                &sources[source_index..source_index + run_len],
                texture_pair.view(previous_index),
                texture_pair.view(next_index),
                dirty_bounds,
            )?;
            if wrote_silo {
                std::mem::swap(&mut previous_index, &mut next_index);
                source_index += run_len;
                continue;
            }
        }

        let effective_fallback_texture = match fallback_texture {
            Some(texture) => texture,
            None => texture_pair.texture(previous_index),
        };
        let did_write = encode_source_with_provider(
            renderer,
            provider,
            state,
            output_size,
            target_origin,
            &sources[source_index],
            texture_pair.texture(previous_index),
            effective_fallback_texture,
            texture_pair.view(previous_index),
            texture_pair.view(next_index),
            pipelines,
            dirty_bounds,
        )?;
        if did_write {
            std::mem::swap(&mut previous_index, &mut next_index);
        }
        source_index += 1;
    }

    Ok((previous_index, next_index))
}
