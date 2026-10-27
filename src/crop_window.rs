//! The full-screen crop screen shown after taking a screenshot.
//!
//! It shows the frozen screenshot with a box around the part that will be
//! saved (the whole screen until the user drags a new box), and a panel
//! saying where the file will go, with Change folder…, Cancel and Save.
//! When it opens, the panel pulses the start-up glow (from `glow`) a few
//! times, so the user notices where to save.
//! Enter saves, Esc cancels. When it closes it posts [`WM_CROP_FINISHED`]
//! to its owner; the saved file and the (possibly changed) folder are then
//! available from [`take_outcome`]. It does not remember the folder itself:
//! the owner stores it in the settings. Layout maths lives in `crop`, file
//! work in `screenshot`.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::time::Instant;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_BITMAP_PROPERTIES,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_ROUNDED_RECT, D2D1CreateFactory, ID2D1Bitmap, ID2D1Factory, ID2D1HwndRenderTarget,
    ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWriteCreateFactory,
    IDWriteFactory, IDWriteTextFormat,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{InvalidateRect, ScreenToClient, ValidateRect};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, VK_ESCAPE, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetSystemMetrics, IDC_ARROW,
    IDC_CROSS, KillTimer, LoadCursorW, MB_ICONERROR, MB_OK, MessageBoxW, PostMessageW,
    RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, SetCursor, SetForegroundWindow, SetTimer,
    ShowWindow, WM_APP, WM_CAPTURECHANGED, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_SETCURSOR, WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{HSTRING, PCWSTR, Result, w};

use crate::callout_placement::PixelRect;
use crate::crop::{
    self, CropButton, PanelLayout, clamp_to_image, is_click_not_drag, rect_from_corners,
};
use crate::glow;
use crate::screenshot::{self, Screenshot};

/// Posted to the owner when the crop screen closes (saved or cancelled).
pub const WM_CROP_FINISHED: u32 = WM_APP + 4;

/// Windows' reference DPI: at 96 DPI one DIP is one pixel.
const BASE_DPI: f32 = 96.0;
/// The crop screen draws in pixels, so the screenshot shows 1:1.
const PIXEL_EXACT_DPI: f32 = 96.0;
/// Long folder paths are shortened in the middle to fit the panel.
const MAX_FOLDER_CHARS: usize = 64;

const FONT: PCWSTR = w!("Segoe UI");
const TEXT_SIZE: f32 = 13.0;
const LABEL_SIZE: f32 = 12.0;
const PANEL_CORNER_RADIUS: f32 = 10.0;
const BUTTON_CORNER_RADIUS: f32 = 6.0;
const BOX_LINE_WIDTH: f32 = 2.0;
const SIZE_LABEL_WIDTH: f32 = 110.0;
const SIZE_LABEL_HEIGHT: f32 = 24.0;
const SIZE_LABEL_OFFSET: f32 = 8.0;

const INSTRUCTION: &str =
    "Drag to choose an area, or save the whole screen. Enter saves, Esc cancels.";

/// Everything outside the box is darkened by this much.
const OUTSIDE_DIM: D2D1_COLOR_F = color(0.0, 0.0, 0.0, 0.5);
const BOX_COLOR: D2D1_COLOR_F = color(0.3, 0.62, 1.0, 1.0);
const PANEL_COLOR: D2D1_COLOR_F = color(0.13, 0.13, 0.14, 0.96);
const TEXT_COLOR: D2D1_COLOR_F = color(0.96, 0.96, 0.96, 1.0);
/// The instruction is secondary; the folder line is bright, so it is
/// obvious where the file will go.
const HINT_TEXT_COLOR: D2D1_COLOR_F = color(0.7, 0.7, 0.72, 1.0);
const BUTTON_COLOR: D2D1_COLOR_F = color(0.25, 0.25, 0.27, 1.0);
const BUTTON_HOVER_COLOR: D2D1_COLOR_F = color(0.33, 0.33, 0.36, 1.0);
const SAVE_COLOR: D2D1_COLOR_F = color(0.2, 0.5, 0.92, 1.0);
const SAVE_HOVER_COLOR: D2D1_COLOR_F = color(0.28, 0.58, 1.0, 1.0);
const LABEL_BACKGROUND: D2D1_COLOR_F = color(0.0, 0.0, 0.0, 0.7);
/// The panel's glow matches the box and the Save button.
const PANEL_GLOW_RGB: [f32; 3] = [BOX_COLOR.r, BOX_COLOR.g, BOX_COLOR.b];
/// Redraws the panel's glow while it pulses.
const GLOW_TIMER_ID: usize = 1;

thread_local! {
    static CROP_STATE: RefCell<Option<CropState>> = const { RefCell::new(None) };
    static OUTCOME: RefCell<Option<CropOutcome>> = const { RefCell::new(None) };
    static IS_CLASS_REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// How the crop screen ended, for its owner.
pub struct CropOutcome {
    /// The saved file, or `None` if the user cancelled.
    pub saved_path: Option<PathBuf>,
    /// The screenshot folder, which the user may have changed.
    pub folder: PathBuf,
}

/// Opens the crop screen over all monitors, showing `shot`, saving to
/// `folder` unless the user picks another one.
pub fn open(owner: HWND, shot: Screenshot, folder: PathBuf) -> Result<()> {
    let window = create_crop_window(owner, &shot)?;
    let state = CropState::new(window, owner, shot, folder)?;
    CROP_STATE.with(|cell| *cell.borrow_mut() = Some(state));
    // SAFETY: plain Win32 calls on our own valid window. The user just
    // clicked our toolbar, so we may take the foreground (for Enter/Esc).
    // The timer is stopped once the glow ends, or dies with the window.
    unsafe {
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        SetTimer(Some(window), GLOW_TIMER_ID, glow::FRAME_MS, None);
    }
    Ok(())
}

/// True while the crop screen is open.
pub fn is_open() -> bool {
    // Busy (borrowed) means it is open and in the middle of something.
    CROP_STATE.with(|cell| {
        cell.try_borrow()
            .map(|state| state.is_some())
            .unwrap_or(true)
    })
}

/// How the crop screen ended, once after each time it closes.
pub fn take_outcome() -> Option<CropOutcome> {
    OUTCOME.with(|cell| cell.borrow_mut().take())
}

/// What a click or key asks for. Carried out outside the state borrow,
/// because the folder picker runs its own message loop and closing the
/// window destroys the state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CropAction {
    Save,
    Cancel,
    ChangeFolder,
}

impl CropButton {
    fn action(self) -> CropAction {
        match self {
            CropButton::ChangeFolder => CropAction::ChangeFolder,
            CropButton::Cancel => CropAction::Cancel,
            CropButton::Save => CropAction::Save,
        }
    }
}

/// Direct2D objects tied to the window.
struct Graphics {
    target: ID2D1HwndRenderTarget,
    brush: ID2D1SolidColorBrush,
    image: ID2D1Bitmap,
}

struct CropState {
    window: HWND,
    owner: HWND,
    shot: Screenshot,
    factory: ID2D1Factory,
    text_format: IDWriteTextFormat,
    button_format: IDWriteTextFormat,
    label_format: IDWriteTextFormat,
    /// Created on first paint, dropped after a drawing error.
    graphics: Option<Graphics>,
    panel: PanelLayout,
    /// Pixels per DIP for the panel and labels (the main screen's scaling).
    scale: f32,
    /// The box to save. `None` means the whole screen.
    selection: Option<PixelRect>,
    /// Where the current drag started, while dragging.
    drag_start: Option<(i32, i32)>,
    hovered_button: Option<CropButton>,
    pressed_button: Option<CropButton>,
    folder: PathBuf,
    /// When the screen opened; the panel's glow pulses from then.
    opened_at: Instant,
}

impl CropState {
    fn new(window: HWND, owner: HWND, shot: Screenshot, folder: PathBuf) -> Result<Self> {
        // SAFETY: plain Win32 call with no pointers.
        let scale = unsafe { GetDpiForSystem() } as f32 / BASE_DPI;
        // SAFETY: plain Direct2D/DirectWrite calls.
        let (factory, text_factory) = unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let text_factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            (factory, text_factory)
        };
        let format = |size: f32, weight, alignment| {
            create_text_format(&text_factory, size * scale, weight, alignment)
        };
        Ok(Self {
            panel: PanelLayout::new(main_screen_in_window(&shot), scale),
            scale,
            text_format: format(
                TEXT_SIZE,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_LEADING,
            )?,
            button_format: format(
                TEXT_SIZE,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_CENTER,
            )?,
            label_format: format(
                LABEL_SIZE,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_CENTER,
            )?,
            window,
            owner,
            shot,
            factory,
            graphics: None,
            selection: None,
            drag_start: None,
            hovered_button: None,
            pressed_button: None,
            folder,
            opened_at: Instant::now(),
        })
    }

    /// The box that will be saved: the selection, or the whole screen.
    fn box_to_save(&self) -> PixelRect {
        self.selection.unwrap_or(PixelRect {
            left: 0,
            top: 0,
            right: self.shot.width as i32,
            bottom: self.shot.height as i32,
        })
    }

    fn on_mouse_down(&mut self, x: i32, y: i32) {
        let (x_f, y_f) = (x as f32, y as f32);
        if let Some(button) = self.panel.button_at(x_f, y_f) {
            self.pressed_button = Some(button);
            return;
        }
        if self.panel.contains(x_f, y_f) {
            return;
        }
        self.drag_start = Some((x, y));
        self.selection = Some(rect_from_corners((x, y), (x, y)));
        // SAFETY: plain Win32 call on our own valid window. Capture keeps
        // the drag going even if the mouse leaves the window.
        unsafe { SetCapture(self.window) };
        self.request_redraw();
    }

    fn on_mouse_move(&mut self, x: i32, y: i32) {
        if let Some(start) = self.drag_start {
            let dragged = rect_from_corners(start, (x, y));
            self.selection = Some(clamp_to_image(dragged, self.shot.width, self.shot.height));
            self.request_redraw();
            return;
        }
        let hovered = self.panel.button_at(x as f32, y as f32);
        if hovered != self.hovered_button {
            self.hovered_button = hovered;
            self.request_redraw();
        }
    }

    /// Ends a drag or a button press. Returns what the click asks for.
    fn on_mouse_up(&mut self, x: i32, y: i32) -> Option<CropAction> {
        if self.drag_start.is_some() {
            self.finish_drag();
            // SAFETY: plain Win32 call. It re-enters `window_proc` with
            // WM_CAPTURECHANGED, which finds the state busy and is ignored.
            let _ = unsafe { ReleaseCapture() };
            return None;
        }
        let pressed = self.pressed_button.take()?;
        let released_on = self.panel.button_at(x as f32, y as f32);
        (released_on == Some(pressed)).then_some(pressed.action())
    }

    /// A tiny drag is a click: it clears the box, so the whole screen is
    /// saved.
    fn finish_drag(&mut self) {
        self.drag_start = None;
        if self.selection.is_some_and(is_click_not_drag) {
            self.selection = None;
        }
        self.request_redraw();
    }

    fn set_folder(&mut self, folder: PathBuf) {
        self.folder = folder;
        self.request_redraw();
    }

    /// Saves the box as a PNG. Returns the file's path, or a message
    /// explaining what went wrong.
    fn save(&self) -> std::result::Result<PathBuf, String> {
        std::fs::create_dir_all(&self.folder)
            .map_err(|error| format!("Could not create the folder:\n{error}"))?;
        let saved_box = self.box_to_save();
        let pixels = crop::crop_bgra(&self.shot.pixels, self.shot.width, saved_box);
        let width = (saved_box.right - saved_box.left) as u32;
        let height = (saved_box.bottom - saved_box.top) as u32;
        let path = screenshot::new_file_path(&self.folder);
        screenshot::save_png(&pixels, width, height, &path)
            .map_err(|error| format!("Could not save the screenshot:\n{error}"))?;
        Ok(path)
    }

    fn apply_cursor(&self, x: i32, y: i32) {
        let over_panel = self.panel.contains(x as f32, y as f32);
        let cursor = if over_panel { IDC_ARROW } else { IDC_CROSS };
        // SAFETY: plain Win32 calls with predefined, shared system cursors.
        unsafe {
            if let Ok(cursor) = LoadCursorW(None, cursor) {
                SetCursor(Some(cursor));
            }
        }
    }

    /// Redraws the panel's glow, and stops the timer once it is over.
    fn on_glow_frame(&self) {
        if glow::pulse_brightness(self.opened_at.elapsed()).is_none() {
            // SAFETY: plain Win32 call on our own valid window.
            let _ = unsafe { KillTimer(Some(self.window), GLOW_TIMER_ID) };
        }
        // Also after the end, so the last frame clears the glow.
        self.request_redraw();
    }

    fn request_redraw(&self) {
        // SAFETY: plain Win32 call on our own valid window.
        let _ = unsafe { InvalidateRect(Some(self.window), None, false) };
    }

    fn paint(&mut self) {
        if self.graphics.is_none() {
            match self.create_graphics() {
                Ok(graphics) => self.graphics = Some(graphics),
                Err(error) => return log_error("creating crop screen graphics", &error),
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
            log_error("drawing crop screen", &error);
            self.graphics = None;
        }
    }

    fn create_graphics(&self) -> Result<Graphics> {
        let size = D2D_SIZE_U {
            width: self.shot.width,
            height: self.shot.height,
        };
        let target_properties = D2D1_RENDER_TARGET_PROPERTIES {
            dpiX: PIXEL_EXACT_DPI,
            dpiY: PIXEL_EXACT_DPI,
            ..Default::default()
        };
        let window_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd: self.window,
            pixelSize: size,
            presentOptions: D2D1_PRESENT_OPTIONS_NONE,
        };
        let image_properties = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: PIXEL_EXACT_DPI,
            dpiY: PIXEL_EXACT_DPI,
        };
        // SAFETY: plain COM calls with fully initialised properties; the
        // screenshot pixels outlive CreateBitmap, which copies them.
        unsafe {
            let target = self
                .factory
                .CreateHwndRenderTarget(&target_properties, &window_properties)?;
            let brush = target.CreateSolidColorBrush(&TEXT_COLOR, None)?;
            let image = target.CreateBitmap(
                size,
                Some(self.shot.pixels.as_ptr().cast()),
                self.shot.width * 4,
                &image_properties,
            )?;
            Ok(Graphics {
                target,
                brush,
                image,
            })
        }
    }

    /// Draws the whole crop screen. Must be called between BeginDraw and
    /// EndDraw.
    fn draw(&self, graphics: &Graphics) {
        let painter = Painter { graphics };
        // SAFETY: plain COM call between BeginDraw and EndDraw.
        unsafe {
            graphics.target.DrawBitmap(
                &graphics.image,
                None,
                1.0,
                D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                None,
            )
        };
        let saved_box = self.box_to_save();
        if self.selection.is_some() {
            self.dim_outside(&painter, saved_box);
        }
        draw_box(&painter, saved_box, self.scale);
        self.draw_panel_glow(graphics);
        self.draw_panel(&painter);
    }

    /// The pulsing glow around the panel, drawn underneath it while the
    /// screen has just opened.
    fn draw_panel_glow(&self, graphics: &Graphics) {
        let Some(brightness) = glow::pulse_brightness(self.opened_at.elapsed()) else {
            return;
        };
        let (left, top, right, bottom) = self.panel.panel();
        let corner_radius = PANEL_CORNER_RADIUS * self.scale;
        let outline = D2D1_ROUNDED_RECT {
            rect: rect(left, top, right, bottom),
            radiusX: corner_radius,
            radiusY: corner_radius,
        };
        glow::draw_layers(
            &graphics.target,
            &graphics.brush,
            &outline,
            glow::GLOW_SPREAD * self.scale,
            brightness,
            PANEL_GLOW_RGB,
        );
    }

    /// Darkens the four areas around the box.
    fn dim_outside(&self, painter: &Painter, saved_box: PixelRect) {
        let (width, height) = (self.shot.width as f32, self.shot.height as f32);
        let (left, top) = (saved_box.left as f32, saved_box.top as f32);
        let (right, bottom) = (saved_box.right as f32, saved_box.bottom as f32);
        painter.fill(rect(0.0, 0.0, width, top), OUTSIDE_DIM);
        painter.fill(rect(0.0, bottom, width, height), OUTSIDE_DIM);
        painter.fill(rect(0.0, top, left, bottom), OUTSIDE_DIM);
        painter.fill(rect(right, top, width, bottom), OUTSIDE_DIM);
    }

    fn draw_panel(&self, painter: &Painter) {
        let scale = self.scale;
        painter.fill_rounded(self.panel.panel(), PANEL_COLOR, PANEL_CORNER_RADIUS * scale);
        painter.text(
            INSTRUCTION,
            &self.text_format,
            self.panel.instruction_line(),
            HINT_TEXT_COLOR,
        );
        let folder = crop::shorten_path(&self.folder.to_string_lossy(), MAX_FOLDER_CHARS);
        let folder_line = format!("Saves to:  {folder}");
        painter.text(
            &folder_line,
            &self.text_format,
            self.panel.folder_line(),
            TEXT_COLOR,
        );
        for button in CropButton::ALL {
            let is_hovered = self.hovered_button == Some(button);
            let background = match (button, is_hovered) {
                (CropButton::Save, false) => SAVE_COLOR,
                (CropButton::Save, true) => SAVE_HOVER_COLOR,
                (_, false) => BUTTON_COLOR,
                (_, true) => BUTTON_HOVER_COLOR,
            };
            let area = self.panel.button(button);
            painter.fill_rounded(area, background, BUTTON_CORNER_RADIUS * scale);
            painter.text(button.label(), &self.button_format, area, TEXT_COLOR);
        }
        self.draw_size_label(painter);
    }

    /// "1280 × 720" at the box's top-left corner, so the user sees what
    /// they will get.
    fn draw_size_label(&self, painter: &Painter) {
        let scale = self.scale;
        let saved_box = self.box_to_save();
        let text = format!(
            "{} \u{00d7} {}",
            saved_box.right - saved_box.left,
            saved_box.bottom - saved_box.top
        );
        let left = saved_box.left as f32 + SIZE_LABEL_OFFSET * scale;
        let top = saved_box.top as f32 + SIZE_LABEL_OFFSET * scale;
        let area = (
            left,
            top,
            left + SIZE_LABEL_WIDTH * scale,
            top + SIZE_LABEL_HEIGHT * scale,
        );
        painter.fill_rounded(area, LABEL_BACKGROUND, BUTTON_CORNER_RADIUS * scale);
        painter.text(&text, &self.label_format, area, TEXT_COLOR);
    }
}

/// The box outline. Drawn just inside its edges, so a whole-screen box is
/// still fully visible.
fn draw_box(painter: &Painter, saved_box: PixelRect, scale: f32) {
    let inset = BOX_LINE_WIDTH * scale / 2.0;
    let area = rect(
        saved_box.left as f32 + inset,
        saved_box.top as f32 + inset,
        saved_box.right as f32 - inset,
        saved_box.bottom as f32 - inset,
    );
    painter.use_color(BOX_COLOR);
    // SAFETY: plain COM call between BeginDraw and EndDraw.
    unsafe {
        painter.graphics.target.DrawRectangle(
            &area,
            &painter.graphics.brush,
            BOX_LINE_WIDTH * scale,
            None,
        )
    };
}

/// Small drawing helpers around one reusable brush.
struct Painter<'a> {
    graphics: &'a Graphics,
}

impl Painter<'_> {
    fn use_color(&self, color: D2D1_COLOR_F) {
        // SAFETY: plain COM call on a valid brush.
        unsafe { self.graphics.brush.SetColor(&color) };
    }

    fn fill(&self, area: D2D_RECT_F, color: D2D1_COLOR_F) {
        self.use_color(color);
        // SAFETY: plain COM call between BeginDraw and EndDraw.
        unsafe {
            self.graphics
                .target
                .FillRectangle(&area, &self.graphics.brush)
        };
    }

    fn fill_rounded(&self, area: (f32, f32, f32, f32), color: D2D1_COLOR_F, radius: f32) {
        self.use_color(color);
        let rounded = D2D1_ROUNDED_RECT {
            rect: rect(area.0, area.1, area.2, area.3),
            radiusX: radius,
            radiusY: radius,
        };
        // SAFETY: plain COM call between BeginDraw and EndDraw.
        unsafe {
            self.graphics
                .target
                .FillRoundedRectangle(&rounded, &self.graphics.brush)
        };
    }

    fn text(
        &self,
        text: &str,
        format: &IDWriteTextFormat,
        area: (f32, f32, f32, f32),
        color: D2D1_COLOR_F,
    ) {
        self.use_color(color);
        let wide_text: Vec<u16> = text.encode_utf16().collect();
        let layout_rect = rect(area.0, area.1, area.2, area.3);
        // SAFETY: plain COM call between BeginDraw and EndDraw; the text
        // and rectangle outlive it.
        unsafe {
            self.graphics.target.DrawText(
                &wide_text,
                format,
                &layout_rect,
                &self.graphics.brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            )
        };
    }
}

fn create_text_format(
    factory: &IDWriteFactory,
    size: f32,
    weight: DWRITE_FONT_WEIGHT,
    alignment: DWRITE_TEXT_ALIGNMENT,
) -> Result<IDWriteTextFormat> {
    // SAFETY: plain DirectWrite calls with valid, null-terminated strings.
    unsafe {
        let format = factory.CreateTextFormat(
            FONT,
            None,
            weight,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!("en-us"),
        )?;
        format.SetTextAlignment(alignment)?;
        format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        Ok(format)
    }
}

/// The main screen, in crop window pixels (the window starts at the top-left
/// of all screens, which may be left of or above the main screen).
fn main_screen_in_window(shot: &Screenshot) -> PixelRect {
    // SAFETY: plain Win32 calls with no pointers.
    let (width, height) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    PixelRect {
        left: -shot.left,
        top: -shot.top,
        right: width - shot.left,
        bottom: height - shot.top,
    }
}

fn create_crop_window(owner: HWND, shot: &Screenshot) -> Result<HWND> {
    let class_name = w!("WindowsPenMarkerCrop");
    // SAFETY: plain Win32 calls; `window_proc` matches WNDPROC and lives
    // for the whole program.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        // The crop screen can open many times; its class is registered once.
        if !IS_CLASS_REGISTERED.get() {
            let window_class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            if RegisterClassW(&window_class) == 0 {
                return Err(windows::core::Error::from_thread());
            }
            IS_CLASS_REGISTERED.set(true);
        }
        CreateWindowExW(
            // TOPMOST: above everything, including the overlay.
            // TOOLWINDOW: not in the taskbar or Alt+Tab.
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!("Pen Marker screenshot"),
            WS_POPUP,
            shot.left,
            shot.top,
            shot.width as i32,
            shot.height as i32,
            Some(owner),
            None,
            Some(instance.into()),
            None,
        )
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let (x, y) = point_from_lparam(lparam);
    let handled = match message {
        WM_PAINT => {
            let painted = with_state(CropState::paint);
            // SAFETY: plain Win32 call; marks the window as painted.
            let _ = unsafe { ValidateRect(Some(window), None) };
            painted
        }
        // Everything is repainted by WM_PAINT; skipping this avoids flicker.
        WM_ERASEBKGND => return LRESULT(1),
        WM_LBUTTONDOWN => with_state(|state| state.on_mouse_down(x, y)),
        WM_MOUSEMOVE => with_state(|state| state.on_mouse_move(x, y)),
        WM_LBUTTONUP => {
            if let Some(Some(action)) = with_state(|state| state.on_mouse_up(x, y)) {
                run_action(window, action);
            }
            Some(())
        }
        WM_CAPTURECHANGED => with_state(CropState::finish_drag),
        WM_TIMER if wparam.0 == GLOW_TIMER_ID => with_state(|state| state.on_glow_frame()),
        WM_KEYDOWN => on_key(window, wparam),
        WM_SETCURSOR => {
            let cursor_position = cursor_position_in(window);
            if with_state(|state| state.apply_cursor(cursor_position.0, cursor_position.1))
                .is_some()
            {
                return LRESULT(1);
            }
            None
        }
        WM_DESTROY => {
            CROP_STATE.with(|cell| cell.borrow_mut().take());
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

fn on_key(window: HWND, wparam: WPARAM) -> Option<()> {
    let key = wparam.0 as u16;
    let action = if key == VK_RETURN.0 {
        CropAction::Save
    } else if key == VK_ESCAPE.0 {
        CropAction::Cancel
    } else {
        return None;
    };
    run_action(window, action);
    Some(())
}

/// Carries out an action outside the state borrow.
fn run_action(window: HWND, action: CropAction) {
    match action {
        CropAction::Cancel => close(window, None),
        CropAction::Save => match with_state(|state| state.save()) {
            Some(Ok(path)) => close(window, Some(path)),
            Some(Err(message)) => show_error(window, &message),
            None => {}
        },
        CropAction::ChangeFolder => {
            let Some(current) = with_state(|state| state.folder.clone()) else {
                return;
            };
            if let Some(folder) = screenshot::pick_folder(window, &current) {
                with_state(|state| state.set_folder(folder));
            }
        }
    }
}

/// Closes the crop screen and tells the owner how it ended.
fn close(window: HWND, saved_path: Option<PathBuf>) {
    let owner = with_state(|state| {
        let outcome = CropOutcome {
            saved_path,
            folder: state.folder.clone(),
        };
        OUTCOME.with(|cell| *cell.borrow_mut() = Some(outcome));
        state.owner
    });
    // SAFETY: plain Win32 calls. DestroyWindow sends WM_DESTROY right away,
    // which drops the state; nothing here holds it borrowed.
    unsafe {
        if let Some(owner) = owner {
            let _ = PostMessageW(Some(owner), WM_CROP_FINISHED, WPARAM(0), LPARAM(0));
        }
        let _ = DestroyWindow(window);
    }
}

fn show_error(window: HWND, message: &str) {
    let text = HSTRING::from(message);
    // SAFETY: plain Win32 call with valid, null-terminated strings.
    unsafe {
        MessageBoxW(Some(window), &text, w!("Pen Marker"), MB_OK | MB_ICONERROR);
    }
}

/// WM_SETCURSOR does not carry the mouse position, so ask for it.
fn cursor_position_in(window: HWND) -> (i32, i32) {
    let mut point = POINT::default();
    // SAFETY: plain Win32 calls; `point` outlives them.
    unsafe {
        let _ = GetCursorPos(&mut point);
        let _ = ScreenToClient(window, &mut point);
    }
    (point.x, point.y)
}

/// Runs `action` on the crop state. Returns `None` if the state does not
/// exist, or is already in use further up the call stack (re-entrancy).
fn with_state<T>(action: impl FnOnce(&mut CropState) -> T) -> Option<T> {
    CROP_STATE.with(|cell| {
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

fn rect(left: f32, top: f32, right: f32, bottom: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left,
        top,
        right,
        bottom,
    }
}

const fn color(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

fn log_error(context: &str, error: &windows::core::Error) {
    eprintln!("[pen-marker] error while {context}: {error}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_buttons_lead_to_matching_actions() {
        assert_eq!(CropButton::Save.action(), CropAction::Save);
        assert_eq!(CropButton::Cancel.action(), CropAction::Cancel);
        assert_eq!(CropButton::ChangeFolder.action(), CropAction::ChangeFolder);
    }
}
