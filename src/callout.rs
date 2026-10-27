//! A small speech-bubble window that points at something on screen. Used
//! for the toolbar's tooltips, the "ready" hint at start-up, and the
//! "still running in the tray" notice when the toolbar is hidden.
//!
//! There is one bubble: showing a new message replaces the current one.
//! Tooltips are dark and discreet; notices are light, so they stand out
//! even on a screen full of dark windows. The bubble is a
//! `composition_window`, so it is click-through and its rounded corners
//! and arrow are truly transparent. It knows nothing about strokes, pens
//! or menus; placement maths lives in `callout_placement`.

use std::cell::RefCell;
use std::f32::consts::FRAC_1_SQRT_2;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ROUNDED_RECT};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_TEXT_METRICS, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetSystemMetrics, HWND_TOPMOST, KillTimer, MA_NOACTIVATE, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_HIDE, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SetTimer, SetWindowPos, ShowWindow, WM_DESTROY, WM_MOUSEACTIVATE, WM_TIMER,
};
use windows::core::{PCWSTR, Result, w};
use windows_numerics::{Matrix3x2, Vector2};

use crate::callout_placement::{ArrowEdge, BubbleSize, PixelRect, place_bubble};
use crate::composition_window::{self, CompositionSurface, scale_transform};

/// How long a bubble stays up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalloutKind {
    /// Stays until [`hide_tooltip`] is called (the mouse moved away).
    Tooltip,
    /// Fades out by itself after this many milliseconds.
    Notice { visible_ms: u32 },
}

/// Windows' reference DPI: at 96 DPI one DIP is one pixel.
const BASE_DPI: f32 = 96.0;

// Bubble shape, in DIPs.
const MAX_TEXT_WIDTH: f32 = 240.0;
const MAX_TEXT_HEIGHT: f32 = 200.0;
const PADDING_X: f32 = 10.0;
const PADDING_Y: f32 = 8.0;
const TITLE_BODY_GAP: f32 = 2.0;
const CORNER_RADIUS: f32 = 7.0;
const BORDER_WIDTH: f32 = 1.0;
/// How far the arrow tip sticks out of the bubble.
const ARROW_LENGTH: f32 = 7.0;
/// Space between the arrow tip and the thing it points at.
const GAP_TO_TARGET: f32 = 2.0;
/// Keeps the arrow clear of the rounded corners.
const ARROW_INSET: f32 = CORNER_RADIUS + ARROW_LENGTH + 2.0;

const FONT: PCWSTR = w!("Segoe UI");
const TITLE_FONT_SIZE: f32 = 12.0;
const BODY_FONT_SIZE: f32 = 11.5;

/// The colours of one bubble style.
struct Palette {
    bubble: D2D1_COLOR_F,
    border: D2D1_COLOR_F,
    title: D2D1_COLOR_F,
    body: D2D1_COLOR_F,
}

/// Tooltips: dark, matching the toolbar, so they stay discreet.
const DARK: Palette = Palette {
    bubble: gray(0.17),
    border: gray(0.36),
    title: gray(0.97),
    body: gray(0.74),
};

/// Notices: near-white with dark text, the strongest contrast on a
/// desktop full of dark windows. The border keeps them crisp on white.
const LIGHT: Palette = Palette {
    bubble: gray(0.98),
    border: gray(0.68),
    title: gray(0.08),
    body: gray(0.3),
};

/// Fades run in this many steps of [`FADE_STEP_MS`] (about 120 ms).
const FADE_STEPS: u32 = 8;
const FADE_STEP_MS: u32 = 15;
const FADE_TIMER_ID: usize = 1;
const NOTICE_TIMER_ID: usize = 2;

// The window procedure is a plain callback, so the bubble's state lives
// here (see the same pattern in `overlay`).
thread_local! {
    static CALLOUT_STATE: RefCell<Option<CalloutState>> = const { RefCell::new(None) };
}

/// Creates the (hidden) bubble window. `owner` keeps it above the overlay.
pub fn create(owner: HWND) -> Result<()> {
    let window = composition_window::create_window(
        w!("WindowsPenMarkerCallout"),
        w!("Pen Marker callout"),
        owner,
        Some(window_proc),
    )?;
    let state = CalloutState::new(window)?;
    CALLOUT_STATE.with(|cell| *cell.borrow_mut() = Some(state));
    Ok(())
}

/// Shows a bubble with a bold `title` and a plain `body`, pointing at
/// `target` (screen pixels), sized for a screen at `dpi`. Replaces any
/// bubble already showing.
pub fn show(title: &str, body: &str, target: PixelRect, dpi: u32, kind: CalloutKind) {
    let shown = with_state(|state| state.show(title, body, target, dpi, kind));
    if let Some(Err(error)) = shown {
        log_error("showing callout", &error);
    }
}

/// Hides the bubble right away, whatever it is showing.
pub fn hide() {
    with_state(CalloutState::hide_now);
}

/// Hides the bubble if it is a tooltip. Notices are left to fade out.
pub fn hide_tooltip() {
    with_state(|state| {
        if state.kind == CalloutKind::Tooltip {
            state.hide_now();
        }
    });
}

/// True while a tooltip (not a notice) is on screen.
pub fn is_tooltip_visible() -> bool {
    with_state(|state| state.kind == CalloutKind::Tooltip && state.fade != Fade::Hidden)
        .unwrap_or(false)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fade {
    Hidden,
    In { step: u32 },
    Shown,
    Out { step: u32 },
}

impl CalloutKind {
    fn palette(self) -> &'static Palette {
        match self {
            CalloutKind::Tooltip => &DARK,
            CalloutKind::Notice { .. } => &LIGHT,
        }
    }
}

impl Fade {
    fn opacity(self) -> f32 {
        let fraction = |step: u32| step as f32 / FADE_STEPS as f32;
        match self {
            Fade::Hidden => 0.0,
            Fade::In { step } => fraction(step),
            Fade::Shown => 1.0,
            Fade::Out { step } => 1.0 - fraction(step),
        }
    }
}

/// The current message, laid out and placed.
struct Content {
    title: IDWriteTextLayout,
    body: IDWriteTextLayout,
    title_height: f32,
    /// The bubble body (without the arrow), in DIPs.
    bubble_size: (f32, f32),
    arrow_edge: ArrowEdge,
    /// Arrow tip distance from the window's left edge, in DIPs.
    arrow_x: f32,
    /// Pixels per DIP.
    scale: f32,
}

struct CalloutState {
    window: HWND,
    surface: CompositionSurface,
    text_factory: IDWriteFactory,
    title_format: IDWriteTextFormat,
    body_format: IDWriteTextFormat,
    content: Option<Content>,
    kind: CalloutKind,
    fade: Fade,
}

impl CalloutState {
    fn new(window: HWND) -> Result<Self> {
        // SAFETY: plain DirectWrite call.
        let text_factory: IDWriteFactory =
            unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        Ok(Self {
            window,
            surface: CompositionSurface::new(window)?,
            title_format: create_text_format(
                &text_factory,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                TITLE_FONT_SIZE,
            )?,
            body_format: create_text_format(
                &text_factory,
                DWRITE_FONT_WEIGHT_NORMAL,
                BODY_FONT_SIZE,
            )?,
            text_factory,
            content: None,
            kind: CalloutKind::Tooltip,
            fade: Fade::Hidden,
        })
    }

    fn show(
        &mut self,
        title: &str,
        body: &str,
        target: PixelRect,
        dpi: u32,
        kind: CalloutKind,
    ) -> Result<()> {
        let scale = dpi as f32 / BASE_DPI;
        let (content, window_rect) = self.lay_out(title, body, target, scale)?;
        self.stop_timers();
        // Going straight from one bubble to the next (e.g. moving between
        // toolbar buttons) skips the fade-in, so it feels instant.
        let was_visible = self.fade != Fade::Hidden;
        self.fade = if was_visible {
            Fade::Shown
        } else {
            Fade::In { step: 0 }
        };
        self.kind = kind;
        self.content = Some(content);
        // Draw before showing, so the old message never flashes.
        self.draw()?;
        // SAFETY: plain Win32 call on our own valid window.
        unsafe {
            SetWindowPos(
                self.window,
                Some(HWND_TOPMOST),
                window_rect.left,
                window_rect.top,
                window_rect.right - window_rect.left,
                window_rect.bottom - window_rect.top,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )?;
        }
        if was_visible {
            self.start_notice_timer();
        } else {
            self.start_timer(FADE_TIMER_ID, FADE_STEP_MS);
        }
        Ok(())
    }

    /// Lays out the text, sizes the bubble, and decides where it goes.
    /// Returns the content and the window's screen rectangle.
    fn lay_out(
        &self,
        title: &str,
        body: &str,
        target: PixelRect,
        scale: f32,
    ) -> Result<(Content, PixelRect)> {
        let title_layout = self.text_layout(title, &self.title_format)?;
        let body_layout = self.text_layout(body, &self.body_format)?;
        let (title_width, title_height) = text_size(&title_layout)?;
        let (body_width, body_height) = text_size(&body_layout)?;
        let bubble_size = (
            title_width.max(body_width) + 2.0 * PADDING_X,
            title_height + TITLE_BODY_GAP + body_height + 2.0 * PADDING_Y,
        );
        let to_px = |dips: f32| (dips * scale).ceil() as i32;
        let size = BubbleSize {
            width: to_px(bubble_size.0),
            height: to_px(bubble_size.1 + ARROW_LENGTH),
            gap: to_px(GAP_TO_TARGET),
            arrow_inset: to_px(ARROW_INSET),
        };
        let placement = place_bubble(target, size, virtual_screen());
        let window_rect = PixelRect {
            left: placement.left,
            top: placement.top,
            right: placement.left + size.width,
            bottom: placement.top + size.height,
        };
        let content = Content {
            title: title_layout,
            body: body_layout,
            title_height,
            bubble_size,
            arrow_edge: placement.arrow_edge,
            arrow_x: placement.arrow_x as f32 / scale,
            scale,
        };
        Ok((content, window_rect))
    }

    fn text_layout(&self, text: &str, format: &IDWriteTextFormat) -> Result<IDWriteTextLayout> {
        let wide_text: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: plain DirectWrite call; `wide_text` outlives it (the
        // layout keeps its own copy).
        unsafe {
            self.text_factory
                .CreateTextLayout(&wide_text, format, MAX_TEXT_WIDTH, MAX_TEXT_HEIGHT)
        }
    }

    fn on_timer(&mut self, timer_id: usize) {
        match (timer_id, self.fade) {
            (FADE_TIMER_ID, Fade::In { step }) if step + 1 >= FADE_STEPS => {
                self.fade = Fade::Shown;
                self.stop_timer(FADE_TIMER_ID);
                self.start_notice_timer();
            }
            (FADE_TIMER_ID, Fade::In { step }) => self.fade = Fade::In { step: step + 1 },
            (FADE_TIMER_ID, Fade::Out { step }) if step + 1 >= FADE_STEPS => {
                return self.hide_now();
            }
            (FADE_TIMER_ID, Fade::Out { step }) => self.fade = Fade::Out { step: step + 1 },
            (NOTICE_TIMER_ID, Fade::Shown) => {
                self.stop_timer(NOTICE_TIMER_ID);
                self.fade = Fade::Out { step: 0 };
                self.start_timer(FADE_TIMER_ID, FADE_STEP_MS);
            }
            _ => return,
        }
        if let Err(error) = self.draw() {
            log_error("animating callout", &error);
        }
    }

    /// Notices fade out by themselves after a while; tooltips do not.
    fn start_notice_timer(&self) {
        if let CalloutKind::Notice { visible_ms } = self.kind {
            self.start_timer(NOTICE_TIMER_ID, visible_ms);
        }
    }

    fn hide_now(&mut self) {
        self.stop_timers();
        self.fade = Fade::Hidden;
        self.content = None;
        // SAFETY: plain Win32 call on our own valid window.
        let _ = unsafe { ShowWindow(self.window, SW_HIDE) };
    }

    fn start_timer(&self, timer_id: usize, delay_ms: u32) {
        // SAFETY: plain Win32 call on our own valid window. WM_TIMER
        // messages arrive through the normal message loop.
        unsafe { SetTimer(Some(self.window), timer_id, delay_ms, None) };
    }

    fn stop_timer(&self, timer_id: usize) {
        // SAFETY: plain Win32 call; stopping a timer that is not running
        // just fails harmlessly.
        let _ = unsafe { KillTimer(Some(self.window), timer_id) };
    }

    fn stop_timers(&self) {
        self.stop_timer(FADE_TIMER_ID);
        self.stop_timer(NOTICE_TIMER_ID);
    }

    fn draw(&self) -> Result<()> {
        let Some(content) = &self.content else {
            return Ok(());
        };
        self.surface.draw_frame(content.scale, |surface| {
            // The whole bubble fades together through the brush opacity.
            // SAFETY: plain COM call on a valid brush.
            unsafe { surface.brush.SetOpacity(self.fade.opacity()) };
            self.draw_bubble(content);
        })
    }

    /// Draws the bubble in DIPs. Must be called inside `draw_frame`.
    fn draw_bubble(&self, content: &Content) {
        let (width, height) = content.bubble_size;
        let body_top = match content.arrow_edge {
            ArrowEdge::Top => ARROW_LENGTH,
            ArrowEdge::Bottom => 0.0,
        };
        let arrow_base_y = match content.arrow_edge {
            ArrowEdge::Top => body_top,
            ArrowEdge::Bottom => body_top + height,
        };
        let context = &self.surface.context;
        let brush = &self.surface.brush;
        let palette = self.kind.palette();
        let outer = rounded(0.0, body_top, width, height, CORNER_RADIUS);
        let inner = rounded(
            BORDER_WIDTH,
            body_top + BORDER_WIDTH,
            width - 2.0 * BORDER_WIDTH,
            height - 2.0 * BORDER_WIDTH,
            CORNER_RADIUS - BORDER_WIDTH,
        );
        let text_left = PADDING_X;
        let title_top = body_top + PADDING_Y;
        let body_text_top = title_top + content.title_height + TITLE_BODY_GAP;
        // SAFETY: plain COM calls between BeginDraw and EndDraw.
        unsafe {
            brush.SetColor(&palette.border);
            context.FillRoundedRectangle(&outer, brush);
            brush.SetColor(&palette.bubble);
            context.FillRoundedRectangle(&inner, brush);
            self.draw_arrow(content, arrow_base_y);
            context.SetTransform(&scale_transform(content.scale));
            brush.SetColor(&palette.title);
            context.DrawTextLayout(
                vector(text_left, title_top),
                &content.title,
                brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
            brush.SetColor(&palette.body);
            context.DrawTextLayout(
                vector(text_left, body_text_top),
                &content.body,
                brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
    }

    /// The arrow is a square turned 45° and centred on the bubble's edge:
    /// the inner half blends into the bubble, the outer half is the point.
    fn draw_arrow(&self, content: &Content, base_y: f32) {
        let half_side = ARROW_LENGTH * FRAC_1_SQRT_2;
        let square = D2D_RECT_F {
            left: -half_side,
            top: -half_side,
            right: half_side,
            bottom: half_side,
        };
        let turn = rotate_45_then_move(content.arrow_x, base_y, content.scale);
        // SAFETY: plain COM calls between BeginDraw and EndDraw.
        unsafe {
            self.surface.context.SetTransform(&turn);
            self.surface
                .context
                .FillRectangle(&square, &self.surface.brush);
        }
    }
}

fn create_text_format(
    factory: &IDWriteFactory,
    weight: DWRITE_FONT_WEIGHT,
    size: f32,
) -> Result<IDWriteTextFormat> {
    // SAFETY: plain DirectWrite call with valid, null-terminated strings.
    unsafe {
        factory.CreateTextFormat(
            FONT,
            None,
            weight,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!("en-us"),
        )
    }
}

/// The size the text actually takes, in DIPs.
fn text_size(layout: &IDWriteTextLayout) -> Result<(f32, f32)> {
    let mut metrics = DWRITE_TEXT_METRICS::default();
    // SAFETY: plain DirectWrite call; `metrics` outlives it.
    unsafe { layout.GetMetrics(&mut metrics)? };
    Ok((metrics.width.ceil(), metrics.height.ceil()))
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TIMER => {
            with_state(|state| state.on_timer(wparam.0));
            LRESULT(0)
        }
        // The bubble must never activate or steal focus.
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_DESTROY => {
            CALLOUT_STATE.with(|cell| cell.borrow_mut().take());
            LRESULT(0)
        }
        // SAFETY: forwarding the unchanged arguments to the default handler.
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

/// Runs `action` on the bubble state. Returns `None` if it does not exist,
/// or is already in use further up the call stack (re-entrancy).
fn with_state<T>(action: impl FnOnce(&mut CalloutState) -> T) -> Option<T> {
    CALLOUT_STATE.with(|cell| {
        let mut state = cell.try_borrow_mut().ok()?;
        state.as_mut().map(action)
    })
}

/// The area spanning all monitors, in screen pixels.
fn virtual_screen() -> PixelRect {
    // SAFETY: plain Win32 calls with no pointers.
    unsafe {
        let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        PixelRect {
            left,
            top,
            right: left + GetSystemMetrics(SM_CXVIRTUALSCREEN),
            bottom: top + GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}

/// Turns by 45°, moves the origin to (`x`, `y`) DIPs, then scales to pixels.
fn rotate_45_then_move(x: f32, y: f32, scale: f32) -> Matrix3x2 {
    let turn = FRAC_1_SQRT_2 * scale; // cos 45° == sin 45°
    Matrix3x2 {
        M11: turn,
        M12: turn,
        M21: -turn,
        M22: turn,
        M31: x * scale,
        M32: y * scale,
    }
}

fn rounded(left: f32, top: f32, width: f32, height: f32, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left,
            top,
            right: left + width,
            bottom: top + height,
        },
        radiusX: radius,
        radiusY: radius,
    }
}

fn vector(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

const fn gray(level: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: level,
        g: level,
        b: level,
        a: 1.0,
    }
}

fn log_error(context: &str, error: &windows::core::Error) {
    eprintln!("[pen-marker] error while {context}: {error}");
}
