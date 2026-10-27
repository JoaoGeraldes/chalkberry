//! A soft, coloured glow that pulses around the toolbar a few times when
//! the app starts, so it is easy to spot even among dark windows.
//!
//! The glow is a `composition_window` placed just below the toolbar: it is
//! click-through and only draws while pulsing (then it hides, costing
//! nothing). It knows nothing about the toolbar beyond its rectangle.

use std::cell::RefCell;
use std::f32::consts::TAU;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_PRIMITIVE_BLEND_COPY, D2D1_PRIMITIVE_BLEND_SOURCE_OVER, D2D1_ROUNDED_RECT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, KillTimer, MA_NOACTIVATE, SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetTimer,
    SetWindowPos, ShowWindow, WM_DESTROY, WM_MOUSEACTIVATE, WM_TIMER,
};
use windows::core::{Result, w};

use crate::callout_placement::PixelRect;
use crate::composition_window::{self, CompositionSurface};

/// Windows' reference DPI: at 96 DPI one DIP is one pixel.
const BASE_DPI: f32 = 96.0;

/// How far the glow reaches out from the toolbar's edge, in DIPs.
const GLOW_SPREAD: f32 = 16.0;
/// The glow is drawn as this many stacked, see-through rounded rectangles,
/// which blend into a soft fade from the toolbar's edge outwards.
const GLOW_LAYERS: u16 = 14;
/// Opacity of each layer at the brightest moment of a pulse.
const LAYER_OPACITY: f32 = 0.09;
/// Corner rounding of the toolbar (Windows 11 "round" corners).
const TOOLBAR_CORNER_RADIUS: f32 = 8.0;

const PULSE_COUNT: f32 = 3.0;
const GLOW_DURATION: Duration = Duration::from_millis(2400);
/// About 60 frames per second while pulsing.
const FRAME_MS: u32 = 16;
const FRAME_TIMER_ID: usize = 1;

const CLEAR: D2D1_COLOR_F = D2D1_COLOR_F {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

// The window procedure is a plain callback, so the glow's state lives here
// (see the same pattern in `overlay`).
thread_local! {
    static GLOW_STATE: RefCell<Option<GlowState>> = const { RefCell::new(None) };
}

/// Creates the (hidden) glow window. `owner` keeps it above the overlay.
pub fn create(owner: HWND) -> Result<()> {
    let window = composition_window::create_window(
        w!("WindowsPenMarkerGlow"),
        w!("Pen Marker glow"),
        owner,
        Some(window_proc),
    )?;
    let state = GlowState {
        window,
        surface: CompositionSurface::new(window)?,
        pulse: None,
    };
    GLOW_STATE.with(|cell| *cell.borrow_mut() = Some(state));
    Ok(())
}

/// Pulses a glow in `color_rgb` around `target` (screen pixels, on a
/// screen at `dpi`), just below the window `below`.
pub fn pulse_around(target: PixelRect, below: HWND, dpi: u32, color_rgb: [f32; 3]) {
    let started = with_state(|state| state.start(target, below, dpi, color_rgb));
    if let Some(Err(error)) = started {
        eprintln!("[pen-marker] error while starting glow: {error}");
    }
}

/// How bright the glow is `elapsed` after it started, from 0.0 to 1.0, or
/// `None` once it is over. Brightness rises and falls smoothly
/// [`PULSE_COUNT`] times.
fn pulse_brightness(elapsed: Duration) -> Option<f32> {
    if elapsed >= GLOW_DURATION {
        return None;
    }
    let progress = elapsed.as_secs_f32() / GLOW_DURATION.as_secs_f32();
    Some(0.5 - 0.5 * (TAU * PULSE_COUNT * progress).cos())
}

/// A pulse in progress.
struct Pulse {
    started_at: Instant,
    color_rgb: [f32; 3],
    /// The toolbar's size in DIPs; the glow surrounds it.
    target_size: (f32, f32),
    /// Pixels per DIP.
    scale: f32,
}

struct GlowState {
    window: HWND,
    surface: CompositionSurface,
    pulse: Option<Pulse>,
}

impl GlowState {
    fn start(
        &mut self,
        target: PixelRect,
        below: HWND,
        dpi: u32,
        color_rgb: [f32; 3],
    ) -> Result<()> {
        let scale = dpi as f32 / BASE_DPI;
        let spread_px = (GLOW_SPREAD * scale).ceil() as i32;
        let width = target.right - target.left;
        let height = target.bottom - target.top;
        self.pulse = Some(Pulse {
            started_at: Instant::now(),
            color_rgb,
            target_size: (width as f32 / scale, height as f32 / scale),
            scale,
        });
        self.draw()?;
        // SAFETY: plain Win32 calls on our own valid window. Inserting
        // after `below` puts the glow just under the toolbar.
        unsafe {
            SetWindowPos(
                self.window,
                Some(below),
                target.left - spread_px,
                target.top - spread_px,
                width + 2 * spread_px,
                height + 2 * spread_px,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )?;
            SetTimer(Some(self.window), FRAME_TIMER_ID, FRAME_MS, None);
        }
        Ok(())
    }

    fn on_frame(&mut self) {
        let brightness = self
            .pulse
            .as_ref()
            .and_then(|pulse| pulse_brightness(pulse.started_at.elapsed()));
        if brightness.is_none() {
            return self.stop();
        }
        if let Err(error) = self.draw() {
            eprintln!("[pen-marker] error while drawing glow: {error}");
            self.stop();
        }
    }

    fn stop(&mut self) {
        self.pulse = None;
        // SAFETY: plain Win32 calls on our own valid window.
        unsafe {
            let _ = KillTimer(Some(self.window), FRAME_TIMER_ID);
            let _ = ShowWindow(self.window, SW_HIDE);
        }
    }

    fn draw(&self) -> Result<()> {
        let Some(pulse) = &self.pulse else {
            return Ok(());
        };
        let brightness = pulse_brightness(pulse.started_at.elapsed()).unwrap_or(0.0);
        self.surface
            .draw_frame(pulse.scale, |surface| draw_glow(surface, pulse, brightness))
    }
}

/// Draws the glow in DIPs. Must be called inside `draw_frame`.
fn draw_glow(surface: &CompositionSurface, pulse: &Pulse, brightness: f32) {
    let [red, green, blue] = pulse.color_rgb;
    let color = D2D1_COLOR_F {
        r: red,
        g: green,
        b: blue,
        a: 1.0,
    };
    let (width, height) = pulse.target_size;
    // SAFETY: plain COM calls inside `draw_frame` (between BeginDraw and
    // EndDraw).
    unsafe {
        surface.brush.SetColor(&color);
        surface.brush.SetOpacity(LAYER_OPACITY * brightness);
        // Outermost (largest) first: every layer adds to the ones under
        // it, so the glow is strongest right at the toolbar's edge.
        for layer in 0..GLOW_LAYERS {
            let reach = GLOW_SPREAD * f32::from(GLOW_LAYERS - layer) / f32::from(GLOW_LAYERS);
            let rect = rounded_around(width, height, reach);
            surface.context.FillRoundedRectangle(&rect, &surface.brush);
        }
        // Punch out the toolbar's own area, so the slightly see-through
        // toolbar on top is not tinted by the glow.
        surface.context.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);
        surface.brush.SetColor(&CLEAR);
        surface.brush.SetOpacity(1.0);
        surface
            .context
            .FillRoundedRectangle(&rounded_around(width, height, 0.0), &surface.brush);
        surface
            .context
            .SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
    }
}

/// The toolbar's outline grown by `reach` DIPs on every side, in window
/// coordinates (the toolbar starts at `GLOW_SPREAD`, `GLOW_SPREAD`).
fn rounded_around(width: f32, height: f32, reach: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: GLOW_SPREAD - reach,
            top: GLOW_SPREAD - reach,
            right: GLOW_SPREAD + width + reach,
            bottom: GLOW_SPREAD + height + reach,
        },
        radiusX: TOOLBAR_CORNER_RADIUS + reach,
        radiusY: TOOLBAR_CORNER_RADIUS + reach,
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TIMER if wparam.0 == FRAME_TIMER_ID => {
            with_state(GlowState::on_frame);
            LRESULT(0)
        }
        // The glow must never activate or steal focus.
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_DESTROY => {
            GLOW_STATE.with(|cell| cell.borrow_mut().take());
            LRESULT(0)
        }
        // SAFETY: forwarding the unchanged arguments to the default handler.
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

/// Runs `action` on the glow state. Returns `None` if it does not exist,
/// or is already in use further up the call stack (re-entrancy).
fn with_state<T>(action: impl FnOnce(&mut GlowState) -> T) -> Option<T> {
    GLOW_STATE.with(|cell| {
        let mut state = cell.try_borrow_mut().ok()?;
        state.as_mut().map(action)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glow_starts_dark_so_it_fades_in() {
        assert_eq!(pulse_brightness(Duration::ZERO), Some(0.0));
    }

    #[test]
    fn glow_peaks_in_middle_of_first_pulse() {
        let half_pulse = GLOW_DURATION.div_f32(PULSE_COUNT * 2.0);
        let peak = pulse_brightness(half_pulse).expect("still pulsing");
        assert!(peak > 0.99, "peak was {peak}");
    }

    #[test]
    fn glow_dims_between_pulses() {
        let one_pulse = GLOW_DURATION.div_f32(PULSE_COUNT);
        let between = pulse_brightness(one_pulse).expect("still pulsing");
        assert!(between < 0.01, "between pulses was {between}");
    }

    #[test]
    fn glow_ends_after_its_duration() {
        assert_eq!(pulse_brightness(GLOW_DURATION), None);
    }
}
