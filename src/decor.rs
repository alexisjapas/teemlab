//! The **arena decor**: a purely-visual pixel-art backdrop — a sunny sand square
//! with a shallow, wobbly-edged water basin covering the playable area — baked
//! deterministically into two textures. Spec and decisions: `docs/arena-decor.md`.
//!
//! Presentation only (DEV Rules 1 & 3): the systems run in `Update` (registered
//! by [`crate::visuals::VisualsPlugin`]), draw from their **own** LCG seeded by a
//! seed *derived* from [`SimConfig::seed`], and nothing here is ever read by the
//! simulation. The pure half ([`bake`] and below) has no Bevy render state and is
//! unit-tested directly.

use crate::config::SimConfig;
use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// World units per texel — the pixel-art grain (the spec's `DC`).
pub const TEXEL_WU: f32 = 2.0;
/// Decor square side as a multiple of the arena half-side — how far the textured
/// sand runs past the walls before fading to the flat [`horizon_sand`] tone,
/// which the windowed `ClearColor` then continues to infinity: scroll anywhere,
/// there is sand. (The camera pan is clamped to the arena — `main.rs` — so this
/// margin is what a legitimate view can actually reach.)
const SIDE_RATIO: f32 = 4.5;
/// Side of the spec's prototype (wu); [`REF_POOL_HALF`] is its pool half-side
/// (`0.34·S`). The spec's macro constants (wobble, bands, dune wavelengths) are
/// absolute values *for that prototype*; we scale them by `h / REF_POOL_HALF` so
/// the composition around the **basin** is scale-invariant (spec §1) whatever
/// the arena size — and independent of how much sand margin [`SIDE_RATIO`] adds.
const REF_SIDE: f32 = 460.0;
/// Pool half-side of the spec's prototype: `0.34 · REF_SIDE`.
const REF_POOL_HALF: f32 = 0.34 * REF_SIDE;
/// Ramp position of the flat horizon tone ([`horizon_sand`], the border fade).
const HORIZON_U: f32 = 0.68;
/// Where the border fade begins, as a fraction of the decor half-side.
const FADE_START: f32 = 0.72;
/// Maximum bank-wobble amplitude (`4+3+2`, at [`REF_SIDE`]) — also the
/// **outward** offset of the basin midline from the walls, so the water provably
/// covers the whole playable square (`|wob| ≤ midline − h` ⇒ `sdf ≥ 0` inside,
/// cf. the tests).
const BANK_AMPLITUDE: f32 = 9.0;

/// Sand ramp, bright (near the light) → dark, with the spec's stop positions.
const SAND_RAMP: [(f32, [f32; 3]); 4] = [
    (0.00, [239.0, 230.0, 207.0]),
    (0.42, [222.0, 210.0, 180.0]),
    (0.78, [202.0, 189.0, 151.0]),
    (1.00, [182.0, 169.0, 128.0]),
];
/// Depth-tint water (multiplied into the basin floor).
const WATER: [f32; 3] = [66.0, 178.0, 192.0];
/// Upper-film tint: a **deep** water tone — the film alpha-blends it over the
/// entities/floor, approximating the spec's *multiply* (it darkens the center,
/// never washes it out).
const FILM: [f32; 3] = [40.0, 110.0, 122.0];
const PEBBLE_DARK: [f32; 3] = [107.0, 96.0, 78.0];
const PEBBLE_LIGHT: [f32; 3] = [214.0, 204.0, 182.0];
/// Shaded crest of the basin wall (`sdf ∈ (−5, 0]`).
const CREST: [f32; 3] = [28.0, 18.0, 8.0];
/// Sunlit lip just outside the crest (`sdf ∈ (−13, −5]`).
const LIP: [f32; 3] = [255.0, 247.0, 218.0];
/// Light ripple glint strewn across the water (the reference's sparkles).
const SPARKLE: [f32; 3] = [214.0, 246.0, 248.0];
/// Number of water-depth bands between the shore and the deep center.
const BANDS: f32 = 6.0;
/// Per-texel jitter at the band seams, in band widths (≈ how far two adjacent
/// bands grain into each other — big enough to read as dithering, small enough
/// that the bands stay distinguishable).
const DITHER: f32 = 0.6;
/// Where the deepest band settles, as a fraction of the basin midline: the
/// bands spread across most of the pool (reference image), the center stays a
/// uniform speckled deep.
const PLATEAU: f32 = 0.85;
/// Shore-compression of the band spread (1 = evenly spaced bands).
const DEPTH_EASE: f32 = 1.4;
/// Texel clusters a sparkle can take (diagonal glints, 2–6 texels).
const SPARKLE_SHAPES: [&[(isize, isize)]; 4] = [
    &[(0, 0), (1, -1), (1, 0)],
    &[(0, 0), (1, 0), (2, -1), (3, -1)],
    &[(0, 0), (-1, 1), (0, 1)],
    &[(0, 0), (1, -1), (2, -2), (2, -1), (3, -2), (0, -1)],
];

/// The spec's LCG (Numerical Recipes), reimplemented verbatim so the *integer*
/// stream is bit-exact on every platform. Deliberately **not**
/// [`crate::rng::Rng`]: the decor must never share (nor appear to share) a sim
/// stream.
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    /// Next sample in `[0, 1)` (named as `rng::Rng::next_f32`, the house idiom).
    pub fn next_f32(&mut self) -> f32 {
        self.state = self
            .state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        (f64::from(self.state) / 4_294_967_296.0) as f32
    }
}

/// The decor's seed, **derived** from the sim seed (the `^ 0xF00D` idiom of
/// `ecology::SimRng`): folded to the LCG's 32 bits and offset so it can't alias a
/// sim stream. Deriving it draws nothing from any sim RNG.
pub fn visual_seed(seed: u64) -> u32 {
    ((seed >> 32) as u32) ^ (seed as u32) ^ 0x8DA3_D251
}

/// The two baked layers, RGBA8 row-major from the texture's top-left texel.
pub struct DecorBake {
    /// Texels per side (the textures are square).
    pub size: u32,
    /// World-unit side of the decor quad: `size · TEXEL_WU`.
    pub side_wu: f32,
    /// Base layer (under the entities): sand, banks, water depth. Opaque.
    pub base: Vec<u8>,
    /// Upper film (above the entities): radial tint + light spots. Straight alpha.
    pub film: Vec<u8>,
}

/// Bake the decor for `(seed, arena_half_extent)` — same inputs, same output
/// (per platform: the LCG is bit-exact, `sin`/`powf` are platform-deterministic).
/// Draw order is part of the format (`docs/arena-decor.md` §2): structure stream
/// = 12 edge phases then 2 light spots; texture stream = one draw per texel
/// (row-major) then 22 pebbles.
pub fn bake(seed: u64, arena_half_extent: f32) -> DecorBake {
    let h = arena_half_extent.max(TEXEL_WU);
    let n = ((SIDE_RATIO * h / TEXEL_WU).round() as usize).max(4);
    let side = n as f32 * TEXEL_WU;
    let scale = h / REF_POOL_HALF; // macro features scale with the basin (spec §1)
    let m = h + BANK_AMPLITUDE * scale; // basin midline half-side

    // -- structure stream ---------------------------------------------------
    let mut rng_struct = Lcg::new(visual_seed(seed));
    let mut phases = [0.0_f32; 12];
    for ph in &mut phases {
        *ph = rng_struct.next_f32() * std::f32::consts::TAU;
    }
    // Film light spots, kept in the central half of the square (world coords).
    let mut spots = [[0.0_f32; 2]; 2];
    for spot in &mut spots {
        spot[0] = (rng_struct.next_f32() - 0.5) * 0.5 * side;
        spot[1] = (rng_struct.next_f32() - 0.5) * 0.5 * side;
    }

    // -- P1 sand (texture stream: one draw per texel, row-major) -------------
    // Texture-space coordinates (wu, origin top-left, y down), so the spec's
    // constants apply verbatim; the light sits up-left of center.
    let mut rng_tex = Lcg::new(visual_seed(seed) ^ 0x9E37_79B9);
    let light = [0.40 * side, 0.28 * side];
    let mut base = vec![[0.0_f32; 3]; n * n];
    for j in 0..n {
        let ty = (j as f32 + 0.5) * TEXEL_WU;
        for i in 0..n {
            let tx = (i as f32 + 0.5) * TEXEL_WU;
            let d = ((tx - light[0]).powi(2) + (ty - light[1]).powi(2)).sqrt() / (1.02 * side);
            let dune = 0.5 * (0.021 * tx / scale + 1.3).sin()
                + 0.5 * (0.026 * ty / scale + 4.1).sin()
                + 0.4 * (0.013 * (tx + ty) / scale).sin();
            let q = rng_tex.next_f32();
            let u = 0.9 * d + 0.06 * dune + ((q * 5.0).floor() - 2.0) * 0.026;
            let mut col = tone(u);
            // Rare bright / dark grains.
            let grain = if q > 0.978 {
                32.0
            } else if q < 0.022 {
                -28.0
            } else {
                0.0
            };
            for c in &mut col {
                *c = (*c + grain).clamp(0.0, 255.0);
            }
            base[j * n + i] = col;
        }
    }

    // -- P2 pebbles (texture stream) -----------------------------------------
    for _ in 0..22 {
        let px = ((rng_tex.next_f32() * n as f32) as usize).min(n - 1);
        let py = ((rng_tex.next_f32() * n as f32) as usize).min(n - 1);
        let w = 1 + (rng_tex.next_f32() * 2.0) as usize; // 1–2 texels
        for dy in 0..w {
            for dx in 0..w {
                let (x, y) = (px + dx, py + dy);
                if x < n && y < n {
                    base[y * n + x] = PEBBLE_DARK;
                }
            }
        }
        // Sun glint: one light texel diagonally up-left of the block.
        if px > 0 && py > 0 {
            base[(py - 1) * n + (px - 1)] = PEBBLE_LIGHT;
        }
    }

    // -- horizon fade ---------------------------------------------------------
    // The outer sand (grain, dunes, pebbles, light gradient) converges smoothly
    // to the flat `horizon_sand` tone at the border; the windowed `ClearColor`
    // (same tone) then continues it past the quad — no visible seam.
    let horizon = tone(HORIZON_U);
    let half = side / 2.0;
    for j in 0..n {
        let wy = ((n as f32 / 2.0 - (j as f32 + 0.5)) * TEXEL_WU).abs();
        for i in 0..n {
            let wx = ((i as f32 + 0.5 - n as f32 / 2.0) * TEXEL_WU).abs();
            let a = wx.max(wy) / half;
            if a <= FADE_START {
                continue;
            }
            let t = ((a - FADE_START) / (1.0 - FADE_START)).clamp(0.0, 1.0);
            blend_over(&mut base[j * n + i], horizon, t * t * (3.0 - 2.0 * t));
        }
    }

    // -- P3 basin edge: signed distance at every texel center (world coords) --
    let mut sdf = vec![0.0_f32; n * n];
    for j in 0..n {
        let wy = (n as f32 / 2.0 - (j as f32 + 0.5)) * TEXEL_WU;
        for i in 0..n {
            let wx = (i as f32 + 0.5 - n as f32 / 2.0) * TEXEL_WU;
            sdf[j * n + i] = basin_sdf(wx, wy, m, scale, &phases);
        }
    }
    for idx in 0..n * n {
        let s = sdf[idx];
        if s <= 0.0 && s > -5.0 * scale {
            blend_over(&mut base[idx], CREST, 0.42);
        } else if s <= -5.0 * scale && s > -13.0 * scale {
            blend_over(&mut base[idx], LIP, 0.18);
        }
    }

    // -- P4 depth bands, granular ----------------------------------------------
    // Several depth bands **dissolving into each other with per-texel noise**
    // (user feedback + reference image: dithered pixel-art seams — neither crisp
    // contours nor smooth gradients). The eased depth is jittered by about a
    // band's width before quantizing, so each seam is a wide grainy mix; a tonal
    // speckle then varies every band from within. Water from the very first
    // texel (wall-to-wall), the deep center uniform but speckled. No floor blur
    // (removed on feedback): the sand grain stays crisp under the tint.
    let plateau = PLATEAU * m;
    let salt = visual_seed(seed);
    for j in 0..n {
        for i in 0..n {
            let idx = j * n + i;
            let s = sdf[idx];
            if s <= 0.0 {
                continue;
            }
            let t = (s / plateau).min(1.0);
            let ease = 1.0 - (1.0 - t).powf(DEPTH_EASE);
            // Coarse-lattice noise (3-texel clumps, salted with a little per-texel
            // grain): the dither reads as chunky pixel-art mottling even zoomed
            // out, instead of averaging into a smooth gradient.
            let clump = texel_hash(i as u32 / 3, j as u32 / 3, salt) - 0.5;
            let fine = texel_hash(i as u32, j as u32, salt) - 0.5;
            let x = ease * BANDS + (0.7 * clump + 0.3 * fine) * DITHER;
            let band = x.floor().clamp(0.0, BANDS - 1.0);
            let speckle = texel_hash(i as u32 / 2, j as u32 / 2, salt ^ 0x5F35_6495) - 0.5;
            let a = (0.80 * (0.28 + 0.72 * band / (BANDS - 1.0)) + speckle * 0.10).clamp(0.0, 1.0);
            for k in 0..3 {
                base[idx][k] *= 1.0 - a + a * WATER[k] / 255.0;
            }
        }
    }

    // -- sparkles: tiny light ripple clusters strewn across the water -----------
    // (texture stream, after the pebbles — the draws are consumed whether or not
    // the spot lands in water, so the stream stays aligned.)
    let sparkle_count = (n * n / 3000).max(8);
    for _ in 0..sparkle_count {
        let sx = ((rng_tex.next_f32() * n as f32) as usize).min(n - 1) as isize;
        let sy = ((rng_tex.next_f32() * n as f32) as usize).min(n - 1) as isize;
        let shape = ((rng_tex.next_f32() * SPARKLE_SHAPES.len() as f32) as usize)
            .min(SPARKLE_SHAPES.len() - 1);
        for &(dx, dy) in SPARKLE_SHAPES[shape] {
            let (x, y) = (sx + dx, sy + dy);
            if x < 0 || y < 0 || x >= n as isize || y >= n as isize {
                continue;
            }
            let idx = y as usize * n + x as usize;
            if sdf[idx] > 0.0 {
                blend_over(&mut base[idx], SPARKLE, 0.7);
            }
        }
    }

    // -- P8 pixel waterline ----------------------------------------------------
    // Interior texels touching the bank: light liner toward up/left, dark toward
    // down/right — a crisp pixel rim signalling the ledge.
    for j in 0..n {
        for i in 0..n {
            let idx = j * n + i;
            if sdf[idx] <= 0.0 {
                continue;
            }
            let exterior = |x: isize, y: isize| {
                x < 0
                    || y < 0
                    || x >= n as isize
                    || y >= n as isize
                    || sdf[y as usize * n + x as usize] <= 0.0
            };
            let (x, y) = (i as isize, j as isize);
            if exterior(x, y - 1) {
                blend_over(&mut base[idx], [255.0; 3], 0.22);
            }
            if exterior(x - 1, y) {
                blend_over(&mut base[idx], [255.0; 3], 0.16);
            }
            if exterior(x, y + 1) {
                blend_over(&mut base[idx], [0.0; 3], 0.34);
            }
            if exterior(x + 1, y) {
                blend_over(&mut base[idx], [0.0; 3], 0.28);
            }
        }
    }

    // -- upper film: radial water tint + surface film + light spots -------------
    // Composed premultiplied over transparent, stored straight-alpha; transparent
    // outside the pool, so the sand never gets filmed over.
    let mut film = vec![0_u8; n * n * 4];
    let spot_radius = 0.16 * side;
    for j in 0..n {
        let wy = (n as f32 / 2.0 - (j as f32 + 0.5)) * TEXEL_WU;
        for i in 0..n {
            let idx = j * n + i;
            if sdf[idx] <= 0.0 {
                continue;
            }
            let wx = (i as f32 + 0.5 - n as f32 / 2.0) * TEXEL_WU;
            let r = ((wx * wx + wy * wy).sqrt() / m).clamp(0.0, 1.0);
            // A *darkening* radial tint (the spec's multiply, approximated with
            // a deep film color): strongest over the center so the depth reads
            // through the film instead of being washed out by it.
            let tint = 0.22 + (0.05 - 0.22) * r;
            let mut color = [FILM[0] * tint, FILM[1] * tint, FILM[2] * tint];
            let mut alpha = tint;
            for spot in &spots {
                let d = ((wx - spot[0]).powi(2) + (wy - spot[1]).powi(2)).sqrt();
                if d < spot_radius {
                    let falloff = 1.0 - d / spot_radius;
                    let a = 0.05 * falloff * falloff;
                    for c in &mut color {
                        *c = 255.0 * a + *c * (1.0 - a);
                    }
                    alpha = a + alpha * (1.0 - a);
                }
            }
            let o = idx * 4;
            if alpha > 0.0 {
                for k in 0..3 {
                    film[o + k] = (color[k] / alpha).round().clamp(0.0, 255.0) as u8;
                }
                film[o + 3] = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    let mut base_px = vec![0_u8; n * n * 4];
    for (idx, col) in base.iter().enumerate() {
        let o = idx * 4;
        for k in 0..3 {
            base_px[o + k] = col[k].round().clamp(0.0, 255.0) as u8;
        }
        base_px[o + 3] = 255;
    }

    DecorBake {
        size: n as u32,
        side_wu: side,
        base: base_px,
        film,
    }
}

/// The flat sand tone the decor fades to at its border — also the windowed
/// `ClearColor` while the decor is on ([`crate::visuals`]), so the sand visually
/// runs to the horizon whatever the pan/zoom.
pub fn horizon_sand() -> Color {
    let [r, g, b] = tone(HORIZON_U);
    Color::srgb(r / 255.0, g / 255.0, b / 255.0)
}

/// Linear interpolation on the sand ramp at `u ∈ [0, 1]` (clamped).
fn tone(u: f32) -> [f32; 3] {
    let u = u.clamp(0.0, 1.0);
    let mut prev = SAND_RAMP[0];
    for stop in SAND_RAMP.iter().skip(1) {
        if u <= stop.0 {
            let t = (u - prev.0) / (stop.0 - prev.0);
            return [
                prev.1[0] + (stop.1[0] - prev.1[0]) * t,
                prev.1[1] + (stop.1[1] - prev.1[1]) * t,
                prev.1[2] + (stop.1[2] - prev.1[2]) * t,
            ];
        }
        prev = *stop;
    }
    SAND_RAMP[SAND_RAMP.len() - 1].1
}

/// Deterministic per-texel noise in `[0, 1)` — a stateless integer hash, **not**
/// a stream draw: the band dithering must not disturb the documented draw order
/// (and must not correlate texels through a sequential state).
fn texel_hash(i: u32, j: u32, salt: u32) -> f32 {
    let mut h = i
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add(j.wrapping_mul(0x85EB_CA6B))
        ^ salt;
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^= h >> 16;
    (f64::from(h) / 4_294_967_296.0) as f32
}

/// Classic "over" blend of `src` at opacity `a` onto `dst`, per channel.
fn blend_over(dst: &mut [f32; 3], src: [f32; 3], a: f32) {
    for k in 0..3 {
        dst[k] = dst[k] * (1.0 - a) + src[k] * a;
    }
}

/// The spec's three-octave edge wobble at abscissa `u`, phases `ph[o..o+3]`,
/// amplitude and wavelength scaled by `scale` (`|wob| ≤ BANK_AMPLITUDE·scale`).
fn wob(u: f32, o: usize, ph: &[f32; 12], scale: f32) -> f32 {
    let u = u / scale;
    scale
        * (4.0 * (0.052 * u + ph[o]).sin()
            + 3.0 * (0.11 * u + ph[o + 1]).sin()
            + 2.0 * (0.22 * u + ph[o + 2]).sin())
}

/// Signed distance (wu) to the basin edge at world `(x, y)`: `> 0` = water. The
/// caller places the midline `m` at least [`BANK_AMPLITUDE`]`·scale` **outside**
/// the walls, so the whole playable square `|x|, |y| ≤ h` is water (the bank
/// never bites into the arena).
fn basin_sdf(x: f32, y: f32, m: f32, scale: f32, ph: &[f32; 12]) -> f32 {
    let east = m + wob(y, 0, ph, scale) - x;
    let west = x + m + wob(y, 3, ph, scale);
    let north = m + wob(x, 6, ph, scale) - y;
    let south = y + m + wob(x, 9, ph, scale);
    east.min(west).min(north).min(south)
}

/// Width (wu) over which the water mask **feathers** to zero across the shoreline, so a
/// masked overlay (the nutrient heatmap) meets the sand with a soft edge, not a hard cut —
/// roughly the crest+lip band the baked decor draws there.
const SHORE_FEATHER: f32 = 8.0;

/// The **basin geometry** of a decor `(seed, arena_half_extent)`: enough to query the water
/// *shape* without re-baking a texture. A **presentation** helper — the nutrient heatmap
/// masks itself to the pond so its tint fills the water and never the sand — never read by
/// the simulation (DEV Rules 1 & 3). Reconstructs the same midline, scale and 12 edge phases
/// [`bake`] uses (the first 12 LCG draws) and the same [`basin_sdf`], so its water shape is
/// identical to the baked backdrop's, texel for texel.
pub struct Basin {
    m: f32,
    scale: f32,
    phases: [f32; 12],
}

impl Basin {
    /// Reconstruct the basin of `(seed, arena_half_extent)`.
    pub fn new(seed: u64, arena_half_extent: f32) -> Self {
        let h = arena_half_extent.max(TEXEL_WU);
        let scale = h / REF_POOL_HALF;
        let m = h + BANK_AMPLITUDE * scale;
        let mut rng = Lcg::new(visual_seed(seed));
        let mut phases = [0.0_f32; 12];
        for ph in &mut phases {
            *ph = rng.next_f32() * std::f32::consts::TAU;
        }
        Self { m, scale, phases }
    }

    /// Water **coverage** at world `(x, y)` in `[0, 1]`: `1` inside the basin, feathering to
    /// `0` across the shoreline ([`SHORE_FEATHER`] wu), `0` on the sand — the alpha a
    /// water-masked overlay multiplies in.
    pub fn coverage(&self, x: f32, y: f32) -> f32 {
        (basin_sdf(x, y, self.m, self.scale, &self.phases) / SHORE_FEATHER).clamp(0.0, 1.0)
    }

    /// Half-side of the water's **bounding box** — the farthest the wobbly shoreline can
    /// reach (`m + BANK_AMPLITUDE·scale`). A quad of this half-side covers the whole pond.
    pub fn water_half_extent(&self) -> f32 {
        self.m + BANK_AMPLITUDE * self.scale
    }
}

// ---------------------------------------------------------------------------
// Bevy glue — the thin `Update` systems registered by `VisualsPlugin`.
// ---------------------------------------------------------------------------

/// Z of the base layer: above the flat play-area quad (−10), below the component
/// heatmaps (−5) — they must stay readable over the sand.
const BASE_Z: f32 = -9.0;
/// Z of the water film: above the agents (0). Gizmos (headings, selection,
/// emitter rings) always render on top of sprites — deliberate, they stay crisp.
const FILM_Z: f32 = 5.0;

/// What the current textures were baked from — rebake only when this changes
/// (the `ComponentLayer` staleness idiom).
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct DecorKey {
    seed: u64,
    half_extent_bits: u32,
}

/// One of the two decor sprites (base under / film above the entities). Carries
/// no sim marker (`Agent`/`Wall`/`Emits`/`ComponentLayer`) on purpose: like
/// `PlayAreaBg` it survives the hot reset and is reconciled here.
#[derive(Component)]
pub struct DecorLayer {
    key: DecorKey,
    film: bool,
}

/// Frames a *changed* key must hold steady before a rebake — soaks up a slider
/// drag (arena size, seed) so we don't bake megapixels per moved frame. The
/// first bake (nothing on screen yet) is immediate.
const REBAKE_DEBOUNCE_FRAMES: u32 = 10;

/// Rendering only: keep the two baked decor layers in sync with the scenario.
/// Off → hidden (the flat `play_area_color` look underneath is intact). On →
/// bake once per `(seed, arena_half_extent)` and show; editing either in the
/// editor rebakes (debounced), and a hot reset touches nothing.
pub(crate) fn render_decor(
    mut commands: Commands,
    config: Res<SimConfig>,
    mut images: ResMut<Assets<Image>>,
    mut layers: Query<(&mut DecorLayer, &mut Sprite, &mut Visibility)>,
    mut pending: Local<Option<(DecorKey, u32)>>,
) {
    if !config.decor.enabled {
        for (_, _, mut vis) in &mut layers {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
        }
        return;
    }
    let key = DecorKey {
        seed: config.seed,
        half_extent_bits: config.arena_half_extent.to_bits(),
    };
    let baked_layers = layers.iter().count();
    if baked_layers == 2 && layers.iter().all(|(l, ..)| l.key == key) {
        for (_, _, mut vis) in &mut layers {
            if *vis != Visibility::Visible {
                *vis = Visibility::Visible;
            }
        }
        *pending = None;
        return;
    }
    // Something is on screen but stale: wait for the key to settle (slider drag).
    if baked_layers == 2 {
        match &mut *pending {
            Some((k, frames)) if *k == key => {
                *frames += 1;
                if *frames < REBAKE_DEBOUNCE_FRAMES {
                    return;
                }
            }
            _ => {
                *pending = Some((key, 0));
                return;
            }
        }
    }
    *pending = None;

    let baked = bake(config.seed, config.arena_half_extent);
    let size = Vec2::splat(baked.side_wu);
    // Index 0 = base, 1 = film; `take`n by the existing sprite of that slot,
    // whatever is left spawns fresh.
    let mut handles = [
        Some(images.add(decor_image(baked.size, baked.base))),
        Some(images.add(decor_image(baked.size, baked.film))),
    ];
    for (mut layer, mut sprite, mut vis) in &mut layers {
        if let Some(handle) = handles[layer.film as usize].take() {
            layer.key = key;
            sprite.image = handle;
            sprite.custom_size = Some(size);
            *vis = Visibility::Visible;
        }
    }
    for (slot, handle) in handles.into_iter().enumerate() {
        let Some(handle) = handle else { continue };
        let film = slot == 1;
        commands.spawn((
            DecorLayer { key, film },
            Sprite {
                image: handle,
                custom_size: Some(size),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, if film { FILM_Z } else { BASE_Z }),
        ));
    }
}

/// A nearest-sampled **white disc** texture, `radius_px` texels of radius — the
/// pixel-art body of an entity, tinted through `Sprite::color`. Shown at a world
/// size of `2·radius` wu its texels read at ≈ [`TEXEL_WU`], the decor's grain,
/// so entities and terrain pixelate alike.
pub fn disc_image(radius_px: u32) -> Image {
    let d = (2 * radius_px).max(1);
    let mut data = vec![0_u8; (d * d * 4) as usize];
    let r = radius_px as f32;
    for j in 0..d {
        for i in 0..d {
            let (x, y) = (i as f32 + 0.5 - r, j as f32 + 0.5 - r);
            if x * x + y * y <= r * r {
                let o = ((j * d + i) * 4) as usize;
                data[o..o + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
    }
    decor_image(d, data)
}

/// A nearest-sampled (crisp pixel-art) square RGBA texture from a baked buffer.
fn decor_image(size: u32, data: Vec<u8>) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The LCG follows the Numerical Recipes reference sequence, bit-exact —
    /// this is what makes the bake reproducible across platforms and what pins
    /// the implementation to the spec's prototype.
    #[test]
    fn lcg_matches_the_reference_sequence() {
        let mut lcg = Lcg::new(0);
        let mut states = Vec::new();
        for _ in 0..4 {
            lcg.next_f32();
            states.push(lcg.state);
        }
        assert_eq!(
            states,
            vec![1_013_904_223, 1_196_435_762, 3_519_870_697, 2_868_466_484]
        );
    }

    /// `next_f32()` stays in `[0, 1)` over a long run (the u32 → f32 mapping).
    #[test]
    fn lcg_samples_stay_in_unit_interval() {
        let mut lcg = Lcg::new(0xDEAD_BEEF);
        for _ in 0..10_000 {
            let q = lcg.next_f32();
            assert!((0.0..1.0).contains(&q), "sample out of range: {q}");
        }
    }

    /// Same `(seed, arena)` ⇒ byte-identical buffers; a different seed diverges.
    #[test]
    fn bake_is_deterministic_per_seed() {
        let a = bake(42, 400.0);
        let b = bake(42, 400.0);
        assert_eq!(a.base, b.base, "base layer reproducible");
        assert_eq!(a.film, b.film, "film layer reproducible");

        let c = bake(43, 400.0);
        assert_ne!(a.base, c.base, "the seed shapes the decor");
    }

    /// The texture resolution follows the arena size (`S = 4.5·h`, 2 wu/texel).
    #[test]
    fn bake_size_follows_the_arena() {
        let baked = bake(42, 250.0);
        assert_eq!(baked.size, 563); // round(4.5 · 250 / 2)
        assert_eq!(baked.side_wu, 1126.0);
        assert_eq!(baked.base.len(), 563 * 563 * 4);
        assert_eq!(baked.film.len(), 563 * 563 * 4);
    }

    /// The border texels sit exactly on the horizon tone (the fade completes),
    /// so the windowed `ClearColor` continues the sand without a seam.
    #[test]
    fn border_fades_to_the_horizon_tone() {
        let baked = bake(42, 400.0);
        let horizon = tone(HORIZON_U).map(|c| c.round() as u8);
        for &corner in &[0_usize, baked.size as usize - 1] {
            let o = corner * 4;
            assert_eq!(
                &baked.base[o..o + 3],
                &horizon[..],
                "corner texel off the horizon tone"
            );
        }
    }

    /// The bank never bites into the arena: the basin covers the whole playable
    /// square, whatever the phases (|wob| ≤ midline − h by construction).
    #[test]
    fn water_covers_the_playable_square() {
        let h = 400.0;
        let scale = h / REF_POOL_HALF;
        let m = h + BANK_AMPLITUDE * scale;
        let mut rng = Lcg::new(visual_seed(0x00C0_FFEE));
        let mut phases = [0.0_f32; 12];
        for ph in &mut phases {
            *ph = rng.next_f32() * std::f32::consts::TAU;
        }
        let mut xy = -h;
        while xy <= h {
            let mut other = -h;
            while other <= h {
                let s = basin_sdf(xy, other, m, scale, &phases);
                assert!(s >= 0.0, "bank inside the arena at ({xy}, {other}): {s}");
                other += 5.0;
            }
            xy += 5.0;
        }
    }

    /// Tuning aid, not part of the suite: dump the default bake (base +
    /// film-composited) as PPM images into `DECOR_DUMP_DIR` (default `.`) to
    /// eyeball a constant change without launching the app.
    /// `DECOR_DUMP_DIR=/tmp cargo test --lib decor::tests::dump_ppm -- --ignored`
    #[test]
    #[ignore]
    fn dump_ppm() {
        let baked = bake(0x00C0_FFEE, 400.0);
        let n = baked.size as usize;
        let dir = std::env::var("DECOR_DUMP_DIR").unwrap_or_else(|_| ".".into());
        for (name, blend_film) in [("base", false), ("composite", true)] {
            let mut ppm = format!("P6\n{n} {n}\n255\n").into_bytes();
            for idx in 0..n * n {
                let o = idx * 4;
                let mut px = [
                    baked.base[o] as f32,
                    baked.base[o + 1] as f32,
                    baked.base[o + 2] as f32,
                ];
                if blend_film {
                    let a = baked.film[o + 3] as f32 / 255.0;
                    for (k, c) in px.iter_mut().enumerate() {
                        *c = baked.film[o + k] as f32 * a + *c * (1.0 - a);
                    }
                }
                ppm.extend(px.map(|c| c as u8));
            }
            std::fs::write(format!("{dir}/{name}.ppm"), ppm).unwrap();
        }
    }

    /// The film is transparent on the sand (corners) and present mid-pool.
    #[test]
    fn film_is_clipped_to_the_pool() {
        let baked = bake(42, 400.0);
        let n = baked.size as usize;
        assert_eq!(baked.film[3], 0, "corner texel (sand) is transparent");
        let center = (n / 2) * n + n / 2;
        assert!(
            baked.film[center * 4 + 3] > 0,
            "pool center carries the film"
        );
    }
}
