use clip_model::{CanvasSize, Rect};

use crate::stream::GpuNormalStackResourceProvider;
use crate::{GpuMaskAtlasSource, GpuMaskResourceKey};

pub(crate) struct MaskAtlasPlan {
    width: u32,
    next_y: u32,
    max_size: u32,
    sources: Vec<GpuMaskAtlasSource>,
}

impl MaskAtlasPlan {
    pub(crate) fn new(base_size: CanvasSize, max_size: u32) -> Self {
        Self {
            width: base_size.width,
            next_y: base_size.height,
            max_size,
            sources: Vec::new(),
        }
    }

    pub(crate) fn size(&self) -> CanvasSize {
        CanvasSize::new(self.width.max(1), self.next_y.max(1))
    }

    pub(crate) fn into_sources(self) -> Vec<GpuMaskAtlasSource> {
        self.sources
    }

    pub(crate) fn append_mask<P>(
        &mut self,
        provider: &P,
        mask_key: Option<GpuMaskResourceKey>,
        bounds: Rect,
    ) -> Option<Option<(u32, u32)>>
    where
        P: GpuNormalStackResourceProvider,
    {
        let Some(key) = mask_key else {
            return Some(None);
        };
        if provider.mask_is_fully_opaque(key) == Some(true) {
            return Some(None);
        }
        if !provider.mask_atlas_tiles_supported() {
            return None;
        }
        let next_y = self.next_y;
        let bottom = next_y.checked_add(bounds.height)?;
        let width = self.width.max(bounds.width);
        if width > self.max_size || bottom > self.max_size {
            return None;
        }
        self.width = width;
        self.next_y = bottom;
        self.sources.push(GpuMaskAtlasSource {
            key,
            atlas_x: 0,
            atlas_y: next_y,
            canvas_bounds: bounds,
        });
        Some(Some((0, next_y)))
    }
}

pub(crate) fn mask_can_lower_as_atlas<P>(provider: &P, mask_key: Option<GpuMaskResourceKey>) -> bool
where
    P: GpuNormalStackResourceProvider,
{
    match mask_key {
        Some(key) => {
            provider.mask_is_fully_opaque(key) == Some(true)
                || provider.mask_atlas_tiles_supported()
        }
        None => true,
    }
}
