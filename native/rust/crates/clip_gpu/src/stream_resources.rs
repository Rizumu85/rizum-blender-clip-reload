use clip_model::CanvasSize;

use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::{CanvasRect, union_optional};
use crate::stream_state::StreamingEncoder;
use crate::{
    GpuMaskResourceCache, GpuMaskResourceInfo, GpuMaskResourceKey, GpuNormalRasterSource,
    GpuRasterResourceCache, GpuRenderError, GpuRenderer,
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
        Option<CanvasRect>,
    ),
    P::Error,
>
where
    P: GpuNormalStackResourceProvider,
{
    let cache = provider.raster_resource(renderer, source)?;
    let resource = cache
        .resource(source.key)
        .ok_or(GpuRenderError::MissingRasterResource {
            layer_id: source.key.layer_id,
            render_mipmap_id: source.key.render_mipmap_id,
        })
        .map_err(P::Error::from)?;
    let info = resource.info();
    state.push_drawn_resource(info);
    let bounds = CanvasRect::from_source(source.offset_x, source.offset_y, info.size, output_size);
    let view = resource
        .texture()
        .create_view(&wgpu::TextureViewDescriptor::default());
    Ok((cache, view, bounds))
}

pub(crate) fn pass_bounds_for_change(
    dirty_bounds: Option<CanvasRect>,
    change_bounds: Option<CanvasRect>,
) -> Option<CanvasRect> {
    change_bounds.map(|change_bounds| {
        union_optional(dirty_bounds, Some(change_bounds)).expect("change bounds must be present")
    })
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

pub(crate) fn mask_view_with_provider<'a, P>(
    renderer: &GpuRenderer,
    provider: &mut P,
    output_size: CanvasSize,
    mask_key: Option<GpuMaskResourceKey>,
    owner_layer_id: clip_model::LayerId,
    fallback_view: &'a wgpu::TextureView,
) -> Result<(Option<GpuMaskResourceCache>, MaskTextureView<'a>), P::Error>
where
    P: GpuNormalStackResourceProvider,
{
    let Some(mask_key) = mask_key else {
        return Ok((None, MaskTextureView::Borrowed(fallback_view)));
    };
    let cache = provider.mask_resource(renderer, mask_key)?;
    let resource = cache
        .resource(mask_key)
        .ok_or(GpuRenderError::MissingMaskResource {
            layer_id: mask_key.layer_id,
            mask_mipmap_id: mask_key.mask_mipmap_id,
        })
        .map_err(P::Error::from)?;
    let info: GpuMaskResourceInfo = resource.info();
    if info.size != output_size {
        return Err(P::Error::from(GpuRenderError::MaskResourceSizeMismatch {
            layer_id: owner_layer_id,
            expected: output_size,
            actual: info.size,
        }));
    }
    let view = resource
        .texture()
        .create_view(&wgpu::TextureViewDescriptor::default());
    Ok((Some(cache), MaskTextureView::Owned(view)))
}

pub(crate) enum MaskTextureView<'a> {
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
