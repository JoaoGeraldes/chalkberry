//! Stroke model and smoothing.
//!
//! Pure Rust: this module must never depend on Windows APIs, so the logic
//! that decides what a stroke looks like can be unit-tested on any machine.

/// Points closer than this to the previous point are ignored. Dropping them
/// avoids tiny jittery segments when the mouse barely moves.
const MIN_POINT_SPACING_PX: f32 = 1.0;

/// A position in overlay pixel coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    fn midpoint(self, other: Point) -> Point {
        Point::new((self.x + other.x) / 2.0, (self.y + other.y) / 2.0)
    }

    fn distance_to(self, other: Point) -> f32 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

/// One piece of a smoothed path. Each segment continues from where the
/// previous one ended (the first one continues from the path's start).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathSegment {
    Line { to: Point },
    Quadratic { control: Point, to: Point },
}

/// What a stroke looks like once smoothed, ready to be drawn.
#[derive(Clone, Debug, PartialEq)]
pub enum StrokeShape {
    /// A click without dragging.
    Dot { center: Point },
    /// A drag: a smooth curve starting at `start`.
    Path {
        start: Point,
        segments: Vec<PathSegment>,
    },
}

/// The raw points of one pen stroke, from mouse down to mouse up.
#[derive(Clone, Debug)]
pub struct Stroke {
    points: Vec<Point>,
}

impl Stroke {
    /// Starts a new stroke at the point where the mouse was pressed.
    pub fn begin(start: Point) -> Self {
        Self {
            points: vec![start],
        }
    }

    /// Adds a mouse position, unless it is too close to the previous one.
    pub fn add_point(&mut self, point: Point) {
        let is_far_enough = self
            .points
            .last()
            .is_none_or(|last| last.distance_to(point) >= MIN_POINT_SPACING_PX);
        if is_far_enough {
            self.points.push(point);
        }
    }

    /// Returns the smoothed shape to draw for the points collected so far.
    pub fn shape(&self) -> StrokeShape {
        match self.points.as_slice() {
            [] => unreachable!("a stroke always has its start point"),
            [only_point] => StrokeShape::Dot {
                center: *only_point,
            },
            [start, ..] => StrokeShape::Path {
                start: *start,
                segments: smooth_path(&self.points),
            },
        }
    }
}

/// Turns a polyline into a smooth curve using midpoint quadratic smoothing.
///
/// Each input point (except the ends) becomes the control point of a
/// quadratic Bézier curve that runs between the midpoints of its neighbouring
/// segments. The curve never overshoots the input, and it starts and ends
/// exactly on the first and last points.
///
/// The returned segments continue from `points[0]`. Fewer than two points
/// produce no segments.
pub fn smooth_path(points: &[Point]) -> Vec<PathSegment> {
    match points {
        [] | [_] => Vec::new(),
        [_, end] => vec![PathSegment::Line { to: *end }],
        [first, second, ..] => {
            let mut segments = Vec::with_capacity(points.len());
            segments.push(PathSegment::Line {
                to: first.midpoint(*second),
            });
            for window in points.windows(2).skip(1) {
                let (control, next) = (window[0], window[1]);
                segments.push(PathSegment::Quadratic {
                    control,
                    to: control.midpoint(next),
                });
            }
            let last = points[points.len() - 1];
            segments.push(PathSegment::Line { to: last });
            segments
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment_end(segment: &PathSegment) -> Point {
        match segment {
            PathSegment::Line { to } | PathSegment::Quadratic { to, .. } => *to,
        }
    }

    #[test]
    fn single_click_creates_dot() {
        let stroke = Stroke::begin(Point::new(10.0, 20.0));
        assert_eq!(
            stroke.shape(),
            StrokeShape::Dot {
                center: Point::new(10.0, 20.0)
            }
        );
    }

    #[test]
    fn tiny_mouse_movement_still_creates_dot() {
        let mut stroke = Stroke::begin(Point::new(10.0, 20.0));
        stroke.add_point(Point::new(10.3, 20.2));
        assert!(matches!(stroke.shape(), StrokeShape::Dot { .. }));
    }

    #[test]
    fn two_points_create_straight_line() {
        let mut stroke = Stroke::begin(Point::new(0.0, 0.0));
        stroke.add_point(Point::new(10.0, 0.0));
        assert_eq!(
            stroke.shape(),
            StrokeShape::Path {
                start: Point::new(0.0, 0.0),
                segments: vec![PathSegment::Line {
                    to: Point::new(10.0, 0.0)
                }],
            }
        );
    }

    #[test]
    fn smoothing_keeps_endpoints() {
        let points = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 5.0),
            Point::new(20.0, 0.0),
            Point::new(30.0, 8.0),
        ];
        let segments = smooth_path(&points);
        let last_segment = segments.last().expect("segments for 4 points");
        assert_eq!(segment_end(last_segment), Point::new(30.0, 8.0));
    }

    #[test]
    fn smoothing_stays_within_input_bounds() {
        let points = [
            Point::new(0.0, 0.0),
            Point::new(50.0, 100.0),
            Point::new(100.0, 0.0),
            Point::new(150.0, 100.0),
        ];
        for segment in smooth_path(&points) {
            let end = segment_end(&segment);
            assert!(end.x.is_finite() && end.y.is_finite());
            assert!((0.0..=150.0).contains(&end.x));
            assert!((0.0..=100.0).contains(&end.y));
        }
    }

    #[test]
    fn smoothing_uses_inner_points_as_controls() {
        let points = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(20.0, 0.0),
        ];
        assert_eq!(
            smooth_path(&points),
            vec![
                PathSegment::Line {
                    to: Point::new(5.0, 5.0)
                },
                PathSegment::Quadratic {
                    control: Point::new(10.0, 10.0),
                    to: Point::new(15.0, 5.0)
                },
                PathSegment::Line {
                    to: Point::new(20.0, 0.0)
                },
            ]
        );
    }
}
