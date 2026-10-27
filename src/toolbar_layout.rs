//! Where everything sits on the floating toolbar, and what is under the
//! mouse.
//!
//! Pure Rust: this module must never depend on Windows APIs. All sizes are
//! in device-independent pixels (DIPs); the toolbar window scales them to
//! the monitor's DPI.
//!
//! Layout: a main row with a drag grip and the buttons, plus, while a
//! picker is open, a second row with that picker's choices.

use crate::config::{COLOR_CHOICES, OPACITY_CHOICES, WIDTH_CHOICES};

/// Space between the toolbar's edge and its contents.
const PADDING: f32 = 5.0;
/// Width of the dotted grip on the left that hints "drag me".
const GRIP_WIDTH: f32 = 12.0;
const BUTTON_SIZE: f32 = 30.0;
const BUTTON_GAP: f32 = 2.0;
/// Extra space separating the pen buttons from the clear/hide buttons.
const GROUP_GAP: f32 = 8.0;
const ROW_GAP: f32 = 4.0;
const CHOICE_SIZE: f32 = 26.0;
const CHOICE_GAP: f32 = 2.0;

/// An axis-aligned rectangle in DIPs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Rect {
    fn from_size(left: f32, top: f32, width: f32, height: f32) -> Self {
        Self {
            left,
            top,
            right: left + width,
            bottom: top + height,
        }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        (self.left..self.right).contains(&x) && (self.top..self.bottom).contains(&y)
    }

    pub fn center(&self) -> (f32, f32) {
        (
            (self.left + self.right) / 2.0,
            (self.top + self.bottom) / 2.0,
        )
    }

    pub fn width(&self) -> f32 {
        self.right - self.left
    }
}

/// The toolbar's buttons, left to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarButton {
    Color,
    Width,
    Opacity,
    Clear,
    Hide,
}

impl ToolbarButton {
    pub const ALL: [ToolbarButton; 5] = [
        ToolbarButton::Color,
        ToolbarButton::Width,
        ToolbarButton::Opacity,
        ToolbarButton::Clear,
        ToolbarButton::Hide,
    ];

    /// The picker this button opens, if it opens one.
    pub fn picker(self) -> Option<Picker> {
        match self {
            ToolbarButton::Color => Some(Picker::Color),
            ToolbarButton::Width => Some(Picker::Width),
            ToolbarButton::Opacity => Some(Picker::Opacity),
            ToolbarButton::Clear | ToolbarButton::Hide => None,
        }
    }

    /// Buttons after the pen settings are set apart by a wider gap.
    fn starts_new_group(self) -> bool {
        self == ToolbarButton::Clear
    }
}

/// A row of choices shown under the main row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picker {
    Color,
    Width,
    Opacity,
}

impl Picker {
    pub fn choice_count(self) -> usize {
        match self {
            Picker::Color => COLOR_CHOICES.len(),
            Picker::Width => WIDTH_CHOICES.len(),
            Picker::Opacity => OPACITY_CHOICES.len(),
        }
    }
}

/// What is under a point on the toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarHit {
    Button(ToolbarButton),
    /// The choice at this index in the open picker.
    Choice(usize),
    /// Anywhere else: dragging here moves the toolbar.
    Background,
}

/// The toolbar's layout for a given open picker (or none).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolbarLayout {
    pub open_picker: Option<Picker>,
}

impl ToolbarLayout {
    pub fn new(open_picker: Option<Picker>) -> Self {
        Self { open_picker }
    }

    /// Total toolbar size as (width, height).
    pub fn size(&self) -> (f32, f32) {
        let main_row_width = self.main_row_right() + PADDING;
        let main_row_height = PADDING + BUTTON_SIZE + PADDING;
        let Some(picker) = self.open_picker else {
            return (main_row_width, main_row_height);
        };
        let choices_width = PADDING + choices_span(picker.choice_count()) + PADDING;
        let height = main_row_height + ROW_GAP + CHOICE_SIZE;
        (main_row_width.max(choices_width), height)
    }

    /// The dotted drag grip at the left of the main row.
    pub fn grip_rect(&self) -> Rect {
        Rect::from_size(PADDING, PADDING, GRIP_WIDTH, BUTTON_SIZE)
    }

    pub fn button_rect(&self, button: ToolbarButton) -> Rect {
        let mut left = PADDING + GRIP_WIDTH + BUTTON_GAP;
        for candidate in ToolbarButton::ALL {
            if candidate.starts_new_group() {
                left += GROUP_GAP;
            }
            if candidate == button {
                break;
            }
            left += BUTTON_SIZE + BUTTON_GAP;
        }
        Rect::from_size(left, PADDING, BUTTON_SIZE, BUTTON_SIZE)
    }

    /// The choice at `index` in the open picker, or `None` if no picker is
    /// open or it has no such choice.
    pub fn choice_rect(&self, index: usize) -> Option<Rect> {
        let picker = self.open_picker?;
        if index >= picker.choice_count() {
            return None;
        }
        let left = PADDING + index as f32 * (CHOICE_SIZE + CHOICE_GAP);
        let top = PADDING + BUTTON_SIZE + PADDING + ROW_GAP;
        Some(Rect::from_size(left, top, CHOICE_SIZE, CHOICE_SIZE))
    }

    /// What lies under (`x`, `y`).
    pub fn hit_test(&self, x: f32, y: f32) -> ToolbarHit {
        let button = ToolbarButton::ALL
            .into_iter()
            .find(|&button| self.button_rect(button).contains(x, y));
        if let Some(button) = button {
            return ToolbarHit::Button(button);
        }
        let choice_count = self.open_picker.map_or(0, Picker::choice_count);
        (0..choice_count)
            .find(|&index| {
                self.choice_rect(index)
                    .is_some_and(|rect| rect.contains(x, y))
            })
            .map_or(ToolbarHit::Background, ToolbarHit::Choice)
    }

    fn main_row_right(&self) -> f32 {
        let last_button = ToolbarButton::ALL[ToolbarButton::ALL.len() - 1];
        self.button_rect(last_button).right
    }
}

fn choices_span(choice_count: usize) -> f32 {
    let count = choice_count as f32;
    count * CHOICE_SIZE + (count - 1.0).max(0.0) * CHOICE_GAP
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicking_a_button_centre_hits_that_button() {
        let layout = ToolbarLayout::new(None);
        for button in ToolbarButton::ALL {
            let (x, y) = layout.button_rect(button).center();
            assert_eq!(layout.hit_test(x, y), ToolbarHit::Button(button));
        }
    }

    #[test]
    fn grip_is_background_so_it_drags() {
        let layout = ToolbarLayout::new(None);
        let (x, y) = layout.grip_rect().center();
        assert_eq!(layout.hit_test(x, y), ToolbarHit::Background);
    }

    #[test]
    fn buttons_do_not_overlap_and_stay_inside() {
        let layout = ToolbarLayout::new(None);
        let (width, height) = layout.size();
        let mut previous_right = layout.grip_rect().right;
        for button in ToolbarButton::ALL {
            let rect = layout.button_rect(button);
            assert!(rect.left >= previous_right, "{button:?} overlaps");
            assert!(rect.right <= width && rect.bottom <= height);
            previous_right = rect.right;
        }
    }

    #[test]
    fn clear_button_is_set_apart_from_pen_buttons() {
        let layout = ToolbarLayout::new(None);
        let opacity = layout.button_rect(ToolbarButton::Opacity);
        let clear = layout.button_rect(ToolbarButton::Clear);
        assert!(clear.left - opacity.right > BUTTON_GAP);
    }

    #[test]
    fn closed_toolbar_has_no_choices() {
        let layout = ToolbarLayout::new(None);
        assert_eq!(layout.choice_rect(0), None);
    }

    #[test]
    fn opening_a_picker_adds_a_row_below() {
        let closed = ToolbarLayout::new(None).size();
        let open = ToolbarLayout::new(Some(Picker::Width)).size();
        assert!(open.1 > closed.1);
        let first_choice = ToolbarLayout::new(Some(Picker::Width))
            .choice_rect(0)
            .expect("width picker has choices");
        assert!(first_choice.top >= closed.1 - PADDING);
    }

    #[test]
    fn every_choice_is_hit_at_its_centre_and_fits() {
        for picker in [Picker::Color, Picker::Width, Picker::Opacity] {
            let layout = ToolbarLayout::new(Some(picker));
            let (width, height) = layout.size();
            for index in 0..picker.choice_count() {
                let rect = layout.choice_rect(index).expect("choice exists");
                let (x, y) = rect.center();
                assert_eq!(layout.hit_test(x, y), ToolbarHit::Choice(index));
                assert!(rect.right <= width && rect.bottom <= height);
            }
            assert_eq!(layout.choice_rect(picker.choice_count()), None);
        }
    }

    #[test]
    fn colour_picker_widens_toolbar_to_fit_all_swatches() {
        let layout = ToolbarLayout::new(Some(Picker::Color));
        let last_index = Picker::Color.choice_count() - 1;
        let last_swatch = layout.choice_rect(last_index).expect("swatch");
        assert!(layout.size().0 >= last_swatch.right + PADDING);
    }

    #[test]
    fn only_pen_buttons_open_pickers() {
        assert_eq!(ToolbarButton::Color.picker(), Some(Picker::Color));
        assert_eq!(ToolbarButton::Clear.picker(), None);
        assert_eq!(ToolbarButton::Hide.picker(), None);
    }
}
