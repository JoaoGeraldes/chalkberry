# Chalkberry

![Chalkberry logo](assets/chalkberry-logo.svg)

A tiny native Windows app for drawing on top of the screen. While it runs,
**hold Ctrl and click or drag** to draw smooth, chalk-textured lines over
anything. Without Ctrl, the mouse works normally.

## Using it

| Action | Shortcut |
|---|---|
| Draw | Hold **Ctrl** + click / drag |
| Erase a stroke | Hold **Ctrl** + right-click it (or right-drag across several) |
| Clear all drawings | **Ctrl + Alt + C** |
| Quit | **Ctrl + Alt + Q** |

While Ctrl is held the pointer turns into Windows' pen cursor, so you know
you are drawing. While erasing it turns into a crosshair for aiming at the
stroke to remove.

When the app starts, a glow in your chalk colour pulses around the toolbar
three times and a bubble points at it for a few seconds, so you know it is
running even on a screen full of dark windows.

### Floating toolbar

A small dark bar appears at the top of the screen. Drag it by any empty spot
(the dotted grip on the left is the obvious handle) to move it anywhere.

| Button | What it does |
|---|---|
| Coloured dot | Opens a row of colours |
| Grey dot | Opens a row of thicknesses (dots drawn at real size) |
| Dot in a ring | Opens a row of opacities |
| Camera | Takes a screenshot with your drawings (see below) |
| Bin | Clears the drawing |
| Dash | Hides the toolbar; a bubble points at the tray icon to show where it went |

A small hint under the buttons reminds you how to draw and erase. Rest the
mouse on any button or choice for a tooltip explaining it. New settings apply
to the next strokes; lines already drawn keep their look.

### Tray icon

The strawberry icon in the taskbar's notification area opens a menu with
the same colour, thickness and opacity choices, **Show toolbar** (brings a
hidden toolbar back), **Clear drawing** and **Quit**. If you cannot see it,
it may be in the hidden-icons overflow (the **^** arrow); drag it onto the
taskbar to keep it in view.

### Screenshots

Click the camera. The toolbar steps aside, the screen (all monitors, with
your drawings) is captured, and a preview opens with a blue box around
everything. Drag to draw a smaller box if you only want part of it; click
without dragging to go back to the whole screen.

The panel at the bottom glows briefly so you notice it. It shows
**Saves to:** with the folder the PNG will go to. **Change folder…** picks
another one (remembered for next time; the first default is
`Pictures\Chalkberry`). **Save** (or Enter) saves, **Cancel** (or Esc)
discards. A bubble then shows the saved file's path.


### Settings

Your choices are saved as soon as you make them, in
`%APPDATA%\Chalkberry\settings.json`, and restored the next time the app
starts: the pen colour, thickness and opacity, where the toolbar is and
whether it is hidden, and the screenshot folder. The file is created with
the defaults on the first start:

```json
{
  "pen_color": "#F2383F",
  "pen_width_px": 10,
  "pen_opacity": 1,
  "toolbar_x": 1104,
  "toolbar_y": 12,
  "toolbar_visible": true,
  "screenshot_folder": "C:\\Users\\you\\Pictures\\Chalkberry"
}
```

You can edit it by hand while the app is closed. Values are matched to the
nearest choice in the toolbar; anything missing or unreadable falls back to
its default, and a toolbar place that is no longer on any screen (an
unplugged monitor) goes back to the top of the main screen. Delete the file
to start over with the defaults.

## Requirements

- Windows 10 (1903+) or Windows 11, x64
- To build: [Rust](https://rustup.rs) with the default MSVC toolchain, and
  Visual Studio Build Tools with the **"Desktop development with C++"**
  workload (provides the linker)

## Build the app

```powershell
cargo run                # debug build, with a console for error messages
cargo build --release    # optimized build, no console window
```

The release build is a single, standalone file:
`target\release\chalkberry.exe`. It needs nothing else installed
(the C runtime is linked in, see [`.cargo/config.toml`](.cargo/config.toml)),
so you can copy it anywhere and double-click it.

### App icon

The logo is `assets/chalkberry-logo.svg`. Windows needs an `.ico` instead,
so `assets/chalkberry.ico` is made from it, with the drawing cropped to fill
the square and sizes 16, 20, 24, 32, 40, 48, 64 and 256 px inside.
[`build.rs`](build.rs) builds that icon into the `.exe` (through
`assets/chalkberry.rc`), so it shows in Explorer, on the taskbar, on
shortcuts and in the tray. If the logo changes, make a new `.ico` with the
same sizes from the SVG (e.g. with Inkscape or ImageMagick) and rebuild.

## Make an installer

The installer is built with the free [Inno Setup](https://jrsoftware.org/isinfo.php).

1. Build the app: `cargo build --release`
2. Compile the installer script, either by opening
   [`installer/chalkberry.iss`](installer/chalkberry.iss) in Inno Setup and
   pressing **Compile**, or from the command line:

   ```powershell
   & "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" installer\chalkberry.iss
   ```

   (Use `C:\Program Files (x86)\Inno Setup 6\ISCC.exe` if you installed
   Inno Setup for all users.)

3. The installer is written to `installer\output\ChalkberrySetup-<version>.exe`.

It installs for the current user (no administrator rights needed), adds a
Start menu shortcut, and offers to start the app with Windows and to add a
desktop shortcut. It can be removed from **Settings → Apps**. When you
release a new version, update `AppVersion` in the script to match
`Cargo.toml`.

## Develop

Run these before finishing any change:

```powershell
cargo fmt
cargo clippy -- -D warnings
cargo test
```

- Code rules and the module map are in [`CLAUDE.md`](CLAUDE.md).
- The menu's colour, thickness and opacity choices, the startup pen, and the
  hotkeys live in [`src/settings/config.rs`](src/settings/config.rs).
