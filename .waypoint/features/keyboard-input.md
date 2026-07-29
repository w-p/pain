# Keyboard input encoding

**Shipped:** 2026-07-29. Replaces the Milestone 1 scaffolding, which was
still in place.

## What exists

`crates/app/src/keys.rs` turns a key press into the bytes the program in the
pane receives. Two encodings, chosen per keypress from the pane's live
`TermMode`:

- **Legacy** (`keys::legacy`) — the xterm-compatible encoding every program
  understands. Cursor and editing keys as CSI sequences, C0 control bytes,
  Alt as an ESC prefix.
- **Kitty keyboard protocol** (`keys::kitty`) — used only while a program
  has turned it on. `crates/pane`'s `Screen::new` sets
  `alacritty_terminal`'s `Config::kitty_keyboard`, which makes the backend
  track the mode stack and answer the query.

`keys::Press` is the input type: the fields of winit's `KeyEvent` that the
encoding depends on, borrowed rather than owned.

## Why it is built this way

**It is not a house style, and there was no room for one.** Programs match
literal byte sequences. The previous implementation handled eight named keys
— Enter, Backspace, Tab, Escape, four arrows — plus Ctrl+letter and a raw
text fallback, and everything else was silently dropped. `Home`, `End`,
`Page Up`/`Down`, `Insert`, `Delete` and `F1`–`F12` produced nothing at all;
`Shift+Tab` sent a plain tab, so it moved *forward* through a form; `Alt`
was not sent as Meta, so readline's word motions did nothing; application
cursor mode was ignored, so arrows were wrong inside `vim` and `less`.

**The kitty protocol is not a feature, it is the fix for a specific
impossibility.** `Shift+Enter`, `Ctrl+Enter` and `Enter` are the same byte
in the legacy encoding — there is no modifier field to put the difference
in. A program that wants them distinguished has to ask for a different
encoding, and this is the one kitty, Ghostty, WezTerm, Alacritty and foot
all implement.

**Enabling the backend's tracking without writing the encoder would have
been worse than leaving it off.** Once the mode is advertised, the program
believes the sequences are coming. The two land together, deliberately.

**`Press` exists to make the rules testable.** winit's `KeyEvent` carries a
per-platform field with no portable constructor, so it cannot be built in a
unit test on any machine. Taking the winit type whole would have left every
encoding rule in this module verifiable only by hand, on hardware. Borrowing
the four fields that matter costs one struct and buys the test suite.

**Chords are resolved before this module runs.** `Router::dispatch_chord`
gets first refusal; anything it doesn't claim reaches `keys::encode`. A key
is a chord or input, never both.

## Consequences worth knowing

- The encoding depends on modes the *program* sets, so it is read from the
  focused pane at press time (`Graphics::focused_term_mode`) and never
  cached.
- Key *releases* only produce bytes under the kitty protocol, and only when
  the program asked for `REPORT_EVENT_TYPES`. Legacy mode drops them, which
  is correct.
- Super/Cmd is never forwarded as input. It belongs to the OS and to this
  app's own chords; passing it through would deliver a bare unmodified
  character to the shell.
- Backspace sends DEL (`0x7f`), not BS, even though Windows composes BS —
  BS is `backward-kill-word` to a line editor, so honoring the OS's
  composition would erase a word per press. `Ctrl+Backspace` is the one
  that means kill-word, and gets BS for that reason.
- The kitty encoder implements `DISAMBIGUATE_ESC_CODES`,
  `REPORT_EVENT_TYPES`, `REPORT_ALL_KEYS_AS_ESC` and
  `REPORT_ASSOCIATED_TEXT`. A program requesting flags beyond these gets the
  behaviour of the ones that are implemented rather than a refusal.
