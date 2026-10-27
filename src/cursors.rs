//! The overlay's cursors: Windows' own pen while drawing, and its
//! crosshair while erasing.
//!
//! Both are built-in system cursors, so Windows draws them sharp at any
//! display scaling and pointer size. They are shared by the system and
//! never destroyed by us.

use windows::Win32::UI::WindowsAndMessaging::{
    HCURSOR, IDC_ARROW, IDC_CROSS, LoadCursorW, SetCursor,
};
use windows::core::PCWSTR;

/// Windows' handwriting pen cursor ("Handwriting" in Mouse settings).
/// It is missing from Microsoft's official definitions, so the `windows`
/// crate has no constant for it; the resource id is 32631.
const IDC_PEN: PCWSTR = PCWSTR(32631 as *const u16);

/// The two cursors the overlay switches between.
pub struct Cursors {
    /// The handwriting pen: "you are writing now".
    drawing: HCURSOR,
    /// The crosshair: "aim at the stroke to remove".
    erasing: HCURSOR,
}

impl Cursors {
    /// Loads both cursors. Never fails: the normal arrow stands in.
    pub fn load() -> Self {
        Self {
            drawing: load_system_cursor(IDC_PEN),
            erasing: load_system_cursor(IDC_CROSS),
        }
    }

    /// Shows the eraser cursor if `is_erasing`, otherwise the drawing one.
    pub fn apply(&self, is_erasing: bool) {
        let chosen = if is_erasing {
            self.erasing
        } else {
            self.drawing
        };
        // SAFETY: plain Win32 call with a valid, shared cursor handle.
        unsafe { SetCursor(Some(chosen)) };
    }
}

fn load_system_cursor(id: PCWSTR) -> HCURSOR {
    // SAFETY: plain Win32 calls loading predefined, shared system cursors.
    unsafe {
        LoadCursorW(None, id)
            .or_else(|_| LoadCursorW(None, IDC_ARROW))
            .unwrap_or_default()
    }
}
