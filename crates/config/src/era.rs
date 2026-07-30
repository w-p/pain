//! Retro "eras": period looks, each a bundle of settings rather than code.
//!
//! An era names a specific machine, not a vague retro mood — the palette,
//! the screen effects, and the typeface that machine actually had.
//! Everything an era does is expressed as data in [`ERAS`], so adding one is
//! a table row and the renderer never grows a branch per era.
//!
//! Palettes are reused straight from [`crate::themes`], which already ships
//! period-accurate schemes, rather than defining a second color format.

/// One era: what to call it, and what it changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Era {
    /// Lowercase, no spaces — this is what a user types into a config file,
    /// a `--era` flag, or an escape sequence.
    pub name: &'static str,
    /// Name of the [`crate::themes`] entry supplying this era's palette.
    pub theme: &'static str,
    /// Scanline strength, 0-100. This is most of what separates an era from
    /// simply being a theme.
    pub scanlines: u32,
    /// Corner darkening, 0-100 — the curved glass of a tube.
    pub vignette: u32,
    /// The drifting mains-hum bar, 0-100. Modest by default: this is the one
    /// effect that animates, and a bright band sweeping the screen stops
    /// being charming quickly.
    pub hum: u32,
    /// Font families to prefer, most period-appropriate first, falling back
    /// to whatever the user configured if none is installed. Empty means
    /// "leave the font alone".
    ///
    /// Looked up in the system font database, the same way emoji fonts are.
    /// Nothing is bundled: an era looks its best for someone who has
    /// installed the font and merely looks *themed* for someone who hasn't.
    /// The README recommends which to install.
    pub fonts: &'static [&'static str],
    /// One line for the settings picker and `--era` listing: the machine
    /// this is, and when.
    pub blurb: &'static str,
}

/// Fonts that reproduce the IBM PC text mode faces, in the order they'd be
/// preferred. VileR's `Px437` series (int10h.org) are the faithful ones;
/// the rest are common substitutes someone may already have.
const DOS_FONTS: &[&str] =
    &["Px437 IBM VGA 8x16", "Px437 IBM VGA 9x16", "PxPlus IBM VGA8", "Perfect DOS VGA 437", "Consolas"];

/// The C64's own face, for anyone who has it installed.
const C64_FONTS: &[&str] = &["C64 Pro Mono", "Commodore 64 Pixelized", "PetMe64"];

/// The DEC terminal face, for the phosphor eras. VT323 is a free (OFL)
/// reproduction and the easiest of these to obtain — it's on Google Fonts.
const TERMINAL_FONTS: &[&str] = &["VT323", "Glass TTY VT220"];

/// Every era. Order is the order the picker shows them in.
///
/// Deliberately short. Each entry has to earn its place by being a machine
/// someone recognises and by looking clearly different from its neighbours —
/// a long list of near-identical greens is worse than five distinct looks.
pub const ERAS: &[Era] = &[
    Era {
        name: "green",
        theme: "Green Phosphor CRT",
        scanlines: 55,
        vignette: 45,
        hum: 30,
        fonts: TERMINAL_FONTS,
        blurb: "IBM 5151, 1981 — P1 green phosphor",
    },
    Era {
        name: "amber",
        theme: "Amber CRT Retro",
        scanlines: 50,
        vignette: 40,
        hum: 25,
        fonts: TERMINAL_FONTS,
        blurb: "Amber CRT, mid-1980s — P3 phosphor, easier on the eyes",
    },
    Era {
        name: "cga",
        theme: "IBM 5153 CGA",
        scanlines: 40,
        vignette: 35,
        hum: 15,
        fonts: DOS_FONTS,
        blurb: "IBM 5153, 1981 — the CGA 16",
    },
    Era {
        name: "bbs",
        theme: "IBM 5153 CGA",
        scanlines: 45,
        vignette: 40,
        hum: 20,
        fonts: DOS_FONTS,
        blurb: "Dial-up BBS, ~1993 — the CGA palette ANSI art was drawn for",
    },
    Era {
        name: "c64",
        theme: "C64",
        scanlines: 35,
        vignette: 35,
        hum: 15,
        fonts: C64_FONTS,
        blurb: "Commodore 64, 1982 — VIC-II blues",
    },
    Era {
        name: "matrix",
        theme: "Matrix",
        scanlines: 60,
        vignette: 50,
        hum: 35,
        fonts: TERMINAL_FONTS,
        blurb: "The Matrix, 1999 — the only era on this list that never existed",
    },
];

/// The era `name` refers to, or `None` if no era has that name.
///
/// Case-insensitive: this value is typed by hand into config files, flags,
/// and `printf` invocations.
pub fn find(name: &str) -> Option<&'static Era> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    ERAS.iter().find(|era| era.name.eq_ignore_ascii_case(name))
}

/// Every era, for the settings picker and the `--era=list` output.
pub fn listed() -> impl Iterator<Item = &'static Era> {
    ERAS.iter()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_era_is_findable_by_its_own_name() {
        for era in ERAS {
            assert_eq!(find(era.name).map(|found| found.name), Some(era.name));
        }
    }

    #[test]
    fn lookup_is_case_insensitive_and_ignores_surrounding_space() {
        assert_eq!(find("GREEN").map(|e| e.name), Some("green"));
        assert_eq!(find("  amber  ").map(|e| e.name), Some("amber"));
    }

    /// An empty era is how "off" is spelled, and must not resolve to
    /// whatever happens to be first in the table.
    #[test]
    fn an_empty_or_unknown_name_resolves_to_nothing() {
        assert!(find("").is_none());
        assert!(find("   ").is_none());
        assert!(find("no such era").is_none());
    }

    /// Every era's palette has to actually exist, or picking it silently
    /// falls back to the default theme and the era looks broken.
    #[test]
    fn every_eras_theme_exists() {
        for era in ERAS {
            assert!(
                crate::themes::find(era.theme).is_some(),
                "era {:?} names theme {:?}, which is not in the built-in table",
                era.name,
                era.theme
            );
        }
    }

    #[test]
    fn era_names_are_unique_lowercase_and_free_of_spaces() {
        let mut seen: Vec<&str> = Vec::new();
        for era in ERAS {
            assert_eq!(era.name, era.name.to_lowercase(), "{:?} should be lowercase", era.name);
            assert!(!era.name.contains(' '), "{:?} should have no spaces", era.name);
            assert!(!seen.contains(&era.name), "duplicate era name {:?}", era.name);
            seen.push(era.name);
        }
    }

    /// The complaint an era has to answer: without effects it is just a
    /// theme. Every era needs at least one non-color characteristic —
    /// scanlines, a vignette, a wire speed, or its own font.
    #[test]
    fn every_era_differs_from_a_plain_theme() {
        for era in ERAS {
            let distinct = era.scanlines > 0 || era.vignette > 0 || era.hum > 0 || !era.fonts.is_empty();
            assert!(distinct, "era {:?} changes nothing but colors, which a theme already does", era.name);
        }
    }

    #[test]
    fn effect_strengths_are_percentages() {
        for era in ERAS {
            assert!(era.scanlines <= 100, "{:?} scanlines out of range", era.name);
            assert!(era.vignette <= 100, "{:?} vignette out of range", era.name);
            assert!(era.hum <= 100, "{:?} hum out of range", era.name);
        }
    }

    /// Legibility comes first: an era exists to be worked in, not looked at.
    /// Anything approaching full-strength darkening would make text unreadable
    /// regardless of how authentic it is.
    #[test]
    fn no_era_darkens_the_screen_enough_to_hurt_legibility() {
        for era in ERAS {
            assert!(era.scanlines <= 65, "era {:?} scanlines are too strong to read through", era.name);
            assert!(era.vignette <= 55, "era {:?} vignette is too strong to read through", era.name);
            assert!(era.hum <= 40, "era {:?} hum bar is distracting rather than atmospheric", era.name);
        }
    }

    #[test]
    fn era_font_preferences_are_non_empty_names() {
        for era in ERAS {
            for font in era.fonts {
                assert!(!font.trim().is_empty(), "era {:?} has a blank font name", era.name);
            }
        }
    }

    #[test]
    fn every_era_has_a_blurb() {
        for era in ERAS {
            assert!(!era.blurb.is_empty(), "era {:?} needs a blurb", era.name);
        }
    }
}
