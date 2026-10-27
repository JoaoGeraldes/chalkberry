# Chalkberry

![Chalkberry logo](assets/chalkberry-logo.svg)

A tiny native Windows app for drawing on top of the screen. While it runs,
**hold Ctrl and click or drag** to draw smooth, chalk-textured lines over
anything. Without Ctrl, the mouse works normally.

## Download

**[Download the latest version](https://github.com/JoaoGeraldes/pen-marker-windows/releases/latest)** (Windows 10 or 11, 64-bit).

On the release page, under **Assets**, pick one:

- **`ChalkberrySetup-<version>.exe`**: the installer. Adds Chalkberry to the
  Start menu and can start it with Windows. Recommended.
- **`chalkberry-<version>-portable.exe`**: the app on its own, nothing to
  install. Put it anywhere and double-click it.

**"Windows protected your PC"?** The files are not code-signed, so Windows
SmartScreen warns about new apps from unknown publishers. Click
**More info → Run anyway**.

**Check your download (optional).** Each release lists a SHA-256 checksum
for every file. In PowerShell, run `Get-FileHash` on the file you downloaded
and check that the hash matches the one on the release page:

```powershell
Get-FileHash .\ChalkberrySetup-0.1.0.exe
```

## Contents

- [Download](#download)
- [Using it](#using-it)
  - [Floating toolbar](#floating-toolbar)
  - [Tray icon](#tray-icon)
  - [Screenshots](#screenshots)
  - [Settings](#settings)
- [Requirements](#requirements)
- [Build the app](#build-the-app)
  - [App icon](#app-icon)
- [Make an installer](#make-an-installer)
  - [Releasing a new version](#releasing-a-new-version)
  - [Publishing on GitHub](#publishing-on-github)
- [Develop](#develop)

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
desktop shortcut. It can be removed from **Settings → Apps**.

### Releasing a new version

Inno Setup does not change the version by itself. It is written by hand in
two places, which must match:

| File | Line |
|---|---|
| `Cargo.toml` | `version = "0.1.0"` |
| `installer/chalkberry.iss` | `#define AppVersion "0.1.0"` |

For each release:

1. Pick the new number: the last part for fixes (`0.1.0` → `0.1.1`), the
   middle one for new features (`0.1.1` → `0.2.0`).
2. Change it in both files.
3. Build the app and compile the installer (steps above). The setup file is
   named after the version, e.g. `ChalkberrySetup-0.2.0.exe`.

Running a newer setup upgrades the installed app in place: same folder, one
entry in **Settings → Apps** showing the new version, and the user's
settings kept. Inno Setup recognises the existing install by `AppId` in the
script, so never change that line.

### Publishing on GitHub

Finished files are published as a **GitHub Release**, never committed to
the repository (they would bloat its history forever; `installer/output/` is
in `.gitignore` for this reason). The examples use version `0.2.0`.

1. **Commit the version change** (both files from the section above) and
   push it.

2. **Tag that commit** with the version, starting with `v`:

   ```powershell
   git tag v0.2.0
   git push origin v0.2.0
   ```

3. **Prepare the files** next to the installer: copy the app as a portable
   version, then get the checksums of both files.

   ```powershell
   Copy-Item target\release\chalkberry.exe installer\output\chalkberry-0.2.0-portable.exe
   Get-FileHash installer\output\*0.2.0*.exe | Format-List Hash, Path
   ```

4. **Create the release.** On GitHub: **Releases → Draft a new release**,
   choose the tag `v0.2.0`, and use `Chalkberry 0.2.0` as the title.

5. **Write the notes.** Paste this and fill it in:

   ```markdown
   ## What's new
   - …

   ## Fixed
   - …

   ## Download
   - **ChalkberrySetup-0.2.0.exe**: installer (recommended)
   - **chalkberry-0.2.0-portable.exe**: no install needed

   Windows may warn that the app is from an unknown publisher:
   click **More info → Run anyway**.

   ## SHA-256 checksums
   - `ChalkberrySetup-0.2.0.exe`: `<hash>`
   - `chalkberry-0.2.0-portable.exe`: `<hash>`
   ```

6. **Attach the files.** Drag `ChalkberrySetup-0.2.0.exe` and
   `chalkberry-0.2.0-portable.exe` from `installer\output\` into the
   assets box. GitHub adds the source code downloads by itself.

7. **Publish**, then open the
   [latest release link]({LATEST}) to check it shows the new
   version. The README's Download link always points there, so it never
   needs updating.

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
