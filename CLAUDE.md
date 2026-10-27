# CLAUDE.md — Chalkberry

Chalkberry, a Windows-native screen marker: hold **Ctrl** and click/drag to draw on top of everything.
Rust (edition 2024) + `windows` crate; Direct2D → DXGI swap chain → DirectComposition.

**Modules** (one folder per feature; *pure* = never imports `windows`):

- `main.rs` start-up + message loop · `tray.rs` tray icon + settings menu · `build.rs` builds the icon (`assets/chalkberry.rc` + `.ico`, made from `chalkberry-logo.svg`) into the .exe
- `app/` the hub: `overlay` full-screen window, wires everything together · `command` user commands + ids (pure)
- `drawing/` `stroke` model + smoothing (pure) · `chalk` grain texture (pure) · `renderer` GPU drawing · `input` Ctrl keyboard hook · `cursors` pen + crosshair cursors
- `toolbar/` `window` floating toolbar window · `layout` layout, hit testing + tooltip texts (pure)
- `screenshot/` `capture` screen capture, PNG saving, folder picker · `crop` crop box, file names, panel layout (pure) · `crop_window` crop screen
- `notices/` `callout` speech bubbles (tooltips, notices) · `placement` bubble placement (pure) · `glow` attention glow (toolbar, save panel) · `composition_window` shared transparent click-through window
- `settings/` `config` pen choices, defaults + hotkeys (pure) · `file` settings.json load/save (no `windows`) · `json` flat JSON read/write (pure)

## Rules

1. **Simplicity first.** Write the smallest clear solution. No abstractions, traits, or features "for later".
2. **Clear names.** Full words that say what things are or do (`begin_stroke`, `is_ctrl_held`, `width_px`). No `tmp`, `data`, `mgr`.
3. **Small functions.** One job each, ~40 lines max, early returns over nesting, no magic numbers.
4. **Low coupling.** Modules share small plain types (`Point`, `StrokeShape`). Pure modules (marked above) never import `windows`; input, tray and toolbar know nothing about strokes.
5. **Few dependencies.** Only `windows` and `windows-numerics` (plus build-time-only `embed-resource` for the icon). Write it yourself if it fits in ~100 clear lines.
6. **Safe `unsafe`.** Keep blocks tiny, add a `// SAFETY:` comment to each, release handles in `Drop`, no `unwrap()` at runtime.
7. **Stay light.** Zero CPU when idle: redraw only on input. No per-frame allocations. Keep hook callbacks fast.
8. **Document what matters.** `//!` on each module (its job and what it must not do), `///` on public items, comments explain *why*.
9. **Test vital logic.** Unit tests in `#[cfg(test)] mod tests` next to the code, named as behaviour (`single_click_creates_dot`). Bug fix → test.
10. **Before finishing:** run `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`. Never `git commit`; leave changes for review.
