//! TOML-backed configuration: parsed at startup with defaults for anything
//! missing (see `.waypoint/design/config-system.md`). Hot reload (5.2) and
//! keybinding-override wiring (5.3) build on top of `load`/`Config` here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub mod era;
pub mod themes;

/// Directory name under the platform's config root — a working name (the
/// product itself doesn't have a settled one yet, same open state as the
/// theme question in CONOPS §8), picked from this repo's own directory
/// name rather than inventing branding. Revisit if/when the project is
/// actually named.
const APP_NAME: &str = "pain";

/// Bounds every numeric appearance value is forced into by
/// [`Config::sanitize`]. These match the settings panel's own slider ranges,
/// so a hand-edited file can't reach a state the UI would never produce.
///
/// The font-size bounds are not cosmetic. `render::measure_cell` builds a
/// `cosmic_text::Metrics` from the font size, and a size of zero gives a
/// zero line height, which `cosmic_text::Buffer` asserts against — a
/// `font_size = 0` in a hand-edited file used to panic the whole app the
/// moment the hot-reload watcher picked the edit up.
pub const MIN_FONT_SIZE: u32 = 6;
pub const MAX_FONT_SIZE: u32 = 48;

/// `transparency` is a percentage: 0 is fully transparent, 100 fully
/// opaque.
pub const MAX_TRANSPARENCY: u32 = 100;
/// Effect strengths are percentages: 0 is off, 100 is as strong as the
/// effect goes.
pub const MAX_EFFECT: u32 = 100;
/// A ceiling on retained history per pane. Scrollback is allocated as it
/// fills rather than up front, so this bounds how much memory a pane can
/// eventually reach, not what it starts at.
pub const MAX_SCROLLBACK_LINES: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub appearance: Appearance,
    pub cursor: Cursor,
    pub retro: Retro,
    /// Chord string (e.g. `"ctrl shift e"`) to action name (e.g.
    /// `"split_vertical"`), overriding the built-in Terminator-equivalent
    /// keymap. `BTreeMap` rather than `HashMap` so `Config::save` writes a
    /// stable, deterministically ordered file. `"none"` as the action name
    /// unbinds the chord without a replacement (Milestone 5.3's job to
    /// apply — this struct just carries the data).
    pub keybindings: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct General {
    /// Empty means "platform default" ($SHELL, or the user's configured
    /// default shell) — not `Option<String>`, so an explicit empty string
    /// in a hand-edited file round-trips the same way as an absent key.
    pub default_shell: String,
    pub scrollback_lines: usize,
    /// Ask before pasting text that would run more than one command in a
    /// program that hasn't enabled bracketed paste. On by default: without
    /// bracketing, every newline in a paste executes the moment it
    /// arrives, so an unreviewed multi-line paste runs arbitrary commands
    /// with no chance to look at them first.
    pub confirm_multiline_paste: bool,
}

impl Default for General {
    fn default() -> Self {
        General { default_shell: String::new(), scrollback_lines: 5000, confirm_multiline_paste: true }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Appearance {
    /// Name of a built-in theme (see [`themes::THEMES`]) — the source of
    /// the 16 ANSI colors and the default foreground/background a cell
    /// falls back to. An unrecognized name resolves to
    /// [`themes::DEFAULT_THEME`] rather than failing to load, the same
    /// "never crash on a bad edit" convention as the color fields.
    ///
    /// No inline color table support: a theme is picked by name, and a
    /// one-off tweak is what `background_color` is for.
    pub theme: String,
    pub font_family: String,
    /// Point size. An integer: a terminal's font size is a small whole
    /// number and the fractional part was never anything a user chose.
    #[serde(deserialize_with = "int_or_float")]
    pub font_size: u32,
    /// Shape each row's text in runs so the font can apply ligatures
    /// (`!=` rendering as `≠`), rather than rasterizing every cell
    /// independently.
    ///
    /// **Off by default, deliberately.** A terminal grid is fixed-width and
    /// this hands glyph positioning to the font's own advances: with a font
    /// designed for it (Fira Code, JetBrains Mono, Cascadia Code, Iosevka)
    /// that lines up exactly, but a font whose ligature advances don't match
    /// its cell width will drift out of the grid. It also costs real work
    /// per frame that the per-character path doesn't — text has to be shaped,
    /// not just looked up. Neither is a cost to impose on someone who never
    /// asked for ligatures.
    pub ligatures: bool,
    /// Window opacity as a percentage: 0 fully transparent, 100 fully
    /// opaque.
    ///
    /// Configs written before 1.9 stored this as a 0.0–1.0 fraction. TOML
    /// distinguishes `0.7` from `70` by type, so those are read as
    /// fractions and converted rather than being rejected — otherwise the
    /// whole file would fail to parse and every other setting in it would
    /// silently revert to a default.
    #[serde(deserialize_with = "percent_or_fraction")]
    pub transparency: u32,
    /// Terminal background override, as `#rrggbb` hex.
    ///
    /// **Empty means "follow the chosen theme"**, which is the default —
    /// a theme's background is part of its identity, and a light theme
    /// forced onto a near-black ground is unreadable. A non-empty value
    /// wins over the theme, so a config that set this before themes
    /// existed keeps doing exactly what its author asked for rather than
    /// silently losing the setting.
    ///
    /// Parse failures (a hand-edited value that isn't valid hex) fall back
    /// to the theme's background via `background_rgb`, not a load error —
    /// consistent with the rest of this config's "never crash on a bad
    /// edit" handling.
    pub background_color: String,
    /// The one accent color used throughout the chrome — cursor, text
    /// selection, focus/interactive highlights in menus and the settings
    /// panel. Deliberately a single user-configurable color rather than a
    /// full theme (CONOPS §8 is still open on that): semantic colors
    /// (e.g. the broadcast-target border) stay fixed regardless, since
    /// they're a distinct signal, not decoration. Same hex format and
    /// same "never crash on a bad edit" fallback convention as
    /// `background_color`.
    pub accent_color: String,
    /// The background of a pane's title bar, as `#rrggbb`.
    ///
    /// Only applies to panes that aren't in a group: a grouped pane's title
    /// bar is colored by its group, which is the whole point of the group
    /// color, and overriding that would remove the only way to tell groups
    /// apart at a glance.
    ///
    /// Same "never crash on a bad edit" fallback as the colors above — an
    /// unparseable value falls back to the default rather than failing the
    /// load.
    pub title_bar_color: String,
}

/// The "Graphite" palette's own accent (a desaturated slate blue) — the
/// default `accent_color`, and the fallback if a hand-edited value fails
/// to parse.
const DEFAULT_ACCENT_RGB: [f32; 3] = [127.0 / 255.0, 162.0 / 255.0, 214.0 / 255.0];

/// The default title bar background — the dark grey the chrome has always
/// drawn, kept as the default so enabling the setting restyles nothing.
const DEFAULT_TITLE_BAR_RGB: [f32; 3] = [20.0 / 255.0, 23.0 / 255.0, 27.0 / 255.0];

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            theme: themes::DEFAULT_THEME.to_string(),
            font_family: "monospace".to_string(),
            font_size: 13,
            ligatures: false,
            transparency: MAX_TRANSPARENCY,
            // Empty: follow the theme. See the field's own doc comment.
            background_color: String::new(),
            accent_color: format_hex_rgb(DEFAULT_ACCENT_RGB),
            title_bar_color: format_hex_rgb(DEFAULT_TITLE_BAR_RGB),
        }
    }
}

/// Unpacks a `0xRRGGBB` theme color into 0.0–1.0 RGB.
fn unpack_rgb(packed: u32) -> [f32; 3] {
    let channel = |shift: u32| ((packed >> shift) & 0xff) as f32 / 255.0;
    [channel(16), channel(8), channel(0)]
}

impl Appearance {
    /// The built-in theme `theme` names, falling back to the default for an
    /// unset or unrecognized name.
    pub fn resolved_theme(&self) -> &'static themes::Theme {
        themes::find(&self.theme).unwrap_or_else(themes::default_theme)
    }

    /// The 16 ANSI colors (0-7 normal, 8-15 bright) of the chosen theme, as
    /// 0.0–1.0 RGB.
    pub fn palette(&self) -> [[f32; 3]; 16] {
        self.resolved_theme().ansi.map(unpack_rgb)
    }

    /// What a cell left at its default foreground color resolves to — the
    /// chosen theme's foreground.
    pub fn foreground_rgb(&self) -> [f32; 3] {
        unpack_rgb(self.resolved_theme().foreground)
    }

    /// The effective terminal background: `background_color` if it's set to
    /// valid hex, otherwise the chosen theme's own background.
    pub fn background_rgb(&self) -> [f32; 3] {
        parse_hex_rgb(&self.background_color).unwrap_or_else(|| unpack_rgb(self.resolved_theme().background))
    }

    /// Sets `background_color` from 0.0–1.0 RGB (e.g. from a UI color
    /// picker), formatted as `#rrggbb` — the inverse of `background_rgb`.
    /// This makes it an explicit override; see `follow_theme_background` to
    /// undo it.
    pub fn set_background_rgb(&mut self, rgb: [f32; 3]) {
        self.background_color = format_hex_rgb(rgb);
    }

    /// Clears any background override, so the background follows whichever
    /// theme is chosen.
    pub fn follow_theme_background(&mut self) {
        self.background_color.clear();
    }

    /// Whether the background currently follows the theme rather than an
    /// explicit override.
    pub fn follows_theme_background(&self) -> bool {
        self.background_color.is_empty()
    }

    /// Parses `accent_color` into 0.0–1.0 RGB, falling back to the
    /// Graphite default if it isn't valid `#rrggbb` (or `rrggbb`) hex.
    pub fn accent_rgb(&self) -> [f32; 3] {
        parse_hex_rgb(&self.accent_color).unwrap_or(DEFAULT_ACCENT_RGB)
    }

    /// Sets `accent_color` from 0.0–1.0 RGB — the inverse of `accent_rgb`.
    pub fn set_accent_rgb(&mut self, rgb: [f32; 3]) {
        self.accent_color = format_hex_rgb(rgb);
    }

    /// Parses `title_bar_color` into 0.0–1.0 RGB, falling back to the
    /// default dark grey if it isn't valid hex.
    pub fn title_bar_rgb(&self) -> [f32; 3] {
        parse_hex_rgb(&self.title_bar_color).unwrap_or(DEFAULT_TITLE_BAR_RGB)
    }

    /// Sets `title_bar_color` from 0.0–1.0 RGB — the inverse of
    /// `title_bar_rgb`.
    pub fn set_title_bar_rgb(&mut self, rgb: [f32; 3]) {
        self.title_bar_color = format_hex_rgb(rgb);
    }
}

/// Prints what [`Config::sanitize`] changed, one line each. Public so the
/// app's hot-reload path reports adjustments in the same voice the initial
/// load does, instead of formatting them itself.
pub fn report(adjustments: &[String]) {
    for adjustment in adjustments {
        eprintln!("config: {adjustment}");
    }
}

/// `value` clamped into `min..=max`, or `fallback` when it's `NaN` —
/// `f32::clamp` propagates `NaN` straight through rather than bounding it,
/// so a `NaN` in the file would otherwise pass this check untouched.
/// A TOML number that may be written either way. Both settings below
/// changed from a float to an integer, and a config file predating that
/// still has to load — serde would otherwise reject the float outright,
/// failing the *whole* file and reverting every unrelated setting in it.
#[derive(Deserialize)]
#[serde(untagged)]
enum Number {
    Int(i64),
    Float(f64),
}

/// Reads an integer, accepting a float from an older config by rounding it.
fn int_or_float<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    Ok(match Number::deserialize(d)? {
        Number::Int(n) => n.clamp(0, i64::from(u32::MAX)) as u32,
        Number::Float(f) if f.is_finite() => f.round().clamp(0.0, f64::from(u32::MAX)) as u32,
        Number::Float(_) => 0,
    })
}

/// Reads a 0-100 percentage, accepting the 0.0-1.0 fraction older configs
/// stored. TOML types tell the two apart: `1` is one percent, `1.0` is the
/// old fully-opaque fraction.
fn percent_or_fraction<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    Ok(match Number::deserialize(d)? {
        Number::Int(n) => n.clamp(0, i64::from(MAX_TRANSPARENCY)) as u32,
        Number::Float(f) if f.is_finite() => (f * 100.0).round().clamp(0.0, f64::from(MAX_TRANSPARENCY)) as u32,
        Number::Float(_) => MAX_TRANSPARENCY,
    })
}

fn format_hex_rgb(rgb: [f32; 3]) -> String {
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", channel(rgb[0]), channel(rgb[1]), channel(rgb[2]))
}

fn parse_hex_rgb(s: &str) -> Option<[f32; 3]> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 {
        return None;
    }
    let channel = |range: std::ops::Range<usize>| -> Option<f32> {
        Some(u8::from_str_radix(hex.get(range)?, 16).ok()? as f32 / 255.0)
    };
    Some([channel(0..2)?, channel(2..4)?, channel(4..6)?])
}

/// Period looks, off by default. See [`era`] for what an era is and why the
/// list has something missing from it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Retro {
    /// Name of an era (see [`era::ERAS`]). **Empty is the default and means
    /// off** — byte-for-byte the same rendering as if this section did not
    /// exist.
    ///
    /// An active era overrides `appearance.theme` with its own palette. It
    /// never writes to it: turning the era off restores whatever theme was
    /// chosen, so trying one on cannot lose a setting. An unrecognised name
    /// is treated as off rather than failing the load, the same convention
    /// as every other name in this file.
    pub era: String,
    /// Scanline strength, 0-100. Absent follows the era; `0` turns them off
    /// while keeping the rest of it.
    pub scanlines: Option<u32>,
    /// Corner darkening, 0-100. Absent follows the era.
    pub vignette: Option<u32>,
    /// Strength of the drifting mains-hum bar, 0-100. Absent follows the era.
    ///
    /// **This is the only effect that animates**, and so the only one that
    /// stops an idle terminal from sleeping. The cost is bounded — it stops
    /// entirely when the window isn't focused, and redraws at a fraction of
    /// the display's rate, since the bar takes seconds to cross the screen —
    /// but `0` is how you get the sleep back completely.
    pub hum: Option<u32>,
}

/// The 16 ANSI colors of `theme`, as 0.0–1.0 RGB.
///
/// Free functions rather than methods because a caller may hold a theme that
/// came from somewhere other than a `Config` — a session-only era override,
/// for instance — and shouldn't have to fabricate a `Config` to unpack it.
pub fn palette_of(theme: &themes::Theme) -> [[f32; 3]; 16] {
    theme.ansi.map(unpack_rgb)
}

/// What a cell left at its default foreground resolves to under `theme`.
pub fn foreground_of(theme: &themes::Theme) -> [f32; 3] {
    unpack_rgb(theme.foreground)
}

/// `theme`'s own background.
pub fn background_of(theme: &themes::Theme) -> [f32; 3] {
    unpack_rgb(theme.background)
}

/// An explicit `background_color`, if one is set and parses. `None` means
/// "follow whichever theme is in effect".
pub fn background_override(appearance: &Appearance) -> Option<[f32; 3]> {
    parse_hex_rgb(&appearance.background_color)
}

impl Config {
    /// The theme actually in effect. An active era's palette overrides the
    /// chosen theme.
    ///
    /// This is an *overlay*, not an assignment: `appearance.theme` is left
    /// exactly as the user set it, so turning an era off restores their
    /// theme and trying one on can never lose a setting. That's why callers
    /// go through here rather than through `appearance` directly.
    pub fn effective_theme(&self) -> &'static themes::Theme {
        if let Some(era) = self.retro.resolved_era()
            && let Some(theme) = themes::find(era.theme)
        {
            return theme;
        }
        self.appearance.resolved_theme()
    }

    /// The 16 ANSI colors in effect, era included.
    pub fn palette(&self) -> [[f32; 3]; 16] {
        self.effective_theme().ansi.map(unpack_rgb)
    }

    /// What a cell left at its default foreground resolves to, era included.
    pub fn foreground_rgb(&self) -> [f32; 3] {
        unpack_rgb(self.effective_theme().foreground)
    }

    /// The effective terminal background. An explicit
    /// `appearance.background_color` still wins over both era and theme —
    /// someone who pinned their background meant it.
    pub fn background_rgb(&self) -> [f32; 3] {
        parse_hex_rgb(&self.appearance.background_color)
            .unwrap_or_else(|| unpack_rgb(self.effective_theme().background))
    }
}

impl Retro {
    /// The era in effect, or `None` when off or unrecognised.
    pub fn resolved_era(&self) -> Option<&'static era::Era> {
        era::find(&self.era)
    }

    /// Effective scanline strength, 0-100.
    pub fn scanlines(&self) -> u32 {
        self.resolve(self.scanlines, |era| era.scanlines).min(MAX_EFFECT)
    }

    /// Effective corner darkening, 0-100.
    pub fn vignette(&self) -> u32 {
        self.resolve(self.vignette, |era| era.vignette).min(MAX_EFFECT)
    }

    /// Effective hum-bar strength, 0-100.
    pub fn hum(&self) -> u32 {
        self.resolve(self.hum, |era| era.hum).min(MAX_EFFECT)
    }

    /// Explicit setting if there is one, else the era's value, else zero.
    ///
    /// The one place the "absent follows the bundle" rule lives, so a new
    /// era-backed setting can't accidentally resolve differently from the
    /// others.
    fn resolve(&self, explicit: Option<u32>, from_era: impl Fn(&era::Era) -> u32) -> u32 {
        match explicit {
            Some(value) => value,
            None => self.resolved_era().map_or(0, from_era),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Cursor {
    pub style: CursorStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    #[default]
    Block,
    Underline,
    Beam,
}

impl Config {
    /// The platform config file path: `$XDG_CONFIG_HOME/pain/config.toml`
    /// (falling back to `~/.config/pain/`) on Linux, `~/Library/Application
    /// Support/pain/config.toml` on macOS, `%APPDATA%\pain\config.toml` on
    /// Windows — per `.waypoint/design/config-system.md`.
    pub fn default_path() -> PathBuf {
        dir().join("config.toml")
    }

    /// Loads config from `path`, falling back to all-defaults on *any*
    /// problem (missing file or unparseable one) and reporting unparseable
    /// ones to stderr. Fine for a first load, where there's no previous
    /// config to fall back to anyway — hot reload (Milestone 5.2) needs to
    /// tell "missing" and "broken" apart instead, since a broken edit
    /// should keep whatever was running, not reset it to defaults; that's
    /// what `try_load` is for.
    pub fn load(path: &Path) -> Config {
        match Self::try_load(path) {
            Ok((config, adjustments)) => {
                report(&adjustments);
                config
            }
            Err(err) => {
                eprintln!("config: failed to parse {}: {err}", path.display());
                Config::default()
            }
        }
    }

    /// Loads config from `path`. A missing (or otherwise unreadable) file
    /// is not an error — `Ok(Config::default())`, exactly as `.waypoint/
    /// design/config-system.md` specifies. A present-but-unparseable file
    /// is `Err`, so a caller doing a hot reload can keep the last-good
    /// config on a bad edit instead of resetting to defaults.
    /// Also returns whatever [`Config::sanitize`] had to change, phrased for
    /// the user, rather than printing it here. A file watcher re-reads the
    /// file several times for a single save (one write is several
    /// filesystem events, and only the caller knows whether the result
    /// differs from what's already loaded), so reporting at parse time
    /// printed the same complaint about a dozen times per edit. The caller
    /// reports it at the point it decides to actually apply the result.
    pub fn try_load(path: &Path) -> Result<(Config, Vec<String>), toml::de::Error> {
        match std::fs::read_to_string(path) {
            Ok(contents) => {
                let mut config: Config = toml::from_str(&contents)?;
                let adjustments = config.sanitize();
                Ok((config, adjustments))
            }
            Err(_) => Ok((Config::default(), Vec::new())),
        }
    }

    /// Forces every numeric value into a range the rest of the app can
    /// actually handle, reporting anything it had to change.
    ///
    /// Applied to every file that parses, so no hand-edited value ever
    /// reaches the renderer or the terminal grid unchecked. This is the same
    /// "never crash on a bad edit" convention `background_color`'s parse
    /// fallback already follows, extended to the numeric fields — which
    /// previously had no validation at all, and where a bad value was not a
    /// cosmetic problem but a panic or a hang (see [`MIN_FONT_SIZE`]).
    ///
    /// Out-of-range values are clamped rather than reset to the default: a
    /// `font_size = 100` is a legible intent ("as big as you'll give me"),
    /// and 48 serves it better than silently dropping back to 13 would.
    /// A non-finite value has no intent to preserve, so it takes the
    /// default.
    fn sanitize(&mut self) -> Vec<String> {
        let mut adjustments = Vec::new();

        let font_size = self.appearance.font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        if font_size != self.appearance.font_size {
            adjustments.push(format!(
                "font_size {} is out of range ({MIN_FONT_SIZE}-{MAX_FONT_SIZE}); using {font_size}",
                self.appearance.font_size
            ));
            self.appearance.font_size = font_size;
        }

        let transparency = self.appearance.transparency.min(MAX_TRANSPARENCY);
        if transparency != self.appearance.transparency {
            adjustments.push(format!(
                "transparency {} is out of range (0-{MAX_TRANSPARENCY}); using {transparency}",
                self.appearance.transparency
            ));
            self.appearance.transparency = transparency;
        }

        if self.general.scrollback_lines > MAX_SCROLLBACK_LINES {
            adjustments.push(format!(
                "scrollback_lines {} exceeds the {MAX_SCROLLBACK_LINES} line maximum; using {MAX_SCROLLBACK_LINES}",
                self.general.scrollback_lines
            ));
            self.general.scrollback_lines = MAX_SCROLLBACK_LINES;
        }

        adjustments
    }

    /// Serializes and writes `self` to `path`, creating its parent
    /// directory first if needed. This is the entire "apply" step for a
    /// settings-panel save — writing the file is all it does; the
    /// already-running hot-reload watcher (Milestone 5.2) picks the change
    /// up exactly the way it would a hand edit, per `.waypoint/design/
    /// config-system.md`'s single-apply-path rule (no separate "apply from
    /// UI" path that could drift out of sync with "apply from file").
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let contents = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, contents)
    }
}

/// The platform config directory itself (`config.toml`'s parent), public
/// so other files this app stores alongside it — the `session` crate's
/// session file — resolve to the same place without duplicating this
/// platform-detection logic.
#[cfg(target_os = "windows")]
pub fn dir() -> PathBuf {
    match std::env::var_os("APPDATA") {
        Some(appdata) => PathBuf::from(appdata).join(APP_NAME),
        None => {
            eprintln!("config: %APPDATA% is not set; using current directory for config storage");
            PathBuf::from(".").join(APP_NAME)
        }
    }
}

#[cfg(target_os = "macos")]
pub fn dir() -> PathBuf {
    home_dir().join("Library").join("Application Support").join(APP_NAME)
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn dir() -> PathBuf {
    match std::env::var_os("XDG_CONFIG_HOME") {
        Some(xdg) => PathBuf::from(xdg).join(APP_NAME),
        None => home_dir().join(".config").join(APP_NAME),
    }
}

#[cfg(not(target_os = "windows"))]
fn home_dir() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home),
        None => {
            eprintln!("config: $HOME is not set; using current directory for config storage");
            PathBuf::from(".")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn missing_file_loads_all_defaults() {
        let path = PathBuf::from("/nonexistent/definitely/not/a/real/path/config.toml");
        assert_eq!(Config::load(&path), Config::default());
    }

    #[test]
    fn present_file_overrides_only_what_it_sets() {
        let dir = std::env::temp_dir().join(format!("pain-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let mut file = std::fs::File::create(&path).unwrap();
        write!(file, "[general]\nscrollback_lines = 1234\n").unwrap();
        drop(file);

        let config = Config::load(&path);
        assert_eq!(config.general.scrollback_lines, 1234);
        // Everything not set in the file keeps its default.
        assert_eq!(config.general.default_shell, "");
        assert_eq!(config.appearance, Appearance::default());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn malformed_file_falls_back_to_defaults() {
        let dir = std::env::temp_dir().join(format!("pain-config-test-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "this is not [valid toml").unwrap();

        assert_eq!(Config::load(&path), Config::default());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn keybinding_overrides_parse_as_a_sparse_map() {
        let dir = std::env::temp_dir().join(format!("pain-config-test-kb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "[keybindings]\n\"ctrl+shift+e\" = \"split_vertical\"\n").unwrap();

        let config = Config::load(&path);
        assert_eq!(config.keybindings.get("ctrl+shift+e"), Some(&"split_vertical".to_string()));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn retro_is_off_by_default_and_changes_nothing() {
        let retro = Retro::default();
        assert!(retro.resolved_era().is_none());
        assert_eq!(retro.scanlines(), 0);
        assert_eq!(retro.vignette(), 0);
        assert_eq!(retro.hum(), 0, "nothing animates by default, so an idle terminal still sleeps");
    }

    /// An era supplies every retro setting that isn't spelled out, which is
    /// what makes `era = "bbs"` a complete look rather than a colour scheme.
    #[test]
    fn an_era_supplies_the_settings_that_are_absent() {
        let retro = Retro { era: "bbs".to_string(), ..Default::default() };
        let era = era::find("bbs").expect("bbs exists");

        assert_eq!(retro.scanlines(), era.scanlines);
        assert_eq!(retro.vignette(), era.vignette);
        assert!(era.scanlines > 0, "an era has to change something a plain theme can't");
    }

    /// Explicit beats the era, including explicit zero — "this era but
    /// without the scanlines" has to be expressible.
    #[test]
    fn an_explicit_setting_overrides_the_era_including_zero() {
        let retro = Retro { era: "bbs".to_string(), scanlines: Some(0), ..Default::default() };

        assert_eq!(retro.scanlines(), 0, "an explicit 0 must turn the effect off, not fall through to the era");
        assert_eq!(retro.vignette(), era::find("bbs").unwrap().vignette, "unset still follows the era");
    }

    #[test]
    fn an_unknown_era_behaves_as_off() {
        let retro = Retro { era: "nineteen-eighty-four".to_string(), ..Default::default() };
        assert!(retro.resolved_era().is_none());
        assert_eq!(retro.scanlines(), 0);
    }

    #[test]
    fn effect_strengths_are_clamped_to_the_documented_range() {
        let retro = Retro { scanlines: Some(4000), vignette: Some(999), ..Default::default() };
        assert_eq!(retro.scanlines(), MAX_EFFECT);
        assert_eq!(retro.vignette(), MAX_EFFECT);
    }

    /// The era overlays the theme without touching it, so turning the era off
    /// restores whatever the user actually chose.
    #[test]
    fn an_era_overlays_the_theme_without_overwriting_it() {
        let mut config = Config::default();
        config.appearance.theme = "Dracula".to_string();
        let chosen = config.effective_theme().name;

        config.retro.era = "green".to_string();
        assert_eq!(config.effective_theme().name, "Green Phosphor CRT");
        assert_eq!(config.appearance.theme, "Dracula", "the era must not rewrite the chosen theme");

        config.retro.era.clear();
        assert_eq!(config.effective_theme().name, chosen, "turning the era off restores the theme");
    }

    /// A pinned background is a deliberate choice and outranks an era, the
    /// same way it already outranks a theme.
    #[test]
    fn an_explicit_background_still_wins_over_an_era() {
        let mut config = Config::default();
        config.retro.era = "green".to_string();
        config.appearance.background_color = "#123456".to_string();

        assert_eq!(config.background_rgb(), parse_hex_rgb("#123456").unwrap());
    }

    #[test]
    fn background_color_parses_hex_with_or_without_hash() {
        assert_eq!(parse_hex_rgb("#ff0080"), Some([1.0, 0.0, 128.0 / 255.0]));
        assert_eq!(parse_hex_rgb("ff0080"), Some([1.0, 0.0, 128.0 / 255.0]));
    }

    #[test]
    fn background_color_falls_back_to_the_theme_when_invalid() {
        assert_eq!(parse_hex_rgb("not-a-color"), None);
        assert_eq!(parse_hex_rgb("#zzzzzz"), None);
        let appearance = Appearance { background_color: "garbage".to_string(), ..Appearance::default() };
        assert_eq!(appearance.background_rgb(), unpack_rgb(themes::default_theme().background));
    }

    /// The default has no override, so the background is the theme's — and
    /// changing theme moves it. A theme's background is part of its
    /// identity; a light theme on a forced near-black ground is unreadable.
    #[test]
    fn an_unset_background_follows_the_chosen_theme() {
        let mut appearance = Appearance::default();
        assert!(appearance.follows_theme_background());
        assert_eq!(appearance.background_rgb(), unpack_rgb(themes::default_theme().background));

        appearance.theme = "Dracula".to_string();
        assert_eq!(appearance.background_rgb(), unpack_rgb(0x282a36));
    }

    /// A config written before themes existed set this explicitly. That
    /// value has to keep winning, or upgrading would silently discard a
    /// setting its author deliberately chose.
    #[test]
    fn an_explicit_background_overrides_the_theme() {
        let appearance =
            Appearance { theme: "Dracula".to_string(), background_color: "#123456".to_string(), ..Default::default() };
        assert!(!appearance.follows_theme_background());
        assert_eq!(appearance.background_rgb(), parse_hex_rgb("#123456").unwrap());
    }

    #[test]
    fn following_the_theme_background_again_clears_an_override() {
        let mut appearance = Appearance { theme: "Dracula".to_string(), ..Default::default() };
        appearance.set_background_rgb([1.0, 0.0, 0.0]);
        assert!(!appearance.follows_theme_background());

        appearance.follow_theme_background();
        assert!(appearance.follows_theme_background());
        assert_eq!(appearance.background_rgb(), unpack_rgb(0x282a36));
    }

    #[test]
    fn an_unrecognized_theme_name_resolves_to_the_default_rather_than_failing() {
        let appearance = Appearance { theme: "no such theme".to_string(), ..Default::default() };
        assert_eq!(appearance.resolved_theme().name, themes::DEFAULT_THEME);
    }

    #[test]
    fn the_palette_unpacks_a_themes_sixteen_ansi_slots() {
        let appearance = Appearance { theme: "Dracula".to_string(), ..Default::default() };
        let palette = appearance.palette();
        assert_eq!(palette[1], unpack_rgb(0xff5555), "ANSI red");
        assert_eq!(palette[8], unpack_rgb(0x6272a4), "ANSI bright black");
        assert_eq!(appearance.foreground_rgb(), unpack_rgb(0xf8f8f2));
    }

    /// The shipped default must be visually identical to what the app
    /// looked like before themes existed, or every existing user's terminal
    /// silently restyles on upgrade.
    #[test]
    fn the_default_theme_preserves_the_original_graphite_look() {
        let appearance = Appearance::default();
        assert_eq!(appearance.background_rgb(), [12.0 / 255.0, 14.0 / 255.0, 17.0 / 255.0]);
        assert_eq!(appearance.foreground_rgb(), [223.0 / 255.0, 226.0 / 255.0, 230.0 / 255.0]);
        // xterm's standard palette, which is what `color.rs` hardcoded.
        assert_eq!(appearance.palette()[0], [0.0, 0.0, 0.0]);
        assert_eq!(appearance.palette()[15], [1.0, 1.0, 1.0]);
    }

    #[test]
    fn set_background_rgb_round_trips_through_hex() {
        let mut appearance = Appearance::default();
        appearance.set_background_rgb([1.0, 0.0, 128.0 / 255.0]);
        assert_eq!(appearance.background_color, "#ff0080");
        assert_eq!(appearance.background_rgb(), [1.0, 0.0, 128.0 / 255.0]);
    }

    #[test]
    fn accent_color_falls_back_to_the_graphite_default_when_invalid() {
        let appearance = Appearance { accent_color: "garbage".to_string(), ..Appearance::default() };
        assert_eq!(appearance.accent_rgb(), DEFAULT_ACCENT_RGB);
    }

    #[test]
    fn set_accent_rgb_round_trips_through_hex() {
        let mut appearance = Appearance::default();
        appearance.set_accent_rgb([0.0, 1.0, 128.0 / 255.0]);
        assert_eq!(appearance.accent_color, "#00ff80");
        assert_eq!(appearance.accent_rgb(), [0.0, 1.0, 128.0 / 255.0]);
    }

    #[test]
    fn set_title_bar_rgb_round_trips_through_hex() {
        let mut appearance = Appearance::default();
        appearance.set_title_bar_rgb([1.0, 0.0, 0.5]);
        assert_eq!(appearance.title_bar_color, "#ff0080");
        assert_eq!(appearance.title_bar_rgb(), [1.0, 0.0, 128.0 / 255.0]);
    }

    #[test]
    fn title_bar_color_falls_back_to_the_default_when_invalid() {
        let appearance = Appearance { title_bar_color: "nonsense".to_string(), ..Appearance::default() };
        assert_eq!(appearance.title_bar_rgb(), DEFAULT_TITLE_BAR_RGB);
    }

    /// A config written before this setting existed has to keep working and
    /// keep looking the same — the field defaults in, at the color the
    /// chrome already drew.
    #[test]
    fn a_config_without_a_title_bar_color_loads_at_the_previous_default() {
        let appearance: Appearance = toml::from_str("theme = \"Dracula\"").expect("older configs still parse");
        assert_eq!(appearance.theme, "Dracula");
        assert_eq!(appearance.title_bar_rgb(), DEFAULT_TITLE_BAR_RGB);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = std::env::temp_dir().join(format!("pain-config-test-save-{}", std::process::id()));
        let path = dir.join("nested").join("config.toml");

        let mut config = Config::default();
        config.appearance.font_size = 21;
        config.general.default_shell = "/bin/zsh".to_string();
        config.keybindings.insert("ctrl+shift+e".to_string(), "close_pane".to_string());

        config.save(&path).expect("save should create parent dirs and write the file");
        assert_eq!(Config::load(&path), config);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Writes `body` to a throwaway `config.toml` and loads it back, so
    /// these exercise the real parse-then-sanitize path rather than calling
    /// `sanitize` directly — the file is where a bad value actually comes
    /// from, and `try_load` is the only thing standing between it and the
    /// renderer.
    fn load_from_toml(name: &str, body: &str) -> Config {
        let dir = std::env::temp_dir().join(format!("pain-config-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, body).unwrap();
        let config = Config::load(&path);
        std::fs::remove_dir_all(&dir).ok();
        config
    }

    /// A zero font size used to take down the whole app: it reaches
    /// `cosmic_text::Metrics` as a zero *line height*, which `Buffer`
    /// asserts against. Via the config watcher that meant saving the file
    /// panicked a running terminal — not a startup-only problem.
    #[test]
    fn a_zero_font_size_is_clamped_rather_than_reaching_the_renderer() {
        let config = load_from_toml("zero-font", "[appearance]\nfont_size = 0.0\n");
        assert_eq!(config.appearance.font_size, MIN_FONT_SIZE);
    }

    /// A negative font size didn't panic — it hung, spinning at 100% CPU
    /// inside text layout and never returning, which is strictly worse
    /// (no message, no exit, nothing to report).
    #[test]
    fn a_negative_font_size_is_clamped_rather_than_reaching_the_renderer() {
        let config = load_from_toml("negative-font", "[appearance]\nfont_size = -13.0\n");
        assert_eq!(config.appearance.font_size, MIN_FONT_SIZE);
    }

    #[test]
    fn an_absurdly_large_font_size_is_clamped_to_the_maximum() {
        let config = load_from_toml("huge-font", "[appearance]\nfont_size = 4000.0\n");
        assert_eq!(config.appearance.font_size, MAX_FONT_SIZE);
    }

    /// A non-numeric value can't round to an integer at all, so it reads
    /// as zero and the range clamp takes it from there — never reaching
    /// the renderer, which is the whole point (a zero size panics text
    /// layout).
    #[test]
    fn a_non_numeric_font_size_is_clamped_rather_than_reaching_the_renderer() {
        let config = load_from_toml("nan-font", "[appearance]\nfont_size = nan\n");
        assert_eq!(config.appearance.font_size, MIN_FONT_SIZE);
    }

    #[test]
    fn transparency_outside_the_percentage_range_is_clamped() {
        assert_eq!(load_from_toml("over-alpha", "[appearance]\ntransparency = 400\n").appearance.transparency, 100);
        assert_eq!(load_from_toml("under-alpha", "[appearance]\ntransparency = -1\n").appearance.transparency, 0);
    }

    /// Configs written before 1.9 stored these as floats — a font size with
    /// a `.0` and transparency as a 0.0-1.0 fraction. Rejecting them would
    /// fail the whole file and silently revert every unrelated setting in
    /// it, so both are read and converted.
    #[test]
    fn a_pre_integer_config_is_read_rather_than_rejected() {
        let config = load_from_toml(
            "legacy-floats",
            "[appearance]\nfont_size = 14.0\ntransparency = 0.7\n[general]\ndefault_shell = \"/bin/zsh\"\n",
        );
        assert_eq!(config.appearance.font_size, 14);
        assert_eq!(config.appearance.transparency, 70, "0.7 is 70%");
        assert_eq!(config.general.default_shell, "/bin/zsh", "unrelated settings survive");
    }

    /// TOML's own types separate the two meanings, which is what makes the
    /// conversion above unambiguous.
    #[test]
    fn an_integer_transparency_is_a_percentage_and_a_float_is_a_fraction() {
        assert_eq!(load_from_toml("int-one", "[appearance]\ntransparency = 1\n").appearance.transparency, 1);
        assert_eq!(load_from_toml("float-one", "[appearance]\ntransparency = 1.0\n").appearance.transparency, 100);
    }

    #[test]
    fn scrollback_lines_is_capped() {
        let config = load_from_toml("huge-scrollback", "[general]\nscrollback_lines = 99999999999\n");
        assert_eq!(config.general.scrollback_lines, MAX_SCROLLBACK_LINES);
    }

    #[test]
    fn values_already_in_range_are_left_exactly_as_written() {
        let config = load_from_toml(
            "in-range",
            "[appearance]\nfont_size = 17\ntransparency = 80\n\n[general]\nscrollback_lines = 200\n",
        );
        assert_eq!(config.appearance.font_size, 17);
        assert_eq!(config.appearance.transparency, 80);
        assert_eq!(config.general.scrollback_lines, 200);
    }
}
