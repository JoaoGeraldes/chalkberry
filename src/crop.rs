//! The maths behind the screenshot crop screen: the box the user drags,
//! cutting it out of the screenshot, where the panel and its buttons sit,
//! and how files are named.
//!
//! Pure Rust: this module must never depend on Windows APIs. Positions are
//! in pixels of the crop window (its top-left is 0, 0).

use crate::callout_placement::PixelRect;

/// Drags shorter than this (in pixels, either way) count as a click: they
/// clear the box, so the whole screen is saved.
const MIN_SELECTION_PX: i32 = 4;
const BYTES_PER_PIXEL: usize = 4;

// Panel layout, in DIPs (multiplied by the UI scale).
const PANEL_WIDTH: f32 = 600.0;
const PANEL_HEIGHT: f32 = 118.0;
const PANEL_BOTTOM_MARGIN: f32 = 48.0;
const PANEL_PADDING: f32 = 16.0;
const TEXT_LINE_HEIGHT: f32 = 20.0;
const BUTTON_HEIGHT: f32 = 32.0;
const BUTTON_GAP: f32 = 8.0;
const FOLDER_BUTTON_WIDTH: f32 = 150.0;
const ACTION_BUTTON_WIDTH: f32 = 104.0;

/// The rectangle between two corners, whichever way the user dragged.
pub fn rect_from_corners(start: (i32, i32), end: (i32, i32)) -> PixelRect {
    PixelRect {
        left: start.0.min(end.0),
        top: start.1.min(end.1),
        right: start.0.max(end.0),
        bottom: start.1.max(end.1),
    }
}

/// True if a dragged box is too small to be meant as a crop.
pub fn is_click_not_drag(rect: PixelRect) -> bool {
    rect.right - rect.left < MIN_SELECTION_PX && rect.bottom - rect.top < MIN_SELECTION_PX
}

/// `rect` limited to a `width` × `height` image.
pub fn clamp_to_image(rect: PixelRect, width: u32, height: u32) -> PixelRect {
    let (width, height) = (width as i32, height as i32);
    PixelRect {
        left: rect.left.clamp(0, width),
        top: rect.top.clamp(0, height),
        right: rect.right.clamp(0, width),
        bottom: rect.bottom.clamp(0, height),
    }
}

/// Copies the part of a BGRA image (`width` pixels wide) inside `rect`,
/// which must lie within the image. Returns its pixels, row by row.
pub fn crop_bgra(pixels: &[u8], width: u32, rect: PixelRect) -> Vec<u8> {
    let row_bytes = width as usize * BYTES_PER_PIXEL;
    let start_column = rect.left as usize * BYTES_PER_PIXEL;
    let end_column = rect.right as usize * BYTES_PER_PIXEL;
    let mut cropped =
        Vec::with_capacity((end_column - start_column) * (rect.bottom - rect.top) as usize);
    for row in rect.top as usize..rect.bottom as usize {
        let row_start = row * row_bytes;
        cropped.extend_from_slice(&pixels[row_start + start_column..row_start + end_column]);
    }
    cropped
}

/// "Pen Marker 2026-10-27 15-35-08.png". Colons are not allowed in Windows
/// file names, hence the dashes in the time.
pub fn screenshot_file_name(date: (u16, u16, u16), time: (u16, u16, u16)) -> String {
    let (year, month, day) = date;
    let (hour, minute, second) = time;
    format!("Pen Marker {year:04}-{month:02}-{day:02} {hour:02}-{minute:02}-{second:02}.png")
}

/// Shortens a long path for display by replacing its middle with "…",
/// keeping the drive at the start and the folder name at the end.
pub fn shorten_path(path: &str, max_chars: usize) -> String {
    let char_count = path.chars().count();
    if char_count <= max_chars {
        return path.to_owned();
    }
    let keep = max_chars.saturating_sub(1);
    let head_chars = keep / 3;
    let tail_chars = keep - head_chars;
    let head: String = path.chars().take(head_chars).collect();
    let tail: String = path.chars().skip(char_count - tail_chars).collect();
    format!("{head}\u{2026}{tail}")
}

/// The crop screen's buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CropButton {
    ChangeFolder,
    Cancel,
    Save,
}

impl CropButton {
    pub const ALL: [CropButton; 3] = [
        CropButton::ChangeFolder,
        CropButton::Cancel,
        CropButton::Save,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CropButton::ChangeFolder => "Change folder\u{2026}",
            CropButton::Cancel => "Cancel",
            CropButton::Save => "Save",
        }
    }
}

/// Where the panel sits: centred near the bottom of the main screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelLayout {
    /// Pixels per DIP.
    scale: f32,
    left: f32,
    top: f32,
}

impl PanelLayout {
    /// Lays the panel out on `main_screen` (in crop window pixels).
    pub fn new(main_screen: PixelRect, scale: f32) -> Self {
        let center_x = (main_screen.left + main_screen.right) as f32 / 2.0;
        Self {
            scale,
            left: center_x - PANEL_WIDTH * scale / 2.0,
            top: main_screen.bottom as f32 - (PANEL_BOTTOM_MARGIN + PANEL_HEIGHT) * scale,
        }
    }

    /// The whole panel as (left, top, right, bottom) pixels.
    pub fn panel(&self) -> (f32, f32, f32, f32) {
        self.rect(0.0, 0.0, PANEL_WIDTH, PANEL_HEIGHT)
    }

    /// The instruction line at the top of the panel.
    pub fn instruction_line(&self) -> (f32, f32, f32, f32) {
        let width = PANEL_WIDTH - 2.0 * PANEL_PADDING;
        self.rect(PANEL_PADDING, PANEL_PADDING, width, TEXT_LINE_HEIGHT)
    }

    /// The "Saves to: …" line under it.
    pub fn folder_line(&self) -> (f32, f32, f32, f32) {
        let width = PANEL_WIDTH - 2.0 * PANEL_PADDING;
        let top = PANEL_PADDING + TEXT_LINE_HEIGHT;
        self.rect(PANEL_PADDING, top, width, TEXT_LINE_HEIGHT)
    }

    /// "Change folder…" on the left; "Cancel" and "Save" on the right.
    pub fn button(&self, button: CropButton) -> (f32, f32, f32, f32) {
        let top = PANEL_HEIGHT - PANEL_PADDING - BUTTON_HEIGHT;
        let right_edge = PANEL_WIDTH - PANEL_PADDING;
        let (left, width) = match button {
            CropButton::ChangeFolder => (PANEL_PADDING, FOLDER_BUTTON_WIDTH),
            CropButton::Save => (right_edge - ACTION_BUTTON_WIDTH, ACTION_BUTTON_WIDTH),
            CropButton::Cancel => (
                right_edge - 2.0 * ACTION_BUTTON_WIDTH - BUTTON_GAP,
                ACTION_BUTTON_WIDTH,
            ),
        };
        self.rect(left, top, width, BUTTON_HEIGHT)
    }

    /// The button under (`x`, `y`), if any.
    pub fn button_at(&self, x: f32, y: f32) -> Option<CropButton> {
        CropButton::ALL.into_iter().find(|&button| {
            let (left, top, right, bottom) = self.button(button);
            (left..right).contains(&x) && (top..bottom).contains(&y)
        })
    }

    /// True if (`x`, `y`) is on the panel (so it does not start a crop).
    pub fn contains(&self, x: f32, y: f32) -> bool {
        let (left, top, right, bottom) = self.panel();
        (left..right).contains(&x) && (top..bottom).contains(&y)
    }

    /// A rectangle given in panel DIPs, as window pixels.
    fn rect(&self, left: f32, top: f32, width: f32, height: f32) -> (f32, f32, f32, f32) {
        let scale = self.scale;
        let left = self.left + left * scale;
        let top = self.top + top * scale;
        (left, top, left + width * scale, top + height * scale)
    }
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

    fn center(rect: (f32, f32, f32, f32)) -> (f32, f32) {
        ((rect.0 + rect.2) / 2.0, (rect.1 + rect.3) / 2.0)
    }

    #[test]
    fn dragging_up_and_left_still_makes_a_proper_box() {
        let rect = rect_from_corners((300, 200), (100, 50));
        assert_eq!(
            rect,
            PixelRect {
                left: 100,
                top: 50,
                right: 300,
                bottom: 200
            }
        );
    }

    #[test]
    fn tiny_drag_counts_as_click() {
        assert!(is_click_not_drag(rect_from_corners((10, 10), (12, 11))));
        assert!(!is_click_not_drag(rect_from_corners((10, 10), (60, 11))));
    }

    #[test]
    fn box_dragged_off_screen_is_clamped() {
        let rect = clamp_to_image(rect_from_corners((-50, 10), (5000, 20)), 1920, 1080);
        assert_eq!((rect.left, rect.right), (0, 1920));
    }

    #[test]
    fn cropping_copies_exactly_the_boxed_pixels() {
        // A 3 × 2 image where each pixel's first byte is its index.
        let pixels: Vec<u8> = (0..6u8).flat_map(|index| [index, 0, 0, 255]).collect();
        let rect = PixelRect {
            left: 1,
            top: 0,
            right: 3,
            bottom: 2,
        };
        let cropped = crop_bgra(&pixels, 3, rect);
        let first_bytes: Vec<u8> = cropped.chunks(4).map(|pixel| pixel[0]).collect();
        assert_eq!(first_bytes, vec![1, 2, 4, 5]);
    }

    #[test]
    fn file_name_has_date_and_time_without_colons() {
        let name = screenshot_file_name((2026, 10, 27), (15, 5, 8));
        assert_eq!(name, "Pen Marker 2026-10-27 15-05-08.png");
        assert!(!name.contains(':'));
    }

    #[test]
    fn short_paths_are_shown_whole() {
        assert_eq!(shorten_path(r"C:\Pictures", 40), r"C:\Pictures");
    }

    #[test]
    fn long_paths_keep_start_and_end() {
        let path = r"C:\Users\someone\OneDrive\Documents\Pictures\Pen Marker";
        let short = shorten_path(path, 30);
        assert_eq!(short.chars().count(), 30);
        assert!(short.starts_with(r"C:\Users"));
        assert!(short.ends_with(r"\Pen Marker"));
        assert!(short.contains('\u{2026}'));
    }

    #[test]
    fn every_button_is_found_at_its_centre_and_inside_panel() {
        let layout = PanelLayout::new(SCREEN, 1.5);
        for button in CropButton::ALL {
            let (x, y) = center(layout.button(button));
            assert_eq!(layout.button_at(x, y), Some(button));
            assert!(layout.contains(x, y));
        }
    }

    #[test]
    fn panel_sits_centred_near_bottom_of_main_screen() {
        let (left, _, right, bottom) = PanelLayout::new(SCREEN, 1.0).panel();
        assert!(((left + right) / 2.0 - 960.0).abs() < 0.5);
        assert!(bottom < 1080.0 && bottom > 900.0);
    }

    #[test]
    fn buttons_do_not_overlap() {
        let layout = PanelLayout::new(SCREEN, 1.0);
        let folder = layout.button(CropButton::ChangeFolder);
        let cancel = layout.button(CropButton::Cancel);
        let save = layout.button(CropButton::Save);
        assert!(folder.2 < cancel.0 && cancel.2 < save.0);
    }

    #[test]
    fn text_lines_sit_above_buttons() {
        let layout = PanelLayout::new(SCREEN, 1.0);
        assert!(layout.folder_line().3 <= layout.button(CropButton::Save).1);
        assert!(layout.instruction_line().3 <= layout.folder_line().1);
    }
}
