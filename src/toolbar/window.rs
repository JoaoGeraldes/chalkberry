//! The small floating toolbar: drag it anywhere to move it, pick the pen
//! colour, thickness and opacity, clear the drawing, or hide it. A short
//! hint at the bottom explains how to draw and erase, and resting the mouse
//! on a button or choice shows a tooltip (drawn by `callout`).
//!
//! The toolbar only draws itself and posts [`WM_TOOLBAR_COMMAND`] (and
//! [`WM_TOOLBAR_MOVED`]) to its owner (the overlay). It never changes pen
//! settings or strokes directly, and does not save its own position.
//! Layout and hit testing live in `toolbar::layout`.

use std::cell::RefCell;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D_SIZE_U, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_SOLID, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_LINE_JOIN_ROUND,
    D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES, D2D1_ROUNDED_RECT,
    D2D1_STROKE_STYLE_PROPERTIES, D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget,
    ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    InvalidateRect, MONITOR_DEFAULTTONULL, MonitorFromPoint, ScreenToClient, ValidateRect,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetSystemMetrics, GetWindowRect, HTCAPTION, HTCLIENT,
    IDC_ARROW, IsWindowVisible, KillTimer, LWA_ALPHA, LoadCursorW, MA_NOACTIVATE, PostMessageW,
    RegisterClassW, SM_CXSCREEN, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOZORDER, SetLayeredWindowAttributes, SetTimer, SetWindowPos, ShowWindow, WM_APP,
    WM_DESTROY, WM_DPICHANGED, WM_EXITSIZEMOVE, WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
    WM_NCHITTEST, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Result, w};
use windows_numerics::Vector2;

use crate::app::command::Command;
use crate::notices::callout::{self, CalloutKind};
use crate::notices::placement::PixelRect;
use crate::settings::config::{COLOR_CHOICES, OPACITY_CHOICES, PenSettings, WIDTH_CHOICES};
use crate::toolbar::layout::{
    Picker, Rect, ToolbarButton, ToolbarHit, ToolbarLayout, tooltip_text,
};

/// Posted to the owner window when the user picks something on the
/// toolbar. `wparam` is the [`Command::id`].
pub const WM_TOOLBAR_COMMAND: u32 = WM_APP + 3;

/// Posted to the owner window when the user has finished dragging the
/// toolbar. Read the new place from [`Toolbar::screen_rect`].
pub const WM_TOOLBAR_MOVED: u32 = WM_APP + 5;

/// Sent when the mouse leaves a window that asked for it with
/// `TrackMouseEvent`. Defined here because the `windows` crate only
/// exports it under `Win32_UI_Controls`, a feature we don't otherwise need.
/// Value from `winuser.h`.
const WM_MOUSELEAVE: u32 = 0x02A3;

/// Resting the mouse this long on a button shows its tooltip.
const TOOLTIP_DELAY_MS: u32 = 450;
const TOOLTIP_TIMER_ID: usize = 1;

/// Windows' reference DPI: at 96 DPI one DIP is one pixel.
const BASE_DPI: f32 = 96.0;
/// Whole-window opacity (0..=255). Slightly see-through keeps it discreet.
const WINDOW_OPACITY: u8 = 235;
/// Gap between the top of the primary screen and the toolbar at start.
const START_TOP_MARGIN: f32 = 12.0;

/// How to draw and erase, shown under the buttons.
const HINT_TEXT: &str = "Ctrl + drag  \u{2192}  draw\nCtrl + right-click  \u{2192}  erase";
const HINT_FONT: windows::core::PCWSTR = w!("Segoe UI");
const HINT_FONT_SIZE: f32 = 10.5;

const HIGHLIGHT_CORNER_RADIUS: f32 = 6.0;
const ICON_LINE_WIDTH: f32 = 1.6;
const SWATCH_RADIUS: f32 = 8.0;
/// The opacity button shows a smaller disc inside a ring, so it does not
/// look like the colour button.
const OPACITY_ICON_DISC_RADIUS: f32 = 4.5;
const SELECTED_RING_RADIUS: f32 = 11.0;
const SELECTED_RING_WIDTH: f32 = 1.5;
const GRIP_DOT_RADIUS: f32 = 1.3;
const GRIP_DOT_SPACING: f32 = 5.0;
/// Thickness previews are drawn at the real width, within these limits.
const WIDTH_PREVIEW_RADIUS_RANGE: (f32, f32) = (1.5, 10.0);

const BACKGROUND_COLOR: D2D1_COLOR_F = gray(0.13);
const HOVER_COLOR: D2D1_COLOR_F = gray(0.24);
const OPEN_PICKER_COLOR: D2D1_COLOR_F = gray(0.3);
const ICON_COLOR: D2D1_COLOR_F = gray(0.86);
const GRIP_COLOR: D2D1_COLOR_F = gray(0.5);
const HINT_COLOR: D2D1_COLOR_F = gray(0.58);
/// Outline around colour swatches, so dark colours stay visible.
const SWATCH_OUTLINE_COLOR: D2D1_COLOR_F = gray(0.45);
const SELECTED_RING_COLOR: D2D1_COLOR_F = gray(1.0);

// The window procedure is a plain callback, so the toolbar's state lives
// here (see the same pattern in `overlay`).
thread_local! {
    static TOOLBAR_STATE: RefCell<Option<ToolbarState>> = const { RefCell::new(None) };
}

/// Handle to the toolbar window. The window is owned by the overlay, so
/// Windows keeps it above the overlay and destroys it with the overlay.
pub struct Toolbar {
    window: HWND,
}

impl Toolbar {
    /// Creates the (still hidden) toolbar showing `pen`, with its top-left
    /// corner at `saved_top_left` (screen pixels). Without one, or if that
    /// place is no longer on any screen (e.g. a monitor was unplugged), it
    /// starts near the top centre of the primary screen.
    pub fn create(
        owner: HWND,
        pen: &PenSettings,
        saved_top_left: Option<(i32, i32)>,
    ) -> Result<Self> {
        let window = create_toolbar_window(owner)?;
        let state = ToolbarState::new(window, owner, *pen)?;
        let (width, height) = state.size_px();
        let start_top_left = saved_top_left
            .filter(|&(x, y)| is_on_a_screen(x + width / 2, y + height / 2))
            .unwrap_or_else(|| default_top_left(width, state.dpi));
        TOOLBAR_STATE.with(|cell| *cell.borrow_mut() = Some(state));
        with_state(|state| state.resize_window(Some(start_top_left)));
        Ok(Self { window })
    }

    pub fn show(&self) {
        // SAFETY: plain Win32 call on our own valid window.
        let _ = unsafe { ShowWindow(self.window, SW_SHOWNOACTIVATE) };
    }

    /// Hides the toolbar, closing any open picker first.
    pub fn hide(&self) {
        with_state(|state| {
            state.set_layout(ToolbarLayout::new(None));
            state.cancel_tooltip();
        });
        // SAFETY: plain Win32 call on our own valid window.
        let _ = unsafe { ShowWindow(self.window, SW_HIDE) };
    }

    /// The toolbar's window, e.g. to place other windows next to it in the
    /// stacking order.
    pub fn window(&self) -> HWND {
        self.window
    }

    /// Where the toolbar is on screen, in pixels.
    pub fn screen_rect(&self) -> PixelRect {
        window_screen_rect(self.window)
    }

    /// The scaling of the monitor the toolbar is on.
    pub fn dpi(&self) -> u32 {
        // SAFETY: plain Win32 call on our own valid window.
        unsafe { GetDpiForWindow(self.window) }
    }

    pub fn is_visible(&self) -> bool {
        // SAFETY: plain Win32 call on our own valid window.
        unsafe { IsWindowVisible(self.window).as_bool() }
    }

    /// Updates the buttons to show the current pen.
    pub fn set_pen(&self, pen: &PenSettings) {
        with_state(|state| {
            state.pen = *pen;
            state.request_redraw();
        });
    }
}

/// Direct2D objects tied to the window. Recreated if the GPU is reset.
struct Graphics {
    target: ID2D1HwndRenderTarget,
    brush: ID2D1SolidColorBrush,
}

struct ToolbarState {
    window: HWND,
    /// Receives the picked commands.
    owner: HWND,
    factory: ID2D1Factory,
    icon_line_style: ID2D1StrokeStyle,
    hint_format: IDWriteTextFormat,
    /// [`HINT_TEXT`] as UTF-16, the form DirectWrite draws.
    hint_text: Vec<u16>,
    /// Created on first paint, dropped after a drawing error.
    graphics: Option<Graphics>,
    layout: ToolbarLayout,
    hovered: Option<ToolbarHit>,
    is_tracking_mouse_leave: bool,
    pen: PenSettings,
    dpi: f32,
}

impl ToolbarState {
    fn new(window: HWND, owner: HWND, pen: PenSettings) -> Result<Self> {
        // SAFETY: plain Win32 and COM calls on our own valid window.
        let (factory, icon_line_style, dpi) = unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let icon_line_style = factory.CreateStrokeStyle(&round_line_style(), None)?;
            (factory, icon_line_style, GetDpiForWindow(window) as f32)
        };
        Ok(Self {
            window,
            owner,
            factory,
            icon_line_style,
            hint_format: create_hint_format()?,
            hint_text: HINT_TEXT.encode_utf16().collect(),
            graphics: None,
            layout: ToolbarLayout::new(None),
            hovered: None,
            is_tracking_mouse_leave: false,
            pen,
            dpi,
        })
    }

    /// The toolbar size in physical pixels at the current DPI.
    fn size_px(&self) -> (i32, i32) {
        let (width, height) = self.layout.size();
        let scale = self.dpi / BASE_DPI;
        (
            (width * scale).ceil() as i32,
            (height * scale).ceil() as i32,
        )
    }

    /// Converts a point in window pixels to layout DIPs.
    fn to_dips(&self, x_px: i32, y_px: i32) -> (f32, f32) {
        let scale = BASE_DPI / self.dpi;
        (x_px as f32 * scale, y_px as f32 * scale)
    }

    fn set_layout(&mut self, layout: ToolbarLayout) {
        if layout != self.layout {
            self.layout = layout;
            self.hovered = None;
            self.cancel_tooltip();
            self.resize_window(None);
        }
    }

    /// Fits the window (and its drawing surface) to the layout. Moves it to
    /// `top_left` if given, otherwise keeps it where it is.
    fn resize_window(&mut self, top_left: Option<(i32, i32)>) {
        let (width, height) = self.size_px();
        let (x, y) = top_left.unwrap_or_default();
        let keep_position = if top_left.is_some() {
            Default::default()
        } else {
            SWP_NOMOVE
        };
        // SAFETY: plain Win32 call on our own valid window. Messages it
        // sends back find the state busy and get default handling.
        let _ = unsafe {
            SetWindowPos(
                self.window,
                None,
                x,
                y,
                width,
                height,
                keep_position | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        if let Some(graphics) = &self.graphics {
            let size = D2D_SIZE_U {
                width: width as u32,
                height: height as u32,
            };
            // SAFETY: plain COM call. On failure the next paint recreates it.
            if unsafe { graphics.target.Resize(&size) }.is_err() {
                self.graphics = None;
            }
        }
        self.request_redraw();
    }

    fn on_click(&mut self, x_px: i32, y_px: i32) {
        self.cancel_tooltip();
        let (x, y) = self.to_dips(x_px, y_px);
        match self.layout.hit_test(x, y) {
            ToolbarHit::Button(button) => self.on_button_click(button),
            ToolbarHit::Choice(index) => self.on_choice_click(index),
            ToolbarHit::Background => {}
        }
    }

    fn on_button_click(&mut self, button: ToolbarButton) {
        if let Some(picker) = button.picker() {
            let is_already_open = self.layout.open_picker == Some(picker);
            let open_picker = if is_already_open { None } else { Some(picker) };
            self.set_layout(ToolbarLayout::new(open_picker));
            return;
        }
        match button {
            ToolbarButton::Screenshot => self.send_command(Command::TakeScreenshot),
            ToolbarButton::Clear => self.send_command(Command::ClearDrawing),
            ToolbarButton::Hide => self.send_command(Command::HideToolbar),
            // Pen buttons opened their picker above.
            ToolbarButton::Color | ToolbarButton::Width | ToolbarButton::Opacity => {}
        }
    }

    fn on_choice_click(&mut self, index: usize) {
        let Some(picker) = self.layout.open_picker else {
            return;
        };
        let command = match picker {
            Picker::Color => Command::PickColor(index),
            Picker::Width => Command::PickWidth(index),
            Picker::Opacity => Command::PickOpacity(index),
        };
        self.send_command(command);
        self.set_layout(ToolbarLayout::new(None));
    }

    /// The user let go of the toolbar after dragging it.
    fn on_move_finished(&self) {
        // SAFETY: posting never blocks or touches our memory. If it fails
        // the new place is just not remembered.
        let _ = unsafe { PostMessageW(Some(self.owner), WM_TOOLBAR_MOVED, WPARAM(0), LPARAM(0)) };
    }

    fn send_command(&self, command: Command) {
        // SAFETY: posting never blocks or touches our memory. If it fails
        // the click simply does nothing.
        let _ = unsafe {
            PostMessageW(
                Some(self.owner),
                WM_TOOLBAR_COMMAND,
                WPARAM(command.id()),
                LPARAM(0),
            )
        };
    }

    fn on_mouse_move(&mut self, x_px: i32, y_px: i32) {
        self.track_mouse_leave();
        let (x, y) = self.to_dips(x_px, y_px);
        let hovered = Some(self.layout.hit_test(x, y)).filter(|hit| *hit != ToolbarHit::Background);
        if hovered != self.hovered {
            self.hovered = hovered;
            self.request_redraw();
            self.on_hover_changed();
        }
    }

    /// A new button or choice is under the mouse (or none is).
    fn on_hover_changed(&self) {
        self.stop_tooltip_timer();
        match self.hovered {
            None => callout::hide_tooltip(),
            // A tooltip is already up: follow the mouse right away.
            Some(_) if callout::is_tooltip_visible() => self.show_tooltip(),
            Some(_) => {
                // SAFETY: plain Win32 call on our own valid window.
                unsafe { SetTimer(Some(self.window), TOOLTIP_TIMER_ID, TOOLTIP_DELAY_MS, None) };
            }
        }
    }

    fn on_tooltip_timer(&mut self) {
        self.stop_tooltip_timer();
        self.show_tooltip();
    }

    /// Shows the tooltip for whatever is under the mouse.
    fn show_tooltip(&self) {
        let Some(hit) = self.hovered else {
            return;
        };
        let rect = match hit {
            ToolbarHit::Button(button) => Some(self.layout.button_rect(button)),
            ToolbarHit::Choice(index) => self.layout.choice_rect(index),
            ToolbarHit::Background => None,
        };
        let text = tooltip_text(hit, self.layout.open_picker);
        if let (Some(rect), Some(text)) = (rect, text) {
            let target = self.to_screen(rect);
            let dpi = self.dpi as u32;
            callout::show(text.title, text.body, target, dpi, CalloutKind::Tooltip);
        }
    }

    fn cancel_tooltip(&self) {
        self.stop_tooltip_timer();
        callout::hide_tooltip();
    }

    fn stop_tooltip_timer(&self) {
        // SAFETY: plain Win32 call; stopping a timer that is not running
        // just fails harmlessly.
        let _ = unsafe { KillTimer(Some(self.window), TOOLTIP_TIMER_ID) };
    }

    /// Converts a layout rectangle (DIPs) to screen pixels.
    fn to_screen(&self, rect: Rect) -> PixelRect {
        let origin = window_screen_rect(self.window);
        let scale = self.dpi / BASE_DPI;
        let to_px = |dips: f32| (dips * scale).round() as i32;
        PixelRect {
            left: origin.left + to_px(rect.left),
            top: origin.top + to_px(rect.top),
            right: origin.left + to_px(rect.right),
            bottom: origin.top + to_px(rect.bottom),
        }
    }

    /// Asks Windows for a WM_MOUSELEAVE, so the hover highlight goes away
    /// when the mouse leaves the toolbar.
    fn track_mouse_leave(&mut self) {
        if self.is_tracking_mouse_leave {
            return;
        }
        let mut request = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: self.window,
            dwHoverTime: 0,
        };
        // SAFETY: `request` is fully initialised and outlives the call.
        self.is_tracking_mouse_leave = unsafe { TrackMouseEvent(&mut request) }.is_ok();
    }

    fn on_mouse_leave(&mut self) {
        self.is_tracking_mouse_leave = false;
        self.cancel_tooltip();
        if self.hovered.take().is_some() {
            self.request_redraw();
        }
    }

    /// Lets Windows drag the toolbar by any spot that is not a button.
    fn non_client_hit_test(&self, screen_x: i32, screen_y: i32) -> LRESULT {
        let mut point = POINT {
            x: screen_x,
            y: screen_y,
        };
        // SAFETY: plain Win32 call; `point` outlives it.
        let _ = unsafe { ScreenToClient(self.window, &mut point) };
        let (x, y) = self.to_dips(point.x, point.y);
        let area = match self.layout.hit_test(x, y) {
            ToolbarHit::Background => HTCAPTION,
            ToolbarHit::Button(_) | ToolbarHit::Choice(_) => HTCLIENT,
        };
        LRESULT(area as isize)
    }

    /// The toolbar moved to a monitor with a different scaling.
    fn on_dpi_changed(&mut self, new_dpi: u32, suggested_rect: &RECT) {
        self.dpi = new_dpi as f32;
        if let Some(graphics) = &self.graphics {
            // SAFETY: plain COM call.
            unsafe { graphics.target.SetDpi(self.dpi, self.dpi) };
        }
        self.resize_window(Some((suggested_rect.left, suggested_rect.top)));
    }

    fn request_redraw(&self) {
        // SAFETY: plain Win32 call on our own valid window.
        let _ = unsafe { InvalidateRect(Some(self.window), None, false) };
    }

    fn paint(&mut self) {
        if self.graphics.is_none() {
            match self.create_graphics() {
                Ok(graphics) => self.graphics = Some(graphics),
                Err(error) => return log_error("creating toolbar graphics", &error),
            }
        }
        let Some(graphics) = &self.graphics else {
            return;
        };
        // SAFETY: plain COM calls; BeginDraw/EndDraw are always paired.
        let drawn = unsafe {
            graphics.target.BeginDraw();
            self.draw(graphics);
            graphics.target.EndDraw(None, None)
        };
        if let Err(error) = drawn {
            // Usually the GPU was reset. Start over on the next paint.
            log_error("drawing toolbar", &error);
            self.graphics = None;
        }
    }

    fn create_graphics(&self) -> Result<Graphics> {
        let (width, height) = self.size_px();
        let target_properties = D2D1_RENDER_TARGET_PROPERTIES {
            dpiX: self.dpi,
            dpiY: self.dpi,
            ..Default::default()
        };
        let window_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd: self.window,
            pixelSize: D2D_SIZE_U {
                width: width as u32,
                height: height as u32,
            },
            presentOptions: D2D1_PRESENT_OPTIONS_NONE,
        };
        // SAFETY: plain COM calls with fully initialised properties.
        unsafe {
            let target = self
                .factory
                .CreateHwndRenderTarget(&target_properties, &window_properties)?;
            let brush = target.CreateSolidColorBrush(&ICON_COLOR, None)?;
            Ok(Graphics { target, brush })
        }
    }

    /// Draws the whole toolbar. Must be called between BeginDraw and EndDraw.
    fn draw(&self, graphics: &Graphics) {
        let painter = Painter {
            graphics,
            line_style: &self.icon_line_style,
        };
        // SAFETY: plain COM call between BeginDraw and EndDraw.
        unsafe { graphics.target.Clear(Some(&BACKGROUND_COLOR)) };
        draw_grip(&painter, self.layout.grip_rect());
        for button in ToolbarButton::ALL {
            let rect = self.layout.button_rect(button);
            if button.picker().is_some() && button.picker() == self.layout.open_picker {
                painter.fill_rounded_rect(rect, OPEN_PICKER_COLOR);
            } else if self.hovered == Some(ToolbarHit::Button(button)) {
                painter.fill_rounded_rect(rect, HOVER_COLOR);
            }
            self.draw_button_icon(&painter, button, rect.center());
        }
        if let Some(picker) = self.layout.open_picker {
            self.draw_choices(&painter, picker);
        }
        painter.text(
            &self.hint_text,
            &self.hint_format,
            self.layout.hint_rect(),
            HINT_COLOR,
        );
    }

    fn draw_button_icon(&self, painter: &Painter, button: ToolbarButton, center: (f32, f32)) {
        match button {
            ToolbarButton::Color => {
                painter.fill_circle(center, SWATCH_RADIUS, rgb(self.pen.color_rgb, 1.0));
                painter.stroke_circle(center, SWATCH_RADIUS, ICON_COLOR, 1.0);
            }
            ToolbarButton::Width => {
                painter.fill_circle(center, width_preview_radius(self.pen.width_px), ICON_COLOR);
            }
            ToolbarButton::Opacity => {
                let disc_color = rgb(self.pen.color_rgb, self.pen.opacity);
                painter.fill_circle(center, OPACITY_ICON_DISC_RADIUS, disc_color);
                painter.stroke_circle(center, SWATCH_RADIUS, ICON_COLOR, 1.0);
            }
            ToolbarButton::Screenshot => draw_camera_icon(painter, center),
            ToolbarButton::Clear => draw_trash_icon(painter, center),
            ToolbarButton::Hide => draw_hide_icon(painter, center),
        }
    }

    fn draw_choices(&self, painter: &Painter, picker: Picker) {
        for index in 0..picker.choice_count() {
            let Some(rect) = self.layout.choice_rect(index) else {
                continue;
            };
            if self.hovered == Some(ToolbarHit::Choice(index)) {
                painter.fill_rounded_rect(rect, HOVER_COLOR);
            }
            let center = rect.center();
            let is_current = match picker {
                Picker::Color => {
                    let color = COLOR_CHOICES[index].1;
                    painter.fill_circle(center, SWATCH_RADIUS, rgb(color, 1.0));
                    painter.stroke_circle(center, SWATCH_RADIUS, SWATCH_OUTLINE_COLOR, 1.0);
                    color == self.pen.color_rgb
                }
                Picker::Width => {
                    let width = WIDTH_CHOICES[index].1;
                    painter.fill_circle(center, width_preview_radius(width), ICON_COLOR);
                    width == self.pen.width_px
                }
                Picker::Opacity => {
                    let opacity = OPACITY_CHOICES[index].1;
                    painter.fill_circle(center, SWATCH_RADIUS, rgb(self.pen.color_rgb, opacity));
                    painter.stroke_circle(center, SWATCH_RADIUS, ICON_COLOR, 1.0);
                    opacity == self.pen.opacity
                }
            };
            if is_current {
                painter.stroke_circle(
                    center,
                    SELECTED_RING_RADIUS.min(rect.width() / 2.0 - 1.0),
                    SELECTED_RING_COLOR,
                    SELECTED_RING_WIDTH,
                );
            }
        }
    }
}

/// Small drawing helpers around one reusable brush.
struct Painter<'a> {
    graphics: &'a Graphics,
    line_style: &'a ID2D1StrokeStyle,
}

impl Painter<'_> {
    fn use_color(&self, color: D2D1_COLOR_F) {
        // SAFETY: plain COM call on a valid brush.
        unsafe { self.graphics.brush.SetColor(&color) };
    }

    fn fill_circle(&self, center: (f32, f32), radius: f32, color: D2D1_COLOR_F) {
        self.use_color(color);
        // SAFETY: plain COM call between BeginDraw and EndDraw.
        unsafe {
            self.graphics
                .target
                .FillEllipse(&circle(center, radius), &self.graphics.brush)
        };
    }

    fn stroke_circle(&self, center: (f32, f32), radius: f32, color: D2D1_COLOR_F, width: f32) {
        self.use_color(color);
        // SAFETY: plain COM call between BeginDraw and EndDraw.
        unsafe {
            self.graphics.target.DrawEllipse(
                &circle(center, radius),
                &self.graphics.brush,
                width,
                self.line_style,
            )
        };
    }

    fn line(&self, from: (f32, f32), to: (f32, f32)) {
        self.use_color(ICON_COLOR);
        // SAFETY: plain COM call between BeginDraw and EndDraw.
        unsafe {
            self.graphics.target.DrawLine(
                vector(from),
                vector(to),
                &self.graphics.brush,
                ICON_LINE_WIDTH,
                self.line_style,
            )
        };
    }

    fn text(&self, text: &[u16], format: &IDWriteTextFormat, rect: Rect, color: D2D1_COLOR_F) {
        self.use_color(color);
        // SAFETY: plain COM call between BeginDraw and EndDraw; `text`
        // and the rectangle outlive it.
        unsafe {
            self.graphics.target.DrawText(
                text,
                format,
                &to_d2d_rect(rect),
                &self.graphics.brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            )
        };
    }

    fn fill_rounded_rect(&self, rect: Rect, color: D2D1_COLOR_F) {
        self.use_color(color);
        let rounded = D2D1_ROUNDED_RECT {
            rect: to_d2d_rect(rect),
            radiusX: HIGHLIGHT_CORNER_RADIUS,
            radiusY: HIGHLIGHT_CORNER_RADIUS,
        };
        // SAFETY: plain COM call between BeginDraw and EndDraw.
        unsafe {
            self.graphics
                .target
                .FillRoundedRectangle(&rounded, &self.graphics.brush)
        };
    }
}

/// Two columns of three dots: the usual "you can drag this" hint.
fn draw_grip(painter: &Painter, rect: Rect) {
    let (center_x, center_y) = rect.center();
    for column in [-0.5, 0.5] {
        for row in [-1.0, 0.0, 1.0] {
            let dot = (
                center_x + column * GRIP_DOT_SPACING,
                center_y + row * GRIP_DOT_SPACING,
            );
            painter.fill_circle(dot, GRIP_DOT_RADIUS, GRIP_COLOR);
        }
    }
}

/// A camera: body outline, a bump on top, and the lens.
fn draw_camera_icon(painter: &Painter, (x, y): (f32, f32)) {
    let (left, right, top, bottom) = (x - 7.5, x + 7.5, y - 4.0, y + 6.0);
    painter.line((left, top), (right, top));
    painter.line((right, top), (right, bottom));
    painter.line((right, bottom), (left, bottom));
    painter.line((left, bottom), (left, top));
    painter.line((x - 3.0, top - 2.5), (x + 3.0, top - 2.5)); // viewfinder bump
    painter.stroke_circle((x, y + 1.0), 3.0, ICON_COLOR, ICON_LINE_WIDTH);
}

fn draw_trash_icon(painter: &Painter, (x, y): (f32, f32)) {
    painter.line((x - 6.0, y - 4.5), (x + 6.0, y - 4.5)); // lid
    painter.line((x - 2.0, y - 7.0), (x + 2.0, y - 7.0)); // handle
    painter.line((x - 4.5, y - 4.5), (x - 3.5, y + 7.0)); // left side
    painter.line((x - 3.5, y + 7.0), (x + 3.5, y + 7.0)); // bottom
    painter.line((x + 3.5, y + 7.0), (x + 4.5, y - 4.5)); // right side
}

/// A minimise-style dash: "put this away".
fn draw_hide_icon(painter: &Painter, (x, y): (f32, f32)) {
    painter.line((x - 6.0, y + 1.0), (x + 6.0, y + 1.0));
}

fn width_preview_radius(width_px: f32) -> f32 {
    let (smallest, largest) = WIDTH_PREVIEW_RADIUS_RANGE;
    (width_px / 2.0).clamp(smallest, largest)
}

/// Small, centred text for the hint.
fn create_hint_format() -> Result<IDWriteTextFormat> {
    // SAFETY: plain DirectWrite calls with valid, null-terminated strings.
    unsafe {
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let format = factory.CreateTextFormat(
            HINT_FONT,
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            HINT_FONT_SIZE,
            w!("en-us"),
        )?;
        format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
        format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        Ok(format)
    }
}

fn create_toolbar_window(owner: HWND) -> Result<HWND> {
    let class_name = w!("WindowsPenMarkerToolbar");
    // SAFETY: plain Win32 calls; `window_proc` matches WNDPROC and lives
    // for the whole program.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        if RegisterClassW(&window_class) == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let window = CreateWindowExW(
            // TOPMOST: above other apps. TOOLWINDOW: not in the taskbar or
            // Alt+Tab. NOACTIVATE: clicks never steal keyboard focus from
            // the app being drawn on. LAYERED: slightly see-through.
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
            class_name,
            w!("Pen Marker toolbar"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            // Owned by the overlay: always stays above it.
            Some(owner),
            None,
            Some(instance.into()),
            None,
        )?;
        SetLayeredWindowAttributes(window, COLORREF(0), WINDOW_OPACITY, LWA_ALPHA)?;
        round_window_corners(window);
        Ok(window)
    }
}

/// Rounded corners on Windows 11. Older Windows ignores this and keeps
/// square corners, which is fine.
fn round_window_corners(window: HWND) {
    let preference = DWMWCP_ROUND;
    // SAFETY: plain Win32 call; `preference` outlives it and its size is
    // passed alongside.
    let _ = unsafe {
        DwmSetWindowAttribute(
            window,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const preference).cast(),
            size_of_val(&preference) as u32,
        )
    };
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let (x, y) = point_from_lparam(lparam);
    let handled = match message {
        WM_NCHITTEST => with_state(|state| state.non_client_hit_test(x, y)),
        // Clicking the toolbar must never activate it or steal focus.
        WM_MOUSEACTIVATE => Some(LRESULT(MA_NOACTIVATE as isize)),
        WM_LBUTTONDOWN => with_state(|state| state.on_click(x, y)).map(|()| LRESULT(0)),
        WM_MOUSEMOVE => with_state(|state| state.on_mouse_move(x, y)).map(|()| LRESULT(0)),
        WM_MOUSELEAVE => with_state(ToolbarState::on_mouse_leave).map(|()| LRESULT(0)),
        WM_TIMER if wparam.0 == TOOLTIP_TIMER_ID => {
            with_state(ToolbarState::on_tooltip_timer).map(|()| LRESULT(0))
        }
        WM_PAINT => {
            let painted = with_state(ToolbarState::paint);
            // SAFETY: plain Win32 call; marks the window as painted.
            let _ = unsafe { ValidateRect(Some(window), None) };
            painted.map(|()| LRESULT(0))
        }
        // Ends Windows' move loop, entered by dragging the background.
        WM_EXITSIZEMOVE => with_state(|state| state.on_move_finished()).map(|()| LRESULT(0)),
        WM_DPICHANGED => {
            let new_dpi = (wparam.0 & 0xFFFF) as u32;
            // SAFETY: for WM_DPICHANGED, Windows guarantees `lparam` points
            // to a valid RECT for the duration of this call.
            let suggested_rect = unsafe { &*(lparam.0 as *const RECT) };
            with_state(|state| state.on_dpi_changed(new_dpi, suggested_rect)).map(|()| LRESULT(0))
        }
        WM_DESTROY => {
            TOOLBAR_STATE.with(|cell| cell.borrow_mut().take());
            Some(LRESULT(0))
        }
        _ => None,
    };
    match handled {
        Some(result) => result,
        // SAFETY: forwarding the unchanged arguments to the default handler.
        None => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

/// Near the top centre of the primary screen, for a toolbar `width` pixels
/// wide on a screen at `dpi`.
fn default_top_left(width: i32, dpi: f32) -> (i32, i32) {
    let top = (START_TOP_MARGIN * dpi / BASE_DPI) as i32;
    // SAFETY: plain Win32 call with no pointers.
    let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    ((screen_width - width) / 2, top)
}

/// True if the point (screen pixels) is on one of the connected screens.
fn is_on_a_screen(x: i32, y: i32) -> bool {
    // SAFETY: plain Win32 call with no pointers.
    let monitor = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONULL) };
    !monitor.is_invalid()
}

/// A window's position and size on screen, in pixels.
fn window_screen_rect(window: HWND) -> PixelRect {
    let mut rect = RECT::default();
    // SAFETY: plain Win32 call; `rect` outlives it. On failure the rect
    // stays empty, which only misplaces a tooltip.
    let _ = unsafe { GetWindowRect(window, &mut rect) };
    PixelRect {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

/// Runs `action` on the toolbar state. Returns `None` if the state does not
/// exist, or is already in use further up the call stack (re-entrancy).
fn with_state<T>(action: impl FnOnce(&mut ToolbarState) -> T) -> Option<T> {
    TOOLBAR_STATE.with(|cell| {
        let mut state = cell.try_borrow_mut().ok()?;
        state.as_mut().map(action)
    })
}

/// Mouse messages pack coordinates into `lparam` as two signed 16-bit
/// values (low word = x, high word = y).
fn point_from_lparam(lparam: LPARAM) -> (i32, i32) {
    let x = (lparam.0 & 0xFFFF) as u16 as i16;
    let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16;
    (i32::from(x), i32::from(y))
}

/// Round ends and corners keep the small icons soft.
fn round_line_style() -> D2D1_STROKE_STYLE_PROPERTIES {
    D2D1_STROKE_STYLE_PROPERTIES {
        startCap: D2D1_CAP_STYLE_ROUND,
        endCap: D2D1_CAP_STYLE_ROUND,
        dashCap: D2D1_CAP_STYLE_ROUND,
        lineJoin: D2D1_LINE_JOIN_ROUND,
        miterLimit: 10.0,
        dashStyle: D2D1_DASH_STYLE_SOLID,
        dashOffset: 0.0,
    }
}

const fn gray(level: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: level,
        g: level,
        b: level,
        a: 1.0,
    }
}

fn rgb([red, green, blue]: [f32; 3], alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: red,
        g: green,
        b: blue,
        a: alpha,
    }
}

fn to_d2d_rect(rect: Rect) -> D2D_RECT_F {
    D2D_RECT_F {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

fn circle(center: (f32, f32), radius: f32) -> D2D1_ELLIPSE {
    D2D1_ELLIPSE {
        point: vector(center),
        radiusX: radius,
        radiusY: radius,
    }
}

fn vector((x, y): (f32, f32)) -> Vector2 {
    Vector2 { X: x, Y: y }
}

fn log_error(context: &str, error: &windows::core::Error) {
    eprintln!("[pen-marker] error while {context}: {error}");
}
