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
/// Decor square side as a multiple of the arena half-side. The spec's pool spans
/// `0.34·S` per half-side; with the pool ≈ the playable square (`±h`), `S ≈
/// h/0.34 ≈ 2.94·h` — the sand overflows past the walls into the off-game area.
const SIDE_RATIO: f32 = 2.94;
/// Side of the spec's prototype (wu). Its macro constants (wobble, bands, dune
/// wavelengths) are absolute values *for this size*; we scale them by
/// `side/REF_SIDE` so the composition is **scale-invariant** (spec §1) — a big
/// arena gets the same look, only finer-grained (the texel stays 2 wu).
const REF_SIDE: f32 = 460.0;
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
/// Upper-film tint: the water lightened — reads as surface, not depth.
const FILM: [f32; 3] = [94.0, 193.0, 205.0];
const PEBBLE_DARK: [f32; 3] = [107.0, 96.0, 78.0];
const PEBBLE_LIGHT: [f32; 3] = [214.0, 204.0, 182.0];
/// Shaded crest of the basin wall (`sdf ∈ (−5, 0]`).
const CREST: [f32; 3] = [28.0, 18.0, 8.0];
/// Sunlit lip just outside the crest (`sdf ∈ (−13, −5]`).
const LIP: [f32; 3] = [255.0, 247.0, 218.0];
/// Submerged wall shadow, strongest up-left (overhang on the lit side).
const WALL_SHADOW: [f32; 3] = [6.0, 12.0, 12.0];

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
    let scale = side / REF_SIDE; // macro features in fractions of the side (spec §1)
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

    // -- P4.1 basin floor: 1-texel separable blur ("sand seen through water") --
    // Horizontal pass over everything (cheap), vertical pass written back only
    // where there is water — the banks stay crisp.
    let copy = base.clone();
    let mut blurred = copy.clone();
    for j in 0..n {
        for i in 0..n {
            let l = copy[j * n + i.saturating_sub(1)];
            let c = copy[j * n + i];
            let r = copy[j * n + (i + 1).min(n - 1)];
            for k in 0..3 {
                blurred[j * n + i][k] = 0.25 * l[k] + 0.5 * c[k] + 0.25 * r[k];
            }
        }
    }
    for j in 0..n {
        for i in 0..n {
            let idx = j * n + i;
            if sdf[idx] <= 0.0 {
                continue;
            }
            let u = blurred[j.saturating_sub(1) * n + i];
            let c = blurred[idx];
            let d = blurred[(j + 1).min(n - 1) * n + i];
            for k in 0..3 {
                base[idx][k] = 0.25 * u[k] + 0.5 * c[k] + 0.25 * d[k];
            }
        }
    }

    // -- P4.2 depth steps + P4.3 submerged wall shadow ------------------------
    // Two water ledges hugging the shore, then a wide uniform deep plateau.
    let plateau = 0.40 * m;
    for j in 0..n {
        let ty = (j as f32 + 0.5) * TEXEL_WU;
        for i in 0..n {
            let idx = j * n + i;
            let s = sdf[idx];
            if s <= 0.0 {
                continue;
            }
            let t = (s / plateau).min(1.0);
            let ease = 1.0 - (1.0 - t).powf(1.8);
            let a = (ease * 2.0).floor() / 2.0 * 0.748;
            for k in 0..3 {
                base[idx][k] *= 1.0 - a + a * WATER[k] / 255.0;
            }
            let reach = 22.0 * scale;
            if s < reach {
                let tx = (i as f32 + 0.5) * TEXEL_WU;
                let t = (reach - s) / reach;
                let dir = (if ty < side / 2.0 { 0.5 } else { 0.0 })
                    + (if tx < side / 2.0 { 0.2 } else { 0.0 });
                blend_over(
                    &mut base[idx],
                    WALL_SHADOW,
                    (t * t * (0.4 + 0.5 * dir)).min(1.0),
                );
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
            let tint = (0.42 + (0.08 - 0.42) * r) * 0.8;
            let mut color = [WATER[0] * tint, WATER[1] * tint, WATER[2] * tint];
            let mut alpha = tint;
            let sheen = 0.14 + (0.05 - 0.14) * r;
            for k in 0..3 {
                color[k] = FILM[k] * sheen + color[k] * (1.0 - sheen);
            }
            alpha = sheen + alpha * (1.0 - sheen);
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
/// (the `NutrientLayer` staleness idiom).
#[derive(Clone, Copy, PartialEq)]
struct DecorKey {
    seed: u64,
    half_extent_bits: u32,
}

/// One of the two decor sprites (base under / film above the entities). Carries
/// no sim marker (`Agent`/`Wall`/`Emits`/`NutrientLayer`) on purpose: like
/// `PlayAreaBg` it survives the hot reset and is reconciled here.
#[derive(Component)]
pub struct DecorLayer {
    key: DecorKey,
    film: bool,
}

/// Marker of the dark outline disc behind an **agent** body (a child entity).
/// The rock counterpart lives in `visuals::render_source_bodies` (siblings, not
/// children — a `SourceBody` child would inherit its radius z-scale).
#[derive(Component)]
pub struct DecorOutline;

/// Marker of the white up-left highlight disc on an **agent** (a child entity).
#[derive(Component)]
pub struct DecorHighlight;

/// Rendering only: keep the two baked decor layers in sync with the scenario.
/// Off → hidden (the flat `play_area_color` look underneath is intact). On →
/// bake once per `(seed, arena_half_extent)` and show; editing either in the
/// editor rebakes live, and a hot reset touches nothing.
pub fn render_decor(
    mut commands: Commands,
    config: Res<SimConfig>,
    mut images: ResMut<Assets<Image>>,
    mut layers: Query<(&mut DecorLayer, &mut Sprite, &mut Visibility)>,
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
    if layers.iter().count() == 2 && layers.iter().all(|(l, ..)| l.key == key) {
        for (_, _, mut vis) in &mut layers {
            if *vis != Visibility::Visible {
                *vis = Visibility::Visible;
            }
        }
        return;
    }

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

/// Rendering only: live-toggle the outline/highlight **children of agents** with
/// the decor. `Inherited` — not `Visible` — so the agents-layer toggle
/// (`visuals::apply_agent_layer`) hiding the parent hides them too. The rock
/// parts are siblings reconciled by `visuals::render_source_bodies` instead.
pub fn style_entity_decor(
    config: Res<SimConfig>,
    mut parts: Query<&mut Visibility, Or<(With<DecorOutline>, With<DecorHighlight>)>>,
) {
    let target = if config.decor.enabled {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut vis in &mut parts {
        if *vis != target {
            *vis = target;
        }
    }
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

    /// The texture resolution follows the arena size (`S = 2.94·h`, 2 wu/texel).
    #[test]
    fn bake_size_follows_the_arena() {
        let baked = bake(42, 250.0);
        assert_eq!(baked.size, 368); // round(2.94 · 250 / 2)
        assert_eq!(baked.side_wu, 736.0);
        assert_eq!(baked.base.len(), 368 * 368 * 4);
        assert_eq!(baked.film.len(), 368 * 368 * 4);
    }

    /// The bank never bites into the arena: the basin covers the whole playable
    /// square, whatever the phases (|wob| ≤ midline − h by construction).
    #[test]
    fn water_covers_the_playable_square() {
        let h = 400.0;
        let scale = SIDE_RATIO * h / REF_SIDE;
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
                    for k in 0..3 {
                        px[k] = baked.film[o + k] as f32 * a + px[k] * (1.0 - a);
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
