# Windows Pen Marker

A tiny native Windows app for drawing on top of the screen. While it runs,
**hold Ctrl and click or drag** to draw smooth marker lines over anything.
Without Ctrl, the mouse works normally.

| Action | Shortcut |
|---|---|
| Draw | Hold **Ctrl** + click / drag |
| Clear drawings | **Ctrl + Alt + C** |
| Quit | **Ctrl + Alt + Q** |

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
- Pen colour, width, and hotkeys live in [`src/config.rs`](src/config.rs).
