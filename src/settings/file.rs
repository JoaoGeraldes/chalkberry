//! The user's settings (pen, toolbar place and visibility, screenshot
//! folder), kept in `%APPDATA%\Chalkberry\settings.json` and saved after
//! every change.
//!
//! Only `std` file work: no Windows APIs (the caller passes in the default
//! screenshot folder, which needs one). A missing or unreadable value falls
//! back to its default; a bad settings file never stops the app.

use std::fs;
use std::io;
use std::path::PathBuf;

use crate::settings::config::{
    COLOR_CHOICES, DEFAULT_PEN, OPACITY_CHOICES, PenSettings, WIDTH_CHOICES,
};
use crate::settings::json::{self, JsonValue};

const SETTINGS_FOLDER_NAME: &str = "Chalkberry";
const SETTINGS_FILE_NAME: &str = "settings.json";
/// Written first and then renamed over the settings file, so a crash in
/// the middle of saving never leaves a half-written file.
const TEMPORARY_FILE_NAME: &str = "settings.json.tmp";

const PEN_COLOR_KEY: &str = "pen_color";
const PEN_WIDTH_KEY: &str = "pen_width_px";
const PEN_OPACITY_KEY: &str = "pen_opacity";
const TOOLBAR_X_KEY: &str = "toolbar_x";
const TOOLBAR_Y_KEY: &str = "toolbar_y";
const TOOLBAR_VISIBLE_KEY: &str = "toolbar_visible";
const SCREENSHOT_FOLDER_KEY: &str = "screenshot_folder";

/// Largest value of one colour channel in `#RRGGBB`.
const MAX_CHANNEL: f32 = 255.0;

/// Everything the app remembers between runs.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// The pen for new strokes. Always one of the menu's choices.
    pub pen: PenSettings,
    /// The toolbar's top-left corner in screen pixels, or `None` to let
    /// the toolbar pick its starting place.
    pub toolbar_top_left: Option<(i32, i32)>,
    pub is_toolbar_visible: bool,
    pub screenshot_folder: PathBuf,
}

impl Settings {
    /// The settings on the very first start.
    pub fn defaults(default_screenshot_folder: PathBuf) -> Self {
        Self {
            pen: DEFAULT_PEN,
            toolbar_top_left: None,
            is_toolbar_visible: true,
            screenshot_folder: default_screenshot_folder,
        }
    }

    /// Reads the settings file. Returns `None` if there is none yet (the
    /// first start). Values that are missing or unreadable get defaults.
    pub fn load(default_screenshot_folder: PathBuf) -> Option<Self> {
        let text = match fs::read_to_string(settings_file()?) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
            Err(error) => {
                eprintln!("[chalkberry] error while reading settings: {error}");
                return Some(Self::defaults(default_screenshot_folder));
            }
        };
        Some(Self::from_json(&text, default_screenshot_folder))
    }

    /// Writes the settings file, replacing the old one in one step.
    pub fn save(&self) -> io::Result<()> {
        let file = settings_file().ok_or_else(|| io::Error::other("APPDATA is not set"))?;
        if let Some(folder) = file.parent() {
            fs::create_dir_all(folder)?;
        }
        let temporary_file = file.with_file_name(TEMPORARY_FILE_NAME);
        fs::write(&temporary_file, self.to_json())?;
        fs::rename(&temporary_file, &file)
    }

    fn from_json(text: &str, default_screenshot_folder: PathBuf) -> Self {
        let defaults = Self::defaults(default_screenshot_folder);
        let Some(entries) = json::parse_flat_object(text) else {
            eprintln!("[chalkberry] the settings file is not valid JSON; using defaults");
            return defaults;
        };
        let entries = Entries(entries);
        let toolbar_x = entries.number(TOOLBAR_X_KEY);
        let toolbar_y = entries.number(TOOLBAR_Y_KEY);
        Self {
            pen: entries.pen(),
            // `as` saturates, so absurd numbers still give a valid pixel.
            toolbar_top_left: toolbar_x.zip(toolbar_y).map(|(x, y)| (x as i32, y as i32)),
            is_toolbar_visible: entries
                .get(TOOLBAR_VISIBLE_KEY)
                .and_then(JsonValue::as_bool)
                .unwrap_or(defaults.is_toolbar_visible),
            // A folder that no longer exists (e.g. on an unplugged drive)
            // falls back to the default.
            screenshot_folder: entries
                .text(SCREENSHOT_FOLDER_KEY)
                .map(PathBuf::from)
                .filter(|folder| folder.is_dir())
                .unwrap_or(defaults.screenshot_folder),
        }
    }

    fn to_json(&self) -> String {
        let mut entries = vec![
            (
                PEN_COLOR_KEY,
                JsonValue::Text(color_to_hex(self.pen.color_rgb)),
            ),
            (PEN_WIDTH_KEY, JsonValue::Number(self.pen.width_px.into())),
            (PEN_OPACITY_KEY, JsonValue::Number(self.pen.opacity.into())),
        ];
        if let Some((x, y)) = self.toolbar_top_left {
            entries.push((TOOLBAR_X_KEY, JsonValue::Number(x.into())));
            entries.push((TOOLBAR_Y_KEY, JsonValue::Number(y.into())));
        }
        entries.push((
            TOOLBAR_VISIBLE_KEY,
            JsonValue::Bool(self.is_toolbar_visible),
        ));
        let folder = self.screenshot_folder.to_string_lossy().into_owned();
        entries.push((SCREENSHOT_FOLDER_KEY, JsonValue::Text(folder)));
        json::write_flat_object(&entries)
    }
}

/// The key/value pairs read from the settings file.
struct Entries(Vec<(String, JsonValue)>);

impl Entries {
    fn get(&self, key: &str) -> Option<&JsonValue> {
        self.0
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    fn number(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(JsonValue::as_number)
    }

    fn text(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(JsonValue::as_text)
    }

    /// The saved pen, each part snapped to the nearest menu choice, so the
    /// toolbar and tray always show it as picked.
    fn pen(&self) -> PenSettings {
        let color_rgb = self
            .text(PEN_COLOR_KEY)
            .and_then(color_from_hex)
            .map(nearest_color);
        let width_px = self
            .number(PEN_WIDTH_KEY)
            .map(|width| nearest_choice(WIDTH_CHOICES.map(|(_, choice)| choice), width as f32));
        let opacity = self.number(PEN_OPACITY_KEY).map(|opacity| {
            nearest_choice(OPACITY_CHOICES.map(|(_, choice)| choice), opacity as f32)
        });
        PenSettings {
            color_rgb: color_rgb.unwrap_or(DEFAULT_PEN.color_rgb),
            width_px: width_px.unwrap_or(DEFAULT_PEN.width_px),
            opacity: opacity.unwrap_or(DEFAULT_PEN.opacity),
        }
    }
}

fn settings_file() -> Option<PathBuf> {
    let app_data = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(app_data)
            .join(SETTINGS_FOLDER_NAME)
            .join(SETTINGS_FILE_NAME),
    )
}

/// `[0.95, 0.22, 0.25]` → `"#F2383F"`.
fn color_to_hex(color_rgb: [f32; 3]) -> String {
    let [red, green, blue] =
        color_rgb.map(|channel| (channel.clamp(0.0, 1.0) * MAX_CHANNEL).round() as u8);
    format!("#{red:02X}{green:02X}{blue:02X}")
}

/// `"#F2383F"` → `[0.949, 0.220, 0.247]`. `None` if it is not `#RRGGBB`.
fn color_from_hex(text: &str) -> Option<[f32; 3]> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |start: usize| {
        u8::from_str_radix(&hex[start..start + 2], 16)
            .ok()
            .map(|value| f32::from(value) / MAX_CHANNEL)
    };
    Some([channel(0)?, channel(2)?, channel(4)?])
}

/// The menu colour closest to `color_rgb`.
fn nearest_color(color_rgb: [f32; 3]) -> [f32; 3] {
    let distance = |choice: &[f32; 3]| -> f32 {
        choice
            .iter()
            .zip(color_rgb)
            .map(|(a, b)| (a - b).powi(2))
            .sum()
    };
    COLOR_CHOICES
        .iter()
        .map(|(_, choice)| *choice)
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
        .unwrap_or(DEFAULT_PEN.color_rgb)
}

/// The choice closest to `value`. `choices` must not be empty.
fn nearest_choice<const COUNT: usize>(choices: [f32; COUNT], value: f32) -> f32 {
    choices
        .into_iter()
        .min_by(|a, b| (a - value).abs().total_cmp(&(b - value).abs()))
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder that exists on every machine, so it survives loading.
    fn existing_folder() -> PathBuf {
        std::env::temp_dir()
    }

    fn missing_folder() -> PathBuf {
        PathBuf::from(r"Z:\no\such\folder\for\pen\marker")
    }

    #[test]
    fn saved_settings_load_back_the_same() {
        let settings = Settings {
            pen: PenSettings {
                color_rgb: COLOR_CHOICES[3].1,
                width_px: WIDTH_CHOICES[4].1,
                opacity: OPACITY_CHOICES[2].1,
            },
            toolbar_top_left: Some((-300, 40)),
            is_toolbar_visible: false,
            screenshot_folder: existing_folder(),
        };
        let loaded = Settings::from_json(&settings.to_json(), missing_folder());
        assert_eq!(loaded, settings);
    }

    #[test]
    fn every_menu_colour_survives_hex_rounding() {
        for (_, color_rgb) in COLOR_CHOICES {
            let read_back = color_from_hex(&color_to_hex(color_rgb)).map(nearest_color);
            assert_eq!(read_back, Some(color_rgb));
        }
    }

    #[test]
    fn hand_typed_values_snap_to_nearest_choice() {
        let text = r##"{"pen_color": "#FF0000", "pen_width_px": 11, "pen_opacity": 0.7}"##;
        let pen = Settings::from_json(text, missing_folder()).pen;
        assert_eq!(pen.color_rgb, COLOR_CHOICES[0].1, "red");
        assert_eq!(pen.width_px, 10.0, "medium");
        assert_eq!(pen.opacity, 0.75);
    }

    #[test]
    fn bad_values_fall_back_to_defaults() {
        let text = r#"{"pen_color": "red", "pen_width_px": "wide", "toolbar_visible": 1}"#;
        let settings = Settings::from_json(text, missing_folder());
        assert_eq!(settings, Settings::defaults(missing_folder()));
    }

    #[test]
    fn invalid_file_gives_defaults() {
        let settings = Settings::from_json("{ not json", missing_folder());
        assert_eq!(settings, Settings::defaults(missing_folder()));
    }

    #[test]
    fn toolbar_place_needs_both_coordinates() {
        let settings = Settings::from_json(r#"{"toolbar_x": 100}"#, missing_folder());
        assert_eq!(settings.toolbar_top_left, None);
    }

    #[test]
    fn missing_screenshot_folder_falls_back_to_default() {
        let text = r#"{"screenshot_folder": "Z:\\gone"}"#;
        let settings = Settings::from_json(text, existing_folder());
        assert_eq!(settings.screenshot_folder, existing_folder());
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let text = r#"{"future_setting": true, "toolbar_visible": false}"#;
        assert!(!Settings::from_json(text, missing_folder()).is_toolbar_visible);
    }
}
