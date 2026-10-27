//! Loads the Chalkberry icon that `build.rs` builds into the .exe (from
//! `assets/chalkberry.rc`), for the tray and the toolbar's logo.
//!
//! Only loads; the caller owns the returned icon and must destroy it.

use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{HICON, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW};
use windows::core::{PCWSTR, Result};

/// The app icon's id in `assets/chalkberry.rc`.
const APP_ICON_RESOURCE_ID: usize = 1;

/// The app icon at `size_px` × `size_px` pixels. Windows picks the closest
/// size inside the .ico and scales it if needed. Release it with
/// `DestroyIcon`.
pub fn load_app_icon(size_px: i32) -> Result<HICON> {
    // Windows names built-in resources by number by passing the number
    // where a string pointer would go (MAKEINTRESOURCE).
    let icon_name = PCWSTR(std::ptr::without_provenance(APP_ICON_RESOURCE_ID));
    // SAFETY: plain Win32 calls. The module handle is our own .exe, which
    // stays loaded for the whole program.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let icon = LoadImageW(
            Some(instance.into()),
            icon_name,
            IMAGE_ICON,
            size_px,
            size_px,
            LR_DEFAULTCOLOR,
        )?;
        Ok(HICON(icon.0))
    }
}
