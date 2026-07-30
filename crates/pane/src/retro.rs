//! Watches PTY output for this terminal's own private escape sequence, so a
//! shell, a script, or a bare `printf` can change the retro era live:
//!
//! ```text
//! printf '\e]7331;era=amber\a'
//! ```
//!
//! Extending a terminal through a private OSC is how this has always been
//! done — DEC's private modes, iTerm2's OSC 1337, kitty's own protocol. It
//! also makes the feature genuinely pleasant to develop against and to test
//! by hand: flipping between eras in a running terminal needs no restart, no
//! config edit, and no menu.
//!
//! Like `crate::cwd`, this is a hand-rolled scanner rather than a patch to
//! `vte`/`alacritty_terminal`, which don't dispatch OSC 7331 (it isn't a
//! real sequence — see below) and have no reason to.
//!
//! # Why this is safe to accept from arbitrary output
//!
//! Program output setting your terminal's appearance sounds alarming, and for
//! OSC 8 hyperlinks the caution was warranted. This is a different risk class
//! in two specific ways:
//!
//! - **The payload is a name, not a value.** It selects from a fixed set of
//!   curated eras. Arbitrary output cannot specify colors, so it cannot
//!   produce an unreadable screen or hide text — the worst it can do is make
//!   your terminal look like 1985.
//! - **It never persists.** The era set this way lives in the running session
//!   only and is never written to the config file, so a restart always
//!   returns you to what you actually chose.
//!
//! # Why 7331
//!
//! It is not a registered or de facto OSC number, which is the point: nothing
//! else uses it, so this cannot collide with a real sequence some program
//! emits for another purpose. It is also `1337` backwards, which for an
//! easter egg is exactly the right amount of stupid.

/// Above this, a started-but-never-terminated sequence is abandoned rather
/// than buffered forever. An era name is a handful of bytes; anything longer
/// is corrupted input or output that merely happens to begin with the marker.
const MAX_PENDING: usize = 256;

const MARKER: &[u8] = b"\x1b]7331;";

/// Watches raw PTY output for `OSC 7331` era requests, remembering the most
/// recent one until a caller takes it.
///
/// PTY output arrives in arbitrary chunks that can split a sequence anywhere,
/// so a partial match is carried across `advance` calls rather than dropped.
pub struct RetroWatcher {
    /// The era name most recently requested and not yet taken.
    requested: Option<String>,
    pending: Vec<u8>,
}

impl Default for RetroWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl RetroWatcher {
    pub fn new() -> Self {
        Self { requested: None, pending: Vec::new() }
    }

    /// Scans `bytes` for era requests. Anything that isn't one is ignored;
    /// this never consumes or alters what the VT parser sees.
    pub fn advance(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.pending.push(byte);
            self.consume_pending();
        }
    }

    /// The era name requested since the last call, clearing it.
    ///
    /// Only the latest survives: two requests between polls means the second
    /// is what was wanted, not both in sequence.
    pub fn take_requested_era(&mut self) -> Option<String> {
        self.requested.take()
    }

    /// Trims `pending` down to whatever could still become a match, pulling
    /// out any complete sequence as it goes.
    fn consume_pending(&mut self) {
        // Still accumulating the marker itself.
        if self.pending.len() <= MARKER.len() {
            if !MARKER.starts_with(&self.pending) {
                self.restart_scan();
            }
            return;
        }

        // Marker matched; the payload runs to BEL or ST (`ESC \`).
        let payload = &self.pending[MARKER.len()..];
        let terminator = payload
            .iter()
            .position(|&b| b == 0x07)
            .map(|at| (at, 1))
            .or_else(|| payload.windows(2).position(|w| w == b"\x1b\\").map(|at| (at, 2)));

        if let Some((end, _)) = terminator {
            if let Some(era) = parse_payload(&payload[..end]) {
                self.requested = Some(era);
            }
            self.pending.clear();
            return;
        }

        if self.pending.len() > MAX_PENDING {
            self.restart_scan();
        }
    }

    /// Drops the buffered bytes, but keeps any trailing `ESC` — the byte that
    /// failed to extend this match may be the start of the next one, and
    /// discarding it would miss a sequence that arrives back-to-back.
    fn restart_scan(&mut self) {
        let restarts = self.pending.last() == Some(&0x1b);
        self.pending.clear();
        if restarts {
            self.pending.push(0x1b);
        }
    }
}

/// Reads `era=NAME` out of a sequence's payload.
///
/// A `key=value` shape rather than a bare name so this can carry further
/// retro settings later without needing a second sequence number. Unknown
/// keys are ignored rather than treated as an error: a newer version's
/// sequence reaching an older build should do nothing, not misbehave.
fn parse_payload(payload: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(payload).ok()?;
    let (key, value) = text.split_once('=')?;
    if !key.trim().eq_ignore_ascii_case("era") {
        return None;
    }
    // Deliberately not validated against the era table here: this crate has
    // no business knowing what the eras are. An unknown name resolves to
    // "off" at the point it's applied, same as an unknown name in the config.
    Some(value.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sequence(payload: &str) -> Vec<u8> {
        format!("\x1b]7331;{payload}\x07").into_bytes()
    }

    #[test]
    fn a_bel_terminated_request_is_picked_up() {
        let mut watcher = RetroWatcher::new();
        watcher.advance(&sequence("era=amber"));
        assert_eq!(watcher.take_requested_era().as_deref(), Some("amber"));
    }

    /// `ST` (`ESC \`) is the other legal OSC terminator and some shells emit
    /// it in preference to BEL.
    #[test]
    fn an_st_terminated_request_is_picked_up() {
        let mut watcher = RetroWatcher::new();
        watcher.advance(b"\x1b]7331;era=green\x1b\\");
        assert_eq!(watcher.take_requested_era().as_deref(), Some("green"));
    }

    #[test]
    fn taking_the_request_clears_it() {
        let mut watcher = RetroWatcher::new();
        watcher.advance(&sequence("era=cga"));
        assert_eq!(watcher.take_requested_era().as_deref(), Some("cga"));
        assert_eq!(watcher.take_requested_era(), None);
    }

    /// The hidden era is reachable this way like any other — the scanner has
    /// no idea which names are secret, which is exactly why the secret is
    /// testable.
    #[test]
    fn a_hidden_era_name_is_carried_like_any_other() {
        let mut watcher = RetroWatcher::new();
        watcher.advance(&sequence("era=matrix"));
        assert_eq!(watcher.take_requested_era().as_deref(), Some("matrix"));
    }

    /// PTY output splits wherever the kernel felt like it, including in the
    /// middle of the marker.
    #[test]
    fn a_request_split_across_arbitrary_chunks_still_arrives() {
        let full = sequence("era=amber");
        for split in 1..full.len() {
            let mut watcher = RetroWatcher::new();
            watcher.advance(&full[..split]);
            watcher.advance(&full[split..]);
            assert_eq!(watcher.take_requested_era().as_deref(), Some("amber"), "failed when split at byte {split}");
        }
    }

    #[test]
    fn a_request_split_one_byte_at_a_time_still_arrives() {
        let mut watcher = RetroWatcher::new();
        for byte in sequence("era=c64") {
            watcher.advance(&[byte]);
        }
        assert_eq!(watcher.take_requested_era().as_deref(), Some("c64"));
    }

    #[test]
    fn ordinary_output_around_a_request_is_ignored() {
        let mut watcher = RetroWatcher::new();
        watcher.advance(b"$ some command\r\n");
        watcher.advance(&sequence("era=green"));
        watcher.advance(b"more output\r\n");
        assert_eq!(watcher.take_requested_era().as_deref(), Some("green"));
    }

    #[test]
    fn plain_output_never_produces_a_request() {
        let mut watcher = RetroWatcher::new();
        watcher.advance(b"nothing to see here\r\n");
        watcher.advance(b"\x1b]7;file://host/home/will\x07"); // a real OSC 7
        watcher.advance(b"\x1b[1;32mcolored\x1b[0m");
        assert_eq!(watcher.take_requested_era(), None);
    }

    #[test]
    fn only_the_latest_of_several_requests_survives() {
        let mut watcher = RetroWatcher::new();
        watcher.advance(&sequence("era=green"));
        watcher.advance(&sequence("era=amber"));
        assert_eq!(watcher.take_requested_era().as_deref(), Some("amber"));
    }

    /// A sequence arriving immediately after a failed partial match must
    /// still be seen — the `ESC` that broke the previous scan is the start of
    /// this one.
    #[test]
    fn a_request_directly_after_a_broken_partial_match_is_found() {
        let mut watcher = RetroWatcher::new();
        watcher.advance(b"\x1b]73");
        watcher.advance(&sequence("era=cga"));
        assert_eq!(watcher.take_requested_era().as_deref(), Some("cga"));
    }

    #[test]
    fn an_unknown_key_is_ignored() {
        let mut watcher = RetroWatcher::new();
        watcher.advance(&sequence("theme=Dracula"));
        assert_eq!(watcher.take_requested_era(), None);
    }

    #[test]
    fn a_malformed_payload_is_ignored_rather_than_panicking() {
        let mut watcher = RetroWatcher::new();
        for payload in ["", "era", "=amber", "era="] {
            watcher.advance(&sequence(payload));
        }
        // `era=` yields an empty name, which resolves to "off" downstream.
        assert_eq!(watcher.take_requested_era().as_deref(), Some(""));
    }

    #[test]
    fn surrounding_whitespace_in_the_name_is_trimmed() {
        let mut watcher = RetroWatcher::new();
        watcher.advance(&sequence("era =  amber  "));
        assert_eq!(watcher.take_requested_era().as_deref(), Some("amber"));
    }

    /// An unterminated sequence must not buffer without bound — a program
    /// emitting the marker and then megabytes of data would otherwise grow
    /// this forever.
    #[test]
    fn an_unterminated_sequence_does_not_grow_without_bound() {
        let mut watcher = RetroWatcher::new();
        watcher.advance(b"\x1b]7331;");
        watcher.advance(&vec![b'x'; MAX_PENDING * 4]);

        assert!(watcher.pending.len() <= MAX_PENDING + 1);
        assert_eq!(watcher.take_requested_era(), None);
    }

    #[test]
    fn invalid_utf8_in_a_payload_is_ignored() {
        let mut watcher = RetroWatcher::new();
        watcher.advance(b"\x1b]7331;era=\xff\xfe\x07");
        assert_eq!(watcher.take_requested_era(), None);
    }
}
