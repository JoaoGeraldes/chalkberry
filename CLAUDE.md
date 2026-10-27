# CLAUDE.md — windows-pen-marker

Windows-native screen marker: hold **Ctrl** and click/drag to draw on top of everything.
Rust (edition 2024) + `windows` crate; Direct2D → DXGI swap chain → DirectComposition.

**Modules:** `main.rs` wiring · `overlay.rs` window + mouse/hotkeys · `input.rs` Ctrl keyboard hook ·
`stroke.rs` stroke model + smoothing (pure) · `chalk.rs` chalk grain texture (pure) ·
`renderer.rs` GPU drawing · `tray.rs` tray icon + settings menu · `toolbar.rs` floating toolbar window ·
`toolbar_layout.rs` toolbar layout, hit testing + tooltip texts (pure) · `command.rs` user commands + ids (pure) ·
`callout.rs` speech-bubble window (tooltips, notices) · `callout_placement.rs` bubble placement (pure) ·
`cursors.rs` native pen + crosshair cursors · `glow.rs` start-up glow around the toolbar ·
`composition_window.rs` shared transparent click-through window (callout, glow) ·
`screenshot.rs` screen capture, PNG saving, folder picker · `crop_window.rs` screenshot crop screen ·
`crop.rs` crop box, cropping, file names, panel layout (pure) ·
`config.rs` pen choices + hotkeys · `settings.rs` settings.json load/save (no `windows`) ·
`json.rs` flat JSON read/write (pure).

## Rules

1. **Simplicity first.** Write the smallest clear solution. No abstractions, traits, or features "for later".
2. **Clear names.** Full words that say what things are or do (`begin_stroke`, `is_ctrl_held`, `width_px`). No `tmp`, `data`, `mgr`.
3. **Small functions.** One job each, ~40 lines max, early returns over nesting, no magic numbers.
4. **Low coupling.** Modules share small plain types (`Point`, `StrokeShape`). Pure modules (`stroke`, `chalk`, `command`, `toolbar_layout`, `callout_placement`, `crop`, `json`, `settings`) never import `windows`; input, tray and toolbar know nothing about strokes.
5. **Few dependencies.** Only `windows` and `windows-numerics`. Write it yourself if it fits in ~100 clear lines.
6. **Safe `unsafe`.** Keep blocks tiny, add a `// SAFETY:` comment to each, release handles in `Drop`, no `unwrap()` at runtime.
7. **Stay light.** Zero CPU when idle: redraw only on input. No per-frame allocations. Keep hook callbacks fast.
8. **Document what matters.** `//!` on each module (its job and what it must not do), `///` on public items, comments explain *why*.
9. **Test vital logic.** Unit tests in `#[cfg(test)] mod tests` next to the code, named as behaviour (`single_click_creates_dot`). Bug fix → test.
10. **Before finishing:** run `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`. Never `git commit`; leave changes for review.
