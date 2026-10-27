# Windows Pen Marker

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

The chalkboard icon in the taskbar's notification area opens a menu with
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
`Pictures\Pen Marker`). **Save** (or Enter) saves, **Cancel** (or Esc)
discards. A bubble then shows the saved file's path.

The chosen folder is remembered in `%APPDATA%\Pen Marker\`. Nothing else is
stored: the pen settings reset to the defaults each time the app starts.

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
`target\release\windows-pen-marker.exe`. It needs nothing else installed
(the C runtime is linked in, see [`.cargo/config.toml`](.cargo/config.toml)),
so you can copy it anywhere and double-click it.

## Make an installer

The installer is built with the free [Inno Setup](https://jrsoftware.org/isinfo.php).

1. Build the app: `cargo build --release`
2. Compile the installer script, either by opening
   [`installer/pen-marker.iss`](installer/pen-marker.iss) in Inno Setup and
   pressing **Compile**, or from the command line:

   ```powershell
   & "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" installer\pen-marker.iss
   ```

   (Use `C:\Program Files (x86)\Inno Setup 6\ISCC.exe` if you installed
   Inno Setup for all users.)

3. The installer is written to `installer\output\PenMarkerSetup-<version>.exe`.

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
  hotkeys live in [`src/config.rs`](src/config.rs).
