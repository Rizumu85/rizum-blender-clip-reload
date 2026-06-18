use clip_model::CanvasSize;

use crate::pass::NormalStackPipelines;
use crate::stream_bounds::CanvasRect;
use crate::stream_provider::GpuNormalStackResourceProvider;
use crate::stream_state::{StreamingEncoder, StreamingTexturePair};
use crate::{GpuRasterResourceInfo, GpuRenderer};

pub(crate) struct StreamingExecutionContext<'a, 'p, P>
where
    P: GpuNormalStackResourceProvider,
{
    pub(crate) renderer: &'a GpuRenderer,
    pub(crate) provider: &'p mut P,
    pub(crate) state: StreamingEncoder<'a, P::Error>,
    pub(crate) output_size: CanvasSize,
    pub(crate) pipelines: NormalStackPipelines,
}

impl<'a, 'p, P> StreamingExecutionContext<'a, 'p, P>
where
    P: GpuNormalStackResourceProvider,
{
    pub(crate) fn new(
        renderer: &'a GpuRenderer,
        provider: &'p mut P,
        output_size: CanvasSize,
        label: &'static str,
    ) -> Self {
        let device = &renderer.context.device;
        let queue = &renderer.context.queue;
        Self {
            renderer,
            provider,
            state: StreamingEncoder::new(device, queue, label),
            output_size,
            pipelines: NormalStackPipelines::new(device),
        }
    }

    pub(crate) fn new_with_render_bounds(
        renderer: &'a GpuRenderer,
        provider: &'p mut P,
        output_size: CanvasSize,
        label: &'static str,
        render_bounds: Option<CanvasRect>,
    ) -> Self {
        let device = &renderer.context.device;
        let queue = &renderer.context.queue;
        Self {
            renderer,
            provider,
            state: StreamingEncoder::new_with_render_bounds(device, queue, label, render_bounds),
            output_size,
            pipelines: NormalStackPipelines::new(device),
        }
    }

    pub(crate) fn texture_pair(
        &self,
        label_a: &'static str,
        label_b: &'static str,
        size: CanvasSize,
        usage: wgpu::TextureUsages,
    ) -> StreamingTexturePair {
        StreamingTexturePair::new(self.state.device(), label_a, label_b, size, usage)
    }

    pub(crate) fn clear_texture_pair(
        &mut self,
        pair: &StreamingTexturePair,
        first_index: usize,
        second_index: usize,
        label: &'static str,
    ) {
        self.state
            .clear_rgba8_texture_pair(pair.view(first_index), pair.view(second_index), label);
    }

    pub(crate) fn finish(mut self) -> Result<Vec<GpuRasterResourceInfo>, P::Error> {
        self.state.flush()?;
        Ok(self.state.into_drawn_resources())
    }
}
