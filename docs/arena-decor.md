# Arena decor — procedural pixel-art backdrop (adapted "8d" spec)

**Status:** binding reference for the visual backdrop (`src/decor.rs`). Purely
presentation: nothing here touches the sim (DEV Rules 1 & 3). Adapted from the
user-provided "Arène 8d" spec to teemlab's coordinates and architecture; the
decisions below were made once — do not re-derive them.

## 0. Render / simulation boundary (the contract)

The generation is **purely aesthetic** and must never influence the simulation.
Information flows strictly **simulation → render**, never back.

- **Entities belong to the sim.** Positions, radii and colors come from the ECS;
  the decor only *draws around* them. The original spec's demo entity placement
  (its P6) is deliberately **not ported** — only its drawing style is (outline +
  highlight, see §5).
- **Logical geometry belongs to the sim.** The walls sit at `±arena_half_extent`
  (half-space colliders, `spawn.rs`); the decorative bank is drawn strictly
  *outside* them (§3) so the water always covers 100 % of the playable square —
  agents never visually overlap the bank.
- **Decoupled seed.** `visual_seed = fold(SimConfig::seed) ^ 0x8DA3_D251`,
  consumed only by the decor's own LCG in render code (`Update`). No sim RNG
  stream moves; removing the whole layer changes nothing in a run at fixed seed.
- **Determinism:** same `(seed, arena_half_extent)` ⇒ same textures, per
  platform (`sin`/`powf` are platform-deterministic only — accepted).

## 1. Layers & z map

Three visual layers on top of the existing stack; everything else unchanged.

| z | what | where |
|---|------|-------|
| −10 | `PlayAreaBg` flat quad (kept: the decor-off look) | `visuals.rs` |
| **−9** | **decor base**: sand + basin, baked `Image`, nearest | `decor.rs` |
| −5 − 0.1·i | nutrient heatmaps (must stay readable → above the sand) | `visuals.rs` |
| −4 | `SourceBody` rock discs (pixel-art sprites) | `visuals.rs` |
| 0 | agent bodies (pixel-art disc sprites, cf. §5) | `visuals.rs` |
| **+5** | **water film**: baked `Image`, straight alpha | `decor.rs` |
| ∞ | gizmos (headings, selection, emitter rings) — always on top | Bevy |

When the decor is enabled, the arena outline gizmo and the rings of **solid**
sources are skipped (clutter over the water). Intangible emitters keep their
ring — it is their only trace. The off-game `ClearColor` becomes the flat
[`horizon_sand`] tone, continuing the decor's border fade to infinity.

## 2. PRNG and draw order

Spec LCG (Numerical Recipes), `u32` state: `state = state·1664525 + 1013904223
(mod 2³²)`; `next() = state / 2³² ∈ [0, 1)`. Two streams so texture grain and
structure stay independent, draws in **fixed order**:

- `rng_struct = Lcg(visual_seed)`: 12 basin-edge phases, then 2 film light-spot
  centers (x, y each).
- `rng_tex = Lcg(visual_seed ^ 0x9E37_79B9)`: 1 draw per texel row-major (the
  same sample drives ramp jitter and grain, as in the spec's prototype), then
  22 pebbles (x, y, size — 3 draws each), then the water sparkles (x, y, shape
  — 3 draws each, consumed whether or not the spot lands in water).
- The depth-band dithering and speckle use a **stateless per-texel hash**
  (`texel_hash`, salted with `visual_seed`), not stream draws — band noise must
  not disturb the documented order.

## 3. Geometry

- Arena: square centered on world `(0,0)`, half-side `h = arena_half_extent`.
- Decor square side `side_wu = 4.5·h` — the sand runs well past the walls, then
  fades (outer 28 % of the half-side, smoothstep) to the flat `horizon_sand`
  tone that `ClearColor` continues. Texel = `DC = 2` wu → `n = round(side_wu /
  2)` texels per side (h = 400 → 900², ≈ 3.2 MB RGBA per layer).
- The spec's macro constants (wobble, bands, dune wavelengths — absolute values
  for its S = 460 prototype) are scaled by `h / (0.34·460)` so the composition
  around the **basin** is scale-invariant (spec §1), independent of the sand
  margin.
- Texel `(i, j)` (top-left origin, y down) → world center:
  `x = (i + 0.5 − n/2)·2`, `y = (n/2 − (j + 0.5))·2` (the `paint_nutrient_image`
  vertical-flip idiom).
- **Basin edge** (in wu): midline `M = h + 9`;
  `wob(u, o) = 4·sin(0.052u + ph[o]) + 3·sin(0.11u + ph[o+1]) + 2·sin(0.22u + ph[o+2])`
  with edge offsets o = 0 east (u = y), 3 west, 6 north (u = x), 9 south;
  `sdf(x, y) = min(M + wob(y,0) − x, x + M + wob(y,3), M + wob(x,6) − y, y + M + wob(x,9))`.
  Water where `sdf > 0`. Since `|wob| ≤ 9 = M − h`, the water provably covers
  the playable square (unit-tested).

## 4. Bake passes (base texture, in order)

All color math on `u8` channels in sRGB space (canvas-style, matches the spec's
prototype); the texture is `Rgba8UnormSrgb`, sampler **nearest**.

1. **Sand.** Light at world `L = (−0.10, +0.22)·side_wu`. Brightness
   `v = clamp(base + range·(1 − 0.9·d) + dunes + jitter)` with
   `d = dist(p, L)/side_wu`, two dune sine fields, jitter quantized from
   `rng_tex` — indexed into the 4-stop ramp `#b6a980 → #cabd97 → #ded2b4 →
   #efe6cf` (dark → bright). Rare grains: second draw `> 0.985` → lighter,
   `< 0.01` → darker. (Exact blend constants are tuning values, not structure.)
2. **Pebbles.** 22×: dark 1–2-texel block + light top-left texel. Drawn
   anywhere; the ones under the pool get blurred + tinted → read as submerged.
3. **Bank bands** (`sdf ≤ 0`): crest shadow `rgba(28,18,8, 0.42)` for
   `sdf ∈ (−5, 0]`; sunlit lip `rgba(255,247,218, 0.18)` for `sdf ∈ (−13, −5]`.
4. **Interior** (`sdf > 0`): **granular depth bands** (user feedback +
   reference image — dithered pixel-art seams, neither crisp contours nor
   smooth gradients). Eased depth `c = 1 − (1 − min(sdf/(0.85·M), 1))^1.4` is
   jittered by coarse-lattice noise (3-texel clumps + fine grain, ±0.6 band)
   before quantizing into 6 bands; alpha `a = 0.80·(0.28 + 0.72·band/5)` ±
   a 2-texel tonal speckle (0.10), multiply toward water `rgb(66,178,192)`.
   Water from the very first texel; the deep center is uniform but speckled.
   Then **sparkles**: ~`n²/3000` small light-cyan glint clusters (2–6 texels)
   strewn over the water. Deliberately NOT ported from the spec (feedback):
   the untinted first step, the submerged wall shadow, and the floor blur —
   the sand grain stays crisp under the tint.
5. **Waterline** (last): interior texels with an exterior 4-neighbor get a 1-texel
   liner per side — white `0.22`/`0.16` toward up/left, black `0.34`/`0.28`
   toward down/right.

## 5. Water film (above entities) & entity style

- **Film texture** (straight alpha, transparent outside the pool): a
  **darkening** radial tint — deep color `rgb(40,110,122)` at
  `α = lerp(0.22, 0.05, r)` (`r` = dist from center / M, clamped) — the
  straight-alpha stand-in for the spec's *multiply* (a light film would wash
  the depth out, feedback); plus 2 soft white spots (`α = 0.05·(1 − d/r)²`,
  radius ≈ 0.16·side_wu) — makes entities read as immersed. A true multiply
  `Material2d` (BlendState `Dst·Src`) sampling the same texture stays the
  drop-in follow-up.
- **Entity style**: bodies are **pixel-art disc sprites** — one cached
  nearest-sampled white disc texture per radius bucket (`radius_px =
  round(r / 2 wu)`, `decor::disc_image`), tinted per entity via `Sprite::color`
  and shown at the exact world diameter `2r`, so entities pixelate at the same
  ≈ 2 wu grain as the terrain. No outline / highlight (tried, removed on
  feedback — the spec's P6 liseré/reflet are NOT ported). `shade_by_reserve`
  dims the sprite tint.

## 6. Bake & reconcile

One system, `Update`, in `VisualsPlugin`: two sprite entities keyed by
`DecorKey { seed, half_extent_bits }` (the `NutrientLayer` staleness pattern).
Key unchanged → no per-frame work; `enabled: false` → `Visibility::Hidden`
(flat look intact underneath). A **changed** key rebakes only after ~10 stable
frames (debounce: an arena-slider drag must not bake megapixels per frame);
the very first bake is immediate. No `Agent/Wall/Emits/NutrientLayer` marker →
the layers survive hot-reset like `PlayAreaBg`. The recorder mounts the same
plugin → videos capture the decor with zero changes.

## 7. Camera notes (v1 scope)

The continuous-zoom camera is untouched: nearest sampling keeps the pixel-art
crisp when zooming in; slight pan shimmer and extreme zoom-out moiré are
accepted. Follow-ups if they ever itch: texel-snapped camera translation,
integer zoom steps, mipmaps on the base texture.
