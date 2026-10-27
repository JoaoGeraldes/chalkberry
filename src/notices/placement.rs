//! Where a callout bubble goes on screen, relative to the thing it points
//! at.
//!
//! Pure Rust: this module must never depend on Windows APIs. Everything
//! is in physical screen pixels.

/// A rectangle in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl PixelRect {
    fn center_x(&self) -> i32 {
        (self.left + self.right) / 2
    }
}

/// Which edge of the bubble the arrow sticks out of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrowEdge {
    /// Bubble below the target, arrow pointing up at it.
    Top,
    /// Bubble above the target, arrow pointing down at it.
    Bottom,
}

/// The result of placing a bubble.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    /// Top-left corner of the bubble's window, in screen pixels.
    pub left: i32,
    pub top: i32,
    pub arrow_edge: ArrowEdge,
    /// The arrow tip's distance from the window's left edge, in pixels.
    pub arrow_x: i32,
}

/// The sizes needed to place a bubble, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BubbleSize {
    /// Whole window, including the arrow.
    pub width: i32,
    pub height: i32,
    /// Space kept between the arrow tip and the target.
    pub gap: i32,
    /// The arrow tip stays at least this far from the window's sides, so it
    /// never sits on a rounded corner.
    pub arrow_inset: i32,
}

/// Puts the bubble below `target` (arrow up), or above it when there is
/// not enough room below. Keeps it inside `screen`, while the arrow still
/// points at the target's centre as closely as the bubble allows.
pub fn place_bubble(target: PixelRect, size: BubbleSize, screen: PixelRect) -> Placement {
    let below_top = target.bottom + size.gap;
    let fits_below = below_top + size.height <= screen.bottom;
    let (arrow_edge, top) = if fits_below {
        (ArrowEdge::Top, below_top)
    } else {
        (ArrowEdge::Bottom, target.top - size.gap - size.height)
    };
    let top = clamp_start(top, size.height, screen.top, screen.bottom);
    let centered_left = target.center_x() - size.width / 2;
    let left = clamp_start(centered_left, size.width, screen.left, screen.right);
    let arrow_x = (target.center_x() - left).clamp(size.arrow_inset, size.width - size.arrow_inset);
    Placement {
        left,
        top,
        arrow_edge,
        arrow_x,
    }
}

/// Moves a span of `length` starting at `start` so it fits between `low`
/// and `high`. If it is too long to fit, it starts at `low`.
fn clamp_start(start: i32, length: i32, low: i32, high: i32) -> i32 {
    start.min(high - length).max(low)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: PixelRect = PixelRect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };

    const SIZE: BubbleSize = BubbleSize {
        width: 200,
        height: 80,
        gap: 4,
        arrow_inset: 16,
    };

    fn target_at(left: i32, top: i32) -> PixelRect {
        PixelRect {
            left,
            top,
            right: left + 40,
            bottom: top + 40,
        }
    }

    #[test]
    fn bubble_goes_below_target_with_arrow_up() {
        let placement = place_bubble(target_at(500, 100), SIZE, SCREEN);
        assert_eq!(placement.arrow_edge, ArrowEdge::Top);
        assert_eq!(placement.top, 144);
    }

    #[test]
    fn bubble_flips_above_target_near_screen_bottom() {
        let placement = place_bubble(target_at(500, 1040), SIZE, SCREEN);
        assert_eq!(placement.arrow_edge, ArrowEdge::Bottom);
        assert_eq!(placement.top + SIZE.height, 1040 - SIZE.gap);
    }

    #[test]
    fn bubble_is_centred_on_target_with_arrow_in_middle() {
        let placement = place_bubble(target_at(500, 100), SIZE, SCREEN);
        assert_eq!(placement.left, 520 - 100);
        assert_eq!(placement.arrow_x, 100);
    }

    #[test]
    fn bubble_stays_on_screen_at_right_edge_and_arrow_still_points() {
        // A tray icon in the bottom-right corner.
        let target = target_at(1880, 1040);
        let placement = place_bubble(target, SIZE, SCREEN);
        assert_eq!(placement.left + SIZE.width, SCREEN.right);
        let arrow_tip_on_screen = placement.left + placement.arrow_x;
        assert!(arrow_tip_on_screen > placement.left + SIZE.width / 2);
        assert!(placement.arrow_x <= SIZE.width - SIZE.arrow_inset);
    }

    #[test]
    fn arrow_keeps_off_rounded_corner_at_left_edge() {
        let placement = place_bubble(target_at(-30, 100), SIZE, SCREEN);
        assert_eq!(placement.left, 0);
        assert_eq!(placement.arrow_x, SIZE.arrow_inset);
    }
}
