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

Each phase ends green (`fmt`/`clippy`/tests). Phase 1 stays byte-identical; Phase 2 is the
**deliberate** breaking change (DEV Rule 3), staged like the Food-dissolution refactor.

- **Phase 1 — multi-component substrate (byte-identical).** `Field` (+`decay`), `Fields(Vec)`,
  `components`/`field_resolution` config, `Source.component`; `emit_nutrients`/`diffuse` loop
  over fields; heatmap renders N fields (shared opacity budget — already envisioned). A
  single-component scenario (every existing one, migrated mechanically: `nutrient:` →
  `components: [(…)]`) is **byte-identical**. Unit tests for decay + multi-field.
- **Phase 2 — the `FieldRelation` table + `Stores` + de-hack (BREAKING).** Add
  `field_relations`; `Stores` per component; rewrite `absorb_nutrients` → `absorb_components`,
  the `reproduce` gate, and `reap` recycling to read the table; **remove**
  `nutrient_absorption`/`nutrient_capacity`/`offspring_nutrient` from
  `Genotype`/`TRAITS`/`Mutability`/`*_bounds`. Also fold the **T3 trophic transfer**
  (`interaction.rs` moves `Nutrients` on predation) onto `Stores`. Migrate **all** component
  scenarios + `species/*`; **re-baseline `tests/mlp`** and green the nutrient drivers
  (`nutrients`, `trophic`, `recycling`, `predator_prey`, `deliberate_eating`, `restraint`,
  `flora`). Inspector/editor updated to the table + per-component stores.
- **Phase 3 — emission (alive) + the pheromone `sense` channel.** `emit_components` system
  (agent→field, alive, from the `emit` verb); `Perception.field_state` + MLP input widen
  (`input_size`, `resize_input_fan` carries the field block, labels, inspector graph);
  regenerate the trained-MLP captures if input widened. Falsifiable unit
  `mlp_reads_field_state_channel` (two perceptions differing only in a sensed component →
  different actions). Playable `scenarios/examples/18_pheromones.ron` + `tests/pheromones.rs`.
- **Phase 4 — docs + memory.** ROADMAP §0/§8/§9, `persistent-ecosystems.md` §3/§7,
  `docs/src/model/*` (nutrients→components, the new table, the sense channel), scenario
  catalog, `scenario-format.md`. Retire this plan's "done" phases.

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
  (per-ray) field sensing; turnover/detritus decomposition scenario; a toxicity scenario (both
  now *config-only* on this substrate); GUI editing of components/sources.
