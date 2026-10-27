//! Stroke model, smoothing, and eraser hit testing.
//!
//! Pure Rust: this module must never depend on Windows APIs, so the logic
//! that decides what a stroke looks like can be unit-tested on any machine.

use crate::config::PenSettings;

/// Points closer than this to the previous point are ignored. Dropping them
/// avoids tiny jittery segments: mouse positions are whole pixels, so very
/// close points turn a diagonal line into a wobbly staircase.
const MIN_POINT_SPACING_PX: f32 = 2.5;

/// How far the pen tip moves toward the cursor on each mouse move, from 0.0
/// (never) to 1.0 (jumps straight there). Below 1.0 the tip trails the
/// cursor slightly, which irons out hand tremor and pixel steps.
const STABILIZER_FOLLOW: f32 = 0.5;

/// The eraser also takes strokes this close to the cursor, so thin lines
/// do not need pixel-perfect aim.
const ERASER_REACH_PX: f32 = 4.0;

/// Curves are checked against this many straight pieces. Plenty for the
/// short curves between neighbouring mouse points.
const CURVE_PIECES_FOR_HIT_TEST: u16 = 8;

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

    fn moved_toward(self, target: Point, fraction: f32) -> Point {
        Point::new(
            self.x + (target.x - self.x) * fraction,
            self.y + (target.y - self.y) * fraction,
        )
    }

    fn distance_to(self, other: Point) -> f32 {
        (self.x - other.x).hypot(self.y - other.y)
    }

    /// Distance from this point to the straight line piece `start`..`end`.
    fn distance_to_segment(self, start: Point, end: Point) -> f32 {
        let (along_x, along_y) = (end.x - start.x, end.y - start.y);
        let length_squared = along_x * along_x + along_y * along_y;
        if length_squared == 0.0 {
            return self.distance_to(start);
        }
        let projection =
            ((self.x - start.x) * along_x + (self.y - start.y) * along_y) / length_squared;
        let closest = start.moved_toward(end, projection.clamp(0.0, 1.0));
        self.distance_to(closest)
    }
}

/// The point at `t` (0.0..=1.0) along a quadratic Bézier curve.
fn quadratic_point(start: Point, control: Point, end: Point, t: f32) -> Point {
    let near = start.moved_toward(control, t);
    let far = control.moved_toward(end, t);
    near.moved_toward(far, t)
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

impl StrokeShape {
    /// The shortest distance from `point` to the stroke's centre line.
    pub fn distance_to(&self, point: Point) -> f32 {
        match self {
            StrokeShape::Dot { center } => point.distance_to(*center),
            StrokeShape::Path { start, segments } => {
                let mut previous_end = *start;
                let mut closest = f32::INFINITY;
                for segment in segments {
                    let distance = distance_to_path_segment(point, previous_end, segment);
                    closest = closest.min(distance);
                    previous_end = segment_end(segment);
                }
                closest
            }
        }
    }
}

fn distance_to_path_segment(point: Point, from: Point, segment: &PathSegment) -> f32 {
    match *segment {
        PathSegment::Line { to } => point.distance_to_segment(from, to),
        PathSegment::Quadratic { control, to } => {
            let mut piece_start = from;
            let mut closest = f32::INFINITY;
            for piece in 1..=CURVE_PIECES_FOR_HIT_TEST {
                let t = f32::from(piece) / f32::from(CURVE_PIECES_FOR_HIT_TEST);
                let piece_end = quadratic_point(from, control, to, t);
                closest = closest.min(point.distance_to_segment(piece_start, piece_end));
                piece_start = piece_end;
            }
            closest
        }
    }
}

fn segment_end(segment: &PathSegment) -> Point {
    match segment {
        PathSegment::Line { to } | PathSegment::Quadratic { to, .. } => *to,
    }
}

/// A finished stroke as it is on screen: its shape and the pen it was
/// drawn with (needed to redraw it after another stroke is erased).
#[derive(Clone, Debug, PartialEq)]
pub struct DrawnStroke {
    pub shape: StrokeShape,
    pub pen: PenSettings,
}

impl DrawnStroke {
    /// True if the eraser at `point` touches this stroke's painted area.
    pub fn is_touched_by(&self, point: Point) -> bool {
        let reach = self.pen.width_px / 2.0 + ERASER_REACH_PX;
        self.shape.distance_to(point) <= reach
    }
}

/// The index of the most recently drawn stroke that the eraser at `point`
/// touches, so overlapping strokes are erased from the top down.
pub fn topmost_stroke_at(strokes: &[DrawnStroke], point: Point) -> Option<usize> {
    strokes
        .iter()
        .rposition(|stroke| stroke.is_touched_by(point))
}

/// The raw points of one pen stroke, from mouse down to mouse up.
#[derive(Clone, Debug)]
pub struct Stroke {
    points: Vec<Point>,
    /// The stabilized pen tip. It follows the cursor smoothly and is moved
    /// on every mouse move, even when no point is recorded.
    pen_tip: Point,
}

impl Stroke {
    /// Starts a new stroke at the point where the mouse was pressed.
    pub fn begin(start: Point) -> Self {
        Self {
            points: vec![start],
            pen_tip: start,
        }
    }

    /// Moves the pen toward a new mouse position and records where it lands.
    pub fn add_point(&mut self, cursor: Point) {
        self.pen_tip = self.pen_tip.moved_toward(cursor, STABILIZER_FOLLOW);
        self.record_point(self.pen_tip);
    }

    /// Finishes the line exactly where the mouse was released, so the
    /// stabilizer's trailing tip does not leave it short.
    pub fn end(&mut self, release_point: Point) {
        self.record_point(release_point);
    }

    /// Records a point, unless it is too close to the previous one.
    fn record_point(&mut self, point: Point) {
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

    fn drawn(shape: StrokeShape, width_px: f32) -> DrawnStroke {
        DrawnStroke {
            shape,
            pen: PenSettings {
                width_px,
                ..crate::config::DEFAULT_PEN
            },
        }
    }

    fn horizontal_line(y: f32) -> StrokeShape {
        StrokeShape::Path {
            start: Point::new(0.0, y),
            segments: vec![PathSegment::Line {
                to: Point::new(100.0, y),
            }],
        }
    }

    #[test]
    fn eraser_on_line_touches_it() {
        let line = drawn(horizontal_line(50.0), 10.0);
        assert!(line.is_touched_by(Point::new(40.0, 50.0)));
    }

    #[test]
    fn eraser_reaches_past_edge_of_thick_line_but_not_far() {
        let line = drawn(horizontal_line(50.0), 10.0);
        // Edge is at 5 px from the centre; reach adds a few pixels more.
        assert!(line.is_touched_by(Point::new(40.0, 58.0)));
        assert!(!line.is_touched_by(Point::new(40.0, 70.0)));
    }

    #[test]
    fn eraser_past_line_end_misses() {
        let line = drawn(horizontal_line(50.0), 4.0);
        assert!(!line.is_touched_by(Point::new(130.0, 50.0)));
    }

    #[test]
    fn eraser_follows_curves() {
        // The curve bulges up toward its control point: its middle is at
        // y = 25, far from the straight line between its ends (y = 0).
        let curve = drawn(
            StrokeShape::Path {
                start: Point::new(0.0, 0.0),
                segments: vec![PathSegment::Quadratic {
                    control: Point::new(50.0, 50.0),
                    to: Point::new(100.0, 0.0),
                }],
            },
            4.0,
        );
        assert!(curve.is_touched_by(Point::new(50.0, 25.0)));
        assert!(!curve.is_touched_by(Point::new(50.0, 2.0)));
    }

    #[test]
    fn eraser_touches_dots() {
        let dot = drawn(
            StrokeShape::Dot {
                center: Point::new(10.0, 10.0),
            },
            6.0,
        );
        assert!(dot.is_touched_by(Point::new(12.0, 12.0)));
        assert!(!dot.is_touched_by(Point::new(30.0, 30.0)));
    }

    #[test]
    fn eraser_picks_most_recent_of_overlapping_strokes() {
        let strokes = [
            drawn(horizontal_line(50.0), 10.0),
            drawn(horizontal_line(52.0), 10.0),
            drawn(horizontal_line(300.0), 10.0),
        ];
        assert_eq!(topmost_stroke_at(&strokes, Point::new(20.0, 51.0)), Some(1));
        assert_eq!(topmost_stroke_at(&strokes, Point::new(20.0, 150.0)), None);
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
    fn click_and_release_elsewhere_creates_straight_line() {
        let mut stroke = Stroke::begin(Point::new(0.0, 0.0));
        stroke.end(Point::new(10.0, 0.0));
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
    fn pen_tip_trails_behind_cursor() {
        let mut stroke = Stroke::begin(Point::new(0.0, 0.0));
        stroke.add_point(Point::new(20.0, 0.0));
        let StrokeShape::Path { segments, .. } = stroke.shape() else {
            panic!("a 20 px drag must create a path");
        };
        let tip = segment_end(segments.last().expect("one segment"));
        assert!(tip.x > 0.0 && tip.x < 20.0, "tip at {tip:?}");
    }

    #[test]
    fn released_stroke_ends_at_release_point() {
        let mut stroke = Stroke::begin(Point::new(0.0, 0.0));
        for step in 1..=10 {
            stroke.add_point(Point::new(step as f32 * 5.0, 0.0));
        }
        stroke.end(Point::new(50.0, 0.0));
        let StrokeShape::Path { segments, .. } = stroke.shape() else {
            panic!("a 50 px drag must create a path");
        };
        let end = segment_end(segments.last().expect("segments"));
        assert_eq!(end, Point::new(50.0, 0.0));
    }

    #[test]
    fn pixel_jitter_is_smoothed_out() {
        // A diagonal drawn with whole-pixel mouse positions zig-zags by
        // up to one pixel. The stabilized points should stay much closer
        // to the true diagonal than the raw ones.
        let mut stroke = Stroke::begin(Point::new(0.0, 0.0));
        for step in 1..=60 {
            let x = step as f32;
            let jitter = if step % 2 == 0 { 1.0 } else { 0.0 };
            stroke.add_point(Point::new(x, (x * 0.5).floor() + jitter));
        }
        let worst_offset = stroke
            .points
            .iter()
            .skip(4) // the tip needs a few moves to catch up with the cursor
            .map(|point| (point.y - point.x * 0.5).abs())
            .fold(0.0, f32::max);
        // The raw mouse positions stray up to 1.0 px from the diagonal.
        assert!(worst_offset < 0.75, "worst offset {worst_offset}");
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
