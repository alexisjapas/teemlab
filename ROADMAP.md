# Evolutionary simulation engine — Design synthesis

> Reference document. Top-down 2D view, entities = circles. **One single engine**;
> each simulation (natural selection, battle, …) is a *scenario file*.
>
> This document **evolves** (status, plan, implementation order). The parts that
> are *inviolable* are distilled into two stable constitutions, which override this
> document where they overlap: [`CONSTITUTION-SIM.md`](CONSTITUTION-SIM.md) (laws of
> the simulated world — the binding form of §1–§4) and
> [`CONSTITUTION-DEV.md`](CONSTITUTION-DEV.md) (rules of development).

---

## 0. Project status (where we stand)

A living summary; the per-item detail lives in §8 (implementation order) and the
open work in §9.

**Done.**

- **P0–P3** (foundations, playable loop, interface, video capture): done.
- **P4 — deepened natural selection + evolved intelligence** (continuous regime),
  items 15–18: done. Generic gene editor (value/bounds/**mutable**),
  `Brain::Hunter`, co-evolutionary predator-prey, brain **per species** +
  inheritance, **homemade MLP + neuroevolution** (+ graph), evolutionary visual
  precision, genealogy.
- **Item 18d — archetype-first** (Phase 1 of "everything is an entity"):
  `archetypes: Vec<Archetype>` is the central data; the species is a first-order
  unit (body + decider + color + count), its index is its identity, the food is one
  of its archetypes (no more number collision). Mutability **and** founding genotype
  **per species**; relations addressed by archetype; create/delete editor. Scenarios
  migrated and pruned (11 → 7).
- **Item 18e — active flight** ("threat" channel): a `threat` perception channel,
  the *inverse symmetric* of the "target" channel (it lights up when the nearest hit
  is a species that can act **on us**, the inverse relation). `Brain::Hunter` gains a
  **flight reflex by subsumption** (§4): beyond a proximity threshold, survival
  short-circuits foraging. The *same* shared brain → a prey **bolts** when its
  predator approaches, an apex predator stays a pure hunter (the counterpart, on the
  flight side, of the item 17 insight). Driver `tests/flight.rs` (mirror of
  `hunter.rs`); `predator_prey` recalibrated (enlarged arena = refuges, the spatial
  lesson of item 17).
- **Phase 2 — editor finishing touches + species library**: the archetype editor can
  **duplicate** (clone at the end of the list, relations not copied) and **reorder**
  (▲/▼, with *transposition* of the indices in the relation table), in addition to
  create/delete. And a species is now a **serializable, reusable** unit across
  scenarios (item 4): export to `species/*.ron`, import by **copy** (the scenario
  stays self-contained, §9) with a provenance link (`Archetype.source`,
  additive/backward-compatible) for a **resynchronization** that preserves the local
  count.
- **World parameters in the UI**: `tick_hz` (sim rate, "(reset)" — re-applied at
  reset, the single passage point that scenario reload also triggers) and the **gene
  bounds** (`*_bounds`, "Gene bounds" section — one per `TRAITS` entry, i.e. 10 at
  the time, **12** since the two flora genes) join the editor, via a
  `TraitSpec::bounds_mut` accessor (loop over `TRAITS` → DRY). No more scenario
  parameter reserved to the RON.
- **Phase 3a — real evolutionary flora** (item 5): a *flora* becomes a full-fledged
  entity, **without any new core system**. Variable genotype by **superset** (a
  decided choice, §9): `Genotype` gains `photosynthesis` (passive energy gain) and
  `seed_dispersal` (seeding distance) — genes **not mutable by default** (RNG-safe;
  the existing drivers stay bit-for-bit identical). `metabolize` integrates
  photosynthesis into the balance; `reproduce` seeds at `seed_dispersal`;
  `Brain::Sessile` is the trivial brain (no-op). The **carrying capacity emerges**
  from intraspecific competition expressed by the **interaction primitive**
  (Plant→Plant relation without transfer, §3) — no new mechanism. Driver
  `tests/flora.rs` (`scenarios/flora.ron`, 4 seeds): the flora grows ~20×, stays
  **bounded far from saturation** and **persists**.
- **Phase 3b — dissolution of `Food`** (item 18h): the special `Food` type no longer
  exists. `ArchetypeKind` is **flattened** — *every archetype is an agent*
  (`Archetype` carries `genotype`/`brain`/`mutable` directly); a *food source* is a
  **sessile photosynthetic** patch (`Brain::Sessile`, `photosynthesis > 0`,
  reproduction off), renewed in place, fixed count. Removed:
  `replenish_food`/`regen`/`FoodRegen`/`spawn_food*`/`FoodSnap`/the `Food` component;
  snapshot, editor and visuals unified on the agent. The dissolution **revealed a
  conservation flaw** in the interaction primitive (§3): N foragers clustered on a
  single patch each received its full value → energy created. `interact` now **scales
  the draws to the target's available reserve** (two passes, order-independent) —
  strict conservation. Breaking RON (flat schema): 6 scenarios + the library
  migrated; `cohabitation`/`mlp_brain` **recalibrated** (sparse-and-slow food: an
  efficient forager keeps the patches depleted → competitive exclusion holds). All
  drivers green again.
- **Threat wired into the learned brain** (item 18g): the `threat` channel (item
  18e), until then consumed only by the deterministic hunter, joins the **MLP's
  input** — `vision|target` (`2 × rays`) → `vision|target|threat` (`3 × rays`, the
  `MlpBrain::CHANNELS` constant). `reproduced`'s per-block resizing seam goes from 2
  to 3 blocks (DRY on `CHANNELS`); the learned brain therefore receives what it needs
  to *learn* to flee, where the hunter applies a hard-wired reflex — exactly like
  `target` (hunter item 16 → MLP item 18b). RNG stream of **non-MLP scenarios
  intact** (no MLP built); the MLP scenarios have a wider network — `tests/mlp`
  revalidated (domination across the 5 seeds preserved). Unit
  `mlp_reads_threat_channel`: falsifiable proof that the channel is no longer ignored
  (two perceptions differing only by the threat → different actions).
- **Native Bevy visualizer (video) + HUD rearrangement**: a second backend for
  rendering the observation panels (stats, curves, inspector) **in Bevy** (Text2d +
  Sprite + gizmos), so they appear **in the video** — the egui overlay, for its part,
  is never filmed (§7). The *data* is shared via a common layer mounted in the lib
  (`metrics`: `History` + `sample_history`, `live_stats`,
  `population_curves`/`trait_curves`) → **exactly** the same numbers/curves as egui,
  two plots. **9:16** composition (square arena on top, visualizer at the bottom;
  viewports + a background camera), with **rotation** of the sections (curves ↔
  inspector) at a configurable interval. **Reserved to the `record` binary**
  (`DataVizPlugin`, active by default, target 1080×1920; `--no-hud` → square
  1080×1080; `--hud-interval`). **Not in the windowed build**: bevy_egui renders egui
  through the sim camera, so recomposing the view would break the UI — the editor
  stays 100% egui (cf. memory). **DejaVu Sans** font embedded (`assets/fonts/`, the
  repo's first asset) because the default Bevy font is ASCII-only (accents, gene
  names included). egui panels rearranged **semantically**: *world* on the left,
  *entities* on the right, *scenario + recording* (on a single line) in the top
  strip, *controls + stats* then *curves + inspector* at the bottom; **Space**
  shortcut = play/pause. **Run snapshots removed** (item 13, unused): UI, systems,
  `src/snapshot.rs` and `tests/snapshot.rs` removed entirely.
- **Generic `nutrients` layer — T2 (the principled population bound, §9)**: the
  *resource-limitation* answer to density (Liebig's law of the minimum). A second
  axis, **decoupled** from energy: a per-cell **concentration field**
  (`src/nutrients.rs`, a grid `Resource` outside Law 11) fed by **sources** (a
  separate `sources` category, spawned as **non-`Agent`** entities) and spread by
  **diffusion** into gradients; agents **absorb** it into a per-entity store and
  **spend** it to reproduce. Three genes appended (`nutrient_absorption`,
  `nutrient_capacity`, `offspring_nutrient`), non-mutable by default → RNG-safe,
  pre-T2 scenarios **byte-identical**. **Decoupling**: energy (sun) governs
  *survival*, the nutrient governs *reproduction* only → a missing nutrient stops
  breeding but never kills (the fix to the T1 death-spiral of `minerals.ron`).
  **Design correction** (vs the plan): the child is born with an **empty** store, the
  nutrient is a **consumable** removed from the pool (endowing the child would make it
  circulate down the lineage → unbounded). **Finding**: with immortal plants the
  nutrient bounds the growth *rate*, not the standing crop — a true carrying capacity
  awaits **turnover** (recycling/mortality, deferred). Driver
  `tests/nutrients.rs`/`scenarios/nutrients.ron` (multi-seed: grows ≫ founders,
  bounded, persists; **and** the falsifiable contrast — no sources ⇒ no growth, no
  collapse).
- **Rendering in toggleable layers ("calques")**: everything is a layer (`Layers`
  resource in `visuals.rs`, an egui "Layers" panel). The **agents** are the main
  layer; each **nutrient field** is a *background* heatmap layer (a linear-sampled
  texture, alpha ∝ concentration, behind the agents at `z = -5`), **off by default**.
  All toggleable; the nutrient layers **share** an opacity budget (`N` active ⇒ `1/N`
  each, so 2 ⇒ 50 %). In the **video** too (`record --nutrients`), off by default →
  existing videos unchanged. (The roadmapped "Nutrient-field visualization — layers"
  of §9, done.)
- Tooling: video recording (headless re-render via ffmpeg, defaults 30 fps / 61 s),
  multi-seed test drivers (`predator_prey`, `mlp`, `cohabitation`, `flight`, `flora`,
  `nutrients`, …), clean `clippy`/`fmt`.
- **UI rework — foundation + semantic reorg** (windowed build, sim untouched). The
  five independent docked-panel systems are **merged into one** `panels::dock` that
  builds a single background-layer root `Ui` and adds each panel with
  `Panel::show_inside` (bevy_egui 0.40 `examples/ui.rs`) — clearing the egui-0.34
  **deprecation debt** (`Panel::show(ctx, …)` and `ctx.available_rect()` are gone). The
  central area left free is read once via `available_rect_before_wrap()` into a
  `CentralRect` resource, the **single source of truth** for "where the sim is": the
  camera frames the sim there *and* the picking/drag/delete systems read it (via
  `panels::pointer_over_ui`) to tell a click on the sim from one on a panel — the
  built-in `is_pointer_over_egui()` no longer works under bevy_egui + `show_inside`
  (it needs `root_ui_available_rect`, only set by egui's own `run_ui`, unsettable from
  user code). **Semantic reorg**: video **Export** left the top bar for a floating
  window opened by an Export button (the fragile reverse-order `right_to_left` recorder
  hack is gone); the view **Layers** left the World panel for a top-bar **View** menu
  (view ≠ scenario data); and the three scattered per-panel status strings funnel into
  one `UiStatus` shown once in the bottom bar. *Next (deferred):* collapsible panels +
  presentation mode, pan/zoom + selection cartouche, per-panel polish.
- **Scenario management — document model.** The scattered scenario IO (a combo +
  `⟲ Reload`, a free-text Load path, a silent `💾 Save`) becomes a single **Scenario
  menu** (New / Open ▸ / Revert / Save / Save As) with the current file name and a
  **`*` modified marker** (amber) next to it. *Modified* is derived by comparing the
  config against a `baseline` snapshot (every config type already derives `PartialEq`).
  Destructive navigation (New / Open / Revert) **confirms** before discarding unsaved
  edits, and **Save protects bundled scenarios**: overwriting a file the user did not
  create this session offers *Save a copy* instead of clobbering it (RON serialization
  drops comments / compact form). Fixed a latent bug surfaced by the dirty check:
  `color_edit_button_rgb` round-trips its `[f32; 3]` through HSVA every frame, drifting
  (and persisting) the stored colors — now gated on `response.changed()` (`color_button`).

**Remaining.**

- **P5 — battle (deferred) + scaling**: generational regime (run → score → breed),
  headless parallelized across matches, then weight crossover / NEAT (§9).
- **Nutrients — the food web, then the closed loop (T3, §9)**, in this order: (1)
  **trophic nutrient transfer** — eating carries *nutrient* (not just energy) from prey
  to predator, the prerequisite to both the emergent targeting (Law 8) and a meaningful
  recycling; (2) **recycling** — a dead body returns its nutrient to the field (a
  *conserving* loop), worthwhile only once the nutrient is conserved in biomass (1),
  whereas with a renewable source it is not needed before; per-species absorption +
  **multiple nutrients** (T3 — a 2nd nutrient layer makes the shared-opacity 50/50
  real); GUI editing of sources; and re-balancing the 4 parked grazed-food tests via the
  nutrient layer. NB: recycling ≠ a population **cap** — a flat standing crop is set by
  **turnover** (mortality / a portable `crush`), an independent lever.

---

## 1. Guiding principle

A single **engine** interprets data. The loop is invariant — **perceive → decide →
act** — and what varies from one scenario to another is the configuration, not the
code.

The modularity rests on **one axis with three authors** (who writes the behavior and
the structure?):

| Author | Moment | Decision via… | Body via… |
|---|---|---|---|
| **Engine** | compile-time | systems that interpret the data | components and their effects |
| **Designer** | config-time | deterministic brain (rules) | archetype-editor values |
| **Evolution** | run-time | neural network weights | genes that mutate |

The axis applies twice: to the **decision** and to the **body**.

---

## 2. Contracts (invariants)

Breaking them loses the modularity.

- **Brain and body = a contract**: `normalized floats in → floats out`. The inside
  (neural network, decision tree, FSM) is interchangeable.
- **Storage as an `enum`, not `Box<dyn>`**: static dispatch, clean `serde`,
  exhaustive `match` checked at compile time. Crossover is intra-type (one does not
  cross a NN with an FSM).
- **The body imposes the shape of the brain's I/O.** The genes vary the *magnitudes*
  (vision range, speed) **and**, since the `vision_rays` gene (item 18c), the *number
  of channels* (the visual precision): the MLP's input layer adapts to it at
  reproduction — a first step toward variable topology. The *hidden* topology stays
  fixed at the founder; full NEAT (cf. item 21) is still deferred.
- **Genotype ≠ phenotype**: we mutate the genotype (an inherited description),
  compiled into a living phenotype (Avian components + brain) at spawn. Evolution
  never touches the current physical state.
- **A characteristic = (value, bounds, cost coupling)** — plus, at editing time, a
  **mutable?** facet: is the gene allowed to mutate (drift, pass on selectable
  variation), or does it stay frozen at the founder's value? It is **transmitted in
  both cases**; the flag only governs the mutation (whence *mutable*, not
  *heritable*). Without a cost, everything converges to the maximum and nothing
  emerges; the cost is defined by the scenario, not by the engine.

---

## 3. Single interaction primitive

Eating and attacking are the same **directed interaction**: A reduces a resource of
B, within range/contact.

- **Predation**: an attack that *transfers* the energy to A.
- **Combat**: an attack that *destroys* without transfer.

The engine exposes only **one primitive**. The scenario sets its semantics: the
resource (energy / HP), transfer or not, and the target filter (trophic relation
predator→prey, or enemy→enemy faction). Likewise for perception: the spatial queries
are engine machinery; the scenario only chooses *which* channels become brain inputs.

---

## 4. Scenario contract and evolutionary regimes

A scenario defines:

- **Spawn**: who, where, how many, which factions.
- **Vocabulary**: available actions and sensors.
- **Interaction table**: who acts on whom, targeted resource, transfer or not.
- **Cost couplings**: what each trait costs (vision → metabolism, speed → energy).
- **Conditions**: of death, of end.
- **Evolutionary regime**: see below.

### Regimes as a grid of axes

A regime is not an atom but a point in a grid of two largely independent axes:

- **Axis A — reproduction timing**: *continuous* (in the sim, at death / at a
  threshold) ↔ *batched* (at a generation boundary, outside the sim).
- **Axis B — fitness source**: *implicit / ecological* (emergent from the world) ↔
  *explicit / by score* (computed → selection → reproduction).

| | Implicit fitness | Explicit fitness |
|---|---|---|
| **Continuous repro** | **Natural selection** | steady-state GA |
| **Batched repro** | "seasonal" regime | **Battle** |

The two canonical regimes occupy the diagonal; the off-diagonal cells are valid
regimes. A continuum exists along axis A (*generation gap*). The axes are not
perfectly orthogonal — implicit fitness imposes an ecological selection —, which
makes the two diagonal corners coherent configurations; axis A stays free.

**Architectural guard.** Do not reify `enum Regime { Continuous, Generational }`:
that would freeze the coupling into the type (generality ≠ modularity). Keep two
**separable seams**: "where reproduction lives" (a sim system in continuous ↔ an
outside-sim orchestrator in generational) and "where the fitness comes from"
(emergent ↔ computed). Validity criterion: a third regime must be a *recomposition*
of these pieces, never a special case.

### Coexistence of brain types

1. **Substitution**: swap NN / deterministic per species (free via the contract).
2. **Cohabitation**: the deterministic one serves as a control group (a NN that does
   not beat it has learned nothing) and as scaffolding (validate the pipeline before
   the NNs exist).
3. **Hybridization**: hard-wired reflexes (flee at critical HP) short-circuiting the
   learned layer (subsumption architecture).

---

## 5. Technical stack

| Layer | Choice | Note |
|---|---|---|
| ECS / engine | **Bevy 0.19** | suited to heavy simulations |
| Physics | **Avian 0.7** | Bevy-native; collisions **and** occlusion raycasting |
| HUD / curves | **bevy_egui** | population, trait drift in real time — *native `bevy_ui`/feathers migration attempted & shelved, see §9* |
| Serialization | **serde + RON** | readable archetypes; binary for the snapshots |
| Brain | **homemade** (MLP + mutation/crossover) | ML libs aim at the big GPU network, the opposite of the need |
| Video | **ffmpeg** | fed by re-render (§7) |

**Trade-offs:**

- **Performance > strict determinism**: parallelism enabled (intra- and
  inter-match), no `enhanced-determinism`.
- **Visual occlusion required**: raycasting as the vision mechanism.
- **Fixed timestep**: for solver stability (a variable dt diverges), not for
  determinism.
- **Avian broad-phase** as the neighborhood structure: no homemade spatial hash.
- **Seeded RNG**: to replay an *experiment configuration* and compare parameters, not
  for bit-for-bit reproducibility (abandoned with parallelism).

---

## 6. Execution model: headless ⇄ direct

All the sim logic and the Avian physics live in the fixed-timestep schedule
(`FixedUpdate` / `FixedPostUpdate`), identical with or without a window. Only the
loop driver and the rendering plugins change.

- **Direct**: `DefaultPlugins` (winit drives, renders, presents).
- **Headless**: `ScheduleRunnerPlugin`, no window, no rendering.

> **Invariant: no sim logic in `Update`** (rendering, input, UI only). Otherwise the
> headless diverges from the direct.

**Two clocks**: the sim rate (fixed timestep, **64 Hz** by default) is constant and
independent of the render rate (`Update`, keyed to vsync). Bevy runs the fixed
schedule 0, 1 or several times per frame to catch up with elapsed time.

- **Headless throughput**: drive the schedule manually in a tight loop (until the end
  condition), not via the real-time accumulator → reproducible number of ticks,
  maximum speed.
- **Pause / speed**: `Time<Virtual>::pause()` and `set_relative_speed(x)` (the fixed
  clock follows).
- **Spiral of death**: if a tick exceeds real time, the catch-up stacks up.
  `set_max_delta()` caps the catch-up; to be tuned as the number of entities grows.
- **Generational evolution**: headless matches parallelized across matches, an
  isolated `World` and one seed per match — that is where throughput grows.

---

## 7. Identified difficulties

- **Video**: without determinism, no replay by seed. Default solution: a fresh
  re-render of the best genome (representative, not the exact historical match). Exact
  alternative: log then replay the trajectories.
- **Raycast vision**: a potential bottleneck (N entities × M rays × tick). Avian
  spatial queries, rays/range capped per species, vision treated as a cost to bound
  the drift.
- **Natural selection**: the central calibration point is the **energy economy**.
  Badly calibrated → collapse or explosion; Lotka-Volterra cycles (predator-prey) to
  stabilize.
- **Battle**: the emergent behavior reflects the **fitness function** (reward kills →
  kamikazes; survival → avoidance). Co-evolution of the factions → instability (Red
  Queen).

---

## 8. Implementation order

Principle: build the decoupled foundation first, validate each slice with
deterministic agents (scaffolding), realize one scenario end-to-end before
generalizing. The second scenario of a given type serves as a test: if the
abstraction holds, it is almost entirely configuration.

Three method principles:

- **Generality ≠ modularity**: a general mechanism can be deeply coupled; modularity
  is falsified only against **plurality** (≥ 2 instances per axis).
- **Editor driven by the scenarios**: each brick is born from a real need and proven
  modular; "complete editor" is a result, not a prerequisite.
- **Stub the behavior, never the schema**: a behavior shell (no-op brain) is
  legitimate scaffolding; a data-contract shell freezes the wrong shape — the schema
  shape *is* the abstraction.

Goal: an **experiment platform** measuring what a learned brain brings against a
deterministic control group. Natural selection (continuous regime) is deepened
first; it already carries predation, competition and co-evolution (cf. Avida,
Tierra, Polyworld). The generational regime (battle) is deferred as the final test
of axis A.

### P0 — Foundations (done)

1. Bevy + Avian, rigid circles, collisions, 2D camera; sim in `FixedUpdate` /
   `FixedPostUpdate`.
2. perceive→decide→act loop with a trivial deterministic brain (wandering).
3. Two entry points sharing the same schedule: direct (`DefaultPlugins`) and headless
   (`ScheduleRunnerPlugin`, counting fixed ticks until the end condition).

### P1 — Playable engine: continuous evolutionary loop (done)

4. Placement: manual drag-and-drop + random spawn in number (windowed editor).
5. Archetype editor + RON save/load; archetype (config) / genome (instance)
   distinction.
6. Raycast vision with occlusion (Avian spatial queries); metabolic cost coupled to
   range × rays.
7. Single interaction primitive (predation/combat) + per-species relation table.
8. Scenario #1 — natural selection: metabolism, feeding, death at zero, reseeding.
9. Reproduction + mutation of a parametric genotype → continuous evolutionary loop;
   finite-rate regrowth → carrying capacity (`scenarios/evolution.ron`: stable
   population, gene drift).

### P2 — Interface (done)

Observation and control tooling, entirely in the windowed binary (`Update` / egui).

10. HUD / curves: population per species, normalized trait drift (read-only). Data
    factored into `metrics` and plotted by two backends (egui + native Bevy, cf. §0).
11. Controls: pause, speed 0.5×–8×, single-step, reset (control of `Time<Virtual>`;
    the reset rebuilds the world from `SimConfig`).
12. Agent inspector: genotype, reserve, perception, current action (read-only).
13. Hot runs/scenarios: RON selector + save/load by path, reload without restarting
    the binary. *(The run snapshots, once serialized here, were removed — cf. §0.)*

### P3 — Video capture (done)

14. Headless render → `ffmpeg` (direct pipe of the frames, no intermediate PNG; fresh
    re-render). Recording menu integrated into the windowed build (launches `record`
    as a subprocess). Sim rendering factored (`VisualsPlugin`) shared windowed ⇄
    recorder. **Overlayable native HUD** (stats / curves / inspector in Bevy, 9:16
    composition; `--hud` by default, `--no-hud`) via `DataVizPlugin` + `MetricsPlugin`
    — **specific to `record`** (the windowed build stays egui, cf. §0).

### P4 — Deepened natural selection + evolved intelligence (continuous regime, in progress)

The evolution of intelligence is the frontier of the abstraction *within* natural
selection. The editor grows here, driven by these scenarios — to date: archetype
genes (value/bounds/**mutable**, including the **visual precision** `vision_rays`),
brain **and max reserve per species** (brain selector targeting the selected species
+ functional description, **MLP architecture & graph**), **world parameters** (arena,
food economy, relation table), placement and **deletion** of entities (Delete key).
The inspector, for its part, shows the **MLP in action** (activations), plus the
**genealogy** (generation, age).

15. Generic characteristic editor **(done)**: (value, bounds) + "mutable?" toggle per
    trait — `TRAITS` table + `Mutability` facet (renamed from `Heritability` at item
    18c: the flag governs the mutation, not the heredity), exposed without dedicated
    code by editor/HUD/inspector; reproduction, metabolism and locomotion cost
    migrated to genes — **and** a brain selector, each `Brain` variant exposing its
    own editable parameters (`turn_rate` for wandering, none for the hunter) via a
    *data-carrying* `BrainKind`. The selector edits by *kind* and exposes the
    variant's parameters via an exhaustive `match`: the *heterogeneous* counterpart
    (a brain = its own fields) of the *homogeneous* `TRAITS` table. (The "selector"
    part came after item 16, which provides its 2nd falsifying variant.)
16. Deterministic `Brain::Hunter` **(done)**: a reflex using perception. A **unified
    steering field** where each ray pushes with a weight `attraction·target +
    openness`: the target *attracts* (graded by proximity), a non-target obstacle
    (wall, other entity) is *skirted* without fleeing it — the food is therefore no
    longer avoided like a wall. The "attack on contact" stays the interaction
    primitive (item 7), the hunter only has to come into contact. Required extending
    perception with a **"target" channel** per ray (is the nearest hit a species
    targeted by the relation table?) — the real driver of the schema extension. Brain
    selection by scenario (`BrainKind`, RON: `Wander(turn_rate: …)`/`Hunter`;
    `scenarios/hunt.ron`). A competent control group; makes the perceive→decide→act
    path meaningful and the brain selector falsifiable (2nd `Brain` variant).
    **Remaining**: substitution *per species* (control/learned cohabitation, §4).
17. Co-evolutionary predator-prey **(done)**: `scenarios/predator_prey.ron`, a
    **three-level trophic chain** (plants → prey → predators) where the *same* shared
    `Brain::Hunter` makes a prey a herbivore and a predator a carnivore — the "target"
    channel (item 16) resolves **by the perceiving species** via the relation table,
    so that two chained relations (predator→prey, prey→plant) suffice to distinguish
    the roles. The "editor driven by the scenarios" method played fully: the **pure
    data** version (round-robin → 50% predators) turned out to be a *knife-edge*
    (coexistence for ~2 seeds out of 5, collapse otherwise) — the structural cause
    (forced ratio, no possible pyramid) **gave rise to the only schema growth**: an
    **`agents_per_species`** field (count per species → "prey ≫ predators" pyramid),
    living in `config` + `spawn` (+ `species_cardinality()` for HUD/editor), **zero
    edits to `movement` / `interaction` / `ecology`** and **backward-compatible**
    (empty → the old uniform sharing; no `.ron` to migrate). The archetype stays
    *shared* between species — only the count differs. Calibration (§7): the decisive
    stabilizer of the Lotka-Volterra cycles turned out to be **spatial** (large arena
    = prey refuges + capped harvesting), not a fine tuning; moderate predation and a
    moderate reproduction threshold dampen the oscillation. **Driver**
    `tests/predator_prey` — multi-seed (5 independent worlds), it encodes the
    falsification criterion: (a) *population band* — no lineage extinct or explosive
    over the 2nd half, for all the seeds; (b) *expected drift* — vision **is
    maintained** (the hunter uses it: ~110-290 depending on the seed, founder 170),
    instead of melting toward the lower bound as under wandering (falsifiable contrast
    with `evolution.ron`). **Remaining**: brain substitution *per species*
    (control/learned cohabitation, §4) and the **full archetype per species**
    (distinct founding genes + brain; the founder seam of §9), deferred until a
    scenario requiring distinct *bodies*. A prey's **active flight**, for its part, is
    **done** (item 18e, "threat" channel) — and motivated the scenario's spatial
    recalibration (arena 480 → 560) that synergizes with it.
18a. "Brain per species" seam + brain inheritance **(done)**: the prerequisite that
    items 16 and 17 left in "Remaining" (substitution *per species* — control/learned
    cohabitation, §4). Two seams, falsified with the **existing deterministic** brains
    before the MLP arrives ("stub the behavior, never the schema", §8). (1)
    `brains_per_species` founds a brain per species — modeled on `agents_per_species`
    (item 17), **additive and backward-compatible** (empty → uniform `brain`;
    `brain_of` resolves, falling back to the uniform; zero `.ron` to migrate); the
    archetype (the *body*) stays **shared**, only the brain (the *decision's author*)
    differs. (2) At reproduction, the child **inherits the parent's brain**
    (`Brain::reproduce`) instead of being rebuilt from the global config — otherwise
    the lineages would converge toward the uniform brain; it is the seam that
    neuroevolution (18b) will extend to **mutate the weights**. RNG stream preserved
    (same draws as before → `predator_prey`/`snapshot` unchanged). Editor: brain
    selector **per species** as soon as there are several, + a **functional
    description** of each variant (`BrainKind::description`, the heterogeneous
    counterpart of `name`). **Driver** `tests/cohabitation`
    (`scenarios/cohabitation.ron`, 5 seeds): hunter (competent control) vs wandering
    (naive control), **same body and same economy**, shared food — only the brain
    differs. A three-part criterion: (a) *inheritance invariant* — every descendant
    of species 0 stays a hunter, of species 1 stays a wanderer; (b) *effective
    reproduction* — the hunters grow beyond their founders; (c) *control domination* —
    clear competitive exclusion (~110 hunters against ~1 wanderer), §4 realized: "a
    brain that does not beat the deterministic one has learned nothing".
18b. Homemade MLP + neuroevolution (core) **(done)**: the **learned** brain, in the
    continuous regime, **in substitution per species** (the 18a seam). `Brain::Mlp` —
    a homemade multilayer perceptron (dense `tanh` layers, seeded Xavier init).
    **Inputs** = the normalized `vision`/`target` channels concatenated (`2 ×
    vision_rays`, not the `ray_dirs` geometry); **output** = 2 neurons read as a
    steering vector *in body frame*, rotated to the world by `perception.heading` →
    orientation-equivariant (the network does not learn the absolute orientation).
    **Neuroevolution**: `Brain::reproduce` extended — the child inherits the topology
    and **mutates its weights** (Gaussian perturbation of std-dev `mutation_rate ·
    WEIGHT_STEP`); weight crossover deferred (permutation, §9), mutation-only first.
    RNG stream of non-MLP scenarios preserved (Wander/Hunter do not draw). **Editable
    architecture** (numeric): `BrainKind::Mlp { hidden }` carries the topology of the
    **hidden layers** (number + width); input/output stay *constrained* by the
    contract (hidden topology = a designer choice fixed at the founder, **not
    mutated** — NEAT/variable topology still deferred, §2/item 21). **Driver**
    `tests/mlp` (`scenarios/mlp_brain.ron`, 5 seeds): MLP vs wandering cohabitation,
    **same body and same economy**, shared and limited food — starting from
    **random** weights, the MLP goes from parity (~145/145) to **domination** by
    competitive exclusion (~220 against ~10, wandering nearly extinct) on **each**
    seed — §4 realized for the learned brain. **Finding (§7)**: neuroevolution from
    random, head-to-head, is high-variance (a mediocre initial cohort is excluded
    before learning); the decisive lever is the **diversity of founders** (40/species
    → 3 seeds out of 5; 70 → all 5) — which all the more motivates the generational
    batches of P5. **Remaining**: the **graph visualizations** (18b-viz).
18b-viz. MLP graph visualization **(done)** — purely UI, without any schema change
    (the 18a seam — one brain per agent, an inspector already reading `Brain` —
    welcomes it). A minimal read API on `MlpBrain` (`layer_sizes`, `weight_layers`,
    `layer_weights`, `activations`); a shared drawer `editor::draw_mlp_graph` (one
    column of nodes per layer, edges between columns, via `egui::Painter`). Two uses:
    (a) **editor** — a *structural* preview (neutral nodes) that follows the
    architecture editing; (b) **inspector** (item 12) — the selected agent's network
    **in action**: nodes colored by each neuron's **current activation** (the last
    `think`, `tanh` scale cold<0<warm) and edges tinted by the weight's
    sign/intensity. Item 18 (MLP + neuroevolution) is thus complete; remaining,
    further on (P5/§9), weight crossover + NEAT.
18c. Evolutionary visual precision + genealogy + 1st body per species **(done)**:
    three extensions of the existing machinery, without any new core system. (1)
    **`vision_rays` becomes a gene** (10th `TRAITS` entry, added at the *end* to
    preserve the other traits' RNG stream; stored as `f32`, rounded at the phenotype):
    visual precision varies per individual, mutable and bounded by its
    already-coupled metabolic cost (range × rays). The **MLP's** input layer
    **adapts** to the child's precision at reproduction (`MlpBrain::reproduced`:
    per-block resize `vision|target`, fresh Xavier weights, identity at constant
    precision) — an assumed *breach* in "locked shape" (§2), a first step toward
    variable topology. (2) **Genealogy**: `Generation` (0 at the founder, parent+1 at
    repro) and `Age` (simulated seconds, `ecology::age_agents` in `FixedUpdate`)
    components, captured at the snapshot and shown in the inspector. (3) **Max reserve
    per species** (`reserve_max_per_species`, `reserve_max_of`) — modeled on
    `brains_per_species`, additive/backward-compatible — edited per species; the
    **fill %** stays normalized `[0,1]` (`Reserve::fraction`), hence comparable. The
    first lever of the per-species *body* (§9, "archetype per species"), after the
    count (17) and the brain (18a). `Heritability → Mutability` rename (the flag
    governs the mutation, the gene is transmitted in all cases). The brain editor is
    now **targeted at the selected species**. UX: "Reload into the world" restarts
    **paused** (a frozen new world, to place/edit before launch).
18d. **Archetype-first** (Phase 1 of "everything is an entity") **(done)**: the
    scenario's central data becomes `archetypes: Vec<Archetype>` — each entry is a
    *first-order species* (`name`, `color`, `count`, `radius`, `reserve_max`, `kind`),
    and its **index** is its identity (`Species`). `ArchetypeKind` is an `enum` `Agent
    { genotype, brain, mutable }` / `Food { regen }`: the food is an archetype like
    any other, with its own index → **end of the** agent/food number collision. The
    **mutability becomes per species** (in `Agent`), the **founding genotype too**
    (distinct bodies — resolves the open point "per-species founder fallback" of §9
    for the agents). The parallel vectors (`agents_per_species`, `brains_per_species`,
    `reserve_max_per_species`, `agent_radius_per_species`) and the scattered `food_*`
    fields merge into the archetypes; bounds, `tick_hz`, arena and seed stay global.
    The **relations are addressed by archetype** (dropdown menus in the editor, no
    more bare numbers). The editor creates/duplicates/deletes archetypes and writes
    *directly* into the `SimConfig` (no more copy+sync). Breaking RON schema: all the
    scenarios migrated, and pruned (11 → 7). **Remaining** (Phases 2-3): editor
    finishing touches, then the **evolutionary flora** — `Food` dissolved into an
    archetype with a sessile genotype (the variable `Genotype` lock of §9).

```mermaid
flowchart TB
  subgraph Scenario["Scenario · SimConfig (RON)"]
    World["World: tick_hz · arena · seed"]
    Bounds["Global bounds (per gene)"]
    Archs["archetypes: Vec&lt;Archetype&gt;"]
    Rels["relations (actor/target = archetype index)"]
  end
  subgraph A["Archetype"]
    Meta["name · color · count · radius · reserve_max"]
    Kind{{"kind"}}
    AgentK["Agent { genotype, brain, mutable }"]
    FoodK["Food { regen }"]
    Kind --> AgentK
    Kind --> FoodK
  end
  Archs --> A
  AgentK --> Geno["Genotype (genes)"]
  AgentK --> BrainK["BrainKind: Wander · Hunter · MLP"]
  AgentK --> Mut["Mutability (per species)"]
  A -- "compile ×count (genotype→phenotype)" --> Ent["ECS entity<br/>Species(index) · Reserve · Radius · Brain · …"]
  Bounds -. bounds the mutation .-> Geno
  Ent --> Loop["perceive → decide → act (FixedUpdate)"]
  Rels --> Loop
  Loop -- "reproduction (mutate per Mutability)" --> Ent
```

18e. **Active flight** ("threat" channel) **(done)**: a perception extension, without
    any new core system, that gives a prey the reflex to **flee** its predator. (1)
    **Schema**: a `threat` channel joins `vision`/`target` in [`Perception`], the
    *inverse symmetric* of item 16's "target" channel — it lights up when a ray's
    nearest hit carries a species that can act **on us** (`acts_on(other, us)`), where
    `target` responded to `acts_on(us, other)`. `perceive` reads the hit's species
    **only once** and the directed table decides both directions. (2) **Behavior**:
    `Brain::Hunter` gains a **flight reflex by subsumption** (§4 — a survival reflex
    short-circuits the foraging layer), and not a simple repulsion *added* to the
    field. *Why subsumption*: with N rays, the fan of clear rays sums forward; a
    linear repulsion never overturns that push for a distant threat (one ray against
    the whole field) without an absurd constant. Beyond a **proximity threshold**
    (`FLEE_THRESHOLD`), the prey switches to flight (moves away from threats AND
    obstacles, without attraction); below it, item 16's foraging mode stays
    **strictly intact** — a distant predator does not starve the prey, and the
    scenarios without threats (hunt, cohabitation, MLP) are *bit-for-bit unchanged*.
    As at item 17, the **same** shared brain suffices: the inverse relation makes a
    prey a forager that bolts, an apex predator a pure hunter. (3) **Driver**
    `tests/flight.rs` — the mirror of `tests/hunter.rs`: a hunter prey at the origin,
    an **immobile** predator (a zero-rate scarecrow) straight ahead; we check (a) that
    the predator registers in the prey's "threat" channel, (b) that it moves clearly
    **away** from it. **Recalibration**: flight shifts the predator-prey equilibrium
    (the prey escape better); `predator_prey` recovers a robust coexistence across the
    5 seeds via item 17's **spatial** stabilizer — arena `480 → 560` (refuges for prey
    that flee), raised food regrowth, slightly gentler predation; no engine system
    touched. **Method** ("stub the behavior, never the schema"; validate on the
    control before the learned brain): the deterministic hunter consumed the channel
    first, then the learned brain received it — exactly like `target` (introduced on
    the hunter at item 16, consumed by the MLP at item 18b). This MLP wiring is
    **done** (item 18g).
18f. **Evolutionary flora** (Phase 3a) **(done)**: a *flora* becomes a full-fledged
    entity, **without any new core system** — three extensions of the existing
    machinery. **Lock lifted** by **superset** (the three outcomes of §9 decided: a
    single `Genotype` struct gains the flora genes, the fauna leaves them inert — the
    safest path to bring a *real* flora to life before reifying the fauna/flora split,
    "falsify against ≥2 instances"). (1) **Genes**: `photosynthesis` (energy
    gained/s, passive) and `seed_dispersal` (seeding distance), added at the **end**
    of `TRAITS` and **not mutable by default** → `mutate` does not draw them for the
    existing scenarios: RNG stream **intact**, `predator_prey`/`mlp`/`cohabitation`
    bit-for-bit unchanged. (2) **Mechanics**: `metabolize` integrates photosynthesis
    into the net balance (gain − expenses, clamp `[0,max]` — a no-op for the fauna,
    eating already capping at `max`); `reproduce` seeds at `seed_dispersal` (fallback
    radius × 2.5 when zero → fauna unchanged, same 2 draws); `Brain::Sessile` is the
    trivial brain (no-op, zero throttle — "stub the behavior, never the schema", §8).
    (3) **Self-limitation without a new mechanism**: the carrying capacity **emerges**
    from intraspecific competition expressed by the **interaction primitive** (§3) — a
    Plant→Plant relation *without transfer* (contested light/space) that drains nearby
    neighbors; high density → drain > photosynthesis → seeding stopped / mortality →
    **stable** negative feedback. **Driver** `tests/flora.rs`
    (`scenarios/flora.ron`, 4 seeds): the flora grows ~20× from its founders, stays
    **bounded far from the arena's physical saturation** (competition slows it into a
    spatial wave), and **persists** at a sustained count. An *evolutionary* species:
    `reproduction_threshold` and `seed_dispersal` mutate under competition pressure.
    **Remaining** (Phase 3b): dissolve the special `Food` type (`replenish_food`,
    `FoodSnap`, `spawn_food`) — it is now only the degenerate case of a flora (no-op
    brain, reproduction off).
18g. **Threat wired into the learned brain** (`threat` channel → MLP input)
    **(done)**: the natural next step after the deterministic hunter validated flight
    (item 18e), exactly like `target` (introduced on the hunter at item 16, consumed
    by the MLP at item 18b — the "validate on the control before the learned brain"
    method, §8). **Schema**: the MLP's input layer goes from `vision|target` (`2 ×
    rays`) to `vision|target|threat` (`3 × rays`), reified into a constant
    `MlpBrain::CHANNELS` (= 3) — `input_size`, `input_vector` and the **per-block
    resizing seam** of `MlpBrain::reproduced` (gene `vision_rays`, item 18c) read a
    single source of truth from it (2 blocks → 3, DRY). The learned brain thus
    receives what it needs to **learn** to flee, where the hunter applies a reflex
    hard-wired by subsumption. **RNG-safe**: no non-MLP brain is touched
    (Wander/Hunter/Sessile do not read the input) → `predator_prey`/`cohabitation`/
    `snapshot` **bit-for-bit unchanged**; only the MLP scenarios have a wider network
    (more Xavier draws at construction) — `tests/mlp` **revalidated**, the MLP >
    wandering domination holds across the **5 seeds** despite the widened input (the
    threat channel is zero in `mlp_brain`, with no predator; it adds no signal but
    does no harm). **Driver** unit `mlp_reads_threat_channel`: two perceptions
    identical *except the threat channel* → **different** actions — the falsifiable
    proof, on the learned side, that the channel is no longer ignored (we do not
    prescribe *how* a random network responds to it, only that it responds; fleeing
    **well** is up to selection, like foraging). **Remaining**: an *ecological*
    scenario where an MLP prey must learn flight (Lotka-Volterra calibration, §7) —
    deferred, since the benefit is better measured in generational batches (P5, the
    variance finding of item 18b).
18h. **Phase 3b — dissolution of `Food`** (the tail end of "everything is an entity")
    **(done)**: the special `Food` type is removed. (1) **Flattened schema**:
    `ArchetypeKind` (the `Agent`/`Food` `enum`) disappears — `Archetype` carries
    `genotype`/`brain`/`mutable` *directly*, *every archetype is an agent*. A *food
    source* is a **sessile photosynthetic patch** (`Brain::Sessile`, `photosynthesis
    > 0`, `reproduction_threshold: 0` → fixed count), renewed **in place** instead of
    reappearing elsewhere. Removed: `replenish_food`/`regen`/`FoodRegen`,
    `spawn_food*`, `FoodSnap`, the `Food` component; spawn (populates *all* the
    archetypes), snapshot, runs, editor, visuals and HUD unified on the agent (a
    "source" = an agent with a `Sessile` brain). The **immortality** of a
    photosynthetic patch emerges for free from the `interact → metabolize → reap`
    order (grazed to zero, it regains `photosynthesis·dt` before reaping) → renewable
    energy supply without a faucet. (2) **Conservation flaw revealed**: the
    interaction primitive (§3) **duplicated energy** when several actors drained the
    **same** target in a tick (the clamp bounded the target's *loss*, not the actors'
    cumulative *gain*) — invisible while the foragers were dispersed (old *sensor*
    food reappearing at random), **explosive** as soon as solid fixed-position patches
    clustered them. `interact` switches to **two passes**: we accumulate the *demand*
    per target, then **scale each draw** to the available reserve (`min(1,
    reserve/demand)`) → strict, order-independent conservation. (3) **Body choice**: a
    sessile entity stays a **solid** body (not a *sensor*) — physical exclusion bounds
    a flora's density (spatial carrying capacity); a forager eats it *within range*
    (the interaction range exceeds the sum of the radii), without overlapping it. (4)
    **Migration + recalibration**: breaking RON schema → 6 scenarios + the library
    migrated (flat schema); `cohabitation`/`mlp_brain` **recalibrated** —
    *sparse-and-slow* food (few patches, weak regrowth) so the efficient forager keeps
    the patches depleted and **excludes** the naive one (without the
    disappearance-reappearance of the old model, fixed food would help wandering too
    much). **Drivers**: all green again (`cohabitation` ~4×, `mlp_brain` ≥2× across
    the 5 seeds, `predator_prey` coexists, `flora` unchanged, `snapshot` unified).
    **Remaining**: nothing on this axis — "everything is an entity" is complete (§9).

### P5 — Battle (deferred) + scaling

The generational regime tests axis A: it must enter as a recomposition along the A/B
seam (§4), without touching any core system.

19. Battle scenario — generational regime: run → score → breed → run loop
    (outside-sim orchestrator), explicit fitness via a menu of engine primitives,
    terminal condition, factions (= species + a `transfer: false` relation).
20. Headless parallelized across matches: isolated `World`s, multi-core batch.
21. Reflex/learned hybridization (subsumption); variable topology / NEAT, if a
    morphology with a variable number of sensors proves necessary.

---

## 9. Open technical points

- **A/B regime seam** (§4): in continuous, reproduction is a sim system
  (`ecology::reproduce`, `FixedUpdate`) with implicit fitness; the generational adds
  an outside-sim orchestrator without the continuous system depending on it. No closed
  `enum Regime`.
- **Toroidal arena — deferred until Bevy/Avian support it natively**: a borderless,
  wrapping arena (a local crowd disperses across an edge instead of piling against a
  wall — closer to nature than a hard box) is **wanted but postponed**. A position-only
  wrap was prototyped and **reverted**: Avian's collisions, vision raycasts and
  interaction queries are not seam-aware, so a band at the seam still behaves as an
  edge, and a hand-rolled seam-aware version (periodic boundaries / edge ghost bodies)
  would shadow Avian's broad-phase against §5 ("no homemade spatial structure"). **We
  resume the full toroidal form once a Bevy/Avian release exposes periodic/wrapping
  boundaries correctly and completely.**
- **Population-pressure (density-dependent mortality) — deferred**: a way to bound a
  population by **crowding** (so a flora cannot carpet, and a predator cannot overshoot),
  *without* draining the energy reserve (unlike an intraspecific competition relation,
  which makes a dense flora worthless food). A **`crush`** prototype was built —
  per-species `crush_threshold`, a `FixedLast` system reading Avian's summed contact
  impulse — and **reverted**: the *concept* (physical crowding death) is sound and
  general, but tying the threshold to the **raw contact impulse** makes its meaning
  **non-portable across body sizes** (impulse ∝ mass ∝ radius²; the same value calibrated
  for a r=6 flora is wrong for a r=8 hunter). **We resume it in a portable formulation**
  — e.g. a count of overlapping neighbours, or summed overlap depth — when a stable
  ecosystem actually needs the bound (the predator-prey overshoot that motivates it is
  itself a known wall; cf. the natural-selection work). Until then, mortal flora is
  bounded by **grazing alone**.
- **Generic nutrients layer — the principled population bound (planned, 3 phases)**.
  The *resource-limitation* answer to the density problem above: a population is bounded
  by its **most limiting resource** (Liebig's law of the minimum), not by an artificial
  crowding death. Plants are only sun-limited today (photosynthesis = infinite) → they
  carpet; make them depend on a **finite mineral** and the carrying capacity emerges
  naturally — and, if the mineral **cycles** (dead bodies decompose back into it), a
  closed nutrient loop. More principled than `crush`, and the bottom of the food chain.
  Staged:
  - **Phase 1 — scenario-only prototype (done, `scenarios/minerals.ron`)**: a `Mineral`
    archetype + a `Plant→Mineral` relation, photosynthesis below base metabolism so the
    plant *depends* on the mineral. **Validated** the bound (plants self-limit, ~7 not a
    carpet) but **fragile** (2/3 seeds collapse): lacking the mineral = *death* (energy
    starvation) → spiral. Lesson → the Phase-2 design below: **decouple survival from
    reproduction** (two axes), so a missing nutrient stops reproduction but does not kill.
  - **Phase 2 — a real `nutrients` engine layer, plant food only** (**done, on `main`**;
    `src/nutrients.rs`, `scenarios/nutrients.ron`, `tests/nutrients.rs`). Two axes:
    per-entity **energy** (the existing `Reserve`, sun-fed, governs survival) + a
    **nutrient** axis governing **reproduction**. Pieces:
    - a **concentration field** per nutrient (a grid `Resource`, *not* an entity → the
      "substrate" category, outside Law 11; not a spatial-query structure, so no §5
      conflict), with **diffusion** (it rebalances — the rate is the *local vs global*
      limitation knob);
    - **local sources** (e.g. a submarine volcanic vent) that emit a nutrient into the
      field at their cell → diffusion makes **gradients**, life clusters around sources
      (vent ecosystems / oases — spatial structure for free). **Locked representation**:
      a source is declared in a **separate `sources` config list** (a *distinct category*,
      **not** an archetype) and spawned as a **non-`Agent` entity** — so the whole life
      machinery (every system queries `With<Agent>`) ignores it *by construction*: no
      death, no metabolism, no reproduction, no decision. Intangible (no collider) but
      renderable;
    - a **per-plant nutrient store** (a component, set up now to prepare Phase 3): the
      plant **absorbs** from the local field into its store and **pays the store** to
      reproduce (no nutrient = no child, but it lives on the sun → no death spiral).
    - **deferred — recycling / closed loop**: a dead body returns its nutrients to the
      field at its cell (the realistic biogeochemical cycle, enabled by the Law-11 mortal
      flora). **Decision (2026-06-25): recycling comes *after* the trophic nutrient
      transfer** ("eating carries nutrient", Phase 3 below), not before. *Why:* with a
      **renewable source** the nutrient is a self-sustaining **faucet + drain** (source →
      field → store → spent at reproduction), so nothing needs recycling yet; recycling
      earns its keep only once the nutrient is **conserved in biomass** and flows up the
      chain — a dying body then **leaks** the nutrient it accumulated, and recycling
      closes that leak. **Nuance:** recycling ≠ the population **cap** — a flat standing
      crop is set by **turnover** (mortality / a portable `crush`), an *independent*
      lever; recycling only closes the conservation loop. (Also needs per-species
      absorption first.)
    - **Detailed step-by-step implementation plan:
      [`docs/nutrients-t2-plan.md`](docs/nutrients-t2-plan.md)** — the binding reference
      (records the implementation decisions, incl. the corrections below).
    - **Built (2026-06-25):** `NutrientField` (grid `Resource` + conservative
      `add`/`take` + mass-conserving `diffuse`), `emit → diffuse → absorb` systems
      between `metabolize` and `reproduce`, the 3 genes appended non-mutable (RNG-safe,
      pre-T2 scenarios byte-identical), sources spawned as non-`Agent` entities, and a
      **toggleable heatmap layer** (windowed + `record --nutrients`, cf. §0). Driver
      green multi-seed, with the *no-sources* falsifiable contrast.
    - **Correction vs the plan:** the child is born with an **empty** nutrient store
      (the nutrient is a **consumable** removed from the pool), not endowed with
      `offspring_nutrient` — endowing it lets the nutrient circulate down the lineage so
      it stops limiting → explosion. **Finding:** with immortal plants the nutrient
      bounds the growth *rate* (≈ emission / `offspring_nutrient`), **not** the standing
      crop; a true carrying capacity (a flat equilibrium) needs **turnover** — mortality
      or a portable density death (`crush`), **independent of recycling**. The 4 parked
      grazed-food tests stay `#[ignore]` until re-balanced via this layer.
    - **Decision (2026-06-25) — conservation invariant (to honour eventually):** the T2
      "nutrient is a **consumable destroyed at reproduction**" is an explicit **interim
      simplification**, justified only while the source is a renewable faucet. To term, a
      nutrient must **never be destroyed** at reproduction — the amount paid for a child
      is **conserved**: carried by the child and/or **transformed** into another nutrient,
      but **not annihilated**. Combined with recycling (dead body → field) and the trophic
      transfer (eating → up the chain), this closes a fully **conservative** loop (Law 9
      in spirit: matter is moved or transformed, never created or destroyed). Revisit the
      "child born empty / spent" choice at that point.
  - **Phase 3 — full generic nutrient web** (long-term vision): elementary nutrients
    exist; some species need them, **metabolize and transform** them; downstream species
    need those products (and possibly others). Targeting then becomes **emergent** — an
    entity eats *what contains the nutrients it needs*, replacing the explicit hunt
    `relations` table (a real change to **SIM Law 8**, hence deferred).
  - **Idea — nutrient-driven spontaneous generation**: an archetype could *appear* where
    a nutrient is concentrated (origin-of-life / colonization). Two guardrails before it
    is worth building: it must stay **conservative** (the new body is *built from* the
    consumed nutrients, never free — Law 9), and it **dilutes natural selection** (cheap
    re-emergence lowers the cost of extinction — the project's core pressure), so dose it.
- **Nutrient-field visualization — layers (done)**: a toggleable **heatmap layer** per
  nutrient (the invisible substrate made observable — gradients, sources, depletion
  around clusters), render-only and off by default. Generalized into a small **layer
  system** (`Layers` resource): the agents are the main layer, the nutrient fields are
  background heatmaps, all toggleable, the nutrient layers sharing an opacity budget
  (`N` ⇒ `1/N`). In the windowed build (egui "Layers" panel) **and** the video
  (`record --nutrients`). The 50/50 two-layer case becomes real once T3 adds a 2nd
  nutrient.
- **GUI editing of sources (basic editing done; click-to-place + markers remain)**: the
  World editor now has a **"Nutrients" section** (`editor::nutrient_section`) editing the
  field (resolution, diffusion — "(reset)") and the `sources` list (color, position,
  rate, visual radius) with add/remove, mirroring the relations editor — so sources are
  no longer hand-edited in the RON. **Remaining (polish):** placing a source by
  **click** in the arena (like the archetype drag-and-drop), **discrete source markers**
  (today a source is only visible through its heatmap halo, cf. the layer above), and a
  global **opacity slider** for the nutrient layers (today the budget is split 1/N
  automatically). All render-only niceties, lower priority.
- **GUI — native-Bevy UI migration: ATTEMPTED & SHELVED (2026-06-26).** The plan was
  to move the whole windowed `bevy_egui` interface to **native Bevy retained UI**
  (`bevy_ui` + `bevy_ui_widgets` / **feathers**, both `experimental_` in 0.19) and drop
  `bevy_egui` — for three reasons still valid: kill the third-party version-lockstep
  churn, get **one ECS/render model** (panels as entities), and **unify** with the
  display half that already exists natively in `dataviz.rs`. A real, panel-by-panel
  attempt was carried out on branch **`native-ui-migration`** (spike + panels ported,
  coexisting with egui): **#1** bottom bar → native transport bar + corner stats HUD;
  **#2** top bar → native top toolbar (scenario dropdown *pick = load* + Save/New/Delete,
  recorder Record + collapsible settings), egui `top_bar` removed; **#4** left → native
  **World editor** (layers, world scalars + colors, 16-gene bounds grid, nutrient
  sources, relation table) on one `number_input` pipeline, egui `left_tools` removed; a
  new `scenarios/example.ron` became the windowed default. **Verdict: the native
  feathers/`bevy_ui` windowed UI is not usable in practice** (judged on the running
  build) → the branch was **deleted** and `main` keeps the egui UI. The work is
  recoverable by hash via the reflog: base **`35ddd2f`** (spike + `docs/native-ui-migration-plan.md`,
  which holds the detailed findings), tip **`32983ef`** (shelved #4) — `git checkout <hash>`
  to inspect.
  - **Lessons for the eventual rework** (so it isn't re-derived): (a) **feathers is
    young** — buttons/sliders/checkboxes/number_input/text_input/theme work, but there
    is **no table/grid and no drag-and-drop**, and the BSN scene API (`spawn_scene` +
    `bsn!`, `@SceneComponent`) is the real (raw) idiom (the `*_bundle` ctors are
    deprecated/half-broken). (b) A **docked egui panel is all-or-nothing** — it anchors
    at the screen edge, so a native panel can't coexist beside it; each side panel must
    be migrated wholesale. (c) The **display half (curves + MLP graph) needs polylines
    → a 2nd camera** (as `dataviz` does), which **breaks egui's primary context while
    any egui panel remains** (cf. the single-`Camera2d` constraint) — so the curves can
    only go native **after** egui is fully gone; do the read-only `bottom_panel` **last**.
    (d) The **central-area coupling** (`set_sim_camera` framing the sim in the space left
    by panels) must be recomputed from the native panels' rects, not `ctx.available_rect()`.
    (e) Binding worked via **external state management** (`SimConfig` the single source of
    truth; forward = entity-scoped `ValueChange`/`Activate` observers, reverse = a system
    pushing values back) — clean, but the dense editor (~40 live-bound fields + color +
    dynamic lists) is the **bulk of the cost**, and the result's ergonomics did not carry.
  - Meanwhile the egui deprecations stay carried behind documented `#[allow(deprecated)]`
    (top-level `Panel::show(ctx, …)` in `panels`, `ctx.available_rect()` in
    `main::set_sim_camera` — egui 0.34 offers no replacement for "the area left after
    panels"). Revisit the whole direction later (perhaps once feathers matures, or with a
    different layout that sidesteps the all-or-nothing docked-panel constraint).
- **Known bug — `record --select off --no-hud` crashes (low priority)**: with the HUD
  disabled *and* selection off, `dataviz::draw_viz` still runs and reads
  `Res<Selection>`, which only `SelectionRenderPlugin` inserts (added solely when
  `--select != off`) → "Resource does not exist" panic. Any other combination is fine
  (default `--select eldest` provides it). Fix: gate `draw_viz` on the HUD being enabled,
  or take `Option<Res<Selection>>`. Found while rendering nutrient videos; not yet fixed.
- **Eating / attacking as a *deliberate, costed* action — not automatic on contact
  (planned, touches Law 8)**: today `interaction::interact` fires on *every* actor that
  has a valid target in range — predation is a reflex. The richer model: the **brain
  decides** whether to act (an output of `Action`), and the act **costs** something
  (energy/effort), so attacking/eating becomes a strategic choice weighed against its
  cost — and, with the `nutrients` web, *what* to eat follows from *which nutrients are
  needed*. A real change to the one-primitive semantics (Law 8: the primitive stays, its
  *triggering* moves from automatic-in-range to brain-driven), hence flagged for a
  deliberate decision.
- **Manual headless stepping**: `app.update()` in a tight loop requires `app.finish()`
  then `app.cleanup()` beforehand (Avian inserts resources in `Plugin::finish()`).
  Proven in `tests/containment.rs`.
- **MLP** (item 18b, **done** for the core): a `Brain::Mlp` variant (the enum already
  `serde`) on the `Perception → Action` contract, in the continuous regime,
  substitution **per species** (the 18a seam). Weights mutated in `Brain::reproduce`
  (mutation-only neuroevolution). Graph visualization **done** (18b-viz: structural
  editor + inspector with activations). The **"threat"** channel is now **wired** into
  the input (item 18g): `vision|target|threat` (`3 × rays`, the `MlpBrain::CHANNELS`
  constant), `MlpBrain::reproduced`'s per-block resizing seam extended from 2 to 3
  blocks. **Remaining**: further on, weight crossover + NEAT (P5).
- **Per-`think` allocations of the MLP** (perf, **deferred after P5**):
  `MlpBrain::think` allocates a `Vec<f32>` per layer (`Layer::forward`) plus the input
  vector (`input_vector`) — i.e. `layers + 1` allocations per MLP agent per tick.
  Negligible in the continuous regime (few MLP agents), but significant under P5's
  **massive generational batch** (item 20, parallelized `World`s). **Postponed for
  lack of a clean path**: a *scratch* field on `MlpBrain` would break the invariant
  "state = topology + weights" (`brain.rs` — equality and serialization carry only
  that); and passing a buffer to `think(&mut self, &Perception) -> Action` would
  **blur the contract** `Perception → Action` (§2), forcing `decide` to know the
  variant. To be handled **profiler in hand**, once P5 is in place: a scratch reused
  in `decide` (a fast path specific to the MLP, without leaking into the public
  contract) or a `thread_local`, validated by measurement — not before (premature
  optimization otherwise). The *safe* hot-path optimizations are, for their part,
  **done**: `atan2` memoized in `perceive`, raycast filter and `interact` buffers
  reused via `Local`.
- **Crossover**: parametric (genes) trivial and safe; on NN weights, the permutation
  problem (competing conventions) → deferred with NEAT, mutation-only neuroevolution
  first.
- **Multi-run capture and re-render of the best genome**: relevant once the
  generational selection and the inter-match batch are in place (P5).
- **Founding-value fallback → archetype per species** (items 15, 17, 18c): `SimConfig`
  today carries the archetype values in scattered fields (`max_speed`, `agility`, …)
  that duplicate those of the `Genotype`. Folding them into a single `founder:
  Genotype` would remove the `base`/`set_base` accessors and this duplication; the
  natural next step is a `founder` **per species** (`Vec<Archetype>`), so that
  predator and prey have distinct *bodies*. Three per-species levers are already laid,
  all additive and backward-compatible (parallel vectors, fallback to the uniform):
  the **count** (`agents_per_species`, item 17), the **brain** (`brains_per_species`,
  item 18a) and the **max reserve** (`reserve_max_per_species`, item 18c — the first
  capacity of the per-species *body*). But the *founding genotype* itself (speed,
  vision, …) stays **shared** between species. Folding it (and making it per-species)
  breaks the RON of all the scenarios (top-level fields → nested) → to be done with a
  migration of the versioned `.ron`s, the day a scenario requires distinct bodies.
- **Persistable, reusable species outside the scenario** (user request) — **done (item
  4)**. Archetype-first (item 18d) had already made the species a first-order unit
  *within* the `SimConfig` (an `Archetype` = body + brain + reserve + mutability +
  color); item 4 makes it **serializable and reusable across scenarios**: an
  `Archetype` exports to `species/*.ron` and imports elsewhere. **Forks decided**:
  *scope* = the whole `Archetype` **minus the relations** (inter-species → scenario);
  *files* = `species/*.ron`, one archetype per file; *referencing* = **copy at
  import** (the scenario stays self-contained and reproducible — no `SimConfig` schema
  change, no migration) **with a provenance link** (`Archetype.source`, an `Option`,
  omitted from the RON when absent) opening a **resynchronization** that **preserves
  the local count** (`count`, specific to the scenario). The count therefore stays
  per-scenario; at resync, everything else (body, brain, color, name, mutability)
  comes from the definition. A versioned demonstration species: `species/hunter.ron`.
- **Conservation of the interaction primitive under contention** (item 18h):
  `interact` (§3) duplicates energy if several actors drain the **same** target in a
  tick — the final clamp bounds the target's *loss* but not the cumulative *gain*.
  Latent while the foragers were dispersed; **exposed** by fixed-position sessile food
  (the foragers cluster on it). Fixed in **two passes** (demand per target → scaling
  by available reserve), order-independent. It is the **only** core-system tweak Phase
  3b required.
- **"Everything is an entity" model and evolutionary flora** — **Phases 3a (item 5)
  and 3b (item 18h) done**. The characteristics specific to a living entity live in
  its genotype, not in global rules (§1, *the body via the genes*): *done* for
  reproduction (`reproduction_threshold`, `offspring_energy`, `mutation_rate`) and the
  costs (`base_metabolism`, `move_cost` — `TRAITS` genes, not mutable by default
  because they *are* the selection pressure, §2). Item 5 adds the energy **gain**
  (`photosynthesis`) and the **dispersal** (`seed_dispersal`): a sessile *flora*
  (`Brain::Sessile`) that lives on photosynthesis and reproduces by local seeding is a
  full-fledged entity, and **self-limits** through intraspecific competition
  (Plant→Plant relation *without transfer* — the **interaction primitive** §3, no new
  mechanism).
  - **Lock lifted** — by **superset** (the three outcomes decided): `Genotype` stays
    **one** struct, augmented with the flora genes (inert for the fauna, and vice
    versa). Chosen over the **`Genotype` enum** (`Brain`-style) and the **ECS
    trait-components** because it is the safest path to bring a *real* flora to life —
    reifying the fauna/flora split will only be justified against a **2nd flora**
    ("generality ≠ modularity", §8). An assumed cost: a slightly loose schema (a plant
    carries an inert `max_speed`). RNG-safe (genes not mutable by default → `mutate`
    does not draw them → existing drivers unchanged).
  - **Driver** born from a real scenario (§8): `scenarios/flora.ron` +
    `tests/flora.rs` (the flora grows ~20×, stays bounded far from saturation,
    persists, across 4 seeds).
  - **Subtlety resolved**: spatial seeding *would* have recalibrated the whole economy
    (Lotka-Volterra, §7); self-competition (a **stable** negative feedback) avoids the
    knife-edge — a robust band without fine calibration, unlike the predator-prey
    coupling.
  - **Phase 3b done (item 18h)**: the special `Food` type is **dissolved**.
    `ArchetypeKind` flattened (every archetype = an agent), a *source* being a sessile
    photosynthetic patch without reproduction (renewed in place);
    `replenish_food`/`regen`/`FoodSnap`/`spawn_food*`/the `Food` component removed. A
    decided subtlety: a sessile entity stays a **solid body** (physical exclusion
    bounds a flora's density; a photosynthetic patch is otherwise *immortal* under the
    interact→metabolize→reap order). The dissolution required the **only** core-system
    fix of the phase — the conservation of `interact` under contention (the point
    above) — and the recalibration of `cohabitation`/`mlp_brain` (sparse-and-slow food
    → competitive exclusion holds without the disappearance-reappearance of the old
    model).
- **Harden the two constitutions** (governance, **next up** — drafted 2026-06-22):
  [`CONSTITUTION-SIM.md`](CONSTITUTION-SIM.md) (the inviolable laws of the simulated
  world) and [`CONSTITUTION-DEV.md`](CONSTITUTION-DEV.md) (the rules of development)
  distil the binding core of §1–§8 into two short, stable, citable documents (form:
  `Law/Rule N — statement / Why / Anchored in`), surfaced to every session through
  `CLAUDE.md`. They are a **first cut**; before the codebase grows much further they
  need a deliberate review pass to be *solid for the long term*, so the project does
  not drift: (a) confirm each article is truly inviolable and that its `Anchored in:`
  still holds — a law whose anchor no longer obeys it is a constitutional bug; (b)
  close gaps — open decisions that currently live only here in §9 (the A/B regime
  seam, founder-per-species) are *not yet* law and may deserve to be; (c) settle the
  **single-source** question: trim §1–§4 above to *point* to `CONSTITUTION-SIM.md`
  rather than restate it, so the two can never diverge. **Deferred** until the
  current development work is done (priority is back on features).
- **Cross-scenario archetype library** (future, builds on item 4): today a species
  is reused by **copy at import** (`species/*.ron`, with an `Archetype.source`
  provenance link + resync that preserves the local `count`) — each scenario stays
  self-contained. A later step is a true **shared library** browsable and usable
  across scenarios from inside the editor: a catalog of archetypes one picks from,
  with tracking of which library entries a scenario uses and a way to propagate
  updates. The copy-vs-reference fork (item 4 chose **copy** for self-containment)
  is revisited here **deliberately**, weighing reproducibility against single-source
  upkeep — not flipped by default.
- **Grazed plants cannot die (renewable-trickle artifact)** (open decision): a
  sessile food/plant grazed to `0` in `interact` is topped up by photosynthesis in
  `metabolize` **before** `reap` runs (order `interact → metabolize → reap`), so it
  never disappears — it keeps delivering ~`photosynthesis` per tick, its throughput
  capped at the regrowth rate. This was the **deliberate Phase-3b choice** (persistent
  renewable food → clean competitive exclusion, no "disappear/reappear" faucet), but
  it forbids over-grazing → local extinction (boom-bust). Making plants killable is
  mechanically small (don't let photosynthesis rescue a `0` reserve, or reorder
  `interact → reap`) — the real work is **re-calibration**: the tuned forager
  scenarios (`mlp_brain`, `cohabitation`, `predator_prey`) assume immortal food and
  would collapse, so they need re-tuning + updating their chaos-sensitive tests. Three
  coherent end-states to pick from: (1) **consumable + reproduction** — grazed-to-0
  dies but the population regrows by reseeding (Lotka-Volterra, the `flora.ron` model;
  most realistic); (2) **per-species `renewable` flag** — showcase scenarios stay
  immortal (byte-identical), new plants consumable; (3) **consumable terminal** — dies
  for good, no safety net (simplest, most disruptive). Deferred pending a decision.
