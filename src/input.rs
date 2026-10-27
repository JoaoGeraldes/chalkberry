//! Global keyboard hook that tracks whether a Ctrl key is held.
//!
//! The hook only posts [`WM_CTRL_STATE_CHANGED`] to the overlay window. It
//! must not draw or touch overlay state: Windows removes low-level hooks
//! that take too long to return.

use std::cell::Cell;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_LCONTROL, VK_RCONTROL};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, PostMessageW, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_SYSKEYDOWN,
};
use windows::core::Result;

/// Posted to the overlay when Ctrl goes down or up.
/// `wparam` is 1 when at least one Ctrl key is held, 0 when none are.
pub const WM_CTRL_STATE_CHANGED: u32 = WM_APP + 1;

const LEFT_CTRL_BIT: u8 = 0b01;
const RIGHT_CTRL_BIT: u8 = 0b10;

// Low-level hooks are called on the thread that installed them, so plain
// thread-local cells are enough. No locks or atomics needed.
thread_local! {
    static TARGET_WINDOW: Cell<HWND> = Cell::new(HWND::default());
    static HELD_CTRL_KEYS: Cell<u8> = const { Cell::new(0) };
}

/// Keeps the keyboard hook installed for as long as it is alive.
pub struct CtrlKeyHook {
    hook: HHOOK,
}

impl CtrlKeyHook {
    /// Installs the hook. Ctrl changes are posted to `target_window`.
    /// Must be called from the thread that runs the message loop.
    pub fn install(target_window: HWND) -> Result<Self> {
        TARGET_WINDOW.with(|cell| cell.set(target_window));
        // SAFETY: plain Win32 calls; `keyboard_hook_proc` matches HOOKPROC
        // and lives for the whole program.
        let hook = unsafe {
            let module = GetModuleHandleW(None)?;
            SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(keyboard_hook_proc),
                Some(module.into()),
                0,
            )?
        };
        Ok(Self { hook })
    }
}

impl Drop for CtrlKeyHook {
    fn drop(&mut self) {
        // SAFETY: `self.hook` came from a successful SetWindowsHookExW call
        // and is unhooked only here, once.
        let _ = unsafe { UnhookWindowsHookEx(self.hook) };
    }
}

unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for HC_ACTION, Windows guarantees `lparam` points to a
        // valid KBDLLHOOKSTRUCT for the duration of this call.
        let key_event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let is_key_down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        if let Some(ctrl_bit) = ctrl_key_bit(key_event.vkCode) {
            record_ctrl_key(ctrl_bit, is_key_down);
        }
    }
    // SAFETY: forwarding the unchanged arguments, as the hook contract requires.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn ctrl_key_bit(virtual_key_code: u32) -> Option<u8> {
    match virtual_key_code {
        code if code == u32::from(VK_LCONTROL.0) => Some(LEFT_CTRL_BIT),
        code if code == u32::from(VK_RCONTROL.0) => Some(RIGHT_CTRL_BIT),
        _ => None,
    }
}

/// Updates the held-keys set and notifies the overlay only when
/// "is any Ctrl held" actually changes (key repeat is ignored).
fn record_ctrl_key(ctrl_bit: u8, is_key_down: bool) {
    let previous_keys = HELD_CTRL_KEYS.get();
    let current_keys = if is_key_down {
        previous_keys | ctrl_bit
    } else {
        previous_keys & !ctrl_bit
    };
    HELD_CTRL_KEYS.set(current_keys);

    let was_held = previous_keys != 0;
    let is_held = current_keys != 0;
    if was_held != is_held {
        let target_window = TARGET_WINDOW.get();
        // SAFETY: posting a message never blocks or touches our memory.
        // If it fails the overlay simply stays in its current mode.
        let _ = unsafe {
            PostMessageW(
                Some(target_window),
                WM_CTRL_STATE_CHANGED,
                WPARAM(usize::from(is_held)),
                LPARAM(0),
            )
        };
    }
}
