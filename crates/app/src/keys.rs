//! Encodes a key press into the bytes a terminal sends to the program
//! running in the pane.
//!
//! This is the xterm-compatible encoding every terminal emulator implements,
//! not an invention of this app's. A shell, `vim`, `htop` or a TUI framework
//! recognizes exactly these byte sequences; anything else reads as garbage or
//! as nothing at all. There is no room here for a house style.
//!
//! Two encodings coexist, chosen per keypress by the pane's current mode:
//!
//! - **Legacy** (`legacy`) — the default, and what every program understands.
//!   Cursor and editing keys are CSI sequences, control characters are the
//!   C0 bytes, Alt is an ESC prefix. Its known limitation is that it cannot
//!   represent many combinations at all: `Shift+Enter`, `Ctrl+Enter` and
//!   plain `Enter` all collapse to a single carriage return, because CR is
//!   the only byte there is for them.
//! - **Kitty keyboard protocol** (`kitty`) — the modern fix for exactly that
//!   ambiguity, supported by kitty, Ghostty, WezTerm, Alacritty and foot, and
//!   what editors and CLI tools now probe for when they want `Shift+Enter` to
//!   mean something different from `Enter`. A program turns it on explicitly
//!   (`CSI > flags u`); `alacritty_terminal` tracks the mode stack for us and
//!   answers the query, so all this module owes it is the matching encoder.
//!
//! Which one applies is never a preference — it is whatever the foreground
//! program asked for, read fresh from the pane's `TermMode` on every press.

use pane::TermMode;
use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// The parts of a key event the encoding actually depends on.
///
/// Borrowed from `winit`'s `KeyEvent` rather than passing that around
/// directly: it carries a per-platform field with no portable constructor,
/// so taking it whole would make this module's rules untestable on any
/// machine that isn't the one they'd break on.
#[derive(Debug, Clone, Copy)]
pub struct Press<'a> {
    pub key: &'a Key,
    /// What the OS composed for this key, if anything — the source of truth
    /// for ordinary typing, including dead keys and IME commits.
    pub text: Option<&'a str>,
    pub state: ElementState,
    /// Whether this is the OS repeating a held key.
    pub repeat: bool,
}

impl<'a> Press<'a> {
    pub fn new(event: &'a KeyEvent) -> Self {
        Self { key: &event.logical_key, text: event.text.as_deref(), state: event.state, repeat: event.repeat }
    }
}

/// Encodes `press` for a pane currently in `mode`.
///
/// `None` means the key produces no input — a bare modifier, a key with no
/// terminal meaning, or (under the legacy encoding) a release event.
pub fn encode(press: Press, modifiers: ModifiersState, mode: TermMode) -> Option<Vec<u8>> {
    if mode.contains(TermMode::DISAMBIGUATE_ESC_CODES) {
        kitty::encode(press, modifiers, mode)
    } else if press.state == ElementState::Pressed {
        legacy::encode(press, modifiers, mode)
    } else {
        None
    }
}

/// xterm's modifier parameter: a bitfield of Shift/Alt/Ctrl/Super, biased by
/// one so that "no modifiers" is 1 rather than 0. `CSI 1;5C` is Ctrl+Right,
/// because 5 is 1 + 4.
fn modifier_param(modifiers: ModifiersState) -> u8 {
    let mut bits = 0;
    if modifiers.shift_key() {
        bits |= 1;
    }
    if modifiers.alt_key() {
        bits |= 2;
    }
    if modifiers.control_key() {
        bits |= 4;
    }
    if modifiers.super_key() {
        bits |= 8;
    }
    bits + 1
}

/// The shape of a functional key's escape sequence. Which of the two a key
/// uses is historical, not principled — `Home` is a letter-terminated
/// sequence and `Insert` is a numbered one because that is what the VT220 and
/// its descendants did, and programs match on the exact bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sequence {
    /// `CSI <letter>`, or `SS3 <letter>` when the pane is in application
    /// cursor mode — cursor keys, `Home`/`End`, and `F1`-`F4`.
    Letter(u8),
    /// `CSI <number> ~` — the editing and function keypad.
    Numbered(u8),
}

/// The escape-sequence form of a named key, or `None` for keys that either
/// have a single-byte encoding of their own (`Enter`, `Tab`, ...) or no
/// terminal meaning at all.
fn sequence(key: &NamedKey) -> Option<Sequence> {
    use Sequence::{Letter, Numbered};
    Some(match key {
        NamedKey::ArrowUp => Letter(b'A'),
        NamedKey::ArrowDown => Letter(b'B'),
        NamedKey::ArrowRight => Letter(b'C'),
        NamedKey::ArrowLeft => Letter(b'D'),
        NamedKey::Home => Letter(b'H'),
        NamedKey::End => Letter(b'F'),
        NamedKey::F1 => Letter(b'P'),
        NamedKey::F2 => Letter(b'Q'),
        NamedKey::F3 => Letter(b'R'),
        NamedKey::F4 => Letter(b'S'),

        NamedKey::Insert => Numbered(2),
        NamedKey::Delete => Numbered(3),
        NamedKey::PageUp => Numbered(5),
        NamedKey::PageDown => Numbered(6),
        // The function keypad's numbering skips 16, 22, 27, 30 and 35 —
        // gaps left by the VT220 keys these were mapped onto, preserved by
        // every terminal since because programs match the literal numbers.
        NamedKey::F5 => Numbered(15),
        NamedKey::F6 => Numbered(17),
        NamedKey::F7 => Numbered(18),
        NamedKey::F8 => Numbered(19),
        NamedKey::F9 => Numbered(20),
        NamedKey::F10 => Numbered(21),
        NamedKey::F11 => Numbered(23),
        NamedKey::F12 => Numbered(24),
        _ => return None,
    })
}

/// Whether a `Letter` sequence is one of the keys application cursor mode
/// (DECCKM) applies to. A program that enables the mode — `vim`, `less`,
/// anything using readline's keypad handling — expects `SS3 A` rather than
/// `CSI A` for these, and will not recognize the other form.
fn is_cursor_key(letter: u8) -> bool {
    matches!(letter, b'A' | b'B' | b'C' | b'D' | b'H' | b'F')
}

/// The C0 control byte `Ctrl` produces with `c`, if any.
///
/// The letters are the obvious range (`Ctrl+A` = 1 .. `Ctrl+Z` = 26); the
/// rest is the ASCII table's own logic, where a control byte is the
/// character with bit 6 cleared. The digit aliases exist because the
/// characters they map to (`@`, `[`, `\`, `]`, `^`, `_`) need Shift on most
/// layouts, and terminals have always accepted the unshifted digit instead.
fn control_byte(c: char) -> Option<u8> {
    Some(match c {
        'a'..='z' => c as u8 - b'a' + 1,
        'A'..='Z' => c as u8 - b'A' + 1,
        ' ' | '@' | '2' => 0x00,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        // `Ctrl+/` is the same US-layout key as `Ctrl+_` without Shift, and
        // readline's "undo" binding is the reason anyone notices.
        '_' | '7' | '/' => 0x1f,
        '8' => 0x7f,
        _ => return None,
    })
}

/// The single character a `Key::Character` carries, or `None` if it is a
/// multi-character string (a dead-key composition or an IME commit), which
/// has no control-byte or modifier interpretation and is sent as text.
fn single_char(key: &Key) -> Option<char> {
    let Key::Character(text) = key else {
        return None;
    };
    let mut chars = text.chars();
    let c = chars.next()?;
    if chars.next().is_some() { None } else { Some(c) }
}

mod legacy {
    use super::*;

    /// The xterm-compatible encoding, used unless the program has turned on
    /// the kitty keyboard protocol.
    pub fn encode(press: Press, modifiers: ModifiersState, mode: TermMode) -> Option<Vec<u8>> {
        // Super/Command is never an input modifier in this encoding — it is
        // the OS's and the app's own (window management, `Cmd+C`). Passing
        // it through would send the bare character as if unmodified.
        if modifiers.super_key() {
            return None;
        }

        if let Key::Named(named) = press.key
            && let Some(bytes) = named_key(named, modifiers, mode)
        {
            return Some(bytes);
        }

        if let Some(c) = single_char(press.key) {
            if modifiers.control_key()
                && let Some(byte) = control_byte(c)
            {
                // Alt+Ctrl+key is the control byte with the same ESC prefix
                // any other Alt combination gets.
                return Some(with_alt(vec![byte], modifiers));
            }
            if modifiers.alt_key() {
                // Alt as Meta: ESC followed by the key's own bytes. This is
                // what makes readline's word-wise motions (`Alt+B`, `Alt+F`,
                // `Alt+D`) work, and what `set meta-flag` expects.
                let mut buffer = [0u8; 4];
                let text = press.text.unwrap_or(c.encode_utf8(&mut buffer));
                return Some(with_alt(text.as_bytes().to_vec(), modifiers));
            }
        }

        // Everything else is literal text, taken from the OS's own
        // composition so that dead keys, IME commits and shifted characters
        // arrive exactly as typed.
        let text = press.text.filter(|text| !text.is_empty())?;
        Some(text.as_bytes().to_vec())
    }

    /// Prefixes `bytes` with ESC when Alt is held — the "Meta sends Escape"
    /// convention, which is the default in every terminal that offers the
    /// choice at all.
    fn with_alt(bytes: Vec<u8>, modifiers: ModifiersState) -> Vec<u8> {
        if !modifiers.alt_key() {
            return bytes;
        }
        let mut prefixed = vec![0x1b];
        prefixed.extend(bytes);
        prefixed
    }

    /// The bytes for a named (non-text) key, or `None` if it has no terminal
    /// meaning — a bare modifier, a media key, `CapsLock`.
    fn named_key(key: &NamedKey, modifiers: ModifiersState, mode: TermMode) -> Option<Vec<u8>> {
        let param = modifier_param(modifiers);

        match key {
            // Shift+Tab is back-tab (CBT), a distinct sequence rather than a
            // modified Tab — it is how every form, pager and TUI moves focus
            // backwards. Sending a plain tab for it, as this used to, means
            // the program sees "forwards" and the key does the opposite of
            // what it says.
            NamedKey::Tab if modifiers.shift_key() => return Some(b"\x1b[Z".to_vec()),
            NamedKey::Tab => return Some(with_alt(b"\t".to_vec(), modifiers)),

            // The legacy encoding genuinely cannot distinguish Shift+Enter
            // or Ctrl+Enter from Enter: there is one byte for the key and no
            // modifier field anywhere to put the difference in. Programs
            // that need the distinction ask for the kitty protocol, which is
            // exactly why it exists — see this module's own docs.
            NamedKey::Enter => return Some(with_alt(b"\r".to_vec(), modifiers)),

            // DEL, not BS, and deliberately: Windows composes Backspace as
            // BS (0x08), but that is `backward-kill-word` to a line editor,
            // so honoring the OS's composition here would erase a whole word
            // per press. Ctrl+Backspace is the one that means "kill word",
            // and gets BS for that reason.
            NamedKey::Backspace if modifiers.control_key() => return Some(with_alt(vec![0x08], modifiers)),
            NamedKey::Backspace => return Some(with_alt(vec![0x7f], modifiers)),

            NamedKey::Escape => return Some(with_alt(vec![0x1b], modifiers)),
            NamedKey::Space if modifiers.control_key() => return Some(with_alt(vec![0x00], modifiers)),
            _ => {}
        }

        match sequence(key)? {
            Sequence::Letter(letter) => {
                if param > 1 {
                    // A modified key is always the CSI form with an explicit
                    // parameter, even in application cursor mode — SS3 has
                    // no parameter field to carry the modifier in.
                    Some(format!("\x1b[1;{param}{}", letter as char).into_bytes())
                } else if is_cursor_key(letter) && mode.contains(TermMode::APP_CURSOR) {
                    Some(format!("\x1bO{}", letter as char).into_bytes())
                } else if is_cursor_key(letter) {
                    Some(format!("\x1b[{}", letter as char).into_bytes())
                } else {
                    // F1-F4 are SS3-terminated regardless of cursor mode.
                    Some(format!("\x1bO{}", letter as char).into_bytes())
                }
            }
            Sequence::Numbered(number) => {
                if param > 1 {
                    Some(format!("\x1b[{number};{param}~").into_bytes())
                } else {
                    Some(format!("\x1b[{number}~").into_bytes())
                }
            }
        }
    }
}

mod kitty {
    use super::*;

    /// Event types, reported as the modifier parameter's sub-parameter when
    /// a program has asked for `REPORT_EVENT_TYPES`.
    const PRESS: u8 = 1;
    const REPEAT: u8 = 2;
    const RELEASE: u8 = 3;

    /// The kitty keyboard protocol encoding, used while a program has it
    /// enabled. Every key becomes unambiguous, which is the entire point:
    /// `Shift+Enter` is `CSI 13;2u` and cannot be confused with `Enter`.
    pub fn encode(press: Press, modifiers: ModifiersState, mode: TermMode) -> Option<Vec<u8>> {
        let report_events = mode.contains(TermMode::REPORT_EVENT_TYPES);
        let event_type = match press.state {
            ElementState::Released => RELEASE,
            ElementState::Pressed if press.repeat => REPEAT,
            ElementState::Pressed => PRESS,
        };
        // Releases exist in this protocol only for programs that asked for
        // them; otherwise they are silently dropped, as in legacy mode.
        if event_type == RELEASE && !report_events {
            return None;
        }

        let all_as_escapes = mode.contains(TermMode::REPORT_ALL_KEYS_AS_ESC);

        // Keys that already have an unambiguous escape sequence keep it,
        // parameterized the same way as in legacy mode. The protocol only
        // replaces encodings that were ambiguous to begin with.
        if let Key::Named(named) = press.key
            && let Some(Sequence::Letter(letter)) = sequence(named)
            && !report_events
        {
            let param = modifier_param(modifiers);
            if param > 1 {
                return Some(format!("\x1b[1;{param}{}", letter as char).into_bytes());
            }
            if is_cursor_key(letter) && mode.contains(TermMode::APP_CURSOR) {
                return Some(format!("\x1bO{}", letter as char).into_bytes());
            }
            return Some(format!("\x1b[{}", letter as char).into_bytes());
        }
        if let Key::Named(named) = press.key
            && let Some(Sequence::Numbered(number)) = sequence(named)
            && !report_events
        {
            let param = modifier_param(modifiers);
            if param > 1 {
                return Some(format!("\x1b[{number};{param}~").into_bytes());
            }
            return Some(format!("\x1b[{number}~").into_bytes());
        }

        let code = key_code(press)?;
        let has_modifiers = modifier_param(modifiers) > 1;
        let needs_escape = has_modifiers || report_events || all_as_escapes;

        // Unmodified text and the four legacy control keys stay in their
        // plain form unless the program asked for everything as escapes —
        // this is what keeps ordinary typing ordinary at the base level of
        // the protocol.
        if !needs_escape {
            return plain(press, code);
        }

        let mut params = format!("{code}");
        let modifier_param = modifier_param(modifiers);
        if modifier_param > 1 || report_events {
            params.push(';');
            params.push_str(&modifier_param.to_string());
            if report_events {
                params.push(':');
                params.push_str(&event_type.to_string());
            }
        }
        if mode.contains(TermMode::REPORT_ASSOCIATED_TEXT)
            && let Some(text) = associated_text(press, modifiers)
        {
            // The text field is a third parameter, so an absent modifier
            // field still has to be written out to hold its place.
            if modifier_param == 1 && !report_events {
                params.push_str(";1");
            }
            params.push(';');
            let codepoints: Vec<String> = text.chars().map(|c| (c as u32).to_string()).collect();
            params.push_str(&codepoints.join(":"));
        }
        Some(format!("\x1b[{params}u").into_bytes())
    }

    /// The unmodified form of a key whose escape encoding isn't required:
    /// the legacy control byte for the four special keys, literal text for
    /// everything else.
    fn plain(press: Press, code: u32) -> Option<Vec<u8>> {
        match code {
            13 => return Some(b"\r".to_vec()),
            9 => return Some(b"\t".to_vec()),
            27 => return Some(vec![0x1b]),
            127 => return Some(vec![0x7f]),
            _ => {}
        }
        let text = press.text.filter(|text| !text.is_empty())?;
        Some(text.as_bytes().to_vec())
    }

    /// The protocol's key number: the Unicode codepoint of the key's
    /// unshifted character, or the codepoint the protocol assigns to a
    /// functional key.
    fn key_code(press: Press) -> Option<u32> {
        if let Some(c) = single_char(press.key) {
            // The *unshifted* codepoint identifies the key, with Shift
            // reported separately in the modifier field — otherwise `A` and
            // `Shift+a` would be two different keys.
            return Some(c.to_lowercase().next().unwrap_or(c) as u32);
        }
        let Key::Named(named) = press.key else {
            return None;
        };
        Some(match named {
            NamedKey::Enter => 13,
            NamedKey::Tab => 9,
            NamedKey::Escape => 27,
            NamedKey::Backspace => 127,
            NamedKey::Space => 32,
            NamedKey::Insert => 57348,
            NamedKey::Delete => 57349,
            NamedKey::PageUp => 57354,
            NamedKey::PageDown => 57355,
            NamedKey::ArrowUp => 57352,
            NamedKey::ArrowDown => 57353,
            NamedKey::ArrowLeft => 57350,
            NamedKey::ArrowRight => 57351,
            NamedKey::Home => 57356,
            NamedKey::End => 57357,
            NamedKey::CapsLock => 57358,
            NamedKey::ScrollLock => 57359,
            NamedKey::NumLock => 57360,
            NamedKey::PrintScreen => 57361,
            NamedKey::Pause => 57362,
            NamedKey::ContextMenu => 57363,
            _ => return function_key_code(named),
        })
    }

    /// `F1`-`F12`'s codepoints in the protocol's private-use block.
    fn function_key_code(key: &NamedKey) -> Option<u32> {
        let index = match key {
            NamedKey::F1 => 0,
            NamedKey::F2 => 1,
            NamedKey::F3 => 2,
            NamedKey::F4 => 3,
            NamedKey::F5 => 4,
            NamedKey::F6 => 5,
            NamedKey::F7 => 6,
            NamedKey::F8 => 7,
            NamedKey::F9 => 8,
            NamedKey::F10 => 9,
            NamedKey::F11 => 10,
            NamedKey::F12 => 11,
            _ => return None,
        };
        Some(57364 + index)
    }

    /// The text a keypress produces, for `REPORT_ASSOCIATED_TEXT`. Control
    /// combinations have no associated text — the program wants the key
    /// identity, not a control byte it never asked for.
    fn associated_text<'a>(press: Press<'a>, modifiers: ModifiersState) -> Option<&'a str> {
        if modifiers.control_key() || modifiers.alt_key() || modifiers.super_key() {
            return None;
        }
        press.text.filter(|text| !text.is_empty() && !text.chars().any(|c| c.is_control()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::SmolStr;

    fn named(key: NamedKey) -> Key {
        Key::Named(key)
    }

    fn character(c: &str) -> Key {
        Key::Character(SmolStr::new(c))
    }

    /// A press of `key`, with the text the OS would compose for it.
    fn press<'a>(key: &'a Key, text: Option<&'a str>) -> Press<'a> {
        Press { key, text, state: ElementState::Pressed, repeat: false }
    }

    fn encoded(press: Press, modifiers: ModifiersState, mode: TermMode) -> String {
        let bytes = encode(press, modifiers, mode).expect("key should produce input");
        String::from_utf8(bytes).expect("encoding should be valid UTF-8")
    }

    /// A named key press, encoded — the common shape of every case below.
    fn key(name: NamedKey, modifiers: ModifiersState, mode: TermMode) -> String {
        encoded(press(&named(name), None), modifiers, mode)
    }

    /// A character key press, encoded.
    fn typed(c: &str, modifiers: ModifiersState, mode: TermMode) -> String {
        encoded(press(&character(c), Some(c)), modifiers, mode)
    }

    const NONE: ModifiersState = ModifiersState::empty();

    /// The keys the developer reported as doing nothing at all. Each of
    /// these had no arm in the old encoder and fell through to an empty
    /// `text`, i.e. silence.
    #[test]
    fn the_editing_keypad_produces_its_standard_sequences() {
        let empty = TermMode::empty();
        assert_eq!(key(NamedKey::Home, NONE, empty), "\x1b[H");
        assert_eq!(key(NamedKey::End, NONE, empty), "\x1b[F");
        assert_eq!(key(NamedKey::PageUp, NONE, empty), "\x1b[5~");
        assert_eq!(key(NamedKey::PageDown, NONE, empty), "\x1b[6~");
        assert_eq!(key(NamedKey::Insert, NONE, empty), "\x1b[2~");
        assert_eq!(key(NamedKey::Delete, NONE, empty), "\x1b[3~");
    }

    #[test]
    fn function_keys_split_between_ss3_and_numbered_forms() {
        let empty = TermMode::empty();
        assert_eq!(key(NamedKey::F1, NONE, empty), "\x1bOP");
        assert_eq!(key(NamedKey::F4, NONE, empty), "\x1bOS");
        assert_eq!(key(NamedKey::F5, NONE, empty), "\x1b[15~");
        assert_eq!(key(NamedKey::F12, NONE, empty), "\x1b[24~");
    }

    /// Back-tab. The old encoder sent a plain tab for this, so Shift+Tab
    /// moved focus *forwards* — the opposite of what the key means.
    #[test]
    fn shift_tab_is_back_tab_not_a_plain_tab() {
        assert_eq!(key(NamedKey::Tab, ModifiersState::SHIFT, TermMode::empty()), "\x1b[Z");
        assert_eq!(key(NamedKey::Tab, NONE, TermMode::empty()), "\t");
    }

    #[test]
    fn modified_cursor_keys_carry_an_xterm_modifier_parameter() {
        let empty = TermMode::empty();
        assert_eq!(key(NamedKey::ArrowRight, ModifiersState::CONTROL, empty), "\x1b[1;5C");
        assert_eq!(key(NamedKey::ArrowLeft, ModifiersState::SHIFT, empty), "\x1b[1;2D");
        assert_eq!(key(NamedKey::Home, ModifiersState::ALT, empty), "\x1b[1;3H");
        let ctrl_shift = ModifiersState::CONTROL.union(ModifiersState::SHIFT);
        assert_eq!(key(NamedKey::Delete, ctrl_shift, empty), "\x1b[3;6~");
    }

    /// A program that enables DECCKM expects SS3 and does not recognize the
    /// CSI form — this is why arrows misbehave inside `vim` and `less`
    /// without it.
    #[test]
    fn application_cursor_mode_switches_the_cursor_keys_to_ss3() {
        assert_eq!(key(NamedKey::ArrowUp, NONE, TermMode::APP_CURSOR), "\x1bOA");
        assert_eq!(key(NamedKey::Home, NONE, TermMode::APP_CURSOR), "\x1bOH");
        // A modifier has nowhere to live in an SS3 sequence, so a modified
        // key reverts to the parameterized CSI form even in this mode.
        assert_eq!(key(NamedKey::ArrowUp, ModifiersState::CONTROL, TermMode::APP_CURSOR), "\x1b[1;5A");
    }

    /// Alt as Meta. Without this, readline's word-wise motions do nothing.
    #[test]
    fn alt_prefixes_a_key_with_escape() {
        let empty = TermMode::empty();
        assert_eq!(typed("b", ModifiersState::ALT, empty), "\x1bb");
        assert_eq!(key(NamedKey::Enter, ModifiersState::ALT, empty), "\x1b\r");
        let backspace = named(NamedKey::Backspace);
        assert_eq!(encode(press(&backspace, None), ModifiersState::ALT, empty), Some(vec![0x1b, 0x7f]));
    }

    #[test]
    fn control_combinations_produce_their_c0_bytes() {
        let empty = TermMode::empty();
        let ctrl = ModifiersState::CONTROL;
        let c = character("c");
        assert_eq!(encode(press(&c, Some("c")), ctrl, empty), Some(vec![0x03]));
        let space = named(NamedKey::Space);
        assert_eq!(encode(press(&space, Some(" ")), ctrl, empty), Some(vec![0x00]));
        let bracket = character("[");
        assert_eq!(encode(press(&bracket, Some("[")), ctrl, empty), Some(vec![0x1b]));
        let slash = character("/");
        assert_eq!(encode(press(&slash, Some("/")), ctrl, empty), Some(vec![0x1f]));
        // Backspace is DEL; Ctrl+Backspace is the one that means kill-word.
        let backspace = named(NamedKey::Backspace);
        assert_eq!(encode(press(&backspace, None), NONE, empty), Some(vec![0x7f]));
        assert_eq!(encode(press(&backspace, None), ctrl, empty), Some(vec![0x08]));
    }

    /// The ambiguity the kitty protocol exists to resolve, and the reason
    /// Shift+Enter "does nothing" in tools that rely on it.
    #[test]
    fn shift_enter_is_indistinguishable_in_legacy_and_distinct_under_the_protocol() {
        let legacy = TermMode::empty();
        assert_eq!(key(NamedKey::Enter, NONE, legacy), "\r");
        assert_eq!(key(NamedKey::Enter, ModifiersState::SHIFT, legacy), "\r");

        let kitty = TermMode::DISAMBIGUATE_ESC_CODES;
        assert_eq!(key(NamedKey::Enter, NONE, kitty), "\r");
        assert_eq!(key(NamedKey::Enter, ModifiersState::SHIFT, kitty), "\x1b[13;2u");
        assert_eq!(key(NamedKey::Enter, ModifiersState::CONTROL, kitty), "\x1b[13;5u");
    }

    /// Ordinary typing has to stay ordinary at the protocol's base level —
    /// a program enabling it is asking to disambiguate the hard cases, not
    /// to receive every letter as an escape sequence.
    #[test]
    fn plain_text_is_unaffected_by_the_protocols_base_level() {
        let kitty = TermMode::DISAMBIGUATE_ESC_CODES;
        assert_eq!(typed("a", NONE, kitty), "a");
        assert_eq!(typed("A", ModifiersState::SHIFT, kitty), "\x1b[97;2u");
        assert_eq!(typed("c", ModifiersState::CONTROL, kitty), "\x1b[99;5u");
    }

    #[test]
    fn key_releases_are_reported_only_when_the_program_asked_for_them() {
        let enter = named(NamedKey::Enter);
        let release = Press { key: &enter, text: None, state: ElementState::Released, repeat: false };
        assert_eq!(encode(release, NONE, TermMode::empty()), None);
        assert_eq!(encode(release, NONE, TermMode::DISAMBIGUATE_ESC_CODES), None);

        let mode = TermMode::DISAMBIGUATE_ESC_CODES.union(TermMode::REPORT_EVENT_TYPES);
        assert_eq!(encoded(release, NONE, mode), "\x1b[13;1:3u");
        let repeat = Press { key: &enter, text: None, state: ElementState::Pressed, repeat: true };
        assert_eq!(encoded(repeat, NONE, mode), "\x1b[13;1:2u");
    }

    /// Super/Command belongs to the OS and to this app's own chords; sending
    /// it as input would deliver a bare unmodified character to the shell.
    #[test]
    fn the_super_modifier_is_never_forwarded_as_input() {
        let c = character("c");
        assert_eq!(encode(press(&c, Some("c")), ModifiersState::SUPER, TermMode::empty()), None);
    }
}
