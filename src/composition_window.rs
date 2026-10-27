//! A transparent, click-through window that never takes focus, drawn with
//! Direct2D through DirectComposition so its see-through parts really show
//! the desktop. Shared by `callout` (speech bubbles) and `glow` (the
//! start-up highlight).
//!
//! This module only sets the window and its drawing surface up; what gets
//! drawn is up to the caller.

use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET, D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1CreateFactory, ID2D1Bitmap1, ID2D1DeviceContext,
    ID2D1Factory1, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::{DXGI_PRESENT, IDXGIDevice, IDXGISurface, IDXGISwapChain1};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, LWA_ALPHA, RegisterClassW, SetLayeredWindowAttributes, WNDCLASSW, WNDPROC,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{Interface, PCWSTR, Result};
use windows_numerics::Matrix3x2;

use crate::renderer::{bitmap_properties, create_d3d_device, create_swap_chain};

/// The drawing surface is allocated once at this size. Callers keep their
/// window smaller than this (even at 300% scaling); the window shows only
/// its own part.
const SURFACE_WIDTH_PX: u32 = 1024;
const SURFACE_HEIGHT_PX: u32 = 512;

const TRANSPARENT: D2D1_COLOR_F = D2D1_COLOR_F {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

/// Creates a hidden, click-through, never-focused popup window. `owner`
/// keeps it above the overlay.
pub fn create_window(
    class_name: PCWSTR,
    title: PCWSTR,
    owner: HWND,
    window_proc: WNDPROC,
) -> Result<HWND> {
    // SAFETY: plain Win32 calls; `window_proc` lives for the whole program.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let window_class = WNDCLASSW {
            lpfnWndProc: window_proc,
            hInstance: instance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };
        if RegisterClassW(&window_class) == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let window = CreateWindowExW(
            // Same click-through, never-focused, composition-only setup as
            // the overlay (see `overlay::create_overlay_window`).
            WS_EX_NOREDIRECTIONBITMAP
                | WS_EX_LAYERED
                | WS_EX_TRANSPARENT
                | WS_EX_TOPMOST
                | WS_EX_TOOLWINDOW
                | WS_EX_NOACTIVATE,
            class_name,
            title,
            WS_POPUP,
            0,
            0,
            1,
            1,
            Some(owner),
            None,
            Some(instance.into()),
            None,
        )?;
        SetLayeredWindowAttributes(window, COLORREF(0), 255, LWA_ALPHA)?;
        Ok(window)
    }
}

/// GPU objects for drawing into one composition window.
pub struct CompositionSurface {
    pub context: ID2D1DeviceContext,
    /// One reusable brush: set its colour and opacity before each use.
    pub brush: ID2D1SolidColorBrush,
    swap_chain: IDXGISwapChain1,
    back_buffer: ID2D1Bitmap1,
    // Never used after setup, but releasing them would remove the content
    // from the window, so they must stay alive.
    _composition_device: IDCompositionDevice,
    _composition_target: IDCompositionTarget,
    _composition_visual: IDCompositionVisual,
}

impl CompositionSurface {
    /// Sets up drawing into `window`.
    pub fn new(window: HWND) -> Result<Self> {
        let d3d_device = create_d3d_device()?;
        let dxgi_device: IDXGIDevice = d3d_device.cast()?;
        let swap_chain = create_swap_chain(&dxgi_device, SURFACE_WIDTH_PX, SURFACE_HEIGHT_PX)?;
        // SAFETY: plain COM calls on valid objects created above.
        unsafe {
            let factory: ID2D1Factory1 =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let context = factory
                .CreateDevice(&dxgi_device)?
                .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            let back_buffer_surface: IDXGISurface = swap_chain.GetBuffer(0)?;
            let back_buffer = context.CreateBitmapFromDxgiSurface(
                &back_buffer_surface,
                Some(&bitmap_properties(
                    D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                )),
            )?;
            let brush = context.CreateSolidColorBrush(&TRANSPARENT, None)?;

            let composition_device: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;
            let composition_target = composition_device.CreateTargetForHwnd(window, true)?;
            let composition_visual = composition_device.CreateVisual()?;
            composition_visual.SetContent(&swap_chain)?;
            composition_target.SetRoot(&composition_visual)?;
            composition_device.Commit()?;

            Ok(Self {
                context,
                brush,
                swap_chain,
                back_buffer,
                _composition_device: composition_device,
                _composition_target: composition_target,
                _composition_visual: composition_visual,
            })
        }
    }

    /// Clears to transparent, runs `draw` in DIPs scaled by `scale`
    /// (pixels per DIP), then shows the result.
    pub fn draw_frame(&self, scale: f32, draw: impl FnOnce(&Self)) -> Result<()> {
        // SAFETY: plain COM calls; BeginDraw/EndDraw are always paired.
        unsafe {
            self.context.SetTarget(&self.back_buffer);
            self.context.BeginDraw();
            self.context.Clear(Some(&TRANSPARENT));
            self.context.SetTransform(&scale_transform(scale));
            draw(self);
            self.context.SetTransform(&scale_transform(1.0));
            self.context.EndDraw(None, None)?;
            self.swap_chain.Present(1, DXGI_PRESENT(0)).ok()
        }
    }
}

/// Scales DIPs to pixels.
pub fn scale_transform(scale: f32) -> Matrix3x2 {
    Matrix3x2 {
        M11: scale,
        M12: 0.0,
        M21: 0.0,
        M22: scale,
        M31: 0.0,
        M32: 0.0,
    }
}
