//! User-tweakable settings: the pen choices offered in the tray menu and
//! the hotkeys, kept as plain constants.
//!
//! Plain data only: no Windows APIs and no logic beyond picking defaults.

/// How the pen draws. Changed at runtime from the tray menu; strokes that
/// are already on screen keep the look they were drawn with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PenSettings {
    /// Red, green, blue in the 0.0..=1.0 range.
    pub color_rgb: [f32; 3],
    /// 0.0 (invisible) to 1.0 (solid).
    pub opacity: f32,
    /// Line thickness in physical pixels.
    pub width_px: f32,
}

/// Chalk colours offered in the menu, as (menu label, red/green/blue).
pub const COLOR_CHOICES: [(&str, [f32; 3]); 8] = [
    ("Red", [0.95, 0.22, 0.25]),
    ("White", [0.96, 0.96, 0.92]),
    ("Yellow", [1.0, 0.88, 0.32]),
    ("Orange", [1.0, 0.6, 0.25]),
    ("Pink", [0.96, 0.55, 0.72]),
    ("Blue", [0.35, 0.68, 1.0]),
    ("Green", [0.4, 0.82, 0.45]),
    ("Black", [0.1, 0.1, 0.1]),
];

/// Line thicknesses offered in the menu, as (menu label, pixels).
pub const WIDTH_CHOICES: [(&str, f32); 5] = [
    ("Fine", 3.0),
    ("Thin", 6.0),
    ("Medium", 10.0),
    ("Thick", 16.0),
    ("Extra thick", 26.0),
];

/// Opacities offered in the menu, as (menu label, opacity).
pub const OPACITY_CHOICES: [(&str, f32); 4] = [
    ("100% (solid)", 1.0),
    ("75%", 0.75),
    ("50%", 0.5),
    ("25%", 0.25),
];

/// The pen on the very first start; after that, the saved one (see `settings`).
pub const DEFAULT_PEN: PenSettings = PenSettings {
    color_rgb: COLOR_CHOICES[0].1,
    opacity: OPACITY_CHOICES[0].1,
    width_px: WIDTH_CHOICES[2].1,
};

/// Ctrl+Alt+this letter clears all drawings.
pub const CLEAR_HOTKEY_LETTER: char = 'C';

/// Ctrl+Alt+this letter quits the app.
pub const QUIT_HOTKEY_LETTER: char = 'Q';
