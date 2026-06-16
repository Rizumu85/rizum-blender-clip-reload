use clip_model::CanvasSize;

use crate::GpuNormalStackSource;
use crate::stream::GpuNormalStackResourceProvider;
use crate::stream_bounds::{CanvasRect, union_optional};
use crate::stream_effects::source_can_affect_output;
use crate::stream_resources::known_raster_source_bounds;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KnownStackBounds {
    Empty,
    Bounded(CanvasRect),
    Unknown,
}

pub(crate) fn known_stack_bounds<P>(
    provider: &P,
    sources: &[GpuNormalStackSource],
    output_size: CanvasSize,
) -> KnownStackBounds
where
    P: GpuNormalStackResourceProvider,
{
    let mut bounds = None;
    let mut saw_unknown = false;
    for source in sources {
        match known_source_change_bounds(provider, source, output_size, bounds, saw_unknown) {
            KnownStackBounds::Bounded(source_bounds) => {
                bounds = Some(source_bounds);
                saw_unknown = false;
            }
            KnownStackBounds::Unknown => saw_unknown = true,
            KnownStackBounds::Empty => {}
        }
    }

    match (bounds, saw_unknown) {
        (Some(bounds), false) => KnownStackBounds::Bounded(bounds),
        (Some(_), true) | (None, true) => KnownStackBounds::Unknown,
        (None, false) => KnownStackBounds::Empty,
    }
}

fn known_source_change_bounds<P>(
    provider: &P,
    source: &GpuNormalStackSource,
    output_size: CanvasSize,
    current_bounds: Option<CanvasRect>,
    current_unknown: bool,
) -> KnownStackBounds
where
    P: GpuNormalStackResourceProvider,
{
    if !source_can_affect_output(source) {
        return KnownStackBounds::Empty;
    }

    match source {
        GpuNormalStackSource::Raster(raster) => merge_source_bounds(
            known_raster_bounds(provider, *raster, output_size),
            current_bounds,
            current_unknown,
        ),
        GpuNormalStackSource::ClippingRun { base, .. } => merge_source_bounds(
            known_raster_bounds(provider, *base, output_size),
            current_bounds,
            current_unknown,
        ),
        GpuNormalStackSource::Container { children, .. } => merge_source_bounds(
            known_stack_bounds(provider, children, output_size),
            current_bounds,
            current_unknown,
        ),
        GpuNormalStackSource::ThroughGroup { children, .. } => merge_source_bounds(
            known_stack_bounds(provider, children, output_size),
            current_bounds,
            current_unknown,
        ),
        GpuNormalStackSource::SolidColor { .. } => CanvasRect::full(output_size)
            .map(KnownStackBounds::Bounded)
            .unwrap_or(KnownStackBounds::Empty),
        GpuNormalStackSource::LutFilter { .. } => {
            if current_unknown {
                KnownStackBounds::Unknown
            } else {
                match current_bounds {
                    Some(bounds) => KnownStackBounds::Bounded(bounds),
                    None => CanvasRect::full(output_size)
                        .map(KnownStackBounds::Bounded)
                        .unwrap_or(KnownStackBounds::Empty),
                }
            }
        }
    }
}

fn merge_source_bounds(
    source_bounds: KnownStackBounds,
    current_bounds: Option<CanvasRect>,
    current_unknown: bool,
) -> KnownStackBounds {
    match source_bounds {
        KnownStackBounds::Bounded(_) if current_unknown => KnownStackBounds::Unknown,
        KnownStackBounds::Bounded(bounds) => KnownStackBounds::Bounded(
            union_optional(current_bounds, Some(bounds)).expect("source bounds must be present"),
        ),
        KnownStackBounds::Empty => KnownStackBounds::Empty,
        KnownStackBounds::Unknown => KnownStackBounds::Unknown,
    }
}

fn known_raster_bounds<P>(
    provider: &P,
    source: crate::GpuNormalRasterSource,
    output_size: CanvasSize,
) -> KnownStackBounds
where
    P: GpuNormalStackResourceProvider,
{
    match known_raster_source_bounds(provider, source, output_size) {
        Some(Some(bounds)) => KnownStackBounds::Bounded(bounds),
        Some(None) => KnownStackBounds::Empty,
        None => KnownStackBounds::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use clip_model::{CanvasSize, LayerId, Rgba8};

    use super::{KnownStackBounds, known_stack_bounds};
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
            unreachable!("bounds checks must not request raster resources")
        }

        fn raster_resource_size(&self, source: GpuNormalRasterSource) -> Option<CanvasSize> {
            self.sizes.get(&source.key).copied()
        }

        fn mask_resource(
            &mut self,
            _renderer: &GpuRenderer,
            _key: GpuMaskResourceKey,
        ) -> Result<GpuMaskResourceCache, Self::Error> {
            unreachable!("bounds checks must not request mask resources")
        }
    }

    #[test]
    fn stack_bounds_union_known_raster_children() {
        let left = raster_key(1);
        let right = raster_key(2);
        let provider = SizeProvider::new(&[
            (left, CanvasSize::new(2, 2)),
            (right, CanvasSize::new(3, 1)),
        ]);
        let sources = vec![
            GpuNormalStackSource::Raster(raster_source(left, 1, 1)),
            GpuNormalStackSource::Raster(raster_source(right, 5, 2)),
        ];

        assert_eq!(
            known_stack_bounds(&provider, &sources, CanvasSize::new(10, 10)),
            KnownStackBounds::Bounded(CanvasRect {
                x: 1,
                y: 1,
                width: 7,
                height: 2,
            })
        );
    }

    #[test]
    fn nonempty_through_group_uses_child_bounds() {
        let key = raster_key(1);
        let provider = SizeProvider::new(&[(key, CanvasSize::new(2, 2))]);
        let sources = vec![GpuNormalStackSource::ThroughGroup {
            children: vec![GpuNormalStackSource::Raster(raster_source(key, 1, 1))],
            opacity: 1.0,
            mask_key: None,
        }];

        assert_eq!(
            known_stack_bounds(&provider, &sources, CanvasSize::new(10, 10)),
            KnownStackBounds::Bounded(CanvasRect {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            })
        );
    }

    #[test]
    fn solid_source_forces_full_bounds() {
        let provider = SizeProvider::new(&[]);
        let sources = vec![GpuNormalStackSource::SolidColor {
            color: Rgba8 {
                r: 1,
                g: 2,
                b: 3,
                a: 255,
            },
            opacity: 1.0,
        }];

        assert_eq!(
            known_stack_bounds(&provider, &sources, CanvasSize::new(4, 5)),
            KnownStackBounds::Bounded(CanvasRect {
                x: 0,
                y: 0,
                width: 4,
                height: 5,
            })
        );
    }

    #[test]
    fn unmasked_lut_filter_keeps_prior_bounded_stack_bounds() {
        let key = raster_key(1);
        let provider = SizeProvider::new(&[(key, CanvasSize::new(2, 2))]);
        let sources = vec![
            GpuNormalStackSource::Raster(raster_source(key, 1, 1)),
            GpuNormalStackSource::LutFilter {
                lut_rgba: Vec::new(),
                opacity: 1.0,
                mask_key: None,
                filter_mode: crate::GpuLutFilterMode::ToneCurveRgb,
            },
        ];

        assert_eq!(
            known_stack_bounds(&provider, &sources, CanvasSize::new(10, 10)),
            KnownStackBounds::Bounded(CanvasRect {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            })
        );
    }

    #[test]
    fn masked_lut_filter_keeps_prior_bounded_stack_bounds() {
        let key = raster_key(1);
        let provider = SizeProvider::new(&[(key, CanvasSize::new(2, 2))]);
        let sources = vec![
            GpuNormalStackSource::Raster(raster_source(key, 1, 1)),
            GpuNormalStackSource::LutFilter {
                lut_rgba: Vec::new(),
                opacity: 1.0,
                mask_key: Some(GpuMaskResourceKey {
                    layer_id: LayerId(9),
                    mask_mipmap_id: 10,
                }),
                filter_mode: crate::GpuLutFilterMode::ToneCurveRgb,
            },
        ];

        assert_eq!(
            known_stack_bounds(&provider, &sources, CanvasSize::new(10, 10)),
            KnownStackBounds::Bounded(CanvasRect {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            })
        );
    }

    #[test]
    fn leading_masked_lut_filter_keeps_full_canvas_bounds() {
        let provider = SizeProvider::new(&[]);
        let sources = vec![GpuNormalStackSource::LutFilter {
            lut_rgba: Vec::new(),
            opacity: 1.0,
            mask_key: Some(GpuMaskResourceKey {
                layer_id: LayerId(9),
                mask_mipmap_id: 10,
            }),
            filter_mode: crate::GpuLutFilterMode::ToneCurveRgb,
        }];

        assert_eq!(
            known_stack_bounds(&provider, &sources, CanvasSize::new(10, 10)),
            KnownStackBounds::Bounded(CanvasRect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            })
        );
    }

    #[test]
    fn zero_opacity_raster_has_empty_stack_bounds() {
        let key = raster_key(1);
        let provider = SizeProvider::new(&[(key, CanvasSize::new(2, 2))]);
        let sources = vec![GpuNormalStackSource::Raster(GpuNormalRasterSource {
            opacity: 0.0,
            ..raster_source(key, 1, 1)
        })];

        assert_eq!(
            known_stack_bounds(&provider, &sources, CanvasSize::new(10, 10)),
            KnownStackBounds::Empty
        );
    }

    #[test]
    fn zero_opacity_container_has_empty_stack_bounds() {
        let key = raster_key(1);
        let provider = SizeProvider::new(&[(key, CanvasSize::new(2, 2))]);
        let sources = vec![GpuNormalStackSource::Container {
            children: vec![GpuNormalStackSource::Raster(raster_source(key, 1, 1))],
            opacity: 0.0,
            mask_key: None,
            blend_mode: GpuRasterBlendMode::Normal,
        }];

        assert_eq!(
            known_stack_bounds(&provider, &sources, CanvasSize::new(10, 10)),
            KnownStackBounds::Empty
        );
    }

    #[test]
    fn transparent_solid_has_empty_stack_bounds() {
        let provider = SizeProvider::new(&[]);
        let sources = vec![GpuNormalStackSource::SolidColor {
            color: Rgba8 {
                r: 1,
                g: 2,
                b: 3,
                a: 0,
            },
            opacity: 1.0,
        }];

        assert_eq!(
            known_stack_bounds(&provider, &sources, CanvasSize::new(10, 10)),
            KnownStackBounds::Empty
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
