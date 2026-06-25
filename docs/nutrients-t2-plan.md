# T2 — `nutrients` engine layer (plant food only): detailed implementation plan

**Status:** designed, not yet implemented. This is the binding reference to resume the
work (cf. [`ROADMAP.md`](../ROADMAP.md) §9 "Generic nutrients layer"). It records every
decision taken in the design discussion so we do **not** diverge.

---

## 0. Why (recap)

Plants are only **sun-limited** today (photosynthesis = infinite) → they carpet. The
principled bound is **resource limitation** (Liebig's law): tie reproduction to a
**finite nutrient**. The T1 scenario prototype (`scenarios/minerals.ron`) **validated the
bound** (plants self-limit, ~7 not a carpet) but was **fragile** (2/3 seeds collapse),
because there *lacking the nutrient = death* (energy starvation) → spiral.

**The fix is the whole design of T2: two axes.**

- **Energy** (existing `Reserve`, sun-/food-fed) → governs **survival** (death at 0, the
  Law-11 reorder). Unchanged.
- **Nutrient** (new) → governs **reproduction** only.

So a plant with no nutrient simply **does not reproduce** (but lives on the sun → **no
death spiral**); the population plateaus gently at the nutrient-supported level. Stable by
construction.

## 1. Locked decisions (do not re-litigate)

1. **Two axes** — energy (survival) + nutrient (reproduction). ✔
2. **Substrate is a distinct category** — a source is **not an `Agent`**. Every life
   system queries `With<Agent>`, so a non-`Agent` entity is ignored by the whole life
   machinery *for free* (no metabolism / death / reproduction / decision). ✔
3. **Nutrients are a concentration field** (a grid `Resource`), not point entities. It is
   environment (the "substrate"), outside Law 11, and **not** a spatial-query structure
   (no §5 conflict — it never does neighbour search; position→cell is a direct hash). ✔
4. **Sources** (e.g. a submarine volcanic vent) are declared in a **separate `sources`
   config list** (a category of their own, **not** archetypes), spawned as **non-`Agent`
   entities** (Transform + `Emits` + a visual, **no** collider → intangible). They emit a
   nutrient into the field; **diffusion** spreads it → **gradients** (life clusters around
   sources). ✔
5. **Per-plant nutrient store** *in T2* (component), to prepare T3. The plant **absorbs**
   field→store and **pays the store** to reproduce. ✔
6. **Source of nutrients in T2 = local sources + diffusion.** **Recycling / closed loop**
   (dead body → nutrients back to its cell) is **deferred** (needs per-species absorption
   first). ✔
7. **Deferred / roadmapped, NOT in T2:** field heatmap **visualization (layers)**; **GUI
   editing of sources**; **per-species absorption + multiple nutrients (T3)**; **emergent
   nutrient-driven targeting (T3, Law 8)**; **nutrient-driven spontaneous generation**;
   **eating/attacking as a costed decision**.

## 2. Data model

### 2.1 `NutrientField` (Bevy `Resource`)

One nutrient in T2 (designed to generalize to `Vec<NutrientField>` in T3).

```rust
#[derive(Resource)]
pub struct NutrientField {
    cells: Vec<f32>,    // res*res concentrations
    res: usize,         // cells per side
    half_extent: f32,   // = arena_half_extent (for pos → cell)
    diffusion: f32,     // [0,1] rebalance fraction per tick (the local↔global knob)
    scratch: Vec<f32>,  // double-buffer for diffuse()
}
```

Methods (all unit-tested):
- `cell_size() = 2*half_extent / res`
- `cell_index(pos) -> usize` — `((pos + half_extent) / cell_size)`, **clamped** to `[0,res)`
  on each axis (the reproduction clamp already keeps agents in-arena, but clamp anyway).
- `sample(pos) -> f32`
- `add(pos, amount)` — deposit (source emission, later recycling).
- `take(pos, amount) -> f32` — remove `min(amount, cell)`, return the amount actually
  taken (**conservation**: a plant gains exactly what the cell loses).
- `diffuse()` — one relaxation step toward the neighbour average using a graph-Laplacian
  stencil with **reflecting (Neumann) boundaries**, which **conserves the total mass**:
  `new[i] = cells[i] + diffusion * (Σ_neighbours cells[j] − deg(i)*cells[i]) / 4`
  (with `diffusion ≤ 1` for stability). Writes into `scratch`, then swaps.

### 2.2 Components

```rust
#[derive(Component)]
pub struct Nutrients { pub current: f32, pub max: f32 }   // per-plant store

#[derive(Component)]
pub struct Emits { pub nutrient: usize, pub rate: f32 }   // on source (substrate) entities
```

`Nutrients` is attached to **every agent** at spawn (`max = nutrient_capacity` gene,
`current` = birth nutrient). With all nutrient genes 0 it is inert → byte-identical.

## 3. Config / scenario schema

`SimConfig` gains (all `#[serde(default)]` → existing scenarios unchanged):

```rust
pub nutrient: NutrientConfig,   // { resolution: usize (default e.g. 48), diffusion: f32 (default 0.0) }
pub sources: Vec<Source>,       // default empty
```

```rust
pub struct Source {
    pub pos: [f32; 2],
    pub nutrient: usize,   // T2: always 0
    pub rate: f32,         // emission per second
    pub color: [f32; 3],
    pub radius: f32,       // visual only (no collider)
}
```

`Genotype` gains three genes **appended at the END** (DEV Rule 3 — appended, **non-mutable
by default**, default `0.0` → `mutate()`'s draw stream and the sim stay byte-identical):

```rust
pub nutrient_absorption: f32,   // field → store rate (per second)
pub nutrient_capacity: f32,     // the store's max
pub offspring_nutrient: f32,    // nutrient paid per child (analogue of offspring_energy)
```

Add a `TraitSpec` entry for each at the end of `genotype::TRAITS` (with bounds in
`SimConfig`, non-mutable in `Mutability::default()`), and update the `TRAITS` length and
the literal `Archetype`/`Genotype` constructions only if they don't already spread
`..Genotype::default()` (most do).

**Byte-identical guarantee:** no sources + absorption/capacity/offspring_nutrient 0 +
diffusion 0 ⇒ emit/diffuse/absorb are no-ops on agent state and the reproduce gate always
passes paying 0. The previously-green scenarios are untouched. (The 4 `#[ignore]`'d
grazed-food tests stay ignored; they will be **re-balanced via this nutrient layer**, the
reason they were parked.)

## 4. Systems and scheduling (`lib.rs`)

Current `FixedUpdate` chain:
`perceive → decide → act → interact → reap → metabolize → age_agents → reproduce`.

Insert the nutrient sub-pipeline **after `metabolize`, before `reproduce`** (so the store
is filled before reproduction reads it):

`… → metabolize → emit_nutrients → diffuse_nutrients → absorb_nutrients → age_agents → reproduce`

- **`emit_nutrients`** — for each `Emits` source: `field.add(pos, rate * dt)`.
- **`diffuse_nutrients`** — `field.diffuse()` once (independent of entities). Early-return
  if `diffusion == 0`.
- **`absorb_nutrients`** — for each agent with `nutrient_capacity > 0`: pull
  `want = min(absorption*dt, capacity − current)`, `got = field.take(pos, want)`, add `got`
  to the store. Conservation via `take`.
- **`reproduce` (extended)** — add `&mut Nutrients` to its query; extend the guard with
  `|| nutrients.current < genotype.offspring_nutrient`; on success
  `nutrients.current -= genotype.offspring_nutrient`; the child is spawned with an
  **empty** store.

  > **Implementation correction (step 7):** the child must be born **empty**, *not*
  > endowed with `offspring_nutrient`. The originally-planned "born with
  > `offspring_nutrient` (conservation: parent → child)" was found, in
  > `tests/nutrients.rs`, to make the nutrient **circulate** down a lineage: a seed
  > endowed with exactly the gate amount meets the gate immediately, so the nutrient
  > never limits anything and the abundant solar-energy axis sets the pace → the
  > population **explodes** (~6400, past saturation, across all seeds). Spending the
  > nutrient (consumable, removed from the pool) makes it a genuine *limiting*
  > resource: each new plant must absorb its **own** fresh nutrient to reproduce, so
  > the growth is throttled by the field's supply (≈ emission / `offspring_nutrient`).
  > The conserving closed loop returns with **recycling** (deferred): a dead body
  > returns its nutrient to the field.

Each new system early-returns cheaply when inert (no sources / no absorbers / diffusion 0).

## 5. Spawn (`spawn.rs`)

- `spawn_agent_with_brain` gains a `nutrients: f32` parameter and attaches
  `Nutrients { current: nutrients, max: genotype.nutrient_capacity }`. Founders born with
  `0`; children born with `offspring_nutrient` (from `reproduce`).
- **`spawn_sources(commands, config)`** — new: for each `config.sources` entry, spawn a
  **non-`Agent`** entity: `Transform` (at `pos`), `Emits { nutrient, rate }`, a visual
  (mesh + colour, sized by `radius`), **no** `Agent` / `Reserve` / `Genotype` / `Brain` /
  `Collider`.
- `populate` = `spawn_arena` + `spawn_agents` + **`spawn_sources`**.
- `SimPlugin::build` inserts the `NutrientField` resource sized from
  `config.nutrient.resolution`, `arena_half_extent`, `config.nutrient.diffusion`.

## 6. Rendering (minimal in T2)

- **Sources**: rendered as colour circles (mesh at `pos`, radius = `Source.radius`) so they
  are visible. A tiny render path (they are not agents, so `visuals` needs a small system
  or the mesh is attached at spawn).
- **Field heatmap**: **deferred** (ROADMAP "Nutrient-field visualization — layers").

## 7. Tests

- **Unit** (`NutrientField`): `cell_index` clamping; `add`/`take` conservation; `diffuse`
  **conserves total mass** and relaxes toward uniform; inert when `diffusion == 0`.
- **Integration** `tests/nutrients.rs` + `scenarios/nutrients.ron`: a world of sources +
  plants. Assert, **across several seeds**, that the Plant population (a) **grows** beyond
  founders, (b) **stays bounded** (no carpet), (c) **persists** (no collapse) — i.e. the
  T1 fragility is **resolved** by the two-axis decoupling. This test is the falsification
  of the whole T2 design.

  > **Done (step 7).** Calibrated and green across the 4 seeds: ~40 founders → ~515 with
  > sources (bounded, no carpet, persists), and a **falsifiable contrast** — with the
  > `sources` removed the *same* plants stay flat at the founder count (no nutrient → no
  > reproduction, but sun-fed → no collapse), proving the nutrient gates **only**
  > reproduction. **Finding:** with immortal plants the nutrient bounds the growth
  > **rate** (≈ emission / `offspring_nutrient`), not the standing crop — the population
  > grows slowly and is bounded *within the run*. A true carrying capacity (a flat
  > equilibrium) needs **turnover** (recycling / mortality), the deferred sub-phases. T2
  > delivers the gating + spiral-free persistence, not yet the closed loop.

## 8. Implementation step order (each independently testable)

1. `NutrientField` resource + methods + **unit tests** (pure data, no ECS).
2. `Nutrients`, `Emits` components.
3. Config: `NutrientConfig`, `sources`, defaults inert + a parse/byte-identical test.
4. `Genotype`: append the 3 nutrient genes (+ bounds, + `Mutability`, + `TRAITS`), all
   non-mutable default 0; fix any literal constructions the compiler flags.
5. Spawn: `Nutrients` on agents; `spawn_sources`; insert `NutrientField` resource.
6. Systems: `emit` / `diffuse` / `absorb`; extend `reproduce`; schedule them.
7. `scenarios/nutrients.ron` + `tests/nutrients.rs`; tune to **bounded + stable** across
   seeds.
8. Minimal source rendering (circles).
9. Re-run the suite: previously-green scenarios still green (byte-identical); the 4 parked
   tests remain `#[ignore]` (their re-balancing via nutrients is a *follow-up*, not part
   of the T2 core).

## 9. Open micro-decisions (settle during implementation, not before)

- Exact diffusion stencil constant and boundary handling (must conserve total).
- Field `resolution` default and whether to expose it per-scenario only (yes) or also as a
  knob in the editor (later).
- `Nutrients` on **all** agents (default 0) vs only on absorbers — lean **all** (uniform,
  simplest, byte-identical).
- Whether `nutrient_capacity` is a gene (Genotype) or an `Archetype` body constant like
  `reserve_max` — lean **gene** (appended, non-mutable), for consistency with
  `offspring_energy`.

## 10. Explicitly out of T2 scope (deferred — see ROADMAP §9)

Recycling / closed loop · per-species absorption + multiple nutrients (T3) · emergent
nutrient-driven targeting replacing the `relations` table (T3, Law 8) · field heatmap
layers · GUI source editing · nutrient-driven spontaneous generation · eating/attacking as
a costed brain decision.
