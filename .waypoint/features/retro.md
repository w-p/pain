# Retro eras

**Shipped:** v1.11.0, 2026-07-30. Built on a `retro` branch over two days and
revised twice against the developer's hands-on testing — the revisions are the
interesting part and are recorded below rather than tidied away.

An opt-in period look — a specific machine, not a general retro mood.
`[retro] era` bundles a palette, screen effects and a typeface under one
name.

- `crates/config/src/era.rs` — the era table (data) plus `find`/`listed`.
- `crates/config/src/lib.rs` — the `[retro]` section and its resolution.
- `crates/pane/src/retro.rs` — the OSC 7331 scanner.
- `crates/render/src/effects.wgsl` — the scanline/vignette overlay pass.

## The design question this answers

The developer's first reaction to eras-as-palettes-only was that they "feel
slightly like just new themes". That was correct, and it's the constraint the
rest of the design hangs off: **an era has to change something a theme
cannot.** There is a test asserting exactly that — every era must have a
non-zero scanline strength, vignette, or font preference.

The three non-colour levers, in order of how much character they add per unit
of engineering:

1. **Effects** — scanlines and a vignette, which is what makes green text look
   like a phosphor tube rather than green text.
2. **Font** — arguably the biggest visual difference of all, though it is the
   one an era can only *ask* for rather than guarantee. See Fonts below.

A third lever, output pacing, was built and then removed; see below.

## Why it is built this way

**Eras are data, not code.** Adding one is a table row; the renderer never
grows a branch per era. Palettes are reused straight from the existing 600+
theme table rather than defining a second colour format — `Green Phosphor
CRT`, `Amber CRT Retro`, `IBM 5153 CGA` and `C64` were already there.

**The era overlays settings, never writes them.** `era_override` lives on
`Graphite`'s own state, deliberately outside `settings`, because the settings
panel saves from `settings` — folding a transient era in there would let
*trying one on* permanently rewrite the user's theme. Turning an era off
restores exactly what was chosen.

**"Unset follows the bundle."** `scanlines` and `vignette` are `Option<u32>`:
absent follows the era, present overrides it — including an explicit `0`, so
"this era but without the scanlines" is expressible. This is
the same relationship `background_color` already has with `theme`, reused
rather than reinvented, and it lives in one place (`Retro::resolve`) so a new
era-backed setting can't resolve differently from the others.

**Precedence has three levels, and it's a free function.** Explicit setting >
session era override > configured value. Extracted as
`graphics::effect_strength` specifically so it is testable: `Graphics` needs a
GPU and can't be built in a unit test, and the precedence is the part worth
pinning.

**The scanlines and vignette are static, and that is the point.** They add one
draw call to a frame that was going to be rendered anyway, so with the hum bar
off an idle terminal still renders nothing and still sleeps. The hum bar is
the deliberate exception; see the animation budget below for how its cost is
kept bounded. Heavier animation (phosphor persistence, flicker, rain) would
need an offscreen target as well and remains out of scope.

**No offscreen render target.** The overlay draws over the finished grid
*inside the same pass*, before egui's own pass — so the chrome stays crisp. A
scanlined settings panel would be unusable, and the chrome isn't part of the
illusion. Screen curvature, bloom and phosphor decay all need an offscreen
target and are deliberately out of scope.

**The vignette must add light as well as remove it.** The first version only
darkened, and the developer reported seeing no vignette at all. That was
correct and the cause is arithmetic: `Green Phosphor CRT`'s background is
`#0b0f0b`, so darkening it by 25% moves it three levels out of 255. Every CRT
palette is near-black, so a darkening-only vignette has nothing to act on.

It now also lifts the centre with a faint glow in the theme's foreground
colour — which is both what makes the darkening visible and what a real
powered tube does, since the phosphor is never perfectly dark and the glass
picks up room light. Premultiplied blending computes `src + dst * (1 - src.a)`,
so a single draw does both: the RGB term adds light, the alpha term removes
it.

**Effects cover pane content rects, not the window.** One instanced quad per
pane content area, with the effect coordinates still computed in *window*
space so several panes share one continuous screen rather than each getting
its own little vignette. This is what keeps the pane title bars clean — they
are chrome, and a scanlined title bar reads as a rendering bug (reported, and
fixed this way).

**Scanline darkening is squared.** A plain cosine spent half its amplitude
dimming everything uniformly, which read as "slightly darker" rather than as
lines. Squaring concentrates it into narrower, deeper bands — more contrast at
the same average brightness.

**Scanline period is a fixed physical size, DPI-scaled.** Tying it to the font
would make the lines coarser just because someone bumped their text size;
tying it to raw pixels would make them invisible on a HiDPI display. The
pattern is a cosine rather than a square wave, because a 1-pixel hard edge
produces moire against the pixel grid instead of looking like a CRT.

## The escape sequence

`printf '\e]7331;era=amber\a'` sets an era live, from a shell or a script.
Extending a terminal through a private OSC is how this has always been done
(DEC private modes, iTerm2's OSC 1337, kitty's protocol), and it makes the
feature genuinely pleasant to develop against — no restart, no config edit, no
menu.

This started out framed as an easter egg, with one era hidden from the picker
and reachable only by name. That was dropped on 2026-07-30: the developer's
call was that it's simply a fun feature and should be documented as one. The
`hidden` flag is gone rather than left set to `false` everywhere — a concept
nothing uses is a concept the next reader has to rule out.

**Why accepting this from arbitrary output is safe**, given the OSC 8 caution:
the payload is a **name, not a value**. It selects from curated eras, so
program output cannot specify colours, cannot make text unreadable, cannot
hide anything. And it never persists — session only, gone on restart. That is
a different risk class from OSC 8, where the failure mode was *executing*
something.

## The hum bar, and the animation budget

The soft band that drifted up a tired CRT: mains ripple at 50/60Hz beating
against the vertical refresh, the two differing by a fraction of a hertz so
the band creeps rather than flickers. Nine seconds to cross the screen.

**It is the only effect that animates**, which makes it the only one that
stops an idle terminal sleeping — the property the v1.5 idle-cost work exists
to protect. Three things bound that:

- **It stops entirely when the window loses focus.** A retro terminal sitting
  behind an editor has no business waking the GPU to move a bar nobody is
  looking at.
- **It redraws at 20fps, not the display's rate.** A bar taking nine seconds
  to cross is visually identical at 20fps and 60, and wakes the GPU a third as
  often.
- **It is off unless an era is active, and `hum = 0` disables it outright.**

Measured under software rendering (llvmpipe, which counts GPU work as CPU and
so overstates it): idle sits around 1.2% of a core, and the hum bar takes that
to roughly 2.7%. Double a small number, on the least favourable renderer.

**The phase is computed on the CPU and passed as 0.0–1.0**, not as elapsed
seconds. An `f32` carrying raw uptime loses resolution after a few hours, and
the failure would be invisible until someone left a terminal open long enough
— so the value never leaves one cycle. There are tests covering a month of
uptime and a zero period.

## Baud: built, then removed

An earlier version paced output at a serial-line speed, so a screen filled at
2400 baud the way a BBS did. It was genuinely fun and it worked exactly as
intended — and it was removed on 2026-07-30, because it makes full-screen
programs unusable.

The reason is structural, not a bug: `htop`, `vim`, `less` and anything else
on the alternate screen repaint the *whole* display continuously. Pacing that
doesn't produce a period feel, it produces a terminal that can never finish
drawing. The feature is fine for `cat` and `ls` and hostile to everything
interactive, and a terminal has to be usable first.

There *is* a version that could work — gate pacing on
`TermMode::ALT_SCREEN`, so it applies to ordinary output and switches itself
off the moment a full-screen program takes over. That was not pursued; it is
recorded here so the option isn't rediscovered from scratch. The
implementation (an exact integer bit-nanosecond limiter with eleven tests)
is in this branch's history if it's ever wanted back.

## Fonts

**Nothing is bundled.** VT323 was briefly compiled in and then removed on
2026-07-30 — the developer's call was to recommend fonts rather than ship
them. That avoids putting third-party licence surface (VileR's Px437 series is
CC BY-SA, a copyleft licence) and 150KB of binary into every install for
something a user can install once themselves.

Eras therefore *name* typefaces and use one if it is present: VT323 for the
phosphor eras, `Px437 IBM VGA 8x16` for the DOS ones, `C64 Pro Mono` for
`c64`. Without one an era still applies its palette and effects and keeps the
configured font.

Worth knowing: **the font makes more difference to a period look than the
palette does.** An era with the right typeface reads as the machine; the same
era without it reads as a colour scheme with scanlines. That is why the README
recommends them explicitly rather than leaving it to be discovered.

## Explicitly not built, and why

- **The VGA 9th-column rule.** Real VGA text mode was 9 pixels wide with an
  8-pixel font, and the hardware duplicated column 8 for characters `C0`–`DF`
  so box-drawing lines connected. This turns out to be a **non-feature here**:
  it was a workaround for that specific hardware mismatch, and a modern
  outline font already draws `─` across its full advance. Implementing it
  would emulate a bug that no longer exists.
- **CP437 byte decoding.** Modern shells emit UTF-8, and the box-drawing and
  block characters are all reachable as Unicode. Decoding incoming bytes as
  CP437 would mean *breaking* UTF-8 for the sake of DOS programs that aren't
  running here.
