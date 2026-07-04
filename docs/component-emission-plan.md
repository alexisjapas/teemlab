# Component emission & the agent↔component redesign — implementation plan

**Status:** binding implementation plan (like [`nutrients-t2-plan.md`](./nutrients-t2-plan.md)
and [`p5-breeding-plan.md`](./p5-breeding-plan.md)). Records the decisions so they are not
re-derived. Laws cited by number ([`CONSTITUTION-SIM.md`](../CONSTITUTION-SIM.md),
[`CONSTITUTION-DEV.md`](../CONSTITUTION-DEV.md)); orientation in
[`persistent-ecosystems.md`](./persistent-ecosystems.md) §3.

## 0. Goal & motivation

Build **component emission** — the agent→environment write, the *symmetric of absorption*
(`persistent-ecosystems.md` §3) — and, in doing so, **fix the agent↔component construction**,
which today is a single-nutrient hack. One mechanism (*emitted component + field-perception
channel + relation-defined semantics*, no per-kind code — Law 11) unifies four wishlist items:
**corpses/turnover**, **organic waste**, **toxicity** (endogenous collapse mode), and
**pheromone communication** — a toxin and a pheromone differ *only* by their relation. First
use-case driven end-to-end: **pheromones** (a `sense` channel + `emit`), so communication can
*emerge*.

### What is wrong today (the thing we are replacing)

The agent↔field relationship is expressed three incompatible ways at once:

- **scalar genes on the monolithic `Genotype`** — `nutrient_absorption`, `nutrient_capacity`,
  `offspring_nutrient` (with matching `TRAITS`/`Mutability`/`*_bounds`); single-nutrient by
  construction, and non-mutable in practice (so not actually evolving — a fake gene);
- a **single** `NutrientField` resource + a single `NutrientConfig`;
- a vestigial `nutrient: usize` index on `Source`/`Emits` that *pretends* multi-component but
  nothing else honours.

Meanwhile agent↔**agent** interactions have a clean declarative `relations` table
(`Relation { actor, target, transfer, rate, range }`). The asymmetry is the rot: with *N*
components each of which a species may absorb / emit / sense / be-harmed-by, the flat genotype
would need *N×* scalar genes — a **matrix crammed into scalars**.

## 1. Target architecture

**"Component" is the generalization.** A nutrient, a toxin, a pheromone, biomass are all just
*components* — concentration fields over the arena, differentiated **only by their relations**.
"Nutrient" stops being a type; it is a component that happens to be absorbed and to gate
reproduction.

### 1a. Components (the substrate)

- `SimConfig.components: Vec<ComponentConfig>` replaces `NutrientConfig`.
  `ComponentConfig { name: String, color: [f32;3], diffusion: f32, decay: f32 }`.
  **`decay` is new** — a per-tick fractional loss (`c *= 1 - decay`): pheromones must fade,
  detritus decomposes; a nutrient uses `decay: 0`. `resolution` stays **global**
  (`SimConfig.field_resolution`, one grid size for all) — simpler, and diffusion/decay are the
  per-component knobs that matter.
- Engine resource `Fields(Vec<Field>)` — one grid per component. `NutrientField` → **`Field`**
  (rename; keep the conservative `add`/`take`/`diffuse`, add `decay_step`). `Fields` is built
  from `components` at `SimPlugin` build + reset.
- `Source.nutrient` → `Source.component: usize` (rename; still an index into `components`).

### 1b. The `FieldRelation` table (the redesign — replaces the nutrient genes)

The environmental analogue of `relations`. One **bundled row per (species, component)** it
relates to (sparse: only the pairs that interact). Verbs, each `0`/`false` = absent:

```rust
struct FieldRelation {
    species: u16,       // archetype index
    component: usize,   // index into `components`
    absorb: f32,        // field → store, per second
    capacity: f32,      // this species' store capacity for this component
    emit: f32,          // store/body → field, per second (ALIVE emission)
    emit_at_death: f32,  // amount deposited into the field at death (recycling, generalized)
    sense: bool,        // local concentration → a brain perception channel
    affect: f32,        // concentration → Reserve, per second (toxicity < 0, healing > 0)
    repro_cost: f32,    // store spent per child (the reproduction gate)
}
```

- **Scenario-authored constants**, like `Relation`'s `rate`/`range` — **not** genes (v1). The
  old genes were non-mutable anyway, so nothing evolvable is lost. Binding a magnitude to a
  gene (`Option<GeneRef>`) is a **deferred** extension, not v1.
- **Directional by construction**: species A's row for component C has `emit > 0`; species B's
  row for C has `affect < 0` — "A poisons B" is two rows, no special code (Law 11). A pheromone
  = an `emit` row + a `sense` row (possibly different species). A nutrient = `absorb` +
  `capacity` + `repro_cost`.
- **Facets are independent and combinable** — a `(species, component)` row is the *complete*
  relationship of that pair, and **any subset** of its verbs may be non-zero at once. A species
  can `sense` a component **and** be `affect`-ed by it; be `affect`-ed **without** sensing it (an
  odorless toxin — humans ↔ CO: `sense: false, affect < 0`); or `emit` **and** `sense` the same
  component (perceive its own trail). One **bundled row per pair** (not one row per verb) is
  deliberate: the store `capacity` is a property of the *pair* — shared by `absorb`, trophic gain
  (eating), `repro_cost` and `emit`-from-store — with no clean home in a per-verb row. (Trade-off:
  a wider struct with mostly-zero fields on simple relations. The one-verb-per-row alternative is
  more orthogonal/sparse but needs a separate store/`capacity` declaration.)

### 1c. Per-component stores

`Nutrients { current, max }` → **`Stores { amounts: Vec<f32>, caps: Vec<f32> }`** (indexed by
component; `caps` from the `capacity` verb, `0` where the species does not hold it). Absorption
fills `amounts[c]`, reproduction spends it, emission can draw from it. Inspector shows each
non-zero store as a reservoir bar (generalizes today's single nutrient bar).

### 1d. Pheromones = the `sense` verb → a brain channel

`sense: true` on a (species, component) row means the agent reads that component's **local
concentration** as a scalar perception input. New `Perception` block `field_state` (one scalar
per sensed component, in component order), appended to the MLP input **after** `self_state` —
the *exact* method used for threat/proprioception (item 18e→18g): extend the contract, wire the
learned brain, hand-written brains ignore it. `input_size = CHANNELS×rays + SELF_CHANNELS +
n_sensed`. Gradient sensing (a direction, per-ray) is a **later** enrichment; v1 is a scalar
(local concentration), like `self_state`. **Emit + sense on a shared component = a communication
substrate whose meaning is evolved** (an emitter writes, a perceiver reads).

## 2. Phased implementation

Each phase ends green (`fmt`/`clippy`/tests). **Phases 1 & 2 are DONE and byte-identical** —
removing the (non-mutable) nutrient genes leaves the mutation RNG stream untouched (`mutate()`
skips non-mutable genes), so the `tests/mlp` tripwire never needed a re-baseline (the earlier
"breaking" framing was pessimistic).

- **Phase 1 — multi-component substrate (DONE, byte-identical; `0d01f7d`).** `Field` (+`decay`),
  `Fields(Vec)`, `components`/`field_resolution` config, `Source.component`; the field systems
  (+ a new `decay_nutrients`) and the heatmap loop over N fields. Every scenario migrated
  (`nutrient:` → `field_resolution` + one `Nutrient` component). Unit test for decay.
- **Phase 2 — the `FieldRelation` table + de-hack (DONE, byte-identical; `e336bfd`, `628b200`,
  `93002c7`).** Added `field_relations` (a bundled row per (species, component));
  `absorb_nutrients`, the spawn store-cap and `reproduce`'s nutrient gate read
  `SimConfig::nutrient_of` (the row's `absorb`/`capacity`/`repro_cost`); **removed** the three
  scalar nutrient genes from `Genotype`/`TRAITS`/`Mutability`/`*_bounds`/`GeneCategory`. Every
  scenario authors its rows; the nutrient drivers (`nutrients`, `trophic`, `recycling`,
  `predator_prey`, `deliberate_eating`, `restraint`, `flora`) + the `mlp` tripwire stay green
  **unchanged** (no re-baseline). Staged 2a (table+shim) → 2b-i (author rows) → 2b-ii (strip
  genes), each byte-identical. **Deferred (§8, no near-term need):** per-component `Stores`
  (only nutrients hold a store — pheromones emit/sense, toxins affect — so the single
  `Nutrients` store is kept, the nutrient being component 0); and wiring `emit_at_death` /
  `affect` (recycling stays the `reap` special-case; toxicity is a later config-only use).
- **Phase 3 — emission (alive) + the pheromone `sense` channel (DONE, byte-identical;
  `0d60c3f`).** `emit_components` (agent→field, the symmetric of absorption, from the
  `emit` verb, alongside the source emission); the `sense` verb → `Perception.field_state`
  (saturating-normalized local concentration `c/(c+1)`), appended to the MLP input after
  `self_state` (`input_size(rays, n_sensed)`; `resize_input_fan` carries the scalar tail —
  proprioception + field-sense — unchanged, n_sensed being per-species constant). Existing
  MLP scenarios (n_sensed 0) byte-identical → **no capture regeneration**. Falsifiable unit
  `mlp_reads_field_state_channel`; playable `18_pheromones.ron` (MLP foragers emit AND sense
  a diffusing/decaying pheromone on the oasis) + `tests/pheromones.rs` (persists AND the
  pheromone field is written). **Deferred:** graph labels for the field-sense input nodes
  (the renderer skips them safely, no panic); an emission cost.
- **Phase 4 — docs + memory (DONE).** ROADMAP §0/§8/§9, `persistent-ecosystems.md` §3/§7,
  `docs/src/model/*` (nutrients→components, the FieldRelation table, the emit/sense channel),
  the scenario catalog (`18_pheromones`), `scenario-format.md`. **Toxicity** (`affect`) is then
  **DONE** (`19_toxicity.ron` + `tests/toxicity.rs`: a self-poisoning monoculture collapses to
  extinction where the emission-off control persists); **turnover/corpses** (`emit_at_death`) is
  **wired and deterministically proven** (`tests/turnover.rs`: a dying body leaves exactly one
  corpse, a living one none) — this **completes the emission substrate** (all five verbs
  absorb/emit/sense/affect/emit_at_death exist, Law 11). A *playable* detritivore scenario,
  though, is **blocked** by the mortality lever (steady prey turnover) + field-navigation
  (carrion-following is MLP-only) — see ROADMAP §0.

## 3. Order-of-battle & invariants

- **Death system order** (`interact → reap → metabolize`) is load-bearing (SIM Law 11) — keep
  it; `emit_at_death` runs inside `reap` (generalizing today's recycling deposit), before
  despawn. Conservation stays the contract (Law 9): `Field::add`/`take` conserve, emission
  moves matter from a store/body into a field, never creating it.
- **`affect` (toxicity)** reads a field and changes `Reserve` — a new field→agent effect,
  placed with the economy (`metabolize`-adjacent). Not needed for the pheromone slice but the
  table carries it (Law 11: no per-kind code — a later toxin scenario is *config only*).
- **RNG safety after Phase 1**: Phase 2 changes `TRAITS` → the mutation stream shifts, so the
  break is confined to Phase 2 and validated by the migrated scenarios + a fresh `mlp` baseline
  (the tripwire is behavioural — thrive+dominate bands — not hardcoded numbers, so it is
  re-*validated*, not re-numbered).
- **Deferred (not v1):** gene-bound table magnitudes (evolvable emission/absorption); gradient
  (per-ray) field sensing; a **playable turnover/detritus scenario** (blocked on the mortality
  lever + field-navigation — the verb itself is wired and proven, `tests/turnover.rs`); GUI
  editing of components/sources.
