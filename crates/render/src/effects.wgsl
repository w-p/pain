// Screen effects, drawn over the finished grid.
//
// One instanced quad per *area* rather than one over the whole window, so the
// pane title bars are excluded — they are chrome, not part of the illusion,
// and a scanlined title bar reads as a rendering bug. The effect coordinates
// are still computed in window space, so several panes share one continuous
// screen rather than each getting its own little vignette.
//
// No offscreen render target is involved: this is one extra draw at the end of
// the existing pass.
//
// The scanlines and vignette are static, so they cost nothing beyond that — a
// frame is only rendered when something actually changed, and the terminal
// keeps sleeping when idle. The hum bar is the exception: it moves, so the
// caller has to keep redrawing while it's enabled. See `Effects::is_animated`
// for how that cost is bounded.

struct Effects {
    screen_size: vec2<f32>,
    // 0.0-1.0. Zero means the app skips this draw entirely.
    scanline_strength: f32,
    // Physical pixels per light/dark scanline cycle.
    scanline_period: f32,
    vignette_strength: f32,
    // The window's own opacity. Effects scale with it so a translucent window
    // isn't dragged back toward opaque by its own scanlines.
    opacity: f32,
    // The drifting mains-hum bar, and where it currently sits (0-1, wrapping).
    hum_strength: f32,
    hum_phase: f32,
    // The theme's foreground, in sRGB — the phosphor color. See `glow` below.
    glow_color: vec3<f32>,
    _padding: f32,
};

@group(0) @binding(0) var<uniform> effects: Effects;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    // Window-space pixel position, so the effect is continuous across the
    // separate quads that skip the title bars.
    @location(0) pixel: vec2<f32>,
};

// Instance = one area to cover, in window pixels.
@vertex
fn vs_main(
    @location(0) corner: vec2<f32>,
    @location(1) area_pos: vec2<f32>,
    @location(2) area_size: vec2<f32>,
) -> VertexOutput {
    let pixel = area_pos + corner * area_size;
    var out: VertexOutput;
    out.clip_position = vec4<f32>(
        (pixel.x / effects.screen_size.x) * 2.0 - 1.0,
        1.0 - (pixel.y / effects.screen_size.y) * 2.0,
        0.0,
        1.0,
    );
    out.pixel = pixel;
    return out;
}

// sRGB to linear — see `shader.wgsl` for the full explanation. Needed here
// because the glow writes a real color rather than pure black.
fn srgb_to_linear(color: vec3<f32>) -> vec3<f32> {
    let cutoff = color <= vec3<f32>(0.04045);
    let low = color / 12.92;
    let high = pow((color + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(high, low, cutoff);
}

// How much phosphor glow the vignette lifts the screen by at full strength.
//
// Deliberately small. This exists so the vignette has something to darken:
// every CRT theme has a near-black background (Green Phosphor CRT's is
// #0b0f0b), and darkening that by even 25% moves it three levels out of 255 —
// invisible. A real powered tube is never pure black either; the phosphor
// retains a faint glow and the glass picks up room light. Lifting the centre
// slightly and letting the vignette fall away from it is both what makes the
// effect visible and what actually happened.
const GLOW_AT_FULL_STRENGTH: f32 = 0.10;

// How bright the hum bar's crest gets at full strength, and how tightly the
// band is concentrated.
//
// A real hum bar is the mains ripple (50/60Hz) beating against the vertical
// refresh: the two frequencies differ by a fraction of a hertz, so a soft wide
// band of slightly different brightness creeps slowly up the screen. It is a
// subtle artifact of imperfect power supply filtering, not a spotlight —
// overdo either of these and it stops looking like a tired monitor and starts
// looking like an effect.
const HUM_GLOW_AT_FULL_STRENGTH: f32 = 0.09;
const HUM_BAND_TIGHTNESS: f32 = 3.0;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.pixel / effects.screen_size;
    var shade = 0.0;
    var glow = vec3<f32>(0.0);

    if effects.scanline_strength > 0.0 {
        // A cosine rather than a hard on/off: a real raster line has soft
        // edges, and a 1-pixel square wave on a high-DPI display produces
        // moire against the pixel grid instead of looking like a CRT.
        //
        // Squared to concentrate the darkening into narrower, deeper bands.
        // The plain cosine spent half its amplitude dimming everything
        // uniformly, which read as "slightly darker" rather than as lines.
        let cycle = 0.5 - 0.5 * cos(6.28318530718 * in.pixel.y / effects.scanline_period);
        shade += effects.scanline_strength * cycle * cycle;
    }

    if effects.vignette_strength > 0.0 {
        // Distance from centre, normalised so the corners reach ~1.
        let offset = (uv - vec2<f32>(0.5)) * 2.0;
        let distance = clamp(length(offset) / 1.41421356, 0.0, 1.0);

        // Darkening ramps up only toward the edges — the point is curved
        // glass, not a filter over the text someone is reading.
        shade += effects.vignette_strength * smoothstep(0.35, 1.0, distance);

        // ...and the matching ambient lift, strongest at the centre. This is
        // what the darkening above actually acts on. See GLOW_AT_FULL_STRENGTH.
        let lift = effects.vignette_strength * GLOW_AT_FULL_STRENGTH * (1.0 - smoothstep(0.0, 1.0, distance));
        // Accumulates rather than assigns: the hum bar below adds to the same
        // term, and an assignment here would silently depend on block order.
        glow += srgb_to_linear(effects.glow_color) * lift;
    }

    if effects.hum_strength > 0.0 {
        // One cycle spans the screen height and wraps, so the band leaves the
        // top as it enters the bottom with no seam.
        let y = fract(uv.y + effects.hum_phase);
        let band = pow(0.5 + 0.5 * cos(6.28318530718 * y), HUM_BAND_TIGHTNESS);
        // Brightens rather than darkens, for the same reason the vignette
        // has to lift the centre: on a near-black CRT palette there is
        // nothing for a darkening band to act on.
        glow += srgb_to_linear(effects.glow_color) * effects.hum_strength * HUM_GLOW_AT_FULL_STRENGTH * band;
    }

    // Premultiplied blending computes `src + dst * (1 - src.a)`, so one draw
    // does both jobs: the RGB term adds light (the glow), and the alpha term
    // removes it (the darkening). Emitting black with an alpha, as this used
    // to, could only ever subtract.
    let alpha = clamp(shade, 0.0, 1.0) * effects.opacity;
    return vec4<f32>(glow * effects.opacity, alpha);
}
