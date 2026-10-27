//! The full-screen, always-on-top, transparent overlay window and its
//! message handling.
//!
//! The overlay is click-through by default. While Ctrl is held (or a stroke
//! is in progress) it captures the mouse so the user can draw. This module
//! wires input to strokes and strokes to the renderer; the drawing itself
//! lives in `renderer`, and the stroke maths in `stroke`.

use std::cell::RefCell;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{InvalidateRect, ValidateRect};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey, ReleaseCapture,
    SetCapture, VK_CONTROL,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWL_EXSTYLE, GetSystemMetrics,
    GetWindowLongPtrW, IDC_CROSS, LWA_ALPHA, LoadCursorW, MA_NOACTIVATE, PostQuitMessage,
    RegisterClassW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    SW_SHOWNOACTIVATE, SetLayeredWindowAttributes, SetWindowLongPtrW, ShowWindow,
    WM_CAPTURECHANGED, WM_DESTROY, WM_HOTKEY, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_PAINT, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{Result, w};

use crate::config::{CLEAR_HOTKEY_LETTER, QUIT_HOTKEY_LETTER};
use crate::input::WM_CTRL_STATE_CHANGED;
use crate::renderer::Renderer;
use crate::stroke::{Point, Stroke};

const CLEAR_HOTKEY_ID: i32 = 1;
const QUIT_HOTKEY_ID: i32 = 2;

// The window procedure is a plain callback, so the overlay's state lives
// here. `RefCell` turns accidental re-entrant access (Windows sometimes
// calls the window procedure from inside our own API calls) into a safe
// "busy, use default handling" instead of undefined behaviour.
thread_local! {
    static OVERLAY_STATE: RefCell<Option<OverlayState>> = const { RefCell::new(None) };
}

/// Creates the overlay covering every monitor, shows it, and registers the
/// clear/quit hotkeys. Returns the window so other modules can target it.
pub fn create_overlay() -> Result<HWND> {
    let window = create_overlay_window()?;
    let (width, height) = virtual_screen_size();
    let renderer = Renderer::new(window, width, height)?;
    OVERLAY_STATE.with(|cell| {
        *cell.borrow_mut() = Some(OverlayState {
            window,
            renderer,
            active_stroke: None,
            is_ctrl_held: false,
        })
    });
    register_hotkeys(window)?;
    // SAFETY: `window` is a valid window we just created.
    unsafe {
        let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
    }
    Ok(window)
}

fn create_overlay_window() -> Result<HWND> {
    let class_name = w!("WindowsPenMarkerOverlay");
    let (left, top) = virtual_screen_origin();
    let (width, height) = virtual_screen_size();

    // SAFETY: plain Win32 calls; `window_proc` matches WNDPROC and lives
    // for the whole program.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            // Shown only while Ctrl is held, as a hint that you can draw.
            hCursor: LoadCursorW(None, IDC_CROSS)?,
            ..Default::default()
        };
        if RegisterClassW(&window_class) == 0 {
            return Err(windows::core::Error::from_thread());
        }

        let window = CreateWindowExW(
            // NOREDIRECTIONBITMAP: content comes only from DirectComposition.
            // LAYERED + TRANSPARENT: mouse clicks pass through to apps below.
            // TOOLWINDOW: hidden from the taskbar and Alt+Tab.
            // NOACTIVATE: never steals keyboard focus.
            WS_EX_NOREDIRECTIONBITMAP
                | WS_EX_LAYERED
                | WS_EX_TRANSPARENT
                | WS_EX_TOPMOST
                | WS_EX_TOOLWINDOW
                | WS_EX_NOACTIVATE,
            class_name,
            w!("Pen Marker"),
            WS_POPUP,
            left,
            top,
            width as i32,
            height as i32,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        // A layered window stays invisible until its attributes are set.
        // Full alpha here; real transparency comes from the swap chain.
        SetLayeredWindowAttributes(window, COLORREF(0), 255, LWA_ALPHA)?;
        Ok(window)
    }
}

fn register_hotkeys(window: HWND) -> Result<()> {
    let modifiers = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
    // SAFETY: plain Win32 calls on a valid window.
    unsafe {
        RegisterHotKey(
            Some(window),
            CLEAR_HOTKEY_ID,
            modifiers,
            CLEAR_HOTKEY_LETTER as u32,
        )?;
        RegisterHotKey(
            Some(window),
            QUIT_HOTKEY_ID,
            modifiers,
            QUIT_HOTKEY_LETTER as u32,
        )?;
    }
    Ok(())
}

/// Everything the overlay needs to remember between messages.
struct OverlayState {
    window: HWND,
    renderer: Renderer,
    /// The stroke being drawn right now, from mouse down to mouse up.
    active_stroke: Option<Stroke>,
    is_ctrl_held: bool,
}

impl OverlayState {
    fn on_ctrl_state_changed(&mut self, is_ctrl_held: bool) {
        self.is_ctrl_held = is_ctrl_held;
        self.update_click_through();
    }

    fn on_mouse_down(&mut self, point: Point) {
        // Safety net: if a Ctrl key-up was missed (e.g. after the lock
        // screen), never block clicks meant for other apps.
        if !is_ctrl_physically_held() {
            self.on_ctrl_state_changed(false);
            return;
        }
        self.active_stroke = Some(Stroke::begin(point));
        // SAFETY: plain Win32 call on our own valid window. Capture keeps
        // mouse messages coming even if the cursor leaves the overlay.
        unsafe { SetCapture(self.window) };
        self.request_redraw();
    }

    fn on_mouse_move(&mut self, point: Point) {
        match &mut self.active_stroke {
            Some(stroke) => {
                stroke.add_point(point);
                self.request_redraw();
            }
            None if !is_ctrl_physically_held() => self.on_ctrl_state_changed(false),
            None => {}
        }
    }

    fn on_mouse_up(&mut self) {
        self.finish_stroke();
        // SAFETY: plain Win32 call. It re-enters `window_proc` with
        // WM_CAPTURECHANGED, which then sees the state as busy and is ignored.
        let _ = unsafe { ReleaseCapture() };
    }

    /// Moves the active stroke into the permanent drawing.
    fn finish_stroke(&mut self) {
        if let Some(stroke) = self.active_stroke.take() {
            if let Err(error) = self.renderer.add_finished_stroke(&stroke.shape()) {
                log_error("saving finished stroke", &error);
            }
            self.request_redraw();
        }
        self.update_click_through();
    }

    fn clear_drawing(&mut self) {
        self.active_stroke = None;
        if let Err(error) = self.renderer.clear_finished_strokes() {
            log_error("clearing drawing", &error);
        }
        self.request_redraw();
    }

    fn paint(&self) {
        let active_shape = self.active_stroke.as_ref().map(Stroke::shape);
        if let Err(error) = self.renderer.draw_frame(active_shape.as_ref()) {
            log_error("drawing frame", &error);
        }
    }

    /// The overlay takes the mouse only while Ctrl is held or a stroke is
    /// still in progress (so releasing Ctrl mid-drag doesn't cut the line).
    fn update_click_through(&self) {
        let wants_mouse = self.is_ctrl_held || self.active_stroke.is_some();
        set_click_through(self.window, !wants_mouse);
    }

    /// Asks Windows for a WM_PAINT. Windows merges repeated requests, so
    /// many mouse moves between frames still cost only one redraw.
    fn request_redraw(&self) {
        // SAFETY: plain Win32 call on our own valid window.
        let _ = unsafe { InvalidateRect(Some(self.window), None, false) };
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let handled = match message {
        WM_CTRL_STATE_CHANGED => with_state(|state| state.on_ctrl_state_changed(wparam.0 != 0)),
        WM_LBUTTONDOWN => with_state(|state| state.on_mouse_down(point_from_lparam(lparam))),
        WM_MOUSEMOVE => with_state(|state| state.on_mouse_move(point_from_lparam(lparam))),
        WM_LBUTTONUP => with_state(OverlayState::on_mouse_up),
        // Another window took the mouse mid-stroke: keep what was drawn.
        WM_CAPTURECHANGED => with_state(OverlayState::finish_stroke),
        WM_PAINT => {
            let painted = with_state(|state| state.paint());
            // SAFETY: plain Win32 call; marks the window as painted so
            // Windows stops sending WM_PAINT.
            let _ = unsafe { ValidateRect(Some(window), None) };
            painted
        }
        WM_HOTKEY => with_state(|state| match wparam.0 as i32 {
            CLEAR_HOTKEY_ID => state.clear_drawing(),
            QUIT_HOTKEY_ID => {
                // SAFETY: destroying our own window; WM_DESTROY follows.
                let _ = unsafe { DestroyWindow(state.window) };
            }
            _ => {}
        }),
        // Clicking the overlay must never activate it or steal focus.
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_DESTROY => {
            OVERLAY_STATE.with(|cell| cell.borrow_mut().take());
            // SAFETY: plain Win32 call; ends the message loop.
            unsafe { PostQuitMessage(0) };
            Some(())
        }
        _ => None,
    };
    match handled {
        Some(()) => LRESULT(0),
        // SAFETY: forwarding the unchanged arguments to the default handler.
        None => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

/// Runs `action` on the overlay state. Returns `None` if the state does not
/// exist yet, or is already in use further up the call stack (re-entrancy).
fn with_state(action: impl FnOnce(&mut OverlayState)) -> Option<()> {
    OVERLAY_STATE.with(|cell| {
        let mut state = cell.try_borrow_mut().ok()?;
        state.as_mut().map(action)
    })
}

fn set_click_through(window: HWND, is_click_through: bool) {
    let click_through_flag = WS_EX_TRANSPARENT.0 as isize;
    // SAFETY: reading and writing the extended style of our own window.
    unsafe {
        let current_style = GetWindowLongPtrW(window, GWL_EXSTYLE);
        let new_style = if is_click_through {
            current_style | click_through_flag
        } else {
            current_style & !click_through_flag
        };
        if new_style != current_style {
            SetWindowLongPtrW(window, GWL_EXSTYLE, new_style);
        }
    }
}

fn is_ctrl_physically_held() -> bool {
    // SAFETY: plain Win32 call with no pointers.
    let key_state = unsafe { GetAsyncKeyState(i32::from(VK_CONTROL.0)) };
    // The most significant bit is set while the key is down.
    key_state < 0
}

/// Mouse messages pack client coordinates into `lparam` as two signed
/// 16-bit values (low word = x, high word = y).
fn point_from_lparam(lparam: LPARAM) -> Point {
    let x = (lparam.0 & 0xFFFF) as u16 as i16;
    let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16;
    Point::new(f32::from(x), f32::from(y))
}

/// The top-left corner of the area spanning all monitors. It can be
/// negative when a monitor sits left of or above the primary one.
fn virtual_screen_origin() -> (i32, i32) {
    // SAFETY: plain Win32 calls with no pointers.
    unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
        )
    }
}

fn virtual_screen_size() -> (u32, u32) {
    // SAFETY: plain Win32 calls with no pointers.
    let (width, height) = unsafe {
        (
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    (width.max(1) as u32, height.max(1) as u32)
}

/// Errors after startup are not fatal: report them and keep running.
/// Visible in the console of debug builds.
fn log_error(context: &str, error: &windows::core::Error) {
    eprintln!("[pen-marker] error while {context}: {error}");
}
