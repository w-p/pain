# 2026-07-29 — sRGB double-encode, the keyboard encoder, and the theme picker

Session opened with the developer frustrated, and fairly: three separate
areas were doing non-standard things in well-trodden territory. The framing
question was "help me understand" — why these classes of bug exist at all in
a terminal inspired by iTerm2 and Terminator.

The honest common cause: each area was built to the minimum that made a demo
look right and never checked against the standard. Worth remembering as a
pattern, not just three fixes.

---

## 1. Every color was one gamma-encode too bright

**Evidence.** Developer's screenshot: pain with `theme = "Ayu"` over
terminalcolors.com's Ayu reference. `ls` on a `/mnt/c` path (everything
other-writable, so `34;42` — blue on green) rendered as pale mint blocks
instead of Ayu's `#7fd962`. Working the arithmetic forward: `0x7f/255 =
0.498`, sRGB-encoded → `0.75`; `0xd9` → `0.937`; `0x62` → `0.66`; i.e.
`#bfefa8`. Exactly the pale mint in the screenshot. That was the confirmation
before touching anything.

**Root cause.** The swapchain is `Bgra8UnormSrgb` — confirmed at runtime, in
egui-wgpu's own startup warning ("Detected a linear (sRGBA aware)
framebuffer Bgra8UnormSrgb"). An sRGB-format target means the GPU
gamma-*encodes* whatever the shader writes. `config::unpack_rgb` produced
`hex / 255.0`, which is an sRGB value, and it went to the shader unchanged.
Encoded twice, displayed far too bright.

**Not new, and previously misdiagnosed as a local problem.** `graphics.rs`
already had an `srgb_encode` helper whose doc comment describes this exact
mechanism correctly — it was written to fix *title-bar text contrast*, by
compensating for the double encode at one call site instead of removing it.
That is the most instructive part of this bug: the mechanism was understood
and the conclusion drawn was "adjust this one calculation."

**Fix.** One conversion point, in `render/src/shader.wgsl`: `srgb_to_linear`
applied to the instance color immediately before the premultiplied write.
This works precisely because *every* color in the app is authored as sRGB —
theme tables, and the hand-picked chrome constants too (`TEXT_COLOR`
`[0.875, 0.886, 0.902]` is `#dfe2e6`, the Graphite foreground; `TITLE_BAR_BG`
is `#14171b`). No caller needed changing.

Three consequences that had to be handled with it:

- **The color-emoji atlas.** `Rgba8Unorm` holding sRGB bitmaps, premultiplied
  on upload. Decoding the premultiplied value directly would darken
  semi-transparent glyph edges by the alpha's own curve, so the shader
  un-premultiplies, decodes, re-premultiplies. Alpha is never gamma-encoded.
- **The clear color** never passes through a shader, so it needs a CPU-side
  `srgb_decode` (new, in `graphics.rs`, replacing `srgb_encode`).
- **`contrasting_text_color` had to stop compensating.** Its `srgb_encode`
  call was correct *for the broken pipeline*; with the source fixed, keeping
  it would be wrong in the other direction.

**Found alongside:** the clear color was not premultiplied by alpha despite
the surface being `CompositeAlphaMode::PreMultiplied`. Under premultiplied
compositing the result is `src + dst*(1-a)`, so an unscaled background
composites brighter than intended at any transparency below 1.0. Fixed in the
same expression.

## 2. The keyboard layer was still Milestone 1 scaffolding

`main.rs`'s `key_bytes` handled eight named keys — Enter, Backspace, Tab,
Escape, four arrows — plus Ctrl+letter and an `event.text` fallback. Nothing
was blocking or ignoring keys; the rest were never written. Its own doc
comment said as much and had been true since Milestone 1.

Missing and now implemented in a new `crates/app/src/keys.rs`:

- `Home`/`End`/`PageUp`/`PageDown`/`Insert`/`Delete`, `F1`–`F12`
- `Shift+Tab` → `CSI Z` (back-tab). It had been sending a plain tab, so the
  key did the *opposite* of what it means.
- xterm modifier parameters on every functional key (`CSI 1;5C`, `CSI 3;6~`)
- Alt as Meta (ESC prefix) — without it readline's `Alt+B`/`Alt+F`/`Alt+D` do
  nothing
- `APP_CURSOR` (DECCKM): `SS3` rather than `CSI` for cursor keys, which is
  what `vim` and `less` expect. Modified keys revert to the CSI form because
  SS3 has no parameter field.
- Ctrl with punctuation and the digit aliases (`Ctrl+[`, `Ctrl+/`, `Ctrl+Space`)
- `Ctrl+Backspace` → BS, keeping plain Backspace as DEL

**Shift+Enter needed more than the legacy encoding.** It is genuinely
inexpressible there — Enter, Shift+Enter and Ctrl+Enter are one byte with no
modifier field. The standard answer is the kitty keyboard protocol.
`alacritty_terminal` already parses it and tracks the mode stack, but
`Config::kitty_keyboard` defaults to false, so we never advertised it. Now
enabled in `pane::Screen::new`, with the matching encoder in `keys::kitty`
covering `DISAMBIGUATE_ESC_CODES`, `REPORT_EVENT_TYPES`,
`REPORT_ALL_KEYS_AS_ESC` and `REPORT_ASSOCIATED_TEXT`.

Enabling the mode without an encoder would have been *worse* than leaving it
off — the program would believe the sequences are coming. Noted in the code.

**Testing note worth keeping:** winit's `KeyEvent` carries a per-platform
field with no portable constructor, so it cannot be built in a unit test.
The encoder therefore takes a small `keys::Press` borrowed from it instead of
the event itself. That is what made 11 tests of the actual encoding rules
possible; taking the winit type whole would have left the whole module
untestable.

## 3. Theme picker — three egui defaults nobody checked

- `ComboBox` defaults to `PopupCloseBehavior::CloseOnClick`, so the click
  that focuses the filter field closed the dropdown. Now
  `CloseOnClickOutside`, with the list closing the popup explicitly on
  selection.
- `ComboBox::show_ui` **already wraps its body in a `ScrollArea`** capped at
  `spacing.combo_height` (200.0). The picker added a second one inside it —
  hence a large scrollbar that moved the whole panel a few pixels while the
  list's real one was clipped. Fixed by sizing the ComboBox to fit the body
  so only the list's own scrollbar is ever live.
- The 100-of-602 cap with its "keep typing" footer is gone;
  `ScrollArea::show_rows` builds only the visible rows, so the full list
  costs nothing. The cap's original justification (hundreds of widgets per
  frame) was real but the remedy was the wrong one.

---

## Verification status

`cargo test` green (274 tests, 11 of them new in `keys`), clippy clean,
formatted. The app was built and launched here to confirm it runs and to
capture the framebuffer format from the log.

**Not visually verified.** Screenshots still cannot be captured from this
WSL session (`import -window root` → "Resource temporarily unavailable"),
the same gap recorded earlier in this project. The color change needs the
developer's eyes on real hardware — and note it is a deliberate, visible
change to *everything*, including chrome the developer may have tuned by eye
against the broken pipeline. Group title bars and dividers will render darker
and more saturated than before. That is correct, but it is a change.

Added `wgpu: surface format {:?}, alpha mode {:?}` under
`--verbose` so the sRGB question is answerable on any machine without
reading egui's warning.

---

## Second pass, same session — developer's follow-up on the four fixes

Confirmed working: dropdown filtering, and the title bar's new (darker,
correct) grey. Four new items.

### Ctrl+Plus/Minus were being eaten by egui

The reported symptom was "after pressing ctrl + or ctrl -, the spawn location
of both right-click menus is offset." Root cause: **nothing in the keymap
bound those chords at all.** egui claims `Ctrl+Plus`/`Minus`/`0` by default
(`Options::zoom_with_keyboard`, applied in `Context::end_pass` via
`gui_zoom::zoom_with_keyboard`) and uses them to change its own
`zoom_factor` in 0.1 steps. That changes `egui_winit::pixels_per_point`
(= `zoom_factor × native_ppp`), which is the divisor `Ui::show` uses to
convert our physical-pixel cursor position into egui points for the menus.

Same shape as the recurring theme in this project: a dependency's default
behaviour silently owning something the app should own.

Fixed on both sides — `zoom_with_keyboard` turned off, and the chords bound
to a real `Action::FontSize`/`ResetFontSize` that changes the terminal font
by one point and saves it.

**`Chord`'s `Display` could not round-trip `+`.** The chord format is
`+`-separated, so `Key::Char('+')` printed as `ctrl++`, which
`parse_chord` reads as two empty segments. Caught immediately by the
existing `every_default_binding_prints_as_something_config_can_parse_back`
test — a good argument for that test existing. `plus`/`minus`/`equals` are
now both parsed and printed.

**"Ctrl+Plus" is one chord to a user and three to the OS.** Unshifted the key
reports `=`, shifted it reports `+`, and a numeric keypad sends `+` with no
Shift. All are bound. The README test (every default binding must appear in
the README) forced this to be documented rather than left as a surprise —
also a good argument for that test.

### The cursor was transparent to the desktop

`cursor_color` was the accent at alpha 0.5 and `selection_color` at 0.45.
That reads as "blend with the pane," but the alpha reaching the renderer is
*also* the window transparency the compositor uses — so both were partly
see-through to the desktop, not to the pane. Pre-existing, but far more
visible after the sRGB fix darkened everything.

Fixed by doing the blend on the CPU against the pane background (`blend`)
and handing the renderer alpha 1.0. The cursor went further, to a solid
block with the glyph under it drawn in reverse video
(`cursor_glyph_color`), which is what every terminal does and what the
developer asked for.

**General rule worth keeping:** in this renderer, alpha is window
transparency. Anything wanting to look translucent *against the pane* must
composite on the CPU and emit alpha 1.0.

### Theme dropdown height

Was `THEME_LIST_HEIGHT = 260.0`, a pixel constant. Now expressed as
`THEME_LIST_ROWS = 12` and derived from the live style
(`theme_row_height`/`theme_list_height`), which is also the row height
`ScrollArea::show_rows` needs to position rows correctly — a hardcoded
number that drifts from the real row height puts the wrong slice on screen.
`auto_shrink([false, false])` keeps the height stable while filtering, so
rows don't move under the pointer.

### Title bar color

New `appearance.title_bar_color`, defaulting to the `#14171b` the chrome
already drew, so nothing restyles on upgrade. Text color is computed by
`contrasting_text_color` rather than fixed, so a pale bar gets dark text.
Applies only to ungrouped panes — a group's color is the only way to tell
groups apart, so it still wins. `#[serde(default)]` on `Appearance` means
older configs load unchanged; there is a test pinning that.

`TITLE_BAR_BG` is gone from `graphics.rs`; the default now lives in
`config` as `DEFAULT_TITLE_BAR_RGB`.

## Still unverified

Everything visual, again — no screenshots from this session. The app builds,
launches and stays up here. Needs the developer's eyes for: the cursor's new
solid/reverse-video look, the twelve-row dropdown, the title bar picker, and
whether the menu offset is actually gone after Ctrl+Plus.

---

## Third pass — chord format and the settings scrollbar

### Chords are space-separated now

Developer's call: `ctrl shift left`, `ctrl +`, `ctrl -` rather than
`ctrl+shift+left`. More readable, and it dissolves the problem the previous
pass had to work around — a separator that is also a key on the keyboard
cannot represent that key, which is why `plus`/`minus`/`equals` had to be
invented at all. Those names are now only accepted, never printed.

Space becomes the one key needing a name (`ctrl space`), which is the same
trade one level down but a far rarer key than `+`.

**Backwards compatibility was the real design question.** Config files with
`"ctrl+shift+e"` are on disk, and a chord that silently stops parsing looks
exactly like a keybinding that stopped working. `segments()` splits on
whitespace first, then splits any remaining segment on `+` — except a
segment that *is* `+`, which is the plus key itself. So both formats parse,
including mixed, and only the new one is ever printed. Tests pin all of it.

The settings panel's keybinding list is `Display`-driven, so it followed for
free — but its tests looked rows up by the printed string and had to be
updated. One of them now deliberately supplies the override in the *old*
format and looks the row up in the *new* one, so the compatibility path has
a test rather than being incidental.

Docs (README tables, man page, config examples) moved to the new form; test
fixtures deliberately kept some old-form strings, since that is the
compatibility path.

### Settings scrollbar overlapped the controls

egui's scroll bars default to `floating: true` with
`floating_allocated_width: 0.0` — they are drawn *over* the content and
reserve no width, so any control sized to `ui.available_width()` runs
underneath the bar. Visible whether or not it is hovered, since the bar is
dimmed rather than hidden when idle.

Fixed in `apply_chrome_style` by setting
`spacing.scroll.floating_allocated_width = bar_width + 4.0`, rather than
subtracting a margin at the one call site — every scrolling region in the
chrome (settings, keybinding list, theme dropdown) gets the same gutter and
no future one can forget it.

**Pattern, third time this session:** a dependency default silently owning
something the app should own — `zoom_with_keyboard`, `CloseOnClick`,
`ComboBox`'s built-in ScrollArea, and now `floating_allocated_width`. Worth
checking egui defaults explicitly when adding chrome rather than assuming
the stock behaviour suits a terminal.

---

## Fourth pass — the keybinding list's presentation

Developer read `ctrl =` and `ctrl +` as accidental duplicates. They aren't:
on a US layout the `=`/`+` key reports `=` unshifted and `+` shifted, and a
numeric keypad's plus is a third key again. All three are genuinely
different inputs meaning one thing to the person pressing them.

But "correct and reads as a bug" is still a bug, in the display. Fixed by
grouping: `effective_binding_rows` now returns one row per action with all
its chords (`chords: Vec<String>`), so the list shows
`ctrl +, ctrl =, ctrl shift +   font_size_increase` on one line.

Grouped on `(action, custom)`, not action alone — a chord the config rebound
and one that came that way by default are different facts about the same
action, and merging them would put "(custom)" on bindings nobody touched.
There is a test for exactly that.

Rendering changed to a two-column `egui::Grid` (no header, no borders) and
the `→` between chord and action is gone. The developer's reasoning is worth
keeping: an aligned column already communicates the pairing, so the arrow was
one more symbol to interpret and nothing more.

Verified the output by temporarily adding a test that printed every row, then
removing it — a cheap way to see list formatting in an environment where
screenshots don't work.
