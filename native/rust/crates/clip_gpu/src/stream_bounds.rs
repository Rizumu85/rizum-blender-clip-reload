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

    pub(crate) fn intersection(self, other: Self) -> Option<Self> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some(Self {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        })
    }

    pub(crate) fn intersects(self, other: Self) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    pub(crate) fn origin_i32(self) -> (i32, i32) {
        (
            i32::try_from(self.x).expect("canvas x origin must fit shader i32"),
            i32::try_from(self.y).expect("canvas y origin must fit shader i32"),
        )
    }

    pub(crate) fn translate_to_local(self, origin: (i32, i32)) -> Option<Self> {
        let local_x = i64::from(self.x) - i64::from(origin.0);
        let local_y = i64::from(self.y) - i64::from(origin.1);
        if local_x < 0 || local_y < 0 {
            return None;
        }
        Some(Self {
            x: u32::try_from(local_x).ok()?,
            y: u32::try_from(local_y).ok()?,
            width: self.width,
            height: self.height,
        })
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

pub(crate) fn target_canvas_bounds(
    target_origin: (i32, i32),
    target_size: CanvasSize,
) -> Option<CanvasRect> {
    if target_size.width == 0 || target_size.height == 0 {
        return None;
    }
    let x = u32::try_from(target_origin.0).ok()?;
    let y = u32::try_from(target_origin.1).ok()?;
    x.checked_add(target_size.width)?;
    y.checked_add(target_size.height)?;
    Some(CanvasRect {
        x,
        y,
        width: target_size.width,
        height: target_size.height,
    })
}

#[cfg(test)]
mod tests {
    use clip_model::CanvasSize;

    use super::{CanvasRect, target_canvas_bounds, union_optional};

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

    #[test]
    fn target_bounds_use_target_origin() {
        assert_eq!(
            target_canvas_bounds((4, 5), CanvasSize::new(6, 7)),
            Some(CanvasRect {
                x: 4,
                y: 5,
                width: 6,
                height: 7,
            })
        );
        assert_eq!(target_canvas_bounds((-1, 0), CanvasSize::new(6, 7)), None);
        assert_eq!(target_canvas_bounds((0, 0), CanvasSize::new(0, 7)), None);
    }

    #[test]
    fn rect_intersection_rejects_touching_edges() {
        let rect = CanvasRect {
            x: 1,
            y: 1,
            width: 4,
            height: 4,
        };
        assert!(rect.intersects(CanvasRect {
            x: 4,
            y: 4,
            width: 2,
            height: 2,
        }));
        assert!(!rect.intersects(CanvasRect {
            x: 5,
            y: 1,
            width: 2,
            height: 2,
        }));
    }

    #[test]
    fn rect_intersection_returns_overlap() {
        let rect = CanvasRect {
            x: 1,
            y: 2,
            width: 5,
            height: 6,
        };

        assert_eq!(
            rect.intersection(CanvasRect {
                x: 4,
                y: 1,
                width: 5,
                height: 3,
            }),
            Some(CanvasRect {
                x: 4,
                y: 2,
                width: 2,
                height: 2,
            })
        );
    }

    #[test]
    fn rect_translates_to_local_origin() {
        assert_eq!(
            CanvasRect {
                x: 5,
                y: 7,
                width: 3,
                height: 2,
            }
            .translate_to_local((4, 6)),
            Some(CanvasRect {
                x: 1,
                y: 1,
                width: 3,
                height: 2,
            })
        );
    }

    #[test]
    fn rect_rejects_translation_before_local_origin() {
        assert_eq!(
            CanvasRect {
                x: 3,
                y: 7,
                width: 3,
                height: 2,
            }
            .translate_to_local((4, 6)),
            None
        );
    }
}
