//! User-tweakable settings, kept as plain constants.
//!
//! Promote these to a settings file only when someone actually needs to
//! change them without rebuilding.

/// Pen colour as red, green, blue in the 0.0..=1.0 range.
pub const PEN_COLOR_RGB: [f32; 3] = [0.95, 0.15, 0.2];

/// Pen opacity, 0.0 (invisible) to 1.0 (solid).
pub const PEN_OPACITY: f32 = 1.0;

/// Pen thickness in physical pixels.
pub const PEN_WIDTH_PX: f32 = 4.0;

/// Ctrl+Alt+this letter clears all drawings.
pub const CLEAR_HOTKEY_LETTER: char = 'C';

/// Ctrl+Alt+this letter quits the app.
pub const QUIT_HOTKEY_LETTER: char = 'Q';
