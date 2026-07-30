# Settings as its own window

**Shipped:** 2026-07-29. Replaces the floating `egui::Window` panel.

## What exists

`crates/app/src/settings_window.rs` — a real OS window with its own
surface and its own `egui::Context`, sharing the terminal window's
`wgpu::Device`. `Graphics` owns it as `Option<SettingsWindow>`; `None` is
"closed", and closing is what discards the in-progress draft.

The form itself did not move. `ui::settings_panel` is the same code,
extracted out of `Ui::show` into a function taking a plain `&mut egui::Ui`,
so it still sits beside the widget helpers and the `SettingsDraft` it is
built from.

## Why it is built this way

**A second window, not egui's multi-viewport.** egui 0.35 has `ViewportId`
and deferred viewports, but the *backend* has to implement
`ViewportCommand` handling — `eframe` does, a hand-rolled winit+wgpu
backend does not get it free. Implementing that is strictly more work than
creating the window ourselves, for a feature used by exactly one panel.

**Shared device, separate surface.** Two surfaces on one `wgpu::Device` is
the normal arrangement and keeps one glyph atlas and one renderer
allocation. This is why `Graphics` now retains the `wgpu::Instance` and
`Adapter`, which used to be locals in `new` and dropped — you need the
instance to create a second surface later.

**The settings window is deliberately ordinary.** The terminal window is
not: transparent-capable, `WS_EX_NOREDIRECTIONBITMAP` on Windows, a
DirectComposition swapchain requesting `PreMultiplied` alpha. None of that
is inherited here. Inheriting it would mean debugging the same compositor
problems twice for a window that is an opaque form.

**One `chrome_context()` builds both contexts.** Every egui default this
project had to override — `zoom_with_keyboard = false`, the scrollbar
gutter, the chrome font and style — is per-`Context`. A second context that
quietly missed one would fail in a way nobody would trace back: settings
widgets rescaling on `Ctrl+Plus` while the terminal font stays put, say.
Sharing the constructor makes that impossible rather than remembered.

**"Settings..." travels back up to `main` as a request.** Creating a window
needs an `ActiveEventLoop`, which `Graphics` never has. `UiRequest::
open_settings` is set during the menu's frame and collected by the event
handler *after* `redraw` returns — before it, and the window would be a
frame late or never appear at all if nothing else asked for a repaint.

## Consequences worth knowing

- **The terminal must be told to repaint when a draft changes.** It is a
  different window; drawing the settings window repaints nothing else. So
  `redraw_settings_window` returns whether the preview changed, and the
  caller requests a terminal redraw when it did. Miss this and the
  transparency slider appears to do nothing until you happen to move the
  mouse over the terminal.
- **The preview is applied on top of `saved_settings`, never on top of
  `settings`.** Applying it to the already-previewed config would make each
  frame's preview the next frame's baseline, and the edits would compound.
- **`CloseRequested` means different things per window.** On the terminal it
  quits the application; on this one it closes the panel and reverts the
  preview, which is what Cancel does — closing without saving *is*
  cancelling.
- The keybinding list is built once, when the window opens. Building it
  runs `Keymap::apply_overrides`, which reports unparseable lines to
  stderr, so a per-frame rebuild would turn one bad config line into an
  endless stream of warnings. `Ui` used to cache it for exactly this
  reason; the window's lifetime is now the right scope on its own.
- Failing to create the window is reported and otherwise ignored. The
  terminal keeps working and `config.toml` is still editable by hand.
