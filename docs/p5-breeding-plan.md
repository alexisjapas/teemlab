# P5 — generational regime (run → score → breed) + MLP breeding: detailed plan

**Status:** designed, not yet implemented. Binding reference to resume the work (cf.
[`ROADMAP.md`](../ROADMAP.md) §8 P5 items 19–20 + §9 "A/B regime seam"). Records every
decision taken in the design discussion so we do **not** diverge.

**Scope of v1 (the three forks, decided with the user):**
1. **Headless orchestrator + a windowed dashboard first** — no live match *spectator* in
   v1 (deferred; the existing renderer becomes the spectator later).
2. **Regime config lives in `SimConfig`** as an additive sub-struct (cleanest long-term —
   the editor is "driven by the scenarios", RON stays backward-compatible).
3. **First carrier scenario = MLP breeding** — the generational extension of the `train`
   bin, motivated by the item-18b finding (from-random neuroevolution is high-variance;
   the decisive lever is *founder diversity* → generational batches).

---

## 0. Why

§4 names two canonical regimes on a grid of two axes. We have the **continuous** corner
(in-sim reproduction `ecology::reproduce` + implicit/ecological fitness). P5 adds the
**batched-repro + explicit-fitness** corner: a `run → score → breed → run` loop run by an
**outside-sim orchestrator**.

The item-18b wall is the concrete motivation: a from-random MLP head-to-head is
high-variance (a mediocre cohort is excluded before it learns); the decisive lever is the
**diversity of founders**. A generational batch over many seeded matches, selecting and
re-seeding the best-evolved genomes, is exactly the meta-loop that answer asks for — and
it extends `train` (which already captures *one* best-evolved MLP from *one* headless run)
into a multi-generation search.

---

## 1. Locked decisions (do not re-litigate)

1. **No `enum Regime`** (§4 architectural guard). The regime is a *recomposition* of two
   separable seams, never a reified type:
   - **where reproduction lives** — continuous = `ecology::reproduce` (a `FixedUpdate`
     system); generational = the orchestrator breeds *between* matches, **without the
     in-sim system depending on it**.
   - **where fitness comes from** — implicit (emergent) vs explicit (a computed score).
   A third regime (e.g. a pure fixed-cohort GA with no in-match reproduction) must drop
   out as a *recomposition*, not a special case.
2. **The inner match is the existing `SimPlugin`, byte-identical.** A match = run a
   `SimConfig` headless for a terminal condition. In-match continuous evolution
   (`ecology::reproduce` mutating weights) **stays on** — it is *how* a cohort improves
   within a match (item 18b). The orchestrator only acts at the **generation boundary**.
   This is the cleanest composition: the meta-loop sits *on top of* the continuous loop.
3. **The orchestrator is outside-sim** (DEV Rule 1: no sim logic in `Update`). It drives
   isolated `World`s headless (§6), exactly like `sweep`/`train` and the test drivers
   (`MinimalPlugins + SimPlugin`, manual `app.update()` loop). It never runs in the
   windowed `Update`.
4. **Regime config = an additive `Option<BatchConfig>` on `SimConfig`** (`#[serde(default,
   skip_serializing_if = "Option::is_none")]`). Default = `None` → no code path runs → every
   existing scenario is **byte-identical**, its RON unchanged (field omitted). Same pattern
   as `Archetype::source` / `captured_brain`.
5. **Selection re-seeds founders via the existing capture seam.** A surviving genome →
   `Archetype::capture(genotype, brain, generation)` → injected as the next cohort's
   founder (`captured_brain` = frozen weights, evolved genotype). No new "how do I carry a
   trained brain" machinery — it already exists (item 4 / `train`).
6. **Determinism is order-of-magnitude, not bit-for-bit** (Law 10 / §5: parallelism over
   strict determinism). Tests assert *monotone trend* / *band* properties across seeds,
   like every other multi-seed driver — never exact final values.
7. **Parallelism is staged.** Correctness first, **sequential** (the `sweep` loop). Then
   `tests/breeding.rs` green. *Then* item 20's cross-match parallelism (`TaskPool` /
   `std::thread`), under measurement — the nested-`App` global-thread-pool contention is
   the real risk and must not gate the first landing.

---

## 2. Schema (`src/config.rs`)

Add one field + one struct + one enum. All additive, all `deny_unknown_fields`.

```rust
/// Generational ("batched repro") regime parameters (§4 axis A + B). `None` (the
/// default, every existing scenario) → the continuous regime, untouched. Present →
/// the `breed` bin / the windowed dashboard can run the run → score → breed loop.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub batch: Option<BatchConfig>,
```

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchConfig {
    /// Number of generation boundaries the orchestrator runs.
    pub generations: usize,
    /// Independent seeded matches per generation (the cohort). The diversity lever.
    pub matches_per_gen: usize,
    /// Terminal condition of one match: a fixed tick budget in v1 (richer
    /// conditions — extinction, score threshold — are a later enum, kept a single
    /// `ticks` for now to avoid a premature abstraction).
    pub match_ticks: u64,
    /// The archetype under selection (the MLP species). v1: one. Battle (item 19)
    /// generalizes to several scored factions.
    pub scored_species: u16,
    /// The explicit fitness function (§4 axis B).
    pub fitness: Fitness,
    /// Top-K genomes carried into the next generation's founders (selection pressure).
    pub survivors: usize,
    /// Base seed for the per-match seeds (`seed_base + gen*matches + m`), so a whole
    /// breeding run replays from one number (an *experiment*, not bit-for-bit).
    pub seed_base: u64,
}
```

```rust
/// Explicit fitness: how a match scores a genome (§4 axis B). A small, growable menu
/// of engine primitives — an exhaustive `match` in `breeding::score`, the homogeneous
/// counterpart of the relation/cost tables.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Fitness {
    /// Best-evolved individual of `scored_species`: highest `Generation`, tie-broken
    /// by `Reserve` — exactly `train`'s current capture rule. The MLP-breeding default.
    BestEvolved,
    /// Standing biomass: living count of `scored_species` at the terminal condition
    /// (an ecological score — coexistence/dominance).
    Population,
    // Future (item 19, battle): SurvivalTime, Kills, EnergyHarvested — added as arms
    // here + in `breeding::score`, no schema reshuffle.
}
```

**Note on nesting the meta-config inside the per-match config.** A `SimConfig` describes
*one* world and `BatchConfig` describes a loop over *many* — but the user's chosen
trade-off (a self-contained scenario file carries its own regime, edited in the same
editor) wins on ergonomics and on "editor driven by the scenarios". The match runner reads
`config` and **ignores** `config.batch` (it is meta); the orchestrator reads `config.batch`
and clears it on each emitted match config (a match never recurses into a batch).

---

## 3. The orchestrator (`src/breeding.rs`, new lib module)

Render-agnostic, in the lib (so the `breed` bin **and** the windowed dashboard share it).
Pure outside-sim driving — it builds and pumps `App`s, it adds no `FixedUpdate` system.

```rust
/// One generation's outcome, for the curve + the leaderboard.
pub struct GenerationReport {
    pub generation: usize,
    pub best_fitness: f64,
    pub mean_fitness: f64,
    pub best: CapturedGenome,        // genotype + Brain + source generation
    pub leaderboard: Vec<Scored>,    // sorted, for the dashboard's right panel
}

/// The breeding loop's state. Drives N generations; each `step` runs one.
pub struct Orchestrator {
    base: SimConfig,                 // the carrier scenario (batch cleared per match)
    batch: BatchConfig,
    survivors: Vec<CapturedGenome>,  // re-seeded as founders next generation
    gen: usize,
}

impl Orchestrator {
    pub fn new(config: SimConfig) -> Option<Self>;       // None if config.batch is None
    pub fn step(&mut self) -> GenerationReport;          // run cohort → score → select
    pub fn is_done(&self) -> bool;                       // gen >= generations
}
```

`step()`:
1. **Build the cohort.** `matches_per_gen` match `SimConfig`s: clone `base`, clear
   `.batch`, set `.seed`, and (gen > 0) **inject `survivors` as founders** of
   `scored_species` via `Archetype::capture` (round-robin if `survivors < count`).
2. **Run each match headless** — the `sweep::run_once` pattern (`MinimalPlugins +
   SimPlugin`, `finish/cleanup`, `match_ticks` × `update()`). Sequential in v1.
3. **Score** each finished world with `score(&world, &batch.fitness, scored_species)` and
   capture its best genome (`Generation`/`Reserve`, like `train`).
4. **Select** the top `survivors` across the cohort → stored for the next `step`.
5. Return a `GenerationReport` (best/mean fitness, the global-best genome, the leaderboard).

The **final** best genome → a catalog variant, reusing `train`'s exact path
(`SpeciesEntry::variant` + `Archetype::capture`, written to `species/saved/`).

**`score`** is a free function (unit-testable without an `App`): an exhaustive `match` over
`Fitness`, reading the final world's `Query<(&Species, &Generation, &Reserve, ...)>`.

---

## 4. Headless `breed` bin (`src/bin/breed.rs`)

CLI sibling of `sweep`/`train`: `cargo run --bin breed -- <scenario.ron> [generations]
[out.ron]`. Loads the scenario, asserts `batch.is_some()`, runs the orchestrator to
completion printing the fitness-per-generation table (the headless face of the dashboard's
curve), and writes the best genome as a catalog variant + (optionally) a showcase scenario
— folding in what `train` does (which becomes the `generations: 1` special case; `train`
can later be retired in favour of `breed`, not in v1).

---

## 5. The dashboard (windowed binary)

The windowed app gains a **Generational mode** (a presentation toggle, **not** an engine
type). Because the orchestrator runs matches headless on a background thread, the windowed
`App` stays responsive and renders only the dashboard in `Update`/egui — Rule 1 intact
(observation, no sim logic). The app's own live `SimPlugin` world is **paused and unused**
in this mode (it returns for the deferred spectator).

**Threading.** The orchestrator runs on an `AsyncComputeTaskPool` task (or a `std::thread`),
emitting `GenerationReport`s back over an `mpsc`/`crossbeam` channel into a Bevy resource
(`BreedingState`) the dashboard reads each frame. The heavy compute never touches the
render loop; isolated `World`s per match honour §6.

Where each piece lands in the **existing** dock (`src/panels.rs`):

| Dock zone (today) | Generational addition |
|---|---|
| **Top bar** transport (`controls_section`) | a **regime toggle** (Spectate ↔ Generational); in Generational mode the transport row becomes **Run / Pause / Stop / Step-generation**, driving the orchestrator task (not `Time<Virtual>`). |
| **Left/World** (`editor::world_section`) | `editor::batch_section` — generations, matches/gen, match_ticks, scored species (dropdown), **fitness** (dropdown over `Fitness`), survivors — mirroring `nutrient_section`/relations editor. |
| **Bottom/curves** (`hud`) | **fitness-vs-generation** plot: a new `BreedingHistory` (best/mean/worst per generation) rendered with the existing `metrics::Curve` + `hud_section` machinery — same plotter, a generation-indexed X axis instead of sim-time. |
| **Right/Analysis** (`inspector`) | a **leaderboard** of the current generation's scored genomes; click → inspect with the existing MLP graph viewer (18b-viz, already reads a `Brain`); **"Save as variant"** reusing `editor::save_variant`. |
| status line (`UiStatus`) | progress: `gen X/G · match m/M · elapsed`. |

**Reuse map (most pieces already exist):**
- outside-sim run loop → `sweep::run_once` / `train`.
- capture a genome → `Archetype::capture`, `SpeciesEntry::variant`, `spawn` honours
  `captured_brain`.
- curve plotting → `metrics::{Curve, History}` + `hud`/`dataviz`.
- genome inspector → 18b-viz.
- **genuinely new** → `BatchConfig`/`Fitness` schema, `breeding::{Orchestrator, score}`,
  the `breed` bin, the dashboard panels + the channel/threading, and (item 20) the
  cross-match parallelism.

---

## 6. First carrier scenario — MLP breeding (decision 3)

`scenarios/examples/13_mlp_breed.ron`: the oasis-flora training ground of `08_mlp_train`
(a cohort of MLP genomes + nutrient-gated flora), plus a `batch`:

```ron
batch: Some((
    generations: 12,
    matches_per_gen: 6,
    match_ticks: 6000,
    scored_species: 0,
    fitness: Population,
    survivors: 3,
    seed_base: 1,
)),
```

(`Population`, not `BestEvolved` — the latter is perverse on free reproducers, cf. §7;
for a forager that must find food to breed, standing biomass is the saner signal. The
committed scenario is trimmed for tractability — MLP 50 / flora 150 / resolution 128 /
generations 8 / matches_per_gen 4 / match_ticks 5000.)

The orchestrator evolves MLPs within each match (continuous loop) **and** re-seeds the
next generation's cohort from the best-evolved survivors — the founder-diversity lever of
item 18b, now a meta-search. Output: a `species/saved/` variant strictly better than
`train`'s single-run capture (the falsifiable claim the driver checks). It directly
supersedes `train`'s pedagogy: `mlp_train` = the ground, `breed` = the search, the captured
variant = `mlp_evolved` reaching (then beating) parity.

---

## 7. Testing (DEV Rule 3 — `tests/mlp.rs` is the tripwire)

- **Byte-identity guard (the cardinal one):** `batch` defaults to `None` → no orchestrator,
  no schema reaches the inner `SimPlugin`, the RNG stream is untouched → `tests/mlp.rs` and
  every continuous driver stay green and bit-for-bit. Add a `config.rs` unit: a scenario
  without `batch` omits the field from its RON (round-trip, `skip_serializing_if`).
- **`tests/breeding.rs` — the orchestrator *mechanism* (done), not emergent improvement.**
  *Planned* as a "best_fitness trends up + `survivors: 0` contrast" driver; **revised in
  practice** to test the **mechanism** instead: `Orchestrator::new` requires a `batch`; it
  runs exactly `generations` generations reporting one score per cohort match; and the
  **core contrast** — with selection it carries an *evolved* elite forward
  (`survivors()` non-empty, `generation ≥ 1`), with `survivors: 0` it carries **nothing**
  (the breeding switch OFF). On a **cheap self-sufficient reproducer** (a photosynthetic
  `Wander` — no food / nutrients / costly raycasting, the population *bounded* by a small
  net energy so it grows to ~30 not exponentially) → ~3 s.
  *Why not "fitness trends up":* designing a fitness landscape that reliably climbs is
  **scenario-design research** — neuroevolution on living food plateaus at **parity**
  (`tests/mlp.rs`, §7 of the ROADMAP), and the controlled contrast was both **too slow**
  (a real ecosystem must persist + improve over many ticks) and **too noisy** (population
  collapse swings the score). The emergent payoff is the `breed` bin's job on
  `13_mlp_breed.ron` — a **generator**, like the `train` bin (likewise not in CI).
- **Finding — `BestEvolved` is *perverse* on a free reproducer.** Selecting the deepest
  in-match lineage rewards a **reproduce-to-collapse** gene (a runaway-low
  `reproduction_threshold`): a re-seeded cohort then *dies out*, scoring **worse** than a
  random restart. `BestEvolved` only behaves where reproduction **requires a skill** (a
  forager that must find food to breed — the `train`-bin case). **`Population`** (standing
  biomass) is the saner forager fitness, and the one `13_mlp_breed.ron` uses (a quick
  `breed` run gives a sensible cohort spread, e.g. `[28 41 6 38]`).
- **Unit `breeding::score`** (done, no `App`): each `Fitness` arm on hand-built individuals
  — `BestEvolved` = deepest generation of the scored species, `Population` = its living
  count, `0.0` when extinct — plus `best_individual` (generation, then reserve).
- **Schema unit** (done): a scenario without `batch` omits the field from its RON
  (`skip_serializing_if`) and reads back `None`; a `batch` scenario round-trips losslessly.

---

## 8. Implementation order

1. **Schema (done)** — `BatchConfig`, `Fitness`, `SimConfig.batch` (+ round-trip/omission
   unit). Every existing scenario byte-identical; `tests/mlp.rs` re-validated green.
2. **`breeding::score` (done)** — the pure core (`Individual`, `score`, `best_individual`)
   + unit tests (no `App`).
3. **`breeding::Orchestrator` (done)** — sequential cohort (`run_match` = the `sweep`/`train`
   pattern) + capture + select, `survivors()` accessor + the **`breed` bin**. Verified:
   `breed` prints the per-generation fitness table and writes a `species/saved/` variant.
4. **`scenarios/examples/13_mlp_breed.ron` (done)** + **`tests/breeding.rs` (done)** — the
   committed showcase (parse-unit-tested, run via the bin) + the **mechanism** driver
   (re-seeding contrast, cf. §7; *not* the planned trend test). `tests/mlp.rs` still green.
5. **Dashboard** — `BreedingState` resource + channel, the background task, `batch_section`
   editor, the Run/Pause/Stop/Step-gen transport, the fitness-vs-generation curve, the
   leaderboard + Save-as-variant. *Checkpoint:* run a breeding session windowed,
   `cargo clippy --all-targets` clean, `cargo fmt`.
6. **Item 20 — cross-match parallelism** (`TaskPool`/`std::thread`, isolated `World`s),
   **profiler in hand**, after correctness. The nested-`App` global-thread-pool contention
   is the known risk; measure before committing a scheme.
7. **ROADMAP/constitution update** — move P5 items 19–20 from "Remaining" with the findings.

---

## 9. Open questions (deferred, not v1)

- **Live spectator** (decision 1 deferred): re-render the best genome's match in the
  windowed live world — the analog of `record`'s "fresh re-render of the best genome" (§7),
  since parallelism forbids exact seed replay.
- **Richer terminal conditions** (`match_ticks` → an enum: extinction / score threshold /
  Red-Queen stop) — when item 19's battle needs them.
- **Crossover** on NN weights (the permutation/competing-conventions problem, §9) — v1 is
  **mutation-only** at the boundary (the in-match `ecology::reproduce` already mutates);
  weight crossover lands with NEAT (item 21).
- **Per-`think` MLP allocations** (§9 perf) — becomes significant under the parallel batch
  (item 20); handle profiler in hand, *after* P5, not before.
- **Battle (item 19)** — multiple scored factions (`scored_species` → a set), a
  `transfer: false` faction relation, co-evolutionary fitness; this plan's seams (orchestrator,
  `Fitness`, capture) are the foundation it recomposes onto.
