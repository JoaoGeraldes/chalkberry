# Windows Pen Marker

A tiny native Windows app for drawing on top of the screen. While it runs,
**hold Ctrl and click or drag** to draw smooth, chalk-textured lines over
anything. Without Ctrl, the mouse works normally.

| Action | Shortcut |
|---|---|
| Draw | Hold **Ctrl** + click / drag |
| Erase a stroke | Hold **Ctrl** + right-click it (or right-drag across several) |
| Clear drawings | **Ctrl + Alt + C** |
| Quit | **Ctrl + Alt + Q** |
| Change colour, thickness, opacity | Floating toolbar, or the chalkboard icon in the taskbar's notification area |

### Floating toolbar

A small dark bar appears at the top of the screen. Drag it by any empty spot
(the dotted grip on the left is the obvious handle) to move it anywhere.

| Button | What it does |
|---|---|
| Coloured dot | Opens a row of colours |
| Grey dot | Opens a row of thicknesses (dots drawn at real size) |
| Dot in a ring | Opens a row of opacities |
| Bin | Clears the drawing |
| Dash | Hides the toolbar; a notification says it is still in the tray |

A small hint under the buttons reminds you how to draw and erase.

Bring a hidden toolbar back with **Show toolbar** in the tray icon's menu.
New settings apply to the next strokes; lines already drawn keep their look.
If the tray icon is not visible, it may be in the hidden-icons overflow (the
**^** arrow); drag it onto the taskbar to keep it in view.

## Requirements

- Windows 10 (1903+) or Windows 11, x64
- [Rust](https://rustup.rs) with the default MSVC toolchain
- Visual Studio Build Tools with the **"Desktop development with C++"** workload (provides the linker)

## Run

```powershell
cargo run            # debug build, with a console for error messages
cargo run --release  # optimized build, no console window
```

The release executable is written to `target\release\windows-pen-marker.exe`.

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
