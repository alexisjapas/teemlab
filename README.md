# teemlab

Evolutionary simulation engine. **One single engine** interprets data; each
simulation (natural selection, battle, …) is a *scenario*. Top-down 2D view,
entities = circles. Single loop: **perceive → decide → act**.

Design and implementation order: [`ROADMAP.md`](ROADMAP.md).

## Status

**Done (P0–P3).**

- **Foundations**: Bevy 0.19 + Avian 0.7, collisions, 2D camera; two entry points
  (windowed / headless) sharing the same fixed-timestep sim schedule.
- **Continuous evolutionary loop**: raycast vision (with metabolic cost), a single
  interaction primitive (predation/combat), energy economy (natural selection),
  reproduction + mutation of a parametric genotype. Scenario = data (RON, partial
  override). `04_selection`: stable population, observable gene drift under selection.
- **Interface** (windowed binary, egui): HUD curves, pause/speed/step/reset
  controls, agent inspector, hot scenario reload, run snapshot.
- **Video capture**: headless `record` render → `ffmpeg` (fresh re-render),
  integrated recording menu.

**P4 — deepened natural selection + evolved intelligence (done).** Continuous regime.

- **Generic genes**: a `TRAITS` table (value, bounds, *mutable?* facet **per
  species**), exposed without dedicated code by the editor / the HUD / the inspector.
  Reproduction, metabolism, locomotion, **visual precision** (`vision_rays`) and
  **photosynthesis / dispersal** (flora) are genes; genealogy (generation, age) in
  the inspector.
- **Brains** (`Brain`, a statically-dispatched enum), **per species** and
  **inherited** at reproduction: `Wander` (naive control), `Hunter` (competent
  control — charge toward the perceived target **and flee threats**: the *target* /
  *threat* channels of perception), `Grazer` (a hunter that eats only when hungry —
  the deterministic **restraint** control), `Sessile` (flora), **`Mlp`** (homemade
  perceptron **learned by neuroevolution**, reading the same *vision/target/threat*
  channels — so it can *learn* to flee —, with an activation graph in the inspector).
  Brain selector in the editor.
- **Pilot scenarios**, all robust across multiple seeds via their drivers (`tests/*`):
  `predator_prey` (three trophic levels, per-species count, prey that flee — now
  `07_foodweb`), `cohabitation` (competent vs naive control → competitive exclusion) &
  `mlp` (learned vs wander → viable parity, on `06_learning`).

**"Everything is an entity" (done).** The species (`Archetype`) is the **central**
data of the scenario: body + brain + genes + count, and its index is its identity.
Complete editor — create / duplicate / reorder / delete, **species library**
reusable (`species/*.ron`, import by copy + resynchronization), and the world
parameters in the UI (including the gene bounds; `tick_hz` stays a scenario-file
parameter, re-applied on Reset). **Evolutionary
flora**: a sessile plant lives on photosynthesis, seeds itself locally and
self-limits through intraspecific competition — the interaction primitive reused,
without any new mechanism. And since **Phase 3b, the special `Food` type is
dissolved**: only `Archetype` (an agent) remains, a *food source* being a **sessile
photosynthetic** patch without reproduction — renewable in place, no more
`replenish_food` faucet. Along the way, the interaction primitive **conserves
energy under contention** (N foragers on a single patch share its reserve, instead
of duplicating it).

**Generic `nutrients` layer + layered visualization (done — T2).** A second,
**decoupled** resource axis bounds *reproduction* (energy from the sun still governs
*survival*): a per-cell **concentration field** (the "substrate", outside the agents
and outside Law 11) fed by emission **sources** and spread by **diffusion** into
gradients; a plant **absorbs** it and **spends** it to breed — no nutrient ⇒ no
offspring, but it does **not** die (no death spiral, the fix to the T1 prototype).
The renderer becomes a set of toggleable **layers** ("calques"): the agents (main
layer) over the nutrient **heatmaps** (background, off by default, sharing an opacity
budget) — in the windowed build (a "Layers" panel) **and** in the video
(`record --components`). Cf. [`ROADMAP.md`](ROADMAP.md) §9 and
[`docs/nutrients-t2-plan.md`](docs/nutrients-t2-plan.md). The **food web (T3, done)** then
closes the loop: **eating carries the nutrient up the chain** (the interaction primitive
transfers a biomass-proportional share on predation) and a **dying body recycles** its
store back into the field — the nutrient now cycles source → field → plant → forager →
death → field, conservatively. **Since then**, a **metabolic** coupling
(`CostLaw::metabolic_cost`, the current producer economy) makes photosynthesis itself
CONSUME the nutrient (Liebig's law of the minimum), so it gates *survival* too: a crowd
around a vent draws the field down and the excess starves — a clean, density-dependent
**carrying capacity** with real turnover (`02_meadow`), replacing an earlier jostle-cost
artefact.

**P5 — generational regime: breeding, battle & co-evolution (done).** The second canonical
regime of the *A/B seam* (§4) — batched reproduction × explicit fitness — as a
**recomposition**, not a reified `enum Regime`. An **outside-sim orchestrator** (`breeding`)
breeds *between* matches while each match stays the byte-identical sim: per generation it
runs a **cohort of headless matches** (parallelized across cores, ~5×), **scores** each by
an explicit `Fitness` (`BestEvolved` / `Population` / **`Dominance`** — combat), **selects**
the top survivors and **re-seeds** them as the next cohort's founders. Two faces: a headless
**`breed` bin** (a generator that captures the best genome into the catalog) and a **windowed
dashboard** (Run/Stop + progress, a fitness-vs-generation curve, a leaderboard with the
genome's MLP graph + Save to library). Carrier: **`09_breeding`** (breed a forager MLP on
the oasis, scored by `Population`). The **battle** (`Dominance`) and **co-evolution** (Red
Queen, `scored_species: [0, 1]`) fitnesses stay in the engine, but the redesigned example
set no longer ships a combat/faction carrier. Cf.
[`docs/p5-breeding-plan.md`](docs/p5-breeding-plan.md).

**Remaining.** P5 **polish** (a live match spectator, Pause/Step) and **weight crossover /
NEAT** (item 21 — the last learned-evolution piece); the nutrient axis's **T3 refinements**
(per-species absorption, multiple nutrients, a conservation invariant at reproduction);
editor long-tail (library management, catalog metadata). Cf. [`ROADMAP.md`](ROADMAP.md)
§0/§8/§9.

**Near-term orientation.** The near-term goal is **rich, non-collapsing** ecosystems (with a
downstream *science of collapse factors*). The **cognitive substrate** that makes behavioural
*restraint* expressible — **proprioception** + **deliberate, costed eating** — is **built**,
and restraint itself is now **demonstrated** (`05_restraint` — greed grazes harder). The
**component-emission** substrate (agent → environment: corpses/detritus, waste, toxicity,
communication) is wired — the `affect` verb is covered by `tests/affect.rs` — but a *robust
inter-species toxin* is **deferred**: the metabolic economy hardcodes one nutrient, so
producers segregate and a mobile victim self-selects out of the toxic patches, leaving no
stationary co-located victim to poison (see ROADMAP §0). Synthesis:
[`docs/persistent-ecosystems.md`](docs/persistent-ecosystems.md).

> **Cardinal invariant**: no simulation logic in `Update`. Agency lives in
> `FixedUpdate`, Avian physics in `FixedPostUpdate`; `Update` is reserved for the
> rendering / UI of the windowed binary.

## Architecture

```
src/
  lib.rs          SimPlugin: the shared render-agnostic core.
  config.rs       SimConfig: the scenario (RON) + loading; Archetype (first-order species: body + brain + genes), species import/export; relation table + components + the FieldRelation table (agent↔component: absorb/emit/sense/…); gene bounds.
  components.rs   Agent body; Vision (raycast); Species/Reserve; Perception (vision/target/threat + proprioceptive self_state + sensed field_state channels) / Action (steering + eat/attack intent) = the brain's contract; genealogy (Generation/Age).
  brain.rs        Brain (enum, static dispatch): Wander (wandering) · Hunter (hunt + flight) · Grazer (hunger-gated hunter — restraint) · Sessile (flora) · Mlp (learned, neuroevolution); BrainKind = scenario choice.
  genotype.rs     Heritable Genotype (generic TRAITS table) + mutation; genotype→phenotype compilation (§2).
  nutrients.rs    Component fields (Field + decay, in a Fields vec, outside Law 11) fed by sources AND agent emission (Emits); Nutrients store; emit/diffuse/decay/absorb systems + the sensed field_state channel — the substrate for nutrients, pheromones and toxins (semantics from the FieldRelation table).
  movement.rs     perceive / decide / act systems (FixedUpdate, chained).
  interaction.rs  Single interaction primitive (predation / combat / competition), conserved under contention, + relation table.
  ecology.rs      Economy: metabolize (expenses + photosynthesis), die, age, reproduce (local seeding).
  breeding.rs     Generational regime (P5): the outside-sim Orchestrator (run → score → breed over cohorts of headless matches, parallelized across cores) + Fitness (BestEvolved / Population / Dominance) + per-faction selection. The inner match stays the byte-identical sim.
  rng.rs          Minimal deterministic PRNG (SplitMix64) + Gaussian draw.
  spawn.rs        Population: arena + agents; spawn_agent (compiles a genotype into a living phenotype).
  main.rs         Windowed binary → `teemlab`: wires the docked panels + frames the sim in the central area (set_sim_camera).
  panels.rs       DOCKED layout of the windowed build: ONE show_inside dock (top: scenario menu · centered transport controls · View · Help · Export — left "Edit": World + Entities — right "Analysis": live stats + inspector — bottom: curves), each region calling its tool module's *_section. Side panels RESIZABLE within a range that always reserves a minimum sim width; the archetype editor opens a second left column on a wide window and folds into the left panel (single column) on a narrow one (cf. layout.rs). Also paints the themed central overlay (run time, a Paused chip, an empty-arena hint) and the shortcuts cheatsheet. User guide: docs/editor.md.
  layout.rs       Pure layout math (windowed only): the side-panel width ranges (min-central guarantee) and the two-/single-column mode of the left region, with hysteresis. Unit-tested; panels.rs is a thin caller.
  theme.rs        Windowed-UI theme: semantic color tokens (one accent, an ink ramp, the perception/MLP encodings) + the global egui Style, installed once at startup. Every color resolves here.
  keymap.rs       Single source of truth for the keyboard/mouse bindings: the input handlers, the button tooltips and the `?` cheatsheet all read one table, so they can't drift.
  editor.rs       egui UI (windowed only): the View-menu Layers toggles, the palette (create / duplicate / reorder / delete, drag-and-drop placement, Delete removes), species library (species/*.ron), the archetype editor (body / genes / brain), and the World editor (arena, seed, gene bounds, relations, nutrient field + sources, appearance).
  hud.rs          egui HUD (windowed only): population curves + gene drift (read-only), composed over the shared plot widget.
  plot.rs         Shared plot widget (windowed only): the homemade time-series plotter with autoscale, round grid steps and label-sized margins; reused by the HUD curves and the breeding dashboard.
  controls.rs     egui controls (windowed only): pause / speed / step / reset (time control; reset rebuilds the world — agents, sources, the nutrient field — and re-applies tick_hz).
  inspector.rs    egui inspector (windowed only): click → genotype / energy / perception / action / MLP graph / genealogy (read-only).
  runs.rs         egui management (windowed only): scenario selector, hot reload, run save/load (modal confirm / Save-As dialogs).
  recorder.rs     egui menu (windowed only): configures and launches the `record` binary as a subprocess.
  dashboard.rs    egui breeding dashboard (windowed only, P5): drives the generational Orchestrator on a BACKGROUND thread (so the render loop stays responsive); a floating window with Run/Stop + progress, a fitness-vs-generation curve and a PER-FACTION leaderboard (inspect a genome's MLP graph + Save to library). Requires a scenario with a `batch`; toggled from the top-bar Breeding button.
  metrics.rs      MetricsPlugin: shared metrics (History + sampling) — population / trait curves, live stats; one source for the egui HUD and the native visualizer.
  visuals.rs      VisualsPlugin: sim rendering (mesh, arena, vision) shared windowed ⇄ recorder; toggleable Layers (agents + component heatmaps, shared opacity).
  dataviz.rs      DataVizPlugin: the NATIVE Bevy visualizer (Text2d / Sprite / gizmos) for the VIDEO (stats / curves / inspector, 9:16) — reserved to `record`.
  selection.rs    Selection (the inspected / highlighted agent) + its rendering (ring + vision rays), shared windowed ⇄ recorder (auto-select drives the video).
  bin/headless.rs Headless binary → `headless` (smoke test, no rendering).
  bin/record.rs   Headless recording binary → `record`: renders without a window, pipes frames to ffmpeg; `--components` overlays the component heatmap layer.
  bin/sweep.rs    Headless `sweep`: runs a scenario many times and scores each final world by biodiversity (a seed or parameter sweep) — the search for a coexistence band.
  bin/train.rs    Headless `train` (generator): trains an MLP on the oasis flora, captures the best brain seen over the whole run (peak generation, before the living-food population fades), and writes the evolved variant + the 06_learning showcase (the trained MLP vs a wander control).
  bin/breed.rs    Headless `breed` (generator, P5): drives the generational Orchestrator on a scenario's `batch`, prints fitness per generation per faction, captures the best genome into the catalog (species/saved/).
scenarios/        Two categories (Open ▸ Examples / Saved); only examples are committed.
  examples/       Curated, committed example scenarios:
    # A concise, PROGRESSIVE set (simplest → most complex; the Open ▸ Examples menu
    # sorts by name), map size scaled to each: the loop, the producer economy, emergent
    # trophic levels, structure, learning, and the generational breed regime.
    01_drift.ron        Bare loop + allometric mortality: immobile discs drift and die by size — perceive → decide → act with nothing else.
    02_meadow.ron       Producers on a scarce nutrient: sessile photosynthesisers whose photosynthesis CONSUMES a diffusing nutrient (Liebig, `metabolic_cost`) → a real carrying capacity with turnover, not a carpet. Cf. tests/flora.rs, tests/nutrients.rs.
    03_grazing.ron      Emergent grazing: a Hunter eats the reeds because the engine COMPUTES it can (size dominance + digestibility, Law 8), carrying the nutrient up the chain; two levels coexist and oscillate.
    04_selection.ron    Natural selection of a priced trait: WANDERERS never act on vision, so selection melts their (costed) eyes down generation by generation while the traits that pay hold.
    05_restraint.ron    Restraint + the commons: a grazer's hunger threshold governs how hard it grazes a shared producer stock (greed grazes harder). Cf. tests/restraint.rs.
    # GENERATED by `cargo run --bin train` — do not hand-edit; re-run to regenerate:
    06_learning.ron     Learning: an evolved MLP forager (frozen `captured_brain`) vs a WANDER control on shared oases — neuroevolution reaching viable parity. Cf. tests/mlp.rs.
    07_foodweb.ron      Three emergent trophic levels from one rule: flora → herbivore → carnivore, the whole web derived from size + digestibility; prey flee (threat channel). Cf. tests/predator_prey.rs.
    08_reef.ron         Space & structure, SIZE-SELECTIVE: rock rings leave ~18 px gaps a small herbivore threads but a large omnivore cannot — refuge gardens the predator is walled out of (emergent, from geometry). Plus anchored kelp + uprooting/grazing turnover (detritus). Cf. tests/reef.rs.
    # The GENERATIONAL regime (P5) — a GENERATOR, not continuous: carries a `batch` block; run with the `breed` bin (or the windowed dashboard).
    09_breeding.ron     Run → score → breed: a cohort of headless matches per generation, scored by standing biomass (Population); the best survivors re-seed the next cohort's founders.
  saved/          Your saved scenarios (editor Save / Save As land here); gitignored — not committed.
species/
  examples/       Committed reusable species (library):
    hunter.ron      A generic hunter, importable into a scenario.
    mlp_trained.ron An evolved MLP variant (frozen captured_brain), generated by the `train` bin.
outputs/          Simulation outputs (videos, images…); contents ignored by git.
```

## Development

The environment (Rust toolchain + Bevy's system dependencies) is provided by Nix:

```sh
nix develop            # or: direnv allow  (then automatic)

# Launch the windowed build — the dev shell's `play` command (see the box below):
play                                           # debug, empty arena (the editor's canvas)
play scenarios/examples/04_selection.ron          # debug, explicit scenario
play --release                                 # release (teemlab AND record in release)
play --release scenarios/examples/02_meadow.ron   # profile + explicit scenario

cargo run --bin headless                                   # headless, default scenario
cargo run --bin headless scenarios/examples/01_drift.ron      # explicit scenario (1st arg = RON)

# Record a run to video (headless render → ffmpeg); output in outputs/:
cargo run --bin record -- scenarios/examples/03_grazing.ron --out outputs/run.mp4
#   options: --out F  --fps N  --seconds S  --width W  --height H  --components
#   (defaults: 30 fps, 61 s, 1080×1080 — the arena is square)
#   --components overlays the component heatmap layer (e.g. for scenarios/examples/02_meadow.ron)

# Generational regime (P5) + dev generators (headless; the breeding one needs a `batch`):
cargo run --bin breed -- scenarios/examples/09_breeding.ron [generations]    # run → score → breed; captures the best genome into species/saved/
cargo run --bin train                                                        # regenerate the 06_learning showcase (+ the catalog variant)
cargo run --bin sweep -- scenarios/examples/07_foodweb.ron                   # biodiversity sweep (seed / parameter) — search a coexistence band

cargo test                            # unit tests + multi-seed drivers + snapshot/containment
cargo fmt                             # formatting — default rustfmt is authoritative
cargo clippy --all-targets            # lint — the tree is kept at zero warnings

cargo bench                           # throughput benchmark — ticks/sec per scenario
#   compare two versions on the SAME machine (the deterministic sim makes it sound):
#     git checkout <old> && cargo bench -- --save-baseline old
#     git checkout <new> && cargo bench -- --baseline old    # prints the % change
flame [scenario.ron]                  # flamegraph of the headless sim → outputs/flamegraph.svg
#   TEEMLAB_TICKS=N sets run length; perf may need:
#     sudo sysctl -w kernel.perf_event_paranoid=-1
```

> **Measuring performance.** `cargo bench` (`benches/throughput.rs`) is the
> version-to-version **comparator**: it steps representative scenarios headless and
> reports ticks/sec. Because the sim is deterministic (seed + tick count ⇒ identical
> work), a Criterion baseline diff is a *real* perf delta, not run-to-run noise —
> the right way to confirm a `perf:` change actually paid off. `flame` is the
> complementary **profiler** (cargo-flamegraph + perf on the headless binary): it
> shows *where* the time goes, to decide what to optimize next.

> **Format convention.** We follow **cargo's formatter** (`cargo fmt`, default
> rustfmt): no `rustfmt.toml`, the tool decides. Every commit must leave
> `cargo fmt --check` clean (and `cargo clippy --all-targets` warning-free). We
> therefore format *before* committing rather than aligning by hand — layout is not
> a review battleground.

> **Releases (CI).** Pushing a `v<major>.<minor>.<patch>` tag (matching the
> `Cargo.toml` version — cf. CONSTITUTION-DEV Rule 11) triggers
> `.github/workflows/release.yml`: it builds the whole workshop (`teemlab`,
> `record`, `headless`, `sweep`) under the `dist` profile (fat LTO, single codegen
> unit — runtime-perf tuned) for **Linux x86_64**, **Windows x86_64** (both with an
> `x86-64-v3` CPU floor) and **macOS arm64**, archives each with the data read at
> launch (`assets/`, `scenarios/`, `species/`), and publishes them as a GitHub
> Release. A tag is cut **on explicit request** (any version, a patch included) or
> **before a minor/major bump** (tag the outgoing version first if it isn't already);
> a patch you don't release stays in `Cargo.toml` untagged. The tag is **annotated**
> and its message is the changelog (the description of what changed since the previous
> tag), which becomes the release notes. To run a release: bump `Cargo.toml`, commit,
> then `git tag -a vX.Y.Z -m "…what changed…" && git push origin vX.Y.Z`.
>
> **Recording needs `ffmpeg`.** The archives deliberately do **not** bundle it:
> `record` only spawns `ffmpeg` as a separate process, so it stays an *external*
> runtime dependency and its GPL terms never reach the tree (the dev shell provides
> it via `flake.nix`). To record from a packaged build, install `ffmpeg` (so it is
> on the `PATH`), or drop an `ffmpeg` binary next to the executables, or point
> `TEEMLAB_FFMPEG` at its path. Without it, `record` exits with a message saying so.

> **Launching the windowed build: the `play` command** (provided by the Nix dev
> shell — `flake.nix`, `writeShellScriptBin`, no versioned script). The recording
> menu launches `record` as a subprocess, looked up *next to* the current
> executable. But `cargo run --bin teemlab` compiles ONLY `teemlab`: without a
> `record` built in the same profile, recording fails ("No such file or
> directory"). `play` first does a `cargo build` (which builds *all* the binaries)
> in the chosen profile, then launches the windowed build — so `record` always
> follows `teemlab`, debug as well as release.

The windowed build adds, on top of the sim, the egui tooling as **docked panels**
that frame the central simulation area (cf. `panels.rs`): **scenario menu · centered
transport · View · Help · Export** in the top strip; a **left** column with the
**World** editor (arena, rate, seed, gene bounds, relation table, **nutrient field +
sources**) and the **archetype** palette (drag-and-drop to place, **Delete** to remove
the entity under the cursor); a **right** "Analysis" column with live stats and the
agent inspector; a **bottom** strip with the HUD curves. Editing an archetype opens its
editor as a second left column (or, on a narrow window, in place of the list). The side
panels are **resizable**, always reserving a minimum width for the sim, so the panels
*reserve* the edges and the simulation stays framed and fully visible in the center.
The **View** menu holds the layer toggles (agents + component heatmaps); the **Help**
menu the inline-help switch and a keyboard-shortcuts cheatsheet (`?`). A single theme
(`theme.rs`) and one keybinding table (`keymap.rs`) keep the look and the shortcuts
consistent. All this tooling lives outside `FixedUpdate` (rendering / UI); the headless
build embeds none of it.

## License

teemlab is dual-licensed under either of

- **MIT license** ([LICENSE-MIT](LICENSE-MIT)), or
- **Apache License, Version 2.0** ([LICENSE-APACHE](LICENSE-APACHE)),

at your option. This is the conventional Rust-ecosystem dual license, matching the
dependencies.

The bundled **fonts** (`assets/fonts/`) keep their own permissive licenses, shipped
alongside the font data: Inter and Departure Mono under the **SIL Open Font License
1.1**, Phosphor under the **MIT license**, and DejaVu Sans under the **Bitstream Vera /
public-domain** terms. Every release archive also includes a generated
**`THIRD-PARTY-LICENSES.html`** reproducing the license notices of the statically
linked dependencies (`cargo about`, `about.toml`); the whole dependency tree is
permissive (MIT / Apache-2.0 / BSD / ISC / Zlib / …), with no copyleft obligation.

Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual-licensed as above, without any additional terms or conditions.
