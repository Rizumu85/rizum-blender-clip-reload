use clip_model::CanvasSize;

use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::{CanvasRect, union_optional};
use crate::stream_state::StreamingEncoder;
use crate::{
    GpuMaskResourceCache, GpuMaskResourceInfo, GpuMaskResourceKey, GpuNormalRasterSource,
    GpuNormalStackSource, GpuRasterResourceCache, GpuRenderError, GpuRenderer,
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

fn known_source_activity<P>(
    provider: &P,
    source: &GpuNormalStackSource,
    output_size: CanvasSize,
) -> KnownSourceActivity
where
    P: GpuNormalStackResourceProvider,
{
    match source {
        GpuNormalStackSource::Raster(raster) => {
            known_raster_activity(provider, *raster, output_size)
        }
        GpuNormalStackSource::ClippingRun { base, .. } => {
            known_clipping_run_activity(provider, *base, output_size)
        }
        GpuNormalStackSource::Container { children, .. }
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use clip_model::{CanvasSize, LayerId, Rgba8};

    use super::{
        KnownSourceActivity, known_clipping_run_activity, known_stack_activity,
        preserving_pass_bounds_for_change,
    };
    use crate::stream_bounds::CanvasRect;
    use crate::{
        GpuMaskResourceCache, GpuMaskResourceKey, GpuNormalRasterSource, GpuNormalStackSource,
        GpuRasterBlendMode, GpuRasterResourceCache, GpuRasterResourceKey, GpuRenderError,
        GpuRenderer,
    };

    struct SizeProvider {
        sizes: HashMap<GpuRasterResourceKey, CanvasSize>,
    }

    impl SizeProvider {
        fn new(sizes: &[(GpuRasterResourceKey, CanvasSize)]) -> Self {
            Self {
                sizes: sizes.iter().copied().collect(),
            }
        }
    }

    impl crate::stream::GpuNormalStackResourceProvider for SizeProvider {
        type Error = GpuRenderError;

        fn raster_resource(
            &mut self,
            _renderer: &GpuRenderer,
            _source: GpuNormalRasterSource,
        ) -> Result<GpuRasterResourceCache, Self::Error> {
            unreachable!("activity checks must not request raster resources")
        }

        fn raster_resource_size(&self, source: GpuNormalRasterSource) -> Option<CanvasSize> {
            self.sizes.get(&source.key).copied()
        }

        fn mask_resource(
            &mut self,
            _renderer: &GpuRenderer,
            _key: GpuMaskResourceKey,
        ) -> Result<GpuMaskResourceCache, Self::Error> {
            unreachable!("activity checks must not request mask resources")
        }
    }

    #[test]
    fn off_canvas_container_stack_is_known_empty() {
        let key = raster_key(1);
        let provider = SizeProvider::new(&[(key, CanvasSize::new(8, 8))]);
        let sources = vec![GpuNormalStackSource::Container {
            children: vec![GpuNormalStackSource::Raster(raster_source(key, 40, 40))],
            opacity: 1.0,
            mask_key: None,
            blend_mode: GpuRasterBlendMode::Normal,
        }];

        assert_eq!(
            known_stack_activity(&provider, &sources, CanvasSize::new(32, 32)),
            KnownSourceActivity::Empty
        );
    }

    #[test]
    fn unknown_raster_size_keeps_stack_activity_unknown() {
        let provider = SizeProvider::new(&[]);
        let sources = vec![GpuNormalStackSource::Raster(raster_source(
            raster_key(1),
            40,
            40,
        ))];

        assert_eq!(
            known_stack_activity(&provider, &sources, CanvasSize::new(32, 32)),
            KnownSourceActivity::Unknown
        );
    }

    #[test]
    fn visible_solid_marks_stack_as_writing() {
        let provider = SizeProvider::new(&[]);
        let sources = vec![GpuNormalStackSource::SolidColor {
            color: Rgba8 {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            opacity: 1.0,
        }];

        assert_eq!(
            known_stack_activity(&provider, &sources, CanvasSize::new(32, 32)),
            KnownSourceActivity::Writes
        );
    }

    #[test]
    fn off_canvas_clipping_base_is_known_empty() {
        let key = raster_key(1);
        let provider = SizeProvider::new(&[(key, CanvasSize::new(8, 8))]);

        assert_eq!(
            known_clipping_run_activity(
                &provider,
                raster_source(key, -16, -16),
                CanvasSize::new(8, 8),
            ),
            KnownSourceActivity::Empty
        );
    }

    #[test]
    fn preserving_pass_bounds_do_not_expand_dirty_area() {
        let dirty = CanvasRect {
            x: 10,
            y: 10,
            width: 12,
            height: 12,
        };
        let larger_source = CanvasRect {
            x: 0,
            y: 0,
            width: 40,
            height: 40,
        };

        assert_eq!(
            preserving_pass_bounds_for_change(Some(dirty), Some(larger_source)),
            Some(dirty)
        );
    }

    #[test]
    fn preserving_pass_bounds_skip_non_overlapping_source() {
        let dirty = CanvasRect {
            x: 10,
            y: 10,
            width: 12,
            height: 12,
        };
        let outside_source = CanvasRect {
            x: 30,
            y: 30,
            width: 4,
            height: 4,
        };

        assert_eq!(
            preserving_pass_bounds_for_change(Some(dirty), Some(outside_source)),
            None
        );
    }

    fn raster_source(
        key: GpuRasterResourceKey,
        offset_x: i32,
        offset_y: i32,
    ) -> GpuNormalRasterSource {
        GpuNormalRasterSource {
            key,
            opacity: 1.0,
            mask_key: None,
            offset_x,
            offset_y,
            blend_mode: GpuRasterBlendMode::Normal,
        }
    }

    fn raster_key(id: u32) -> GpuRasterResourceKey {
        GpuRasterResourceKey {
            layer_id: LayerId(id),
            render_mipmap_id: id,
        }
    }
}
