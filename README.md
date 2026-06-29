# teemlab

Evolutionary simulation engine. **One single engine** interprets data; each
simulation (natural selection, battle, …) is a *scenario*. Top-down 2D view,
entities = circles. Single loop: **perceive → decide → act**.

Design and implementation order: [`ROADMAP.md`](ROADMAP.md).

## Status

**Done (P0–P3).**

- **Foundations**: Bevy 0.18 + Avian 0.6, collisions, 2D camera; two entry points
  (windowed / headless) sharing the same fixed-timestep sim schedule.
- **Continuous evolutionary loop**: raycast vision (with metabolic cost), a single
  interaction primitive (predation/combat), energy economy (natural selection),
  reproduction + mutation of a parametric genotype. Scenario = data (RON, partial
  override). `evolution.ron`: stable population, observable gene drift.
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
  *threat* channels of perception), `Sessile` (flora), **`Mlp`** (homemade
  perceptron **learned by neuroevolution**, reading the same *vision/target/threat*
  channels — so it can *learn* to flee —, with an activation graph in the inspector).
  Brain selector in the editor.
- **Pilot scenarios**, all robust across multiple seeds via their drivers:
  `predator_prey` (3-level trophic chain, per-species count, prey that flee),
  `cohabitation` & `mlp_brain` (control vs learned → competitive exclusion).

**"Everything is an entity" (done).** The species (`Archetype`) is the **central**
data of the scenario: body + brain + genes + count, and its index is its identity.
Complete editor — create / duplicate / reorder / delete, **species library**
reusable (`species/*.ron`, import by copy + resynchronization), and all the world
parameters in the UI (including `tick_hz` and the gene bounds). **Evolutionary
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
(`record --nutrients`). Cf. [`ROADMAP.md`](ROADMAP.md) §9 and
[`docs/nutrients-t2-plan.md`](docs/nutrients-t2-plan.md).

**Remaining.** **P5 — battle** (generational regime, the final test of the
abstraction along a clean *A/B seam*); and, on the nutrient axis, the **food web**
(eating carries nutrient) then the **closed loop** (recycling) — cf. §9.

> **Cardinal invariant**: no simulation logic in `Update`. Agency lives in
> `FixedUpdate`, Avian physics in `FixedPostUpdate`; `Update` is reserved for the
> rendering / UI of the windowed binary.

## Architecture

```
src/
  lib.rs          SimPlugin: the shared render-agnostic core.
  config.rs       SimConfig: the scenario (RON) + loading; Archetype (first-order species: body + brain + genes), species import/export; relation table; gene bounds.
  components.rs   Agent body; Vision (raycast); Species/Reserve; Perception (vision/target/threat channels) / Action = the brain's contract; genealogy (Generation/Age).
  brain.rs        Brain (enum, static dispatch): Wander (wandering) · Hunter (hunt + flight) · Sessile (flora) · Mlp (learned, neuroevolution); BrainKind = scenario choice.
  genotype.rs     Heritable Genotype (generic TRAITS table) + mutation; genotype→phenotype compilation (§2).
  nutrients.rs    NutrientField (the substrate: a concentration grid + diffusion, outside Law 11) + Nutrients/Emits + emit/diffuse/absorb systems: the T2 second axis (gates reproduction, not survival).
  movement.rs     perceive / decide / act systems (FixedUpdate, chained).
  interaction.rs  Single interaction primitive (predation / combat / competition), conserved under contention, + relation table.
  ecology.rs      Economy: metabolize (expenses + photosynthesis), die, age, reproduce (local seeding).
  rng.rs          Minimal deterministic PRNG (SplitMix64) + Gaussian draw.
  spawn.rs        Population: arena + agents; spawn_agent (compiles a genotype into a living phenotype).
  main.rs         Windowed binary → `teemlab`: wires the docked panels + frames the sim in the central area (set_sim_camera).
  panels.rs       DOCKED layout of the windowed build: ONE show_inside dock (top: scenario menu · centered transport controls · View · Export — left "Edit": World + Entities — right "Analysis": live stats + inspector — bottom: curves), each region calling its tool module's *_section. User guide: docs/editor.md.
  editor.rs       egui UI (windowed only): the View-menu Layers toggles, the palette (create / duplicate / reorder / delete, drag-and-drop placement, Delete removes), species library (species/*.ron), the archetype editor (body / genes / brain), and the World editor (arena, seed, gene bounds, relations, nutrient field + sources, appearance).
  hud.rs          egui HUD (windowed only): population curves + gene drift (read-only).
  controls.rs     egui controls (windowed only): pause / speed / step / reset (time control; reset rebuilds the world — agents, sources, the nutrient field — and re-applies tick_hz).
  inspector.rs    egui inspector (windowed only): click → genotype / energy / perception / action / MLP graph / genealogy (read-only).
  runs.rs         egui management (windowed only): scenario selector, hot reload, run save/load.
  recorder.rs     egui menu (windowed only): configures and launches the `record` binary as a subprocess.
  metrics.rs      MetricsPlugin: shared metrics (History + sampling) — population / trait curves, live stats; one source for the egui HUD and the native visualizer.
  visuals.rs      VisualsPlugin: sim rendering (mesh, arena, vision) shared windowed ⇄ recorder; toggleable Layers (agents + nutrient heatmaps, shared opacity).
  dataviz.rs      DataVizPlugin: the NATIVE Bevy visualizer (Text2d / Sprite / gizmos) for the VIDEO (stats / curves / inspector, 9:16) — reserved to `record`.
  selection.rs    Selection (the inspected / highlighted agent) + its rendering (ring + vision rays), shared windowed ⇄ recorder (auto-select drives the video).
  bin/headless.rs Headless binary → `headless` (smoke test, no rendering).
  bin/record.rs   Headless recording binary → `record`: renders without a window, pipes frames to ffmpeg; `--nutrients` overlays the nutrient heatmap layer.
scenarios/        Two categories (Open ▸ Examples / Saved); only examples are committed.
  examples/       Curated, committed example scenarios:
    # Numbered by DISCOVERY ORDER (simplest → most complex; the Open ▸ Examples menu
    # sorts by name). Resources first (they underpin every forager scenario), then the
    # evolutionary loop, brains, ecosystems, and the closed nutrient loop as the finale.
    00_empty.ron        Blank canvas (count 0): author from scratch; == SimConfig::empty(); the windowed build's no-argument fallback.
    01_default.ron      The starting template: one default species, kept == SimConfig::default().
    02_nutrients.ron    The nutrient SUBSTRATE (T2): sun-fed plants whose REPRODUCTION is gated by a finite nutrient (Liebig) from sources + diffusion — the resource layer the foragers rely on.
    03_flora.ron        Evolutionary sessile flora: photosynthesis + local seeding, self-limited by intraspecific competition (item 5).
    04_evolution.ron    Natural selection: a WANDER grazer reproduces + mutates → gene drift, on the nutrient-bounded flora.
    05_hunt.ron         The HUNTER brain (target channel): hunters forage the flora oases in a self-regulating ecosystem.
    06_cohabitation.ron Control vs control: Hunter vs Wander on flora oases → the competent brain finds them and excludes the naive one.
    # The MLP learning story, in three scenarios (07 & 09 are GENERATED by `cargo run --bin train`):
    07_mlp_brain.ron    Naive learned brain: a from-random MLP vs Wander → the wanderer out-forages it (the baseline before training).
    08_mlp_train.ron    Training ground: MLPs evolve ALONE on the flora oases; the `train` bin captures an evolved individual.
    09_mlp_evolved.ron  Trained variant in action: the captured MLP vs Wander → it reaches parity (no longer out-foraged). Cf. tests/mlp.rs.
    10_predator_prey.ron 3-level trophic chain (flora oases → prey → predators): count pyramid, shared Hunter brain, prey that flee (threat channel).
    11_factions.ron     COMBAT: two factions wage war (transfer:false — destruction without transfer) while foraging a shared flora.
    12_nutrient_web.ron T3 food web (the finale): the closed loop — source → flora → herbivore (trophic transfer) → death → recycle; watch it in the inspector + heatmap.
  saved/          Your saved scenarios (editor Save / Save As land here); gitignored — not committed.
species/
  examples/       Committed reusable species (library):
    hunter.ron      A generic hunter, importable into a scenario.
    mlp_trained.ron An evolved MLP variant (frozen captured_brain), generated by the `train` bin from mlp_train.ron.
outputs/          Simulation outputs (videos, images…); contents ignored by git.
```

## Development

The environment (Rust toolchain + Bevy's system dependencies) is provided by Nix:

```sh
nix develop            # or: direnv allow  (then automatic)

# Launch the windowed build — the dev shell's `play` command (see the box below):
play                                           # debug, empty arena (the editor's canvas)
play scenarios/examples/04_evolution.ron          # debug, explicit scenario
play --release                                 # release (teemlab AND record in release)
play --release scenarios/examples/03_flora.ron    # profile + explicit scenario

cargo run --bin headless                                   # headless, default scenario
cargo run --bin headless scenarios/examples/01_default.ron    # explicit scenario (1st arg = RON)

# Record a run to video (headless render → ffmpeg); output in outputs/:
cargo run --bin record -- scenarios/examples/04_evolution.ron --out outputs/run.mp4
#   options: --out F  --fps N  --seconds S  --width W  --height H  --nutrients
#   (defaults: 30 fps, 61 s, 1080×1080 — the arena is square)
#   --nutrients overlays the nutrient heatmap layer (e.g. for scenarios/examples/02_nutrients.ron)

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
> Release. Tags are cut on **minor** bumps only; patches still increment
> `Cargo.toml` but ship untagged. The tag is **annotated** and its message is the
> changelog (the description of what changed since the previous tag), which becomes
> the release notes. To run a release: bump `Cargo.toml`, commit, then
> `git tag -a vX.Y.0 -m "…what changed…" && git push origin vX.Y.0`.

> **Launching the windowed build: the `play` command** (provided by the Nix dev
> shell — `flake.nix`, `writeShellScriptBin`, no versioned script). The recording
> menu launches `record` as a subprocess, looked up *next to* the current
> executable. But `cargo run --bin teemlab` compiles ONLY `teemlab`: without a
> `record` built in the same profile, recording fails ("No such file or
> directory"). `play` first does a `cargo build` (which builds *all* the binaries)
> in the chosen profile, then launches the windowed build — so `record` always
> follows `teemlab`, debug as well as release.

The windowed build adds, on top of the sim, the egui tooling as **docked panels**
that frame the central simulation area (cf. `panels.rs`): **scenario + recording** in
the top strip; a **left** column with the **Layers** toggles (agents + nutrient
heatmaps) and the **World** editor (arena, rate, seed, gene bounds, relation table,
**nutrient field + sources**); a **right** column with the **archetype** palette
(drag-and-drop to place, **Delete** to remove the entity under the cursor) and the
editor of the selected archetype; a **bottom** strip with controls + stats, then the
HUD curves and the agent inspector. The panels *reserve* the edges, so the sim is
always framed and fully visible in the center. All this tooling lives outside
`FixedUpdate` (rendering / UI); the headless build embeds none of it.
