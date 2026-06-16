use clip_model::CanvasSize;

use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::{CanvasRect, union_optional};
use crate::stream_effects::{raster_can_affect_output, source_can_affect_output};
use crate::stream_state::StreamingEncoder;
use crate::{
    GpuMaskResourceCache, GpuMaskResourceInfo, GpuMaskResourceKey, GpuMaskSamplingInfo,
    GpuNormalRasterSource, GpuNormalStackSource, GpuRasterResourceCache, GpuRenderError,
    GpuRenderer,
};

pub(crate) fn raster_view_with_provider<P>(
    renderer: &GpuRenderer,
    provider: &mut P,
    state: &mut StreamingEncoder<'_, P::Error>,
    output_size: CanvasSize,
    source: GpuNormalRasterSource,
) -> Result<
    (
        GpuRasterResourceCache,
        wgpu::TextureView,
        GpuNormalRasterSource,
        Option<CanvasRect>,
    ),
    P::Error,
>
where
    P: GpuNormalStackResourceProvider,
{
    let cache = state
        .retained_raster_cache(source.key)
        .map(Ok)
        .unwrap_or_else(|| provider.raster_resource(renderer, source))?;
    let resource = cache
        .resource(source.key)
        .ok_or(GpuRenderError::MissingRasterResource {
            layer_id: source.key.layer_id,
            render_mipmap_id: source.key.render_mipmap_id,
        })
        .map_err(P::Error::from)?;
    let info = resource.info();
    state.push_drawn_resource(info);
    let (offset_x, offset_y) = provider
        .raster_resource_offset(source)
        .unwrap_or((source.offset_x, source.offset_y));
    let effective_source = GpuNormalRasterSource {
        offset_x,
        offset_y,
        ..source
    };
    let bounds = CanvasRect::from_source(offset_x, offset_y, info.size, output_size);
    let view = resource
        .texture()
        .create_view(&wgpu::TextureViewDescriptor::default());
    Ok((cache, view, effective_source, bounds))
}

pub(crate) fn pass_bounds_for_change(
    dirty_bounds: Option<CanvasRect>,
    change_bounds: Option<CanvasRect>,
) -> Option<CanvasRect> {
    change_bounds.map(|change_bounds| {
        union_optional(dirty_bounds, Some(change_bounds)).expect("change bounds must be present")
    })
}

pub(crate) fn preserving_pass_bounds_for_change(
    dirty_bounds: Option<CanvasRect>,
    change_bounds: Option<CanvasRect>,
) -> Option<CanvasRect> {
    let dirty_bounds = dirty_bounds?;
    let change_bounds = change_bounds?;
    dirty_bounds
        .intersects(change_bounds)
        .then_some(dirty_bounds)
}

pub(crate) fn known_raster_source_bounds<P>(
    provider: &P,
    source: GpuNormalRasterSource,
    output_size: CanvasSize,
) -> Option<Option<CanvasRect>>
where
    P: GpuNormalStackResourceProvider,
{
    provider
        .raster_resource_size(source)
        .map(|size| CanvasRect::from_source(source.offset_x, source.offset_y, size, output_size))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KnownSourceActivity {
    Empty,
    Writes,
    Unknown,
}

impl KnownSourceActivity {
    pub(crate) fn is_empty(self) -> bool {
        self == Self::Empty
    }
}

pub(crate) fn known_stack_activity<P>(
    provider: &P,
    sources: &[GpuNormalStackSource],
    output_size: CanvasSize,
) -> KnownSourceActivity
where
    P: GpuNormalStackResourceProvider,
{
    let mut activity = KnownSourceActivity::Empty;
    for source in sources {
        match known_source_activity(provider, source, output_size) {
            KnownSourceActivity::Writes => return KnownSourceActivity::Writes,
            KnownSourceActivity::Unknown => activity = KnownSourceActivity::Unknown,
            KnownSourceActivity::Empty => {}
        }
    }
    activity
}

pub(crate) fn known_clipping_run_activity<P>(
    provider: &P,
    base: GpuNormalRasterSource,
    output_size: CanvasSize,
) -> KnownSourceActivity
where
    P: GpuNormalStackResourceProvider,
{
    known_raster_activity(provider, base, output_size)
}

pub(crate) fn known_clipped_sibling_activity<P>(
    provider: &P,
    base: GpuNormalRasterSource,
    clipped: &[GpuNormalRasterSource],
    output_size: CanvasSize,
) -> KnownSourceActivity
where
    P: GpuNormalStackResourceProvider,
{
    let base_bounds = match known_raster_source_bounds(provider, base, output_size) {
        Some(Some(bounds)) => Some(bounds),
        Some(None) => return KnownSourceActivity::Empty,
        None => None,
    };
    let mut activity = KnownSourceActivity::Empty;
    for clipped_source in clipped {
        if !raster_can_affect_output(*clipped_source) {
            continue;
        }
        match known_raster_source_bounds(provider, *clipped_source, output_size) {
            Some(Some(bounds)) => {
                let Some(base_bounds) = base_bounds else {
                    activity = KnownSourceActivity::Unknown;
                    continue;
                };
                if base_bounds.intersects(bounds) {
                    return KnownSourceActivity::Writes;
                }
            }
            Some(None) => {}
            None => activity = KnownSourceActivity::Unknown,
        }
    }
    activity
}

fn known_source_activity<P>(
    provider: &P,
    source: &GpuNormalStackSource,
    output_size: CanvasSize,
) -> KnownSourceActivity
where
    P: GpuNormalStackResourceProvider,
{
    if !source_can_affect_output(source) {
        return KnownSourceActivity::Empty;
    }

    match source {
        GpuNormalStackSource::Raster(raster) => {
            known_raster_activity(provider, *raster, output_size)
        }
        GpuNormalStackSource::ClippingRun { base, .. } => {
            known_clipping_run_activity(provider, *base, output_size)
        }
        GpuNormalStackSource::ContainerClippingRun { children, .. }
        | GpuNormalStackSource::Container { children, .. }
        | GpuNormalStackSource::ThroughGroup { children, .. } => {
            known_stack_activity(provider, children, output_size)
        }
        GpuNormalStackSource::SolidColor { .. } | GpuNormalStackSource::LutFilter { .. } => {
            if CanvasRect::full(output_size).is_some() {
                KnownSourceActivity::Writes
            } else {
                KnownSourceActivity::Empty
            }
        }
    }
}

fn known_raster_activity<P>(
    provider: &P,
    source: GpuNormalRasterSource,
    output_size: CanvasSize,
) -> KnownSourceActivity
where
    P: GpuNormalStackResourceProvider,
{
    match known_raster_source_bounds(provider, source, output_size) {
        Some(Some(_)) => KnownSourceActivity::Writes,
        Some(None) => KnownSourceActivity::Empty,
        None => KnownSourceActivity::Unknown,
    }
}

pub(crate) fn mask_view_with_provider<'a, P>(
    renderer: &GpuRenderer,
    provider: &mut P,
    state: &mut StreamingEncoder<'_, P::Error>,
    output_size: CanvasSize,
    mask_key: Option<GpuMaskResourceKey>,
    _owner_layer_id: clip_model::LayerId,
    fallback_view: &'a wgpu::TextureView,
) -> Result<(Option<GpuMaskResourceCache>, MaskTextureBinding<'a>), P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let _ = output_size;
    let Some(mask_key) = mask_key else {
        return Ok((None, MaskTextureBinding::fallback(fallback_view)));
    };
    let cache = state
        .retained_mask_cache(mask_key)
        .map(Ok)
        .unwrap_or_else(|| provider.mask_resource(renderer, mask_key))?;
    let resource = cache
        .resource(mask_key)
        .ok_or(GpuRenderError::MissingMaskResource {
            layer_id: mask_key.layer_id,
            mask_mipmap_id: mask_key.mask_mipmap_id,
        })
        .map_err(P::Error::from)?;
    let info: GpuMaskResourceInfo = resource.info();
    let view = resource
        .texture()
        .create_view(&wgpu::TextureViewDescriptor::default());
    Ok((
        Some(cache),
        MaskTextureBinding {
            view: MaskTextureView::Owned(view),
            sampling: info.sampling_info(),
        },
    ))
}

pub(crate) struct MaskTextureBinding<'a> {
    view: MaskTextureView<'a>,
    sampling: GpuMaskSamplingInfo,
}

impl<'a> MaskTextureBinding<'a> {
    fn fallback(view: &'a wgpu::TextureView) -> Self {
        Self {
            view: MaskTextureView::Borrowed(view),
            sampling: GpuMaskSamplingInfo::default(),
        }
    }

    pub(crate) fn view(&self) -> &wgpu::TextureView {
        self.view.as_ref()
    }

    pub(crate) fn sampling(&self) -> GpuMaskSamplingInfo {
        self.sampling
    }
}

enum MaskTextureView<'a> {
    Borrowed(&'a wgpu::TextureView),
    Owned(wgpu::TextureView),
}

impl MaskTextureView<'_> {
    pub(crate) fn as_ref(&self) -> &wgpu::TextureView {
        match self {
            Self::Borrowed(view) => view,
            Self::Owned(view) => view,
        }
    }
}
