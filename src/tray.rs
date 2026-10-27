//! The notification-area (tray) icon and its settings menu.
//!
//! Clicking the icon opens a menu to pick the pen colour, thickness and
//! opacity, show or hide the toolbar, clear the drawing, or quit. It can
//! also say where the icon is, so a callout can point at it. This module
//! only reports which
//! [`Command`] was picked; applying it is the overlay's job. It never draws
//! strokes or touches overlay state.

use std::cell::Cell;

use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, NOTIFYICONIDENTIFIER,
    Shell_NotifyIconGetRect, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, GetSystemMetrics, HICON,
    HMENU, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW, MENU_ITEM_FLAGS, MF_CHECKED, MF_POPUP,
    MF_SEPARATOR, MF_STRING, MF_UNCHECKED, PostMessageW, RegisterWindowMessageW, SM_CXSMICON,
    SM_CYSMICON, SetForegroundWindow, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
    WM_APP, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP,
};
use windows::core::{HSTRING, PCWSTR, Result, w};

use crate::app::command::Command;
use crate::notices::placement::PixelRect;
use crate::settings::config::{COLOR_CHOICES, OPACITY_CHOICES, PenSettings, WIDTH_CHOICES};

/// Posted to the overlay when the user interacts with the tray icon.
/// The low word of `lparam` is the mouse message, e.g. `WM_LBUTTONUP`.
pub const WM_TRAY_ICON: u32 = WM_APP + 2;

/// Identifies our icon among the overlay window's tray icons (it has one).
const TRAY_ICON_ID: u32 = 1;

const TOOLTIP: &str = "Chalkberry: hold Ctrl and drag to draw";

/// The app icon's id in `assets/chalkberry.rc`, built into the .exe by
/// `build.rs`.
const APP_ICON_RESOURCE_ID: usize = 1;

thread_local! {
    /// Explorer broadcasts this message when the taskbar is (re)created,
    /// e.g. after Explorer crashes. Every tray icon must then be re-added.
    static TASKBAR_CREATED_MESSAGE: Cell<u32> = const { Cell::new(0) };
}

/// Keeps the tray icon in the notification area for as long as it is alive.
pub struct TrayIcon {
    window: HWND,
    icon: HICON,
}

impl TrayIcon {
    /// Prepares a tray icon whose clicks are posted to `window` as
    /// [`WM_TRAY_ICON`]. Call [`TrayIcon::show`] to put it on the taskbar.
    pub fn new(window: HWND) -> Result<Self> {
        // SAFETY: plain Win32 call with a valid, null-terminated string.
        let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
        TASKBAR_CREATED_MESSAGE.set(taskbar_created);
        let icon = load_app_icon()?;
        Ok(Self { window, icon })
    }

    /// Adds the icon to the taskbar. Call again after a taskbar restart.
    pub fn show(&self) -> Result<()> {
        let icon_data = self.icon_data();
        // SAFETY: `icon_data` is fully initialised and outlives the call.
        unsafe { Shell_NotifyIconW(NIM_ADD, &icon_data).ok() }
    }

    /// Where the icon is on screen, in pixels. `None` if Windows cannot
    /// say, e.g. while the taskbar is restarting.
    pub fn screen_rect(&self) -> Option<PixelRect> {
        let identifier = NOTIFYICONIDENTIFIER {
            cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: self.window,
            uID: TRAY_ICON_ID,
            ..Default::default()
        };
        // SAFETY: `identifier` is fully initialised and outlives the call.
        let rect = unsafe { Shell_NotifyIconGetRect(&identifier) }.ok()?;
        Some(PixelRect {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        })
    }

    fn icon_data(&self) -> NOTIFYICONDATAW {
        let mut icon_data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.window,
            uID: TRAY_ICON_ID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY_ICON,
            hIcon: self.icon,
            ..Default::default()
        };
        copy_wide_text(TOOLTIP, &mut icon_data.szTip);
        icon_data
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        let icon_data = self.icon_data();
        // SAFETY: removing our own icon, then destroying the icon handle we
        // created. Both happen only here, once.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &icon_data);
            let _ = DestroyIcon(self.icon);
        }
    }
}

/// Writes `text` into a fixed-size, zero-terminated UTF-16 buffer that is
/// all zeros. The last slot is never written, so even text that is too
/// long and gets cut off still ends in a zero.
fn copy_wide_text(text: &str, buffer: &mut [u16]) {
    let capacity = buffer.len().saturating_sub(1);
    for (slot, unit) in buffer[..capacity].iter_mut().zip(text.encode_utf16()) {
        *slot = unit;
    }
}

/// True if `message` means the taskbar was recreated and the icon must be
/// shown again.
pub fn is_taskbar_created(message: u32) -> bool {
    let taskbar_created = TASKBAR_CREATED_MESSAGE.get();
    taskbar_created != 0 && message == taskbar_created
}

/// True if a [`WM_TRAY_ICON`] event is a click that should open the menu.
pub fn is_menu_click(lparam: LPARAM) -> bool {
    let mouse_message = (lparam.0 & 0xFFFF) as u32;
    matches!(mouse_message, WM_LBUTTONUP | WM_RBUTTONUP)
}

/// Shows the settings menu at the mouse cursor and waits until it closes.
/// Checkmarks show the current `pen` and whether the toolbar is visible.
/// Returns what was picked, if anything.
///
/// The menu runs its own message loop, so callers must not hold any state
/// borrowed while calling this.
pub fn show_menu(window: HWND, pen: &PenSettings, is_toolbar_visible: bool) -> Option<Command> {
    let menu = match build_menu(pen, is_toolbar_visible) {
        Ok(menu) => menu,
        Err(error) => {
            eprintln!("[chalkberry] error while building tray menu: {error}");
            return None;
        }
    };
    let mut cursor = POINT::default();
    // SAFETY: plain Win32 calls on our own valid window and menu. The menu
    // is destroyed once, after the user closed it.
    let picked_id = unsafe {
        let _ = GetCursorPos(&mut cursor);
        // Without this the menu would not close when clicking elsewhere.
        let _ = SetForegroundWindow(window);
        let picked = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            cursor.x,
            cursor.y,
            Some(0),
            window,
            None,
        );
        // Documented workaround so the menu also works on the next click.
        let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu); // also destroys the submenus
        picked.0 as usize
    };
    Command::from_id(picked_id)
}

fn build_menu(pen: &PenSettings, is_toolbar_visible: bool) -> Result<HMENU> {
    // SAFETY: plain Win32 call.
    let menu = unsafe { CreatePopupMenu()? };
    if let Err(error) = fill_menu(menu, pen, is_toolbar_visible) {
        // SAFETY: destroying the menu we just created, once.
        let _ = unsafe { DestroyMenu(menu) };
        return Err(error);
    }
    Ok(menu)
}

fn fill_menu(menu: HMENU, pen: &PenSettings, is_toolbar_visible: bool) -> Result<()> {
    append_choices_submenu(
        menu,
        "Colour",
        &COLOR_CHOICES,
        pen.color_rgb,
        Command::PickColor,
    )?;
    append_choices_submenu(
        menu,
        "Thickness",
        &WIDTH_CHOICES,
        pen.width_px,
        Command::PickWidth,
    )?;
    append_choices_submenu(
        menu,
        "Opacity",
        &OPACITY_CHOICES,
        pen.opacity,
        Command::PickOpacity,
    )?;
    append_separator(menu)?;
    append_item(
        menu,
        Command::ToggleToolbar,
        "Show toolbar",
        is_toolbar_visible,
    )?;
    append_item(
        menu,
        Command::ClearDrawing,
        "Clear drawing\tCtrl+Alt+C",
        false,
    )?;
    append_separator(menu)?;
    append_item(menu, Command::Quit, "Quit\tCtrl+Alt+Q", false)
}

fn append_separator(menu: HMENU) -> Result<()> {
    // SAFETY: plain Win32 call on a valid menu; separators have no text.
    unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()) }
}

/// Adds a submenu titled `title` with one item per choice. The choice equal
/// to `current` gets a checkmark.
fn append_choices_submenu<T: Copy + PartialEq>(
    menu: HMENU,
    title: &str,
    choices: &[(&str, T)],
    current: T,
    command_for_choice: fn(usize) -> Command,
) -> Result<()> {
    // SAFETY: plain Win32 call.
    let submenu = unsafe { CreatePopupMenu()? };
    // SAFETY: plain Win32 call on valid menus; the title is copied. Once
    // attached, destroying `menu` also destroys the submenu.
    let attached =
        unsafe { AppendMenuW(menu, MF_POPUP, submenu.0 as usize, &HSTRING::from(title)) };
    if let Err(error) = attached {
        // SAFETY: the submenu is not attached anywhere, so we still own it.
        let _ = unsafe { DestroyMenu(submenu) };
        return Err(error);
    }
    for (index, &(label, value)) in choices.iter().enumerate() {
        append_item(submenu, command_for_choice(index), label, value == current)?;
    }
    Ok(())
}

fn append_item(menu: HMENU, command: Command, label: &str, is_checked: bool) -> Result<()> {
    let check_flag: MENU_ITEM_FLAGS = if is_checked { MF_CHECKED } else { MF_UNCHECKED };
    let label = HSTRING::from(label);
    // SAFETY: plain Win32 call on a valid menu; the label is copied.
    unsafe { AppendMenuW(menu, MF_STRING | check_flag, command.id(), &label) }
}

/// Loads the app icon built into the .exe, at the size Windows uses for
/// small icons on this screen (picked from the sizes inside the .ico).
fn load_app_icon() -> Result<HICON> {
    // Windows names built-in resources by number by passing the number
    // where a string pointer would go (MAKEINTRESOURCE).
    let icon_name = PCWSTR(std::ptr::without_provenance(APP_ICON_RESOURCE_ID));
    // SAFETY: plain Win32 calls. The module handle is our own .exe, which
    // stays loaded; the returned icon is destroyed in `TrayIcon::drop`.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let (width, height) = (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON));
        let icon = LoadImageW(
            Some(instance.into()),
            icon_name,
            IMAGE_ICON,
            width,
            height,
            LR_DEFAULTCOLOR,
        )?;
        Ok(HICON(icon.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_text_is_cut_off_but_stays_zero_terminated() {
        let mut buffer = [0u16; 4];
        copy_wide_text("abcdef", &mut buffer);
        assert_eq!(
            buffer,
            [u16::from(b'a'), u16::from(b'b'), u16::from(b'c'), 0]
        );
    }

    #[test]
    fn short_text_is_copied_whole() {
        let mut buffer = [0u16; 8];
        copy_wide_text("hi", &mut buffer);
        assert_eq!(&buffer[..3], &[u16::from(b'h'), u16::from(b'i'), 0]);
    }
}
