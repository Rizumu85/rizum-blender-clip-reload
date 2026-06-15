use clip_model::CanvasSize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CanvasRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl CanvasRect {
    pub(crate) fn full(size: CanvasSize) -> Option<Self> {
        if size.width == 0 || size.height == 0 {
            return None;
        }
        Some(Self {
            x: 0,
            y: 0,
            width: size.width,
            height: size.height,
        })
    }

    pub(crate) fn from_source(
        offset_x: i32,
        offset_y: i32,
        source_size: CanvasSize,
        canvas_size: CanvasSize,
    ) -> Option<Self> {
        let x0 = i64::from(offset_x).max(0);
        let y0 = i64::from(offset_y).max(0);
        let x1 =
            (i64::from(offset_x) + i64::from(source_size.width)).min(i64::from(canvas_size.width));
        let y1 = (i64::from(offset_y) + i64::from(source_size.height))
            .min(i64::from(canvas_size.height));
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some(Self {
            x: u32::try_from(x0).ok()?,
            y: u32::try_from(y0).ok()?,
            width: u32::try_from(x1 - x0).ok()?,
            height: u32::try_from(y1 - y0).ok()?,
        })
    }

    pub(crate) fn union(self, other: Self) -> Self {
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = self.right().max(other.right());
        let y1 = self.bottom().max(other.bottom());
        Self {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        }
    }

    fn right(self) -> u32 {
        self.x + self.width
    }

    fn bottom(self) -> u32 {
        self.y + self.height
    }
}

pub(crate) fn union_optional(
    left: Option<CanvasRect>,
    right: Option<CanvasRect>,
) -> Option<CanvasRect> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.union(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use clip_model::CanvasSize;

    use super::{CanvasRect, union_optional};

    #[test]
    fn source_rect_clips_to_canvas() {
        assert_eq!(
            CanvasRect::from_source(-2, 3, CanvasSize::new(5, 4), CanvasSize::new(10, 10)),
            Some(CanvasRect {
                x: 0,
                y: 3,
                width: 3,
                height: 4,
            })
        );
    }

    #[test]
    fn source_rect_rejects_empty_intersection() {
        assert_eq!(
            CanvasRect::from_source(10, 0, CanvasSize::new(2, 2), CanvasSize::new(10, 10)),
            None
        );
    }

    #[test]
    fn optional_union_keeps_present_side() {
        let rect = CanvasRect {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        };
        assert_eq!(union_optional(Some(rect), None), Some(rect));
        assert_eq!(union_optional(None, Some(rect)), Some(rect));
    }
}
