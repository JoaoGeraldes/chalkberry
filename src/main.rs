//! Chalkberry: hold Ctrl and click/drag to draw on top of the screen.
//!
//! This file only wires the pieces together. See `CLAUDE.md` for the
//! module map and code rules.

// Release builds run without a console window. Debug builds keep it so
// `eprintln!` messages are visible while developing.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod drawing;
mod notices;
mod screenshot;
mod settings;
mod toolbar;
mod tray;

use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MB_ICONERROR, MB_OK, MSG, MessageBoxW, TranslateMessage,
};
use windows::core::{HSTRING, Result, w};

fn main() {
    if let Err(error) = run() {
        show_startup_error(&error);
    }
}

fn run() -> Result<()> {
    enable_per_monitor_dpi_awareness();
    // COM is needed for saving screenshots as PNG and for the folder picker.
    // SAFETY: plain call, once, on the UI thread, before any COM use.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
    let overlay_window = app::overlay::create_overlay()?;
    // Kept alive until the message loop ends; dropping it removes the hook.
    let _ctrl_key_hook = drawing::input::CtrlKeyHook::install(overlay_window)?;
    run_message_loop();
    Ok(())
}

/// Makes Windows report real pixel coordinates on every monitor, so lines
/// are drawn exactly under the cursor and stay sharp at any scaling.
fn enable_per_monitor_dpi_awareness() {
    // SAFETY: plain Win32 call with a predefined constant. It only fails if
    // DPI awareness was already set (e.g. by a manifest), which is fine.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

/// Dispatches window messages until the overlay posts WM_QUIT.
fn run_message_loop() {
    let mut message = MSG::default();
    // SAFETY: `message` is a valid MSG for every call.
    unsafe {
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

fn show_startup_error(error: &windows::core::Error) {
    let text = HSTRING::from(format!("Chalkberry could not start:\n\n{error}"));
    // SAFETY: plain Win32 call with valid, null-terminated strings.
    unsafe {
        MessageBoxW(None, &text, w!("Chalkberry"), MB_OK | MB_ICONERROR);
    }
}
