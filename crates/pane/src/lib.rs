//! PTY wrapper: spawns a shell and exposes it as a plain byte stream.

mod cwd;
mod term;

use std::io::{Read, Write};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub use alacritty_terminal::selection::SelectionRange;
pub use alacritty_terminal::term::TermMode;
pub use alacritty_terminal::term::cell::Flags;
pub use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
pub use term::{RenderCell, Screen, SelectionKind};

/// Terminal dimensions, in character cells.
#[derive(Clone, Copy, Debug)]
pub struct Size {
    pub rows: u16,
    pub cols: u16,
}

/// The user's default shell, resolved the way `portable_pty` itself
/// resolves it — `$SHELL` first, then the current user's `/etc/passwd`
/// entry.
///
/// Needed because naming the shell explicitly is what keeps it non-login
/// (see `shell_command`), and a launch context where `$SHELL` isn't
/// exported — some desktop/GUI-launcher paths, unlike an interactive
/// terminal — would otherwise fall through to `new_default_prog` and
/// silently give a login shell.
#[cfg(unix)]
fn resolve_default_shell() -> Option<String> {
    std::env::var("SHELL").ok().or_else(passwd_shell)
}

#[cfg(unix)]
fn passwd_shell() -> Option<String> {
    // Matches `portable_pty::unix::get_shell()`'s own approach exactly
    // (down to using the same raw, not-thread-safe libc call it does —
    // no worse here than in the dependency already making this call).
    let ent = unsafe { libc::getpwuid(libc::getuid()) };
    if ent.is_null() {
        return None;
    }
    let shell = unsafe { std::ffi::CStr::from_ptr((*ent).pw_shell) };
    shell.to_str().ok().map(str::to_string)
}

#[cfg(not(unix))]
fn resolve_default_shell() -> Option<String> {
    None
}

/// Builds the command for a pane's shell, starting it the way the
/// platform's own terminals start one.
///
/// Whether a shell is a *login* shell decides which startup files it
/// reads, and the two platforms disagree. On Linux a graphical session has
/// already read the profile files by the time you open a terminal, so
/// terminals start an interactive **non-login** shell reading only the
/// bashrc files — GNOME Terminal, Konsole, xterm and Alacritty all do.
/// macOS GUI sessions never read them at all, so terminals there start a
/// **login** shell instead; Terminal.app and iTerm2 both do, which is why
/// a Mac user's `PATH` usually lives in `~/.bash_profile` or `~/.zprofile`.
///
/// This can't be left to `CommandBuilder::new_default_prog()`, which looks
/// like the neutral choice and isn't: it sets `argv[0]` to `-bash`, the
/// Unix convention for "this is a login shell". Using it for the
/// unconfigured case and `CommandBuilder::new` for the configured one —
/// which is what happened here once bash stopped being spawned explicitly
/// — silently made shell type depend on whether the user had set
/// `default_shell`, giving most Linux users a login shell.
#[cfg(target_os = "macos")]
fn shell_command(shell: Option<&str>, _resolved: Option<&str>) -> CommandBuilder {
    match shell {
        // `new_default_prog`'s `argv[0]` trick is the more faithful
        // mechanism than passing `-l`, since a shell that doesn't know the
        // flag still can't misinterpret the name it was invoked under.
        None => CommandBuilder::new_default_prog(),
        Some(shell) => {
            let mut cmd = CommandBuilder::new(shell);
            cmd.arg("-l");
            cmd
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn shell_command(shell: Option<&str>, resolved: Option<&str>) -> CommandBuilder {
    // Naming the shell explicitly is what keeps it non-login, so the
    // resolved `$SHELL` is used even when the user configured nothing.
    // Falling back to `new_default_prog` when even that fails would give a
    // login shell, but there is nothing else left to spawn at that point.
    match shell.or(resolved) {
        Some(shell) => CommandBuilder::new(shell),
        None => CommandBuilder::new_default_prog(),
    }
}

#[cfg(not(unix))]
fn shell_command(shell: Option<&str>, _resolved: Option<&str>) -> CommandBuilder {
    // Windows has no login-shell concept to match.
    match shell {
        Some(shell) => CommandBuilder::new(shell),
        None => CommandBuilder::new_default_prog(),
    }
}

/// A spawned shell process behind a pseudo-terminal.
pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

impl Pty {
    /// Spawns `shell` (or the platform default shell when `None`) behind a
    /// new PTY sized to `size`, starting in `cwd` if given (session
    /// restore's job — `None` elsewhere, which leaves the shell to inherit
    /// this process's own cwd, same as running it by hand).
    ///
    /// Nothing is injected into the shell. A pane's working directory comes
    /// from the OS process table (`app::foreground_process`) and, where the
    /// user's own configuration emits it, OSC 7 (`crate::cwd`) — never from
    /// a generated startup script this spawns the shell with. See
    /// `session_cwd`'s doc comment for what that costs on Windows.
    pub fn spawn(shell: Option<&str>, size: Size, cwd: Option<&std::path::Path>) -> anyhow::Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize { rows: size.rows, cols: size.cols, pixel_width: 0, pixel_height: 0 })?;

        // An explicit `shell` always wins; `None` is resolved the same way
        // `portable_pty` itself would on Unix, so that naming it keeps it
        // non-login. Windows' own default (`%ComSpec%`) needs no such
        // resolution — there is no login-shell concept to preserve there.
        let resolved = shell.map(str::to_string).or_else(resolve_default_shell);
        let mut cmd = shell_command(shell, resolved.as_deref());
        Self::set_terminal_env(&mut cmd);
        if let Some(cwd) = cwd {
            cmd.cwd(cwd);
        }
        let child = pair.slave.spawn_command(cmd)?;
        let writer = pair.master.take_writer()?;

        Ok(Self { master: pair.master, writer, child })
    }

    /// Tells the child shell what kind of terminal it's talking to.
    ///
    /// Nothing set this before, so the shell simply inherited whatever
    /// the GUI process happened to have. Launched from an existing
    /// terminal that's harmless — `TERM` is inherited and looks right by
    /// accident. Launched from a desktop launcher (Finder, the macOS
    /// Dock, a Linux `.desktop` entry) there is usually **no `TERM` at
    /// all**, and a shell that can't identify the terminal degrades:
    /// zsh in particular disables or cripples its line editor, which
    /// shows up as ordinary keys like Backspace doing nothing.
    ///
    /// `xterm-256color` rather than a bespoke `pain` entry: a custom
    /// `TERM` only works where its terminfo is installed, so it breaks
    /// the moment someone SSHes to a machine that's never heard of this
    /// app. `xterm-256color` is present essentially everywhere and
    /// accurately describes what `alacritty_terminal` implements.
    /// `COLORTERM` is the conventional out-of-band signal for 24-bit
    /// color, which the renderer does support.
    fn set_terminal_env(cmd: &mut CommandBuilder) {
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
    }

    /// Returns a fresh handle for reading the shell's output.
    ///
    /// Only one reader should be kept alive per `Pty` — each byte of PTY
    /// output is delivered to whichever reader happens to read it first.
    pub fn try_clone_reader(&self) -> anyhow::Result<Box<dyn Read + Send>> {
        self.master.try_clone_reader()
    }

    /// Writes raw bytes to the shell's input.
    pub fn write(&mut self, data: &[u8]) -> anyhow::Result<()> {
        self.writer.write_all(data)?;
        Ok(())
    }

    /// Resizes the underlying PTY, informing the shell of the new dimensions.
    pub fn resize(&self, size: Size) -> anyhow::Result<()> {
        self.master.resize(PtySize { rows: size.rows, cols: size.cols, pixel_width: 0, pixel_height: 0 })?;
        Ok(())
    }

    /// Reports whether the child shell has already exited.
    pub fn has_exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    /// Returns the child's exit status once it has exited, for diagnostics.
    pub fn exit_status(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.to_string()),
            _ => None,
        }
    }

    /// Terminates the child shell process. Best-effort: called on drop, so
    /// closing a pane always frees its process regardless of whether the
    /// caller does this explicitly.
    pub fn kill(&mut self) -> anyhow::Result<()> {
        self.child.kill()?;
        Ok(())
    }

    /// The pid of the spawned shell itself (the direct child of this PTY),
    /// if the platform reports one.
    pub fn shell_pid(&self) -> Option<u32> {
        self.child.process_id()
    }

    /// The process group currently in the foreground of this PTY, if any —
    /// `tcgetpgrp` on the PTY master, via `portable_pty`'s own
    /// `process_group_leader`. Unix only: a shell puts each foreground job
    /// in its own process group, led by the job itself, so for a simple
    /// foreground command this is almost always that command's own pid —
    /// the correct, direct signal for "what's running in this pane right
    /// now," which Windows/ConPTY has no equivalent of.
    #[cfg(unix)]
    pub fn foreground_pgid(&self) -> Option<u32> {
        self.master.process_group_leader().and_then(|pid| u32::try_from(pid).ok())
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn spawns_shell_and_echoes_input() {
        let mut pty = Pty::spawn(None, Size { rows: 24, cols: 80 }, None).expect("spawn shell");
        let mut reader = pty.try_clone_reader().expect("clone reader");
        pty.write(b"echo hello_pty_test\n").expect("write input to shell");

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let mut seen = String::new();
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                seen.push_str(&String::from_utf8_lossy(&buf[..n]));
                if seen.contains("hello_pty_test") {
                    break;
                }
            }
            let _ = tx.send(seen);
        });

        let output = rx.recv_timeout(Duration::from_secs(5)).expect("shell produced no output within timeout");
        assert!(output.contains("hello_pty_test"), "expected echoed output, got: {output:?}");
    }

    #[cfg(unix)]
    #[test]
    fn spawns_the_shell_in_the_given_starting_directory() {
        let dir = std::env::temp_dir();
        let expected = dir.canonicalize().unwrap_or_else(|_| dir.clone());
        let needle = expected.to_str().expect("temp dir should be valid UTF-8").to_string();

        // `/bin/sh`, not the platform default shell (`None`): a real
        // interactive login shell can run dotfiles (`.bashrc`/`.profile`)
        // that `cd` somewhere themselves, which would silently override
        // whatever `cwd` was passed here and has nothing to do with
        // whether that argument was actually honored at spawn time — this
        // dev environment's own `.bashrc` does exactly that.
        let mut pty =
            Pty::spawn(Some("/bin/sh"), Size { rows: 24, cols: 80 }, Some(&dir)).expect("spawn shell with a cwd");
        let mut reader = pty.try_clone_reader().expect("clone reader");
        pty.write(b"echo $PWD\n").expect("write input to shell");

        // Accumulates every chunk read for a fixed window, rather than
        // guessing "done" from the buffer's content — the shell's own
        // startup banner and its verbatim echo of our typed command both
        // legitimately show up before the command actually runs, so there's
        // no reliable partial-output marker to stop on early. `$PWD`'s
        // *expanded* value never appears in the command source itself
        // (only the literal text `$PWD` does), so plainly waiting long
        // enough and then checking for it is simpler and more robust than
        // trying to detect completion.
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });

        let mut seen = String::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
            match rx.recv_timeout(remaining) {
                Ok(chunk) => seen.push_str(&String::from_utf8_lossy(&chunk)),
                Err(_) => break,
            }
        }

        assert!(seen.contains(&needle), "expected $PWD output to contain {needle:?}, got: {seen:?}");
    }

    /// Shell type must not depend on whether the user happens to have set
    /// `default_shell`. It silently did: naming the shell explicitly gives
    /// a non-login shell, while `new_default_prog()` sets `argv[0]` to
    /// `-bash` and gives a login one — so the same machine produced
    /// different startup behaviour depending on a config key that has
    /// nothing to do with it.
    ///
    /// Checked through `is_default_prog`, which is exactly the distinction
    /// that was leaking.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_panes_are_non_login_whether_or_not_a_shell_is_configured() {
        let configured = shell_command(Some("/bin/bash"), Some("/bin/bash"));
        let unconfigured = shell_command(None, Some("/bin/bash"));

        assert!(!configured.is_default_prog());
        assert!(
            !unconfigured.is_default_prog(),
            "an unconfigured shell must still be named explicitly, or argv[0] makes it a login shell"
        );
    }

    /// The mirror of the above: macOS terminals *do* start login shells,
    /// so a configured shell has to be told to be one.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_panes_are_login_shells_whether_or_not_a_shell_is_configured() {
        let configured = shell_command(Some("/bin/bash"), Some("/bin/bash"));
        assert!(configured.get_argv().iter().any(|a| a == "-l"), "a configured shell needs `-l` to be a login shell");
        // Unconfigured relies on `new_default_prog`'s own `-bash` argv[0].
        assert!(shell_command(None, Some("/bin/bash")).is_default_prog());
    }

    /// The behaviour the construction above is a proxy for, measured on a
    /// real shell rather than inferred: `shopt -q login_shell` sets `$?`
    /// to 0 for a login shell and 1 otherwise.
    ///
    /// Reads the status out of `$?` rather than echoing a marker word,
    /// because the pty echoes the command itself — a probe printing
    /// "IS_LOGIN" or "IS_NONLOGIN" matches both, in its own echo, before
    /// the shell has answered anything.
    #[cfg(unix)]
    #[test]
    fn a_real_bash_pane_matches_the_platforms_login_convention() {
        let mut pty = Pty::spawn(Some("bash"), Size { rows: 24, cols: 80 }, None).expect("spawn bash");
        let mut reader = pty.try_clone_reader().expect("clone reader");
        pty.write(b"shopt -q login_shell; echo \"STATUS=$?\"\nexit\n").expect("write probe");

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let mut all = Vec::new();
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                all.extend_from_slice(&buf[..n]);
            }
            let _ = tx.send(String::from_utf8_lossy(&all).into_owned());
        });

        let output = rx.recv_timeout(Duration::from_secs(10)).expect("bash should have answered");
        let expect_login = cfg!(target_os = "macos");
        assert_eq!(
            output.contains("STATUS=0"),
            expect_login,
            "expected login_shell={expect_login} on this platform, got: {output:?}"
        );
    }

    /// A pane's shell must be spawned with **no arguments at all** on
    /// every platform, so it reads its own startup files exactly as it
    /// would in any other terminal.
    ///
    /// This is the whole claim of removing shell integration, and it is
    /// asserted rather than left implicit because the injection it
    /// replaced was invisible in normal use — it surfaced only as mangled
    /// prompts, and on Windows as an antivirus detection for writing a
    /// script to the temp directory and spawning a shell against it.
    #[test]
    fn a_shell_is_spawned_with_no_injected_arguments() {
        let cmd = shell_command(Some("bash"), Some("/bin/bash"));
        let args: Vec<_> = cmd.get_argv().iter().skip(1).collect();
        let expected: usize = if cfg!(target_os = "macos") { 1 } else { 0 };
        assert_eq!(args.len(), expected, "only macOS's login `-l` is ever added, got {args:?}");
    }
}

#[cfg(test)]
mod env_tests {
    use super::*;

    /// Guards a regression that is invisible whenever the app is launched
    /// from an existing terminal — the inherited `TERM` masks it — and
    /// only appears from a desktop launcher, which is exactly how it went
    /// unnoticed until a user opened the macOS `.app` from Finder and
    /// found Backspace dead in zsh.
    ///
    /// Asserts against the `CommandBuilder` directly rather than spawning
    /// a shell and reading `$TERM` back: a spawn test would have to clear
    /// `TERM` from this process to prove the value wasn't merely
    /// inherited, and mutating process-global environment while the rest
    /// of the suite runs in parallel threads is a worse hazard than the
    /// coverage is worth.
    #[test]
    fn a_spawned_shell_is_told_which_terminal_it_is_talking_to() {
        let mut cmd = CommandBuilder::new("sh");
        Pty::set_terminal_env(&mut cmd);
        assert_eq!(cmd.get_env("TERM").and_then(|v| v.to_str()), Some("xterm-256color"));
        assert_eq!(cmd.get_env("COLORTERM").and_then(|v| v.to_str()), Some("truecolor"));
    }
}
