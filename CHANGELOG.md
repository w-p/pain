# Changelog

## Unreleased

## v1.11.0

- Retro eras, off by default. An era is a period look for a specific machine —
  `green` (IBM 5151), `amber`, `cga` (IBM 5153), `bbs`, `c64` — bundling a
  palette, scanlines, a curved-glass vignette, and a typeface. Set `[retro]
  era`, pick one in Settings under **Retro**, or try one for a session with
  `pain --era=amber`. `pain --era=list` shows what's available.

  An era overrides your theme while active but never writes to it, so turning
  it off restores what you had. Anything you set explicitly — background
  colour, scanline or vignette strength — still wins over the era.

  Scanlines and the vignette have sliders in Settings, each showing the era's
  value until you move it, with a button to hand control back to the era.

- Eras name period typefaces — VT323 for the phosphor eras, `Px437 IBM VGA
  8x16` for the DOS ones, C64 Pro Mono for `c64` — and use one if it's
  installed. None are bundled; the README says where to get them. The font
  makes more difference to a period look than the palette does, so it's worth
  installing one.

- A drifting **hum bar**, the soft band that crept up an old monitor when its
  power supply and the mains were slightly out of step. It's the only effect
  that moves, and so the only one that keeps the terminal drawing while idle —
  it stops when the window loses focus, redraws well below your display's
  rate, and `hum = 0` turns it off entirely.

- Eras can also be set live by a shell or script, with this terminal's own
  escape sequence:

  ```sh
  printf '\e]7331;era=green\a'
  ```

  Session-only and never saved, and the payload is an era name rather than
  colours — so program output can't leave you with an unreadable screen.

- The scanline and vignette effects are static: they add one draw to a frame
  that was already being rendered, so with the hum bar off an idle terminal
  still renders nothing and still sleeps.

## v1.10.0

- The process scan behind the pane title bars now collects only what those
  titles actually read (a process's name, parent, and start time). It had
  been collecting memory and CPU counters, disk I/O, and the thread list of
  every process on the system, every half second, and keeping it all.

- Settings opens in its own window now, rather than as a panel floating
  inside the terminal. It can be dragged anywhere, including onto another
  monitor, and no longer competes with your panes for space or has to be
  shrunk to fit a small terminal window.

  Everything else about it is unchanged: edits still preview live in the
  terminal as you make them, Save writes `config.toml`, and Cancel — or
  closing the window — reverts the preview.

## v1.9.0

- Removed shell integration on Windows. To track a pane's working
  directory, pain wrote a startup script to the temp directory and spawned
  the shell against it — `--rcfile` for bash, `-Command` for PowerShell,
  and a script executed inside WSL. Windows Defender flagged the result as
  `Behavior:Win32/DefensiveEvasion.A!ml` and moved to quarantine it, which
  is a fair reading of what that looks like from the outside.

  What it cost: on Windows, a restored session reopens panes in your home
  directory rather than their last working directory. Layout, window size,
  shell and group still restore. cmd.exe never had it. Linux and macOS are
  unaffected — they read the working directory from the OS process table
  and never had anything injected.

  Windows bash also goes back to reading its own startup files rather than
  a generated one.

- `font_size` and `transparency` are whole numbers now. `transparency` is a
  percentage, so `1.0` becomes `100` and `0.7` becomes `70`. A config file
  written before this is read and converted rather than rejected — TOML
  tells `70` and `0.7` apart by type — so nothing else in the file reverts.

## v1.8.0

- Fixed: every color rendered noticeably brighter and more washed out than
  the color it was supposed to be. Themes made this obvious — Ayu's greens
  arriving as pale mint — but it applied to the default palette and the
  app's own chrome just as much. The window is an sRGB-format surface, so
  the GPU applies gamma encoding to whatever is drawn; colors were being
  handed over already encoded and so got encoded a second time. Colors now
  match their published hex values.

  The window background also renders correctly at transparency levels
  below 1.0, where it was previously composited without being scaled by its
  own alpha and read brighter than the opaque one.

- Fixed: many standard keys did nothing. `Home`, `End`, `Page Up`,
  `Page Down`, `Insert`, `Delete` and `F1`–`F12` had no encoding at all and
  were silently dropped. `Shift+Tab` sent a plain tab, so it moved forward
  through a form or completion menu instead of back. Modified cursor keys
  (`Ctrl+Left`, `Shift+End`, ...) lost their modifier. `Alt` was not sent as
  Meta, so readline's word-wise motions — `Alt+B`, `Alt+F`, `Alt+D` — did
  nothing. Application cursor mode was ignored, so arrow keys used the wrong
  form inside `vim`, `less` and anything else that enables it.

  Keyboard input is now the full xterm-compatible encoding rather than the
  handful of keys the first version implemented.

- The kitty keyboard protocol is now supported, which is what lets a program
  tell `Shift+Enter`, `Ctrl+Enter` and plain `Enter` apart. These are the
  same byte in the traditional encoding and cannot be distinguished in it at
  all; editors and CLI tools that offer `Shift+Enter` for a newline ask for
  this protocol to get it. Programs that don't ask are unaffected.

- Fixed, theme picker: clicking the filter box closed the dropdown, so the
  filter could not be used. The list also stopped at the first 100 of 600
  themes with a "keep typing" note at the bottom, and had two nested
  scrollbars, the visible one belonging to the wrong thing. The whole list
  now scrolls, filtering works, and it shows twelve themes at a time.

- `Ctrl+Plus` and `Ctrl+Minus` change the font size by a point, and `Ctrl+0`
  returns it to the default. The new size is saved, so it survives a
  restart.

  These chords previously did something else entirely: nothing had bound
  them, so the UI toolkit's own handler claimed them and scaled the app's
  menus and panels instead of the terminal font — which is what was leaving
  the right-click menus offset from the pointer afterwards.

- Fixed: the text cursor was partly transparent, so at any transparency
  setting below fully opaque the desktop showed through it. It is now a
  solid block with the character under it drawn in reverse video, which is
  what other terminals draw. The selection highlight had the same problem
  and keeps its blended appearance without being see-through.

- The title bar color is now configurable, in Settings or as
  `appearance.title_bar_color`. Its text color follows automatically, so a
  pale title bar gets dark text. Grouped panes still take their color from
  their group — that is the only way to tell groups apart.

- Keybindings are written with spaces instead of `+`: `"ctrl shift e"`,
  `"ctrl +"`, `"ctrl -"`. It reads better, and it means `+` and `-` are
  writable as themselves rather than spelled out — a separator that is also
  a key on the keyboard can't represent that key. The space key is written
  `space`.

  Existing config files are unaffected: the older `"ctrl+shift+e"` form is
  still read.

- Fixed, Settings: the scroll bar overlapped the right edge of the controls
  beside it. It now has a margin of its own.

## v1.7.0

- Fixed, Windows: starting pain opened a console window that then sat there
  for as long as the terminal was running, and a shell you launched it from
  stayed blocked until you closed it. The executable was built as a console
  application — the default, and the wrong one for something that opens its
  own window — so Windows gave it a console whether it wanted one or not.
  It's now built as a normal windowed application, which is why neither
  macOS nor Linux ever showed this.

  `--help`, `--version` and `--verbose` still print to the terminal you ran
  them from, and redirecting to a file still works. One difference is
  unavoidable now that the shell no longer waits: their output arrives just
  after the prompt comes back, which is how every windowed Windows program
  with a command line behaves.

- Panes now show a dot in their title bar when something has happened —
  blue when the shell produced output, red when a program rang the terminal
  bell. This is for the side pane running a build or tailing a log: you can
  tell it moved without watching it. The terminal bell was previously read
  and discarded, so a program ringing it did nothing at all.

  The two clear differently, on purpose. Output only matters for a pane you
  aren't watching, so focusing it is enough. A bell is a program asking for
  attention and usually rings the instant a command starts — while its own
  pane is still focused, because you just pressed Enter there — so it
  survives focus and clears when you next type into that pane.
- Over 600 built-in color themes, compiled into the binary — `Dracula`,
  `Tokyo Night`, `Gruvbox Dark`, `Catppuccin Mocha`, `Solarized Light` and the
  rest of the familiar names. Pick one from Settings, where the list is
  filterable, or set `appearance.theme` by hand. A theme supplies the 16 ANSI
  colors programs draw with plus the default foreground and background, so
  switching restyles panes that are already open.

  The default is unchanged: `Graphite`, exactly the palette the app has always
  shipped, so upgrading doesn't restyle anything on its own.
  `appearance.background_color` is now an override — leave it empty and the
  background follows the theme, which is what you want since a theme's
  background is part of its design. A config that already set it keeps that
  value and keeps overriding.
- Links a program explicitly marks with the OSC 8 escape sequence — what
  `cargo` and `gcc` emit — are now `Ctrl+click`-able. Previously only text
  that *looked* like a URL was matched, so a link whose visible text was an
  ordinary word was invisible. The same scheme restriction applies either way:
  only `http`, `https`, `ftp`, `ssh` and `mailto` are ever handed to the
  operating system, so program output can't turn a click into opening an
  arbitrary local file.
- Emoji now render in color. They previously came out as flat monochrome
  silhouettes, because the color information a font provides was being
  thrown away and only the shape kept.

  Color glyphs get their own small texture rather than widening the existing
  one, so ordinary text keeps exactly as much room as it had. Symbols that
  programs print as text — `✓`, `✗`, `★`, `➜` — deliberately stay
  monochrome and one cell wide; they have emoji forms, but a test suite's
  worth of checkmarks turning into colored pictures is not an improvement.
  Needs a color emoji font installed, which every mainstream desktop has:
  Noto Color Emoji on Linux, Apple Color Emoji on macOS, Segoe UI Emoji on
  Windows.
- Optional ligature support, off by default: `appearance.ligatures`, or the
  checkbox in Settings. Renders `!=` as `≠` and `=>` as `⇒` with a font that
  provides them (Fira Code, JetBrains Mono, Cascadia Code, Iosevka). Never
  applied across a color change or across the cursor, so editing `!=` still
  shows which character you're on.

  Off by default deliberately, and it stays a separate rendering path rather
  than replacing the existing one: shaping hands glyph positioning to the
  font's own advances instead of the cell grid, and costs real per-frame work.
  Neither is worth imposing on someone who didn't ask for ligatures.

## v1.6.1

- Fixed: changing the font size a few times crashed the terminal. Every
  size you passed through kept its own permanent copy of the character
  set in the glyph texture, and once that texture filled up the next
  character written to it ran off the end — which the graphics driver
  treats as fatal. Only one font size is kept now, the texture is four
  times larger, and running out of room falls back to reusing it rather
  than crashing. ([#1](https://github.com/w-p/pain/issues/1))
- Fixed: `scrollback_lines` did nothing. The setting was saved, shown in
  the settings panel and documented as defaulting to 5000, but the number
  never reached the terminal grid — every pane kept a fixed 10000 lines
  regardless. It now works, and changing it applies to panes that are
  already open rather than only new ones. **If you never set it, panes now
  keep 5000 lines of history instead of 10000** — raise it in the settings
  panel or your config file if you want the old depth back.
- Fixed: a multi-command paste could skip the confirmation prompt. The
  check counted only line feeds, but a carriage return submits a command
  just as well, so text separated with those ran every command it
  contained without asking. Both count now, and the prompt reports the
  line count correctly for either.
- Fixed: hand-editing `font_size` to 0 crashed the app the moment the file
  was saved, and a negative value froze it at 100% CPU with no way back.
  Numeric settings are now clamped to their documented ranges on load —
  `font_size` to 6–48, `transparency` to 0.0–1.0, `scrollback_lines` to at
  most 1000000 — and say so on stderr.
- Fixed: dragging a pane divider and releasing the mouse over an open menu
  or the settings panel left the divider stuck to the pointer, resizing on
  every later mouse movement with no button held. Losing window focus
  mid-drag did the same. Text selections and mouse-driven programs had the
  same problem. All of them now end properly.
- Fixed: when a pane's shell failed to start while restoring a saved
  session, the pane stayed in the layout as a dead blank rectangle that
  could take focus and silently swallow everything typed into it. It is
  now removed, and the remaining panes take the space.
- Fixed: if a split failed to start its shell, the pane that was split kept
  a terminal sized for half the space while drawing at full width.

## v1.6.0

- On Linux and macOS, nothing is injected into your shell at all any more.
  Working directories for session restore are read straight from the
  operating system's process table instead of relying on the shell to
  report them, so bash starts exactly as it would in any other terminal —
  no generated startup file, no `--rcfile`, nothing added to
  `PROMPT_COMMAND`. It also means **zsh and fish panes now restore their
  working directory**, which they never did, along with any other shell.
  Windows still uses the old mechanism, having no way to read another
  process's working directory.
- Panes now start the shell the way the platform's own terminals do:
  interactive non-login on Linux, where a desktop session has already read
  the profile files, and login on macOS, where it hasn't — matching GNOME
  Terminal and Konsole on one side and Terminal.app and iTerm2 on the
  other. On macOS that means `~/.bash_profile` and `~/.zprofile` are read
  again, which is where a Mac user's `PATH` usually lives.
- Fixed: whether a pane's shell was a login shell depended on whether you
  had set `default_shell` in your config — a setting that has nothing to
  do with it. Leaving it unset gave a login shell and setting it gave a
  non-login one, so two machines with the same dotfiles behaved
  differently for no visible reason.
- Fixed: bash panes started as if they were login shells, so `~/.bashrc`
  was often run **twice**. The stock `~/.bash_profile` on Fedora and RHEL
  — and commonly on Debian and Ubuntu — ends by sourcing `~/.bashrc`, and
  we then sourced it again ourselves. Anything written to append rather
  than assign did it twice too: duplicated `PATH` entries, duplicated
  `PROMPT_COMMAND`, and prompt frameworks installing their hooks on top of
  themselves, which is why it showed up as colors and prompts coming out
  wrong. It also ran login-only setup (`ssh-agent`, tmux auto-attach) once
  per pane instead of once per login, and printed the login message every
  time a pane opened. Panes now start an ordinary interactive non-login
  shell — the system bashrc and `~/.bashrc`, exactly like every other
  terminal.
- Fixed: on Fedora, RHEL and macOS the system bashrc (`/etc/bashrc`) was
  never read at all, losing the system default prompt and the interactive
  half of `/etc/profile.d`. Only Debian's `/etc/bash.bashrc` happened to
  get picked up, and only indirectly.

- Fedora, RHEL, Rocky and Alma get a GPG-signed DNF repository, so
  `dnf install pain` and `dnf upgrade` work the same way `apt` already
  did on Debian and Ubuntu.
- An AppImage, for every other distribution — one file, no install, no
  root, and it works on immutable systems like Silverblue, Kinoite and
  Bazzite where layering a package means a reboot. See the README if your
  distribution doesn't ship libfuse2 (Fedora and Ubuntu 24.04 among them).
- Fixed: the Linux build picked up its glibc requirement from whatever
  the CI runner happened to be running, which silently rose each time
  GitHub updated that image and would eventually have stopped the package
  installing on older distributions for no visible reason. The build now
  happens in a pinned container, fixing the floor at glibc 2.35 — Debian
  12+, Ubuntu 22.04+, and every current Fedora — until it's moved
  deliberately.
- The Linux packages now recommend a font. The application aborts if there
  isn't one on disk, and while any machine with a graphical session has
  fonts already, apt and dnf both install recommendations by default — so
  an ordinary install can't land in that state.
- Fixed: the Vulkan driver was a recommended dependency rather than a
  required one, so installing with `--no-install-recommends` produced an
  install that could never start. It's now required, as an alternation
  that an NVIDIA driver satisfies too, so nobody is forced to install
  Mesa's driver to satisfy it.

## v1.5.0

- Clipboard shortcuts now match what people actually expect. On Windows
  and Linux, `Ctrl+C` copies when text is selected and still sends an
  interrupt when nothing is — so it gains the familiar meaning without
  ever costing you the ability to stop a running command — and `Ctrl+V`
  pastes. On macOS, `Cmd+C`/`Cmd+V` copy and paste (they previously did
  nothing at all), `Cmd+Q` quits and `Cmd+W` closes a pane; `Ctrl` is
  left entirely alone there, since Command is what the clipboard belongs
  on.

  `Ctrl+Shift+C`/`Ctrl+Shift+V` are no longer bound. Those chords only
  ever existed because the unshifted pair wasn't available, which is no
  longer true on either platform — keeping them would give one action two
  shortcuts, the second of them the awkward one. Add
  `"ctrl+shift+c" = "copy"` and `"ctrl+shift+v" = "paste"` to
  `[keybindings]` if you have the muscle memory.

- Menus, panels, and dialogs are drawn whenever they need to be. Hover
  highlights update, closing one no longer leaves it on screen until some
  unrelated click forces a repaint, and a click on a menu item can no
  longer also start a text selection in the pane behind it. All three came
  from the same place: the overlay's own repaint requests were being
  ignored, and egui's idea of where the pointer is only advances when a
  frame runs — so a skipped frame left it answering questions about a
  stale pointer position.
- Menus and dialogs now render at their content's full size and scroll
  only when the app window is genuinely too small for them. Several
  scrolled regardless: the paste preview was pinned to 160 pixels tall,
  and the context menus sized themselves against their own height from the
  previous frame — a loop that settles at whatever height a menu first
  happened to take and leaves a scrollbar up for good, however much room
  the window has. Panels squeezed narrow by a small window also now grow
  back when it's widened again, instead of staying squeezed.
- The settings panel's keybinding list is now a collapsible section,
  folded away by default. It's reference material, not something you need
  open while changing a font size.
- Documentation. There's now a man page (`man pain`, shipped in the `.deb`),
  and `pain --help`/`--version` do something — `--help` prints the config
  file path resolved for the machine you're on, since "where does this keep
  its settings" was previously answerable only by reading the source. The
  README documents every keyboard shortcut and mouse action, and the config
  file: where it lives per platform, every key with its default, and what
  happens when the file is malformed.
- The settings panel's Keybindings section now lists the bindings actually
  in effect, defaults included, marking the ones your config changed. It
  previously showed only overrides, so anyone who had never edited their
  config — the people most likely to look — saw an empty box telling them
  defaults existed without saying what they were.

- Fixed: right-click menus, the settings panel, and the paste dialog were
  cut off by the window edge when the window was small. They now shrink to
  fit and scroll for whatever still doesn't, so every action stays
  reachable at any window size.

- Massively reduced resource use when idle. Three things were wrong:
  on Windows the swapchain defaulted to a present mode with **no vsync
  cap**, so the GPU rendered as fast as it physically could; the event
  loop asked for a fresh frame on every iteration whether or not anything
  had changed; and it never slept, so it spun the CPU continuously. The
  loop now sleeps until something actually happens — PTY output wakes it
  directly — and only repaints when the screen genuinely changed. An idle
  terminal now measures at essentially zero CPU and does no GPU work at
  all.

## v1.4.1

- Fixed: shells were never told what terminal they were running in — `TERM`
  was left to whatever the app process happened to inherit. Launched from a
  desktop launcher (Finder, the Dock, a Linux applications menu) there is
  usually no `TERM` at all, which degrades the shell: in zsh it disables the
  line editor, so Backspace and other ordinary keys stop working. Shells now
  get `TERM=xterm-256color` and `COLORTERM=truecolor`.
- The macOS `.app` is now ad-hoc code-signed, so the bundle carries a
  proper seal covering its `Info.plist` and resources. This isn't a real
  developer-signed build — Gatekeeper still needs the quarantine flag
  cleared — but the bundle is no longer unsealed.
- README: documented how to actually launch `pain.app` from a terminal.
  A `.app` is a directory, so `./pain.app` fails with "permission denied"
  (zsh) or "Is a directory" (bash); use `open pain.app`, or run
  `./pain.app/Contents/MacOS/pain` directly to see log output.

## v1.4.0

- Holding `Ctrl` now underlines the URL under the pointer and switches to
  a hand cursor, so it's clear what a `Ctrl+click` will open before you
  click it.
- Fixed: the paste confirmation dialog (and the settings panel) left a
  large empty gap above their buttons, making both windows much taller
  than their content.

## v1.3.0

- Paste is now safe by default. Text is wrapped in bracketed-paste markers
  when the running program supports them, so shells hold it on the prompt
  for review instead of executing every line as it arrives. When the
  program *doesn't* support them, a multi-line paste asks for confirmation
  first and shows exactly what will be sent (`confirm_multiline_paste` in
  config turns this off).
- Copy and paste keyboard shortcuts: `Ctrl+Shift+C` and `Ctrl+Shift+V`
  (both remappable as `copy`/`paste`). Previously paste was reachable only
  through the right-click menu.
- Double-click selects a word, triple-click selects a line.
- `Ctrl+click` opens a URL in your browser.
- Fixed: window transparency did nothing on macOS. The Metal backend only
  ever offers a `PostMultiplied` composite mode, which the surface setup
  didn't accept, so every Mac ran fully opaque regardless of the
  configured transparency level.

## v1.2.0

- An application icon, and a desktop entry on Linux — the app now appears
  in the applications menu after `apt install` (previously it could only
  be launched by typing `pain` into some other terminal) and shows its own
  icon in the taskbar and alt-tab switcher.
- macOS releases now ship a proper universal `pain.app` bundle — one
  download that runs natively on both Intel and Apple Silicon, launchable
  from Finder and Spotlight, instead of a bare per-architecture binary.
- The window title is now "pain" rather than "Terminal Emulator (dev)".

## v1.1.0

- A GPG-signed APT repository (hosted on GitHub Pages) for Debian/Ubuntu,
  published automatically on every release — `apt install`/`apt upgrade`
  support instead of manually downloading the `.deb` each time. See the
  README for the `sources.list` setup.

## v1.0.0

- A close button on every pane's title bar, and a "Close" action on both
  right-click menus (the pane-management one and the terminal content
  one) — closing a pane no longer requires the `Ctrl+Shift+W` chord. The
  close button is a proper square, evenly padded from the title bar's
  top, right, and bottom edges alike, rather than a tall sliver shaped by
  raw monospace-cell metrics.
- Fixed: closing a pane in the middle of an arranged row/column only grew
  its immediate structural neighbor, leaving everything else at its old
  size (e.g. closing the middle of three equal horizontal panes left one
  at its original third and ballooned the other to two-thirds). Closing a
  pane now rebalances every pane in the same visual row/column to an
  equal share of the freed space.
- Settings now live-preview as you edit — background/accent color,
  transparency, and font family/size update the terminal immediately
  while the panel is open, not just after Save; closing the panel via
  Cancel (or its own close button) without saving reverts to the last
  saved values.
- Fixed: the terminal grid's font size ignored the OS's display-scaling
  setting entirely — on a 125%-scaled display, text rendered noticeably
  smaller than every other (DPI-aware) app on screen, even though the
  configured size was unchanged. Font size is now scaled by the window's
  DPI factor, recomputed live if the window moves to a monitor with a
  different scaling setting.
- The project has a name: **pain**. The `app` crate/binary is now `pain`
  (`cargo run -p pain`); a `.deb` package can be built with `cargo deb -p
  pain` (requires `cargo install cargo-deb` once) for Debian/Ubuntu
  distribution.
- A new right-click terminal context menu (Copy/Paste) when right-clicking
  a pane's terminal content; the existing pane-management menu
  (Broadcast/Split/Arrange/Group/Swap shell/Settings) now only opens from
  a right-click on the pane's title bar specifically.
- Fixed: Tab-key completion silently did nothing in every shell — egui's
  own focus-cycling convention was unconditionally swallowing every Tab
  keypress before it could reach the pty.
- Refined the context menu and settings panel layout: a uniform 2px corner
  radius throughout, bordered sections with small-caps monospace headers
  in the context menu (Broadcast/Split/Arrange/Group/Swap shell), a
  plain-link "Settings..." entry, and a grid-aligned four-section settings
  panel (Appearance/Terminal/Shell/Keybindings) with evenly distributed
  shell quick-pick buttons.
- A new default look ("Graphite"): a cool near-black palette, a
  user-configurable accent color (Settings) driving the cursor and
  selection highlight, and native system-font chrome for the context menu
  and settings panel instead of a generic toolkit look.
- A right-click "Arrange all panes" action (Horizontal/Vertical/Grid) to
  instantly retile every open pane into a preset layout.
- Session persistence: layout, window size, and each pane's working
  directory, chosen shell, and group membership are saved on quit and
  restored on next launch (never restarts whatever was running).
- Automatic OSC 7 (working-directory reporting) shell integration for bash
  and PowerShell panes, so session restore's directory tracking actually
  works without any manual shell configuration.
- Colored terminal output: full ANSI/256-color/true-color rendering.
- Scrollback: mouse-wheel scrolling through a pane's history.
- A font-family selector in Settings, listing installed monospaced fonts.
- A "Swap shell" pane context-menu action, for switching a pane's shell
  in place (e.g. into WSL) without closing it.
- `--verbose` now accepts categories (`mouse`/`pty`/`foreground`/`all`) so
  high-frequency diagnostic streams don't drown out everything else.
- Fixed: a WSL-rooted pane's title could get stuck on `conhost.exe`
  forever, regardless of what was actually running in the shell.
- Fixed: brighter pane-group title-bar colors weren't switching to dark
  text for readability.
- Project scaffolding: Cargo workspace with `pane`, `layout`, `router`,
  `config`, `render`, and `app` crates. MIT license.
