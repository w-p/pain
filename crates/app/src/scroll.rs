//! Turning scroll deltas into whole lines of scrollback.
//!
//! A mouse wheel reports discrete notches — one event, one line — and needs
//! nothing clever. A trackpad reports *pixels*, in a stream of small deltas
//! as your fingers move, and that is what this exists for.
//!
//! Converting each delta to lines and rounding it independently throws away
//! everything smaller than half a line. On a Retina display a text row is
//! around 33 physical pixels, so a trackpad's individual deltas — commonly
//! under ten pixels — all round to zero and are discarded. The terminal
//! never scrolls at all, however long you swipe. Carrying the fraction
//! between events is the whole fix: the leftovers add up until they make a
//! line, instead of being dropped one event at a time.

/// Accumulates fractional scroll, emitting whole lines as they add up.
#[derive(Debug, Default)]
pub struct Accumulator {
    /// Lines' worth of scrolling received but not yet emitted, always in
    /// `(-1.0, 1.0)`.
    ///
    /// `f64` because that is what winit reports pixel deltas in; converting
    /// down to `f32` first threw away precision for nothing.
    remainder: f64,
}

impl Accumulator {
    /// Adds `lines` (which may be fractional) and returns the whole lines
    /// that are now owed, keeping the rest for next time.
    pub fn take_lines(&mut self, lines: f64) -> i32 {
        // A non-finite delta would poison the remainder permanently, and
        // nothing would ever scroll again. The realistic source is a zero
        // cell height producing a division by zero upstream.
        if !lines.is_finite() {
            return 0;
        }

        self.remainder += lines;
        let whole = self.remainder.trunc();
        self.remainder -= whole;
        whole as i32
    }

    /// Drops any carried fraction.
    ///
    /// Used when the scroll moves to a different pane: half a line of
    /// momentum built up over one pane shouldn't spend itself on the next
    /// one the pointer happens to cross.
    pub fn reset(&mut self) {
        self.remainder = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reported bug: a trackpad's deltas are each a fraction of a line,
    /// and rounding them independently discarded every one of them.
    #[test]
    fn small_deltas_accumulate_instead_of_being_discarded() {
        let mut scroll = Accumulator::default();

        // Ten pixels against a 33-pixel row is 0.3 of a line — three of
        // these make a line, where rounding each in isolation made none.
        assert_eq!(scroll.take_lines(0.3), 0);
        assert_eq!(scroll.take_lines(0.3), 0);
        assert_eq!(scroll.take_lines(0.3), 0);
        assert_eq!(scroll.take_lines(0.3), 1);
    }

    /// However finely a swipe is chopped up, the lines that come out match
    /// the distance that went in.
    ///
    /// Stated as "within one line" rather than exactly, because floating
    /// point cannot promise that a hundred additions of a tenth sum to
    /// exactly ten. What matters is that there is no *systematic* loss: the
    /// error stays bounded by the fraction still being carried, however many
    /// events it took.
    #[test]
    fn finely_divided_scrolling_loses_nothing_significant() {
        for events_per_line in [2, 3, 5, 10, 100] {
            let mut scroll = Accumulator::default();
            let per_event = 1.0 / f64::from(events_per_line);
            let lines = 100;

            let total: i32 = (0..events_per_line * lines).map(|_| scroll.take_lines(per_event)).sum();

            assert!((total - lines).abs() <= 1, "{events_per_line} events per line over {lines} lines gave {total}");
        }
    }

    /// A wheel notch still scrolls immediately: this must not make discrete
    /// scrolling feel laggy.
    #[test]
    fn a_whole_line_scrolls_at_once() {
        let mut scroll = Accumulator::default();
        assert_eq!(scroll.take_lines(1.0), 1);
        assert_eq!(scroll.take_lines(-1.0), -1);
        assert_eq!(scroll.take_lines(3.0), 3);
    }

    #[test]
    fn scrolling_back_and_forth_nets_out() {
        let mut scroll = Accumulator::default();
        assert_eq!(scroll.take_lines(0.6), 0);
        assert_eq!(scroll.take_lines(-0.6), 0);
        // The two cancelled, so a fresh full line still emits exactly one.
        assert_eq!(scroll.take_lines(1.0), 1);
    }

    #[test]
    fn upward_fractions_accumulate_the_same_way() {
        let mut scroll = Accumulator::default();
        assert_eq!(scroll.take_lines(-0.4), 0);
        assert_eq!(scroll.take_lines(-0.4), 0);
        assert_eq!(scroll.take_lines(-0.4), -1);
    }

    /// The remainder must stay bounded, or a long scroll would slowly build
    /// a debt that discharges all at once.
    #[test]
    fn the_carried_fraction_stays_below_one_line() {
        let mut scroll = Accumulator::default();
        for _ in 0..1000 {
            scroll.take_lines(0.37);
            assert!(scroll.remainder.abs() < 1.0, "remainder grew to {}", scroll.remainder);
        }
    }

    /// A zero cell height upstream divides by zero. Left unguarded that
    /// poisons the remainder and nothing scrolls again for the rest of the
    /// session.
    #[test]
    fn a_non_finite_delta_is_ignored_rather_than_poisoning_the_remainder() {
        let mut scroll = Accumulator::default();
        assert_eq!(scroll.take_lines(f64::NAN), 0);
        assert_eq!(scroll.take_lines(f64::INFINITY), 0);
        assert_eq!(scroll.take_lines(1.0), 1, "the accumulator should still work afterwards");
    }

    #[test]
    fn resetting_drops_the_carried_fraction() {
        let mut scroll = Accumulator::default();
        scroll.take_lines(0.9);
        scroll.reset();
        assert_eq!(scroll.take_lines(0.5), 0, "the 0.9 should not have carried over");
    }
}
