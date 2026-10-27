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
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, NOTIFYICONIDENTIFIER,
    Shell_NotifyIconGetRect, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos,
    GetSystemMetrics, HICON, HMENU, ICONINFO, MENU_ITEM_FLAGS, MF_CHECKED, MF_POPUP, MF_SEPARATOR,
    MF_STRING, MF_UNCHECKED, PostMessageW, RegisterWindowMessageW, SM_CXSMICON,
    SetForegroundWindow, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP,
    WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP,
};
use windows::core::{HSTRING, PCWSTR, Result, w};

use crate::callout_placement::PixelRect;
use crate::command::Command;
use crate::config::{COLOR_CHOICES, OPACITY_CHOICES, PenSettings, WIDTH_CHOICES};

/// Posted to the overlay when the user interacts with the tray icon.
/// The low word of `lparam` is the mouse message, e.g. `WM_LBUTTONUP`.
pub const WM_TRAY_ICON: u32 = WM_APP + 2;

/// Identifies our icon among the overlay window's tray icons (it has one).
const TRAY_ICON_ID: u32 = 1;

const TOOLTIP: &str = "Pen Marker: hold Ctrl and drag to draw";

/// Icon artwork proportions, relative to the icon size.
const ICON_CORNER_RADIUS: f32 = 0.22;
const ICON_STROKE_WIDTH: f32 = 0.16;
const ICON_STROKE_START: (f32, f32) = (0.27, 0.73);
const ICON_STROKE_END: (f32, f32) = (0.73, 0.27);
const ICON_BOARD_RGB: [f32; 3] = [0.17, 0.31, 0.24];
const ICON_CHALK_RGB: [f32; 3] = [0.97, 0.97, 0.93];

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
        let icon = create_icon_image()?;
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
            eprintln!("[pen-marker] error while building tray menu: {error}");
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

/// Builds the tray icon from [`icon_pixels_bgra`], at the size Windows
/// uses for small icons on this screen.
fn create_icon_image() -> Result<HICON> {
    // SAFETY: plain Win32 call with no pointers.
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.clamp(16, 64);
    let color_pixels = icon_pixels_bgra(size as u32);
    // The 1-bit mask is unused for 32-bit icons with alpha, but required.
    // Its rows are padded to 16 bits.
    let mask_row_bytes = (size as usize).div_ceil(16) * 2;
    let mask_bits = vec![0u8; mask_row_bytes * size as usize];
    // SAFETY: both pixel buffers have exactly the size the bitmaps need and
    // outlive the calls. CreateIconIndirect copies the bitmaps, so ours are
    // deleted right after.
    unsafe {
        let color_bitmap = CreateBitmap(size, size, 1, 32, Some(color_pixels.as_ptr().cast()));
        let mask_bitmap = CreateBitmap(size, size, 1, 1, Some(mask_bits.as_ptr().cast()));
        let icon_info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask_bitmap,
            hbmColor: color_bitmap,
        };
        let icon = CreateIconIndirect(&icon_info);
        let _ = DeleteObject(color_bitmap.into());
        let _ = DeleteObject(mask_bitmap.into());
        icon
    }
}

/// Draws the app icon, a little chalkboard with a chalk stroke across it,
/// as `size` × `size` BGRA pixels with straight (not premultiplied) alpha.
fn icon_pixels_bgra(size: u32) -> Vec<u8> {
    let scale = size as f32;
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for row in 0..size {
        for column in 0..size {
            // Sample pixel centres, in 0..1 icon units.
            let x = (column as f32 + 0.5) / scale;
            let y = (row as f32 + 0.5) / scale;
            // Signed distances in pixels; coverage fades over one pixel,
            // which anti-aliases the edges.
            let board_coverage = (0.5 - rounded_square_distance(x, y) * scale).clamp(0.0, 1.0);
            let stroke_distance = segment_distance(x, y, ICON_STROKE_START, ICON_STROKE_END);
            let chalk_coverage =
                (0.5 + (ICON_STROKE_WIDTH / 2.0 - stroke_distance) * scale).clamp(0.0, 1.0);
            let [red, green, blue] = [0, 1, 2].map(|channel| {
                mix(
                    ICON_BOARD_RGB[channel],
                    ICON_CHALK_RGB[channel],
                    chalk_coverage,
                )
            });
            pixels.extend_from_slice(&[
                unit_to_byte(blue),
                unit_to_byte(green),
                unit_to_byte(red),
                unit_to_byte(board_coverage),
            ]);
        }
    }
    pixels
}

/// Signed distance from (`x`, `y`) to the edge of the icon's rounded square
/// (which fills the whole 0..1 icon). Negative inside.
fn rounded_square_distance(x: f32, y: f32) -> f32 {
    let half_inner = 0.5 - ICON_CORNER_RADIUS;
    let offset_x = ((x - 0.5).abs() - half_inner).max(0.0);
    let offset_y = ((y - 0.5).abs() - half_inner).max(0.0);
    offset_x.hypot(offset_y) - ICON_CORNER_RADIUS
}

/// Distance from (`x`, `y`) to the line segment from `start` to `end`.
fn segment_distance(x: f32, y: f32, start: (f32, f32), end: (f32, f32)) -> f32 {
    let (segment_x, segment_y) = (end.0 - start.0, end.1 - start.1);
    let along = ((x - start.0) * segment_x + (y - start.1) * segment_y)
        / (segment_x * segment_x + segment_y * segment_y);
    let along = along.clamp(0.0, 1.0);
    let closest_x = start.0 + segment_x * along;
    let closest_y = start.1 + segment_y * along;
    (x - closest_x).hypot(y - closest_y)
}

fn mix(from: f32, to: f32, amount: f32) -> f32 {
    from + (to - from) * amount
}

fn unit_to_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_corners_are_transparent_and_centre_is_chalk() {
        let size = 32;
        let pixels = icon_pixels_bgra(size);
        let alpha_at = |x: u32, y: u32| pixels[((y * size + x) * 4 + 3) as usize];
        let red_at = |x: u32, y: u32| pixels[((y * size + x) * 4 + 2) as usize];
        assert_eq!(pixels.len(), (size * size * 4) as usize);
        assert_eq!(alpha_at(0, 0), 0);
        assert_eq!(alpha_at(size / 2, size / 2), 255);
        assert!(
            red_at(size / 2, size / 2) > 200,
            "centre should be chalk white"
        );
    }

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
