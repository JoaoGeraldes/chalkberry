//! The actions a user can trigger from the tray menu or the toolbar.
//!
//! Pure Rust: this module must never depend on Windows APIs. Each command
//! has a small number id, used both as a menu item id and as the message
//! parameter the toolbar posts to the overlay, so one decoder serves both.

use crate::config::{COLOR_CHOICES, OPACITY_CHOICES, PenSettings, WIDTH_CHOICES};

// Each group of choices gets its own range of ids, so an id tells both the
// group and the choice. 0 is never used: menus report it for "nothing".
const CLEAR_DRAWING_ID: usize = 1;
const QUIT_ID: usize = 2;
const TOGGLE_TOOLBAR_ID: usize = 3;
const HIDE_TOOLBAR_ID: usize = 4;
const TAKE_SCREENSHOT_ID: usize = 5;
const COLOR_IDS_START: usize = 100;
const WIDTH_IDS_START: usize = 200;
const OPACITY_IDS_START: usize = 300;
const IDS_PER_GROUP: usize = 100;

/// Something the user asked for. Choice indices point into the lists in
/// `config` (`COLOR_CHOICES`, `WIDTH_CHOICES`, `OPACITY_CHOICES`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    PickColor(usize),
    PickWidth(usize),
    PickOpacity(usize),
    ClearDrawing,
    /// From the tray menu: show the toolbar if hidden, hide it if shown.
    ToggleToolbar,
    /// From the toolbar's own hide button. Tells the user where it went.
    HideToolbar,
    /// Capture the screen (drawings included), let the user crop it, save.
    TakeScreenshot,
    Quit,
}

impl Command {
    /// The number that stands for this command.
    pub fn id(self) -> usize {
        match self {
            Command::PickColor(index) => COLOR_IDS_START + index,
            Command::PickWidth(index) => WIDTH_IDS_START + index,
            Command::PickOpacity(index) => OPACITY_IDS_START + index,
            Command::ClearDrawing => CLEAR_DRAWING_ID,
            Command::ToggleToolbar => TOGGLE_TOOLBAR_ID,
            Command::HideToolbar => HIDE_TOOLBAR_ID,
            Command::TakeScreenshot => TAKE_SCREENSHOT_ID,
            Command::Quit => QUIT_ID,
        }
    }

    /// The pen after applying this pick to `pen`, or `None` if this
    /// command does not change the pen (or the choice does not exist).
    pub fn pen_after(self, pen: PenSettings) -> Option<PenSettings> {
        match self {
            Command::PickColor(index) => COLOR_CHOICES
                .get(index)
                .map(|&(_, color_rgb)| PenSettings { color_rgb, ..pen }),
            Command::PickWidth(index) => WIDTH_CHOICES
                .get(index)
                .map(|&(_, width_px)| PenSettings { width_px, ..pen }),
            Command::PickOpacity(index) => OPACITY_CHOICES
                .get(index)
                .map(|&(_, opacity)| PenSettings { opacity, ..pen }),
            Command::ClearDrawing
            | Command::ToggleToolbar
            | Command::HideToolbar
            | Command::TakeScreenshot
            | Command::Quit => None,
        }
    }

    /// The command for `id`, or `None` if it is 0, unknown, or points past
    /// the end of a choice list.
    pub fn from_id(id: usize) -> Option<Self> {
        let fixed_command = match id {
            CLEAR_DRAWING_ID => Some(Command::ClearDrawing),
            QUIT_ID => Some(Command::Quit),
            TOGGLE_TOOLBAR_ID => Some(Command::ToggleToolbar),
            HIDE_TOOLBAR_ID => Some(Command::HideToolbar),
            TAKE_SCREENSHOT_ID => Some(Command::TakeScreenshot),
            _ => None,
        };
        fixed_command
            .or_else(|| {
                choice_in_group(id, COLOR_IDS_START, COLOR_CHOICES.len()).map(Command::PickColor)
            })
            .or_else(|| {
                choice_in_group(id, WIDTH_IDS_START, WIDTH_CHOICES.len()).map(Command::PickWidth)
            })
            .or_else(|| {
                choice_in_group(id, OPACITY_IDS_START, OPACITY_CHOICES.len())
                    .map(Command::PickOpacity)
            })
    }
}

/// The choice index `id` stands for in the group starting at `group_start`,
/// if it is one of that group's `choice_count` choices.
fn choice_in_group(id: usize, group_start: usize, choice_count: usize) -> Option<usize> {
    let index = id.checked_sub(group_start)?;
    (index < choice_count.min(IDS_PER_GROUP)).then_some(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_command() -> Vec<Command> {
        let mut commands = vec![
            Command::ClearDrawing,
            Command::ToggleToolbar,
            Command::HideToolbar,
            Command::TakeScreenshot,
            Command::Quit,
        ];
        commands.extend((0..COLOR_CHOICES.len()).map(Command::PickColor));
        commands.extend((0..WIDTH_CHOICES.len()).map(Command::PickWidth));
        commands.extend((0..OPACITY_CHOICES.len()).map(Command::PickOpacity));
        commands
    }

    #[test]
    fn every_command_survives_id_round_trip() {
        for command in every_command() {
            assert_eq!(Command::from_id(command.id()), Some(command));
        }
    }

    #[test]
    fn ids_are_unique_and_never_zero() {
        let mut ids: Vec<usize> = every_command().into_iter().map(Command::id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count);
        assert!(!ids.contains(&0));
    }

    #[test]
    fn nothing_picked_gives_no_command() {
        assert_eq!(Command::from_id(0), None);
    }

    #[test]
    fn ids_past_last_choice_are_ignored() {
        let past_last_colour = Command::PickColor(COLOR_CHOICES.len() - 1).id() + 1;
        assert_eq!(Command::from_id(past_last_colour), None);
        assert_eq!(Command::from_id(99), None);
    }

    #[test]
    fn picking_a_colour_changes_only_the_colour() {
        let pen = crate::config::DEFAULT_PEN;
        let picked = Command::PickColor(3)
            .pen_after(pen)
            .expect("colour 3 exists");
        assert_eq!(picked.color_rgb, COLOR_CHOICES[3].1);
        assert_eq!(
            (picked.width_px, picked.opacity),
            (pen.width_px, pen.opacity)
        );
    }

    #[test]
    fn picking_width_and_opacity_changes_them() {
        let pen = crate::config::DEFAULT_PEN;
        let thicker = Command::PickWidth(4)
            .pen_after(pen)
            .expect("width 4 exists");
        assert_eq!(thicker.width_px, WIDTH_CHOICES[4].1);
        let fainter = Command::PickOpacity(3)
            .pen_after(pen)
            .expect("opacity 3 exists");
        assert_eq!(fainter.opacity, OPACITY_CHOICES[3].1);
    }

    #[test]
    fn non_pen_commands_and_missing_choices_leave_pen_alone() {
        let pen = crate::config::DEFAULT_PEN;
        assert_eq!(Command::ClearDrawing.pen_after(pen), None);
        assert_eq!(Command::PickColor(COLOR_CHOICES.len()).pen_after(pen), None);
    }

    #[test]
    fn choice_groups_fit_in_their_id_ranges() {
        assert!(COLOR_CHOICES.len() <= IDS_PER_GROUP);
        assert!(WIDTH_CHOICES.len() <= IDS_PER_GROUP);
        assert!(OPACITY_CHOICES.len() <= IDS_PER_GROUP);
    }
}
