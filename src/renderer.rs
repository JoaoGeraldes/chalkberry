//! GPU drawing of strokes onto the transparent overlay.
//!
//! Pipeline: Direct3D 11 device → DXGI swap chain with premultiplied alpha →
//! DirectComposition visual on the overlay window → Direct2D draws into it.
//!
//! Finished strokes are drawn once into a cached bitmap, so each frame only
//! redraws the stroke in progress. This module knows nothing about input.

use std::mem::ManuallyDrop;

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_COMPOSITE_MODE_SOURCE_OVER,
    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_OPTIONS, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_SOLID,
    D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_LINE_JOIN_ROUND, D2D1_QUADRATIC_BEZIER_SEGMENT,
    D2D1_STROKE_STYLE_PROPERTIES1, D2D1_STROKE_TRANSFORM_TYPE_NORMAL, D2D1CreateFactory,
    ID2D1Bitmap1, ID2D1DeviceContext, ID2D1Factory1, ID2D1SolidColorBrush, ID2D1StrokeStyle1,
};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_PRESENT, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice, IDXGIFactory2,
    IDXGISurface, IDXGISwapChain1,
};
use windows::core::{Interface, Result};
use windows_numerics::Vector2;

use crate::config::{PEN_COLOR_RGB, PEN_OPACITY, PEN_WIDTH_PX};
use crate::stroke::{PathSegment, Point, StrokeShape};

/// Direct2D's default DPI. Using it everywhere means 1 DIP == 1 pixel.
const PIXEL_EXACT_DPI: f32 = 96.0;

const TRANSPARENT: D2D1_COLOR_F = D2D1_COLOR_F {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

/// Owns every GPU object needed to draw on the overlay.
pub struct Renderer {
    factory: ID2D1Factory1,
    context: ID2D1DeviceContext,
    swap_chain: IDXGISwapChain1,
    /// The swap chain's back buffer, as a Direct2D target.
    back_buffer: ID2D1Bitmap1,
    /// All finished strokes, so they are not re-drawn every frame.
    finished_strokes: ID2D1Bitmap1,
    pen_brush: ID2D1SolidColorBrush,
    pen_style: ID2D1StrokeStyle1,
    // Composition objects are never used after setup, but releasing them
    // would remove the content from the window, so they must stay alive.
    _composition_device: IDCompositionDevice,
    _composition_target: IDCompositionTarget,
    _composition_visual: IDCompositionVisual,
}

impl Renderer {
    /// Creates the drawing pipeline for `window`, sized `width` × `height` pixels.
    pub fn new(window: HWND, width: u32, height: u32) -> Result<Self> {
        let d3d_device = create_d3d_device()?;
        let dxgi_device: IDXGIDevice = d3d_device.cast()?;
        let swap_chain = create_swap_chain(&dxgi_device, width, height)?;

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
                Some(&target_bitmap_properties(D2D1_BITMAP_OPTIONS_CANNOT_DRAW)),
            )?;
            let finished_strokes = context.CreateBitmap(
                D2D_SIZE_U { width, height },
                None,
                0,
                &target_bitmap_properties(D2D1_BITMAP_OPTIONS::default()),
            )?;

            let [red, green, blue] = PEN_COLOR_RGB;
            let pen_color = D2D1_COLOR_F {
                r: red,
                g: green,
                b: blue,
                a: PEN_OPACITY,
            };
            let pen_brush = context.CreateSolidColorBrush(&pen_color, None)?;
            let pen_style = factory.CreateStrokeStyle(&round_pen_style(), None)?;

            let composition_device: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;
            let composition_target = composition_device.CreateTargetForHwnd(window, true)?;
            let composition_visual = composition_device.CreateVisual()?;
            composition_visual.SetContent(&swap_chain)?;
            composition_target.SetRoot(&composition_visual)?;
            composition_device.Commit()?;

            let renderer = Self {
                factory,
                context,
                swap_chain,
                back_buffer,
                finished_strokes,
                pen_brush,
                pen_style,
                _composition_device: composition_device,
                _composition_target: composition_target,
                _composition_visual: composition_visual,
            };
            // A new bitmap's contents are undefined; start fully transparent.
            renderer.clear_finished_strokes()?;
            Ok(renderer)
        }
    }

    /// Draws all finished strokes plus `active_stroke` (if any) and shows the result.
    pub fn draw_frame(&self, active_stroke: Option<&StrokeShape>) -> Result<()> {
        // SAFETY: plain COM calls; BeginDraw/EndDraw are always paired.
        unsafe {
            self.context.SetTarget(&self.back_buffer);
            self.context.BeginDraw();
            self.context.Clear(Some(&TRANSPARENT));
            self.context.DrawImage(
                &self.finished_strokes,
                None,
                None,
                D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                D2D1_COMPOSITE_MODE_SOURCE_OVER,
            );
            let drawn = match active_stroke {
                Some(shape) => self.draw_shape(shape),
                None => Ok(()),
            };
            self.context.EndDraw(None, None)?;
            drawn?;
            // Sync interval 1 waits for the next vertical refresh, which also
            // naturally limits redraws to the monitor's refresh rate.
            self.swap_chain.Present(1, DXGI_PRESENT(0)).ok()
        }
    }

    /// Permanently adds a finished stroke to the cached drawing.
    pub fn add_finished_stroke(&self, shape: &StrokeShape) -> Result<()> {
        self.draw_into_finished_strokes(|renderer| renderer.draw_shape(shape))
    }

    /// Erases every finished stroke.
    pub fn clear_finished_strokes(&self) -> Result<()> {
        self.draw_into_finished_strokes(|renderer| {
            // SAFETY: called between BeginDraw and EndDraw.
            unsafe { renderer.context.Clear(Some(&TRANSPARENT)) };
            Ok(())
        })
    }

    fn draw_into_finished_strokes(&self, draw: impl FnOnce(&Self) -> Result<()>) -> Result<()> {
        // SAFETY: plain COM calls; BeginDraw/EndDraw are always paired.
        unsafe {
            self.context.SetTarget(&self.finished_strokes);
            self.context.BeginDraw();
            let drawn = draw(self);
            self.context.EndDraw(None, None)?;
            drawn
        }
    }

    /// Draws one stroke shape. Must be called between BeginDraw and EndDraw.
    fn draw_shape(&self, shape: &StrokeShape) -> Result<()> {
        match shape {
            StrokeShape::Dot { center } => {
                let radius = PEN_WIDTH_PX / 2.0;
                let dot = D2D1_ELLIPSE {
                    point: to_d2d_point(*center),
                    radiusX: radius,
                    radiusY: radius,
                };
                // SAFETY: plain COM call on valid objects.
                unsafe { self.context.FillEllipse(&dot, &self.pen_brush) };
                Ok(())
            }
            StrokeShape::Path { start, segments } => self.draw_path(*start, segments),
        }
    }

    fn draw_path(&self, start: Point, segments: &[PathSegment]) -> Result<()> {
        // SAFETY: plain COM calls; the geometry sink is opened and closed once.
        unsafe {
            let geometry = self.factory.CreatePathGeometry()?;
            let sink = geometry.Open()?;
            sink.BeginFigure(to_d2d_point(start), D2D1_FIGURE_BEGIN_HOLLOW);
            for segment in segments {
                match *segment {
                    PathSegment::Line { to } => sink.AddLine(to_d2d_point(to)),
                    PathSegment::Quadratic { control, to } => {
                        sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT {
                            point1: to_d2d_point(control),
                            point2: to_d2d_point(to),
                        })
                    }
                }
            }
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            // Drawing the whole stroke as one geometry means places where it
            // crosses itself are not painted twice, so translucent pens stay even.
            self.context
                .DrawGeometry(&geometry, &self.pen_brush, PEN_WIDTH_PX, &self.pen_style);
        }
        Ok(())
    }
}

fn create_d3d_device() -> Result<ID3D11Device> {
    let mut device = None;
    // SAFETY: `device` is a valid out-pointer for the duration of the call.
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            Default::default(),
            // BGRA support is required for Direct2D interop.
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )?;
    }
    device.ok_or_else(windows::core::Error::empty)
}

fn create_swap_chain(
    dxgi_device: &IDXGIDevice,
    width: u32,
    height: u32,
) -> Result<IDXGISwapChain1> {
    let description = DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        // Premultiplied alpha is what lets the desktop show through.
        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
        ..Default::default()
    };
    // SAFETY: plain COM calls with a fully initialised description.
    unsafe {
        let dxgi_factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
        dxgi_factory.CreateSwapChainForComposition(dxgi_device, &description, None)
    }
}

fn target_bitmap_properties(extra_options: D2D1_BITMAP_OPTIONS) -> D2D1_BITMAP_PROPERTIES1 {
    D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: PIXEL_EXACT_DPI,
        dpiY: PIXEL_EXACT_DPI,
        bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | extra_options,
        colorContext: ManuallyDrop::new(None),
    }
}

/// Round ends and corners make strokes look like a real marker.
fn round_pen_style() -> D2D1_STROKE_STYLE_PROPERTIES1 {
    D2D1_STROKE_STYLE_PROPERTIES1 {
        startCap: D2D1_CAP_STYLE_ROUND,
        endCap: D2D1_CAP_STYLE_ROUND,
        dashCap: D2D1_CAP_STYLE_ROUND,
        lineJoin: D2D1_LINE_JOIN_ROUND,
        miterLimit: 10.0,
        dashStyle: D2D1_DASH_STYLE_SOLID,
        dashOffset: 0.0,
        transformType: D2D1_STROKE_TRANSFORM_TYPE_NORMAL,
    }
}

fn to_d2d_point(point: Point) -> Vector2 {
    Vector2 {
        X: point.x,
        Y: point.y,
    }
}
