//! Taking and storing screenshots: capturing every screen (drawings
//! included), writing PNG files, and choosing (and remembering) the folder
//! they go to.
//!
//! This module knows nothing about the crop screen; `crop_window` uses it.

use std::ffi::c_void;
use std::fs;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{GENERIC_WRITE, HWND};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, GetDC, HBITMAP, HDC, ROP_CODE, ReleaseDC,
    SRCCOPY, SelectObject,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA,
    IWICImagingFactory, WICBitmapEncoderNoCache,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Shell::{
    FOLDERID_Pictures, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog,
    IShellItem, KF_FLAG_DEFAULT, SHCreateItemFromParsingName, SHGetKnownFolderPath,
    SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};
use windows::core::{HSTRING, PWSTR, Result, w};

use crate::crop::screenshot_file_name;

const BYTES_PER_PIXEL: u32 = 4;
/// Screenshots go here (inside the user's Pictures folder) until the user
/// picks another folder.
const DEFAULT_FOLDER_NAME: &str = "Pen Marker";
/// The chosen folder is remembered in `%APPDATA%\Pen Marker\` in this file.
const SETTINGS_FOLDER_NAME: &str = "Pen Marker";
const FOLDER_SETTING_FILE: &str = "screenshot-folder.txt";

/// A capture of every screen, as BGRA pixels (fully opaque), row by row.
pub struct Screenshot {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Screen position of the top-left pixel. Negative when a monitor sits
    /// left of or above the main one.
    pub left: i32,
    pub top: i32,
}

/// Captures everything currently on all screens, including the drawings.
pub fn capture_all_screens() -> Result<Screenshot> {
    // SAFETY: plain Win32 calls with no pointers.
    let (left, top, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1),
            GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1),
        )
    };
    let mut pixels = copy_screen_pixels(left, top, width, height)?;
    // The screen copy leaves the alpha byte at 0; screenshots are opaque.
    for pixel in pixels.chunks_exact_mut(BYTES_PER_PIXEL as usize) {
        pixel[3] = u8::MAX;
    }
    Ok(Screenshot {
        pixels,
        width: width as u32,
        height: height as u32,
        left,
        top,
    })
}

/// Copies a screen area into memory with GDI. CAPTUREBLT also includes
/// see-through windows such as the drawing overlay.
fn copy_screen_pixels(left: i32, top: i32, width: i32, height: i32) -> Result<Vec<u8>> {
    let byte_count = (width * height) as usize * BYTES_PER_PIXEL as usize;
    // SAFETY: the bitmap holds exactly `byte_count` bytes at `bits` while it
    // exists; every GDI object created here is released before returning.
    unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let result = create_top_down_bitmap(memory, width, height).and_then(|(bitmap, bits)| {
            let previous = SelectObject(memory, bitmap.into());
            let raster = ROP_CODE(SRCCOPY.0 | CAPTUREBLT.0);
            let copied = BitBlt(memory, 0, 0, width, height, Some(screen), left, top, raster);
            let _ = GdiFlush();
            let pixels = std::slice::from_raw_parts(bits, byte_count).to_vec();
            SelectObject(memory, previous);
            let _ = DeleteObject(bitmap.into());
            copied.map(|()| pixels)
        });
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        result
    }
}

/// A 32-bit bitmap whose rows run top to bottom, and its pixel memory.
fn create_top_down_bitmap(device: HDC, width: i32, height: i32) -> Result<(HBITMAP, *const u8)> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Negative height: rows run top to bottom.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    // SAFETY: `info` and `bits` outlive the call; Windows fills `bits`.
    let bitmap =
        unsafe { CreateDIBSection(Some(device), &info, DIB_RGB_COLORS, &mut bits, None, 0)? };
    Ok((bitmap, bits.cast_const().cast()))
}

/// Writes BGRA pixels to a PNG file at `path`.
pub fn save_png(pixels: &[u8], width: u32, height: u32, path: &Path) -> Result<()> {
    let wide_path = HSTRING::from(path);
    // SAFETY: plain COM calls on objects created here; `pixels` holds
    // `height` rows of `width` BGRA pixels and outlives WritePixels.
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let stream = factory.CreateStream()?;
        stream.InitializeFromFilename(&wide_path, GENERIC_WRITE.0)?;
        let encoder = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let mut frame = None;
        let mut frame_options = None;
        encoder.CreateNewFrame(&mut frame, &mut frame_options)?;
        let frame = frame.ok_or_else(windows::core::Error::empty)?;
        frame.Initialize(frame_options.as_ref())?;
        frame.SetSize(width, height)?;
        let mut pixel_format = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut pixel_format)?;
        frame.WritePixels(height, width * BYTES_PER_PIXEL, pixels)?;
        frame.Commit()?;
        encoder.Commit()
    }
}

/// A new file path in `folder`, named after the current date and time.
/// Adds " (2)", " (3)"… if a file with that name already exists.
pub fn new_file_path(folder: &Path) -> PathBuf {
    // SAFETY: plain Win32 call with no pointers.
    let now = unsafe { GetLocalTime() };
    let name = screenshot_file_name(
        (now.wYear, now.wMonth, now.wDay),
        (now.wHour, now.wMinute, now.wSecond),
    );
    let first_choice = folder.join(&name);
    if !first_choice.exists() {
        return first_choice;
    }
    let stem = name.trim_end_matches(".png");
    (2..)
        .map(|number| folder.join(format!("{stem} ({number}).png")))
        .find(|path| !path.exists())
        .unwrap_or(first_choice)
}

/// The folder screenshots are saved to: the one the user last picked, or
/// `Pictures\Pen Marker`.
pub fn remembered_folder() -> PathBuf {
    folder_setting_file()
        .and_then(|file| fs::read_to_string(file).ok())
        .map(|text| PathBuf::from(text.trim()))
        .filter(|folder| folder.is_dir())
        .unwrap_or_else(|| pictures_folder().join(DEFAULT_FOLDER_NAME))
}

/// Remembers `folder` for next time. Failing to remember is not worth
/// bothering the user about, so errors are only logged.
pub fn remember_folder(folder: &Path) {
    let Some(file) = folder_setting_file() else {
        return;
    };
    let written = file
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(&file, folder.to_string_lossy().as_bytes()));
    if let Err(error) = written {
        eprintln!("[pen-marker] error while remembering screenshot folder: {error}");
    }
}

fn folder_setting_file() -> Option<PathBuf> {
    let app_data = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(app_data)
            .join(SETTINGS_FOLDER_NAME)
            .join(FOLDER_SETTING_FILE),
    )
}

/// The user's Pictures folder (which may be redirected, e.g. to OneDrive).
fn pictures_folder() -> PathBuf {
    // SAFETY: plain Win32 call; the returned string is ours to free.
    let known = unsafe { SHGetKnownFolderPath(&FOLDERID_Pictures, KF_FLAG_DEFAULT, None) };
    known
        .ok()
        .and_then(take_wide_string)
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("Pictures"))
        })
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Opens Windows' folder picker, starting in `current`. Returns the chosen
/// folder, or `None` if the user cancelled.
///
/// The picker runs its own message loop, so callers must not hold any
/// state borrowed while calling this.
pub fn pick_folder(owner: HWND, current: &Path) -> Option<PathBuf> {
    match try_pick_folder(owner, current) {
        Ok(folder) => folder,
        Err(error) => {
            eprintln!("[pen-marker] error while picking folder: {error}");
            None
        }
    }
}

fn try_pick_folder(owner: HWND, current: &Path) -> Result<Option<PathBuf>> {
    // SAFETY: plain COM calls on objects created here.
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        let options = dialog.GetOptions()?;
        dialog.SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)?;
        dialog.SetTitle(w!("Choose where to save screenshots"))?;
        let start_folder: Result<IShellItem> =
            SHCreateItemFromParsingName(&HSTRING::from(current), None);
        if let Ok(start_folder) = start_folder {
            let _ = dialog.SetFolder(&start_folder);
        }
        // Showing fails when the user cancels: that just means "no change".
        if dialog.Show(Some(owner)).is_err() {
            return Ok(None);
        }
        let chosen = dialog.GetResult()?;
        let path = chosen.GetDisplayName(SIGDN_FILESYSPATH)?;
        Ok(take_wide_string(path).map(PathBuf::from))
    }
}

/// Converts a string Windows allocated for us, then frees it.
fn take_wide_string(text: PWSTR) -> Option<String> {
    // SAFETY: `text` came from a Windows API that hands ownership to the
    // caller; it is read once and freed once, here.
    unsafe {
        let converted = text.to_string().ok();
        CoTaskMemFree(Some(text.0.cast_const().cast()));
        converted
    }
}
