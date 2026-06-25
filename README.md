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
  panels.rs       DOCKED layout of the windowed build: fixed egui panels (top: scenario + recording · left: Layers + World · right: archetypes + editor · bottom: controls/stats then curves/inspector), each calling its tool module's *_section.
  editor.rs       egui UI (windowed only): Layers toggles, palette (create / duplicate / reorder / delete, drag-and-drop placement, Delete removes), species library (species/*.ron), World editor (arena, tick_hz, gene bounds, relations, nutrient field + sources).
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
scenarios/
  default.ron     Default scenario, all fields documented.
  empty.ron       Empty arena: the editor's canvas (no-argument fallback of the windowed build).
  evolution.ron   Continuous evolutionary loop: reproduction + gene mutation (wandering brains).
  hunt.ron        Hunter brains on a food source: the competent control group (item 16).
  cohabitation.ron     Competent control (Hunter) vs naive (wandering), same body: competitive exclusion (item 18a).
  mlp_brain.ron        LEARNED brain (MLP) vs wandering: dominates starting from random weights (item 18b).
  predator_prey.ron    3-level trophic chain (plants → prey → predators): pyramid
                  by counts, Hunter brains, prey that flee (items 17, 18e).
  flora.ron       Self-limited sessile flora: photosynthesis + local seeding + competition (item 5, Phase 3a).
  nutrients.ron   T2 nutrient layer: sun-fed plants whose REPRODUCTION is gated by a finite nutrient emitted by sources (Liebig); grows around the sources, no death spiral.
  minerals.ron    T1 nutrient prototype (scenario-only): plants depend on a finite mineral — validated the bound but fragile (kept for reference).
species/
  hunter.ron      Reusable species (library): a generic hunter, importable into a scenario.
outputs/          Simulation outputs (videos, images…); contents ignored by git.
```

## Development

The environment (Rust toolchain + Bevy's system dependencies) is provided by Nix:

```sh
nix develop            # or: direnv allow  (then automatic)

# Launch the windowed build — the dev shell's `play` command (see the box below):
play                                  # debug, empty arena (the editor's canvas)
play scenarios/evolution.ron          # debug, explicit scenario
play --release                        # release (teemlab AND record in release)
play --release scenarios/flora.ron    # profile + explicit scenario

cargo run --bin headless                          # headless, default scenario
cargo run --bin headless scenarios/default.ron    # explicit scenario (1st arg = RON)

# Record a run to video (headless render → ffmpeg); output in outputs/:
cargo run --bin record -- scenarios/evolution.ron --out outputs/run.mp4
#   options: --out F  --fps N  --seconds S  --width W  --height H  --nutrients
#   (defaults: 30 fps, 61 s, 1080×1080 — the arena is square)
#   --nutrients overlays the nutrient heatmap layer (e.g. for scenarios/nutrients.ron)

cargo test                            # unit tests + multi-seed drivers + snapshot/containment
cargo fmt                             # formatting — default rustfmt is authoritative
cargo clippy --all-targets            # lint — the tree is kept at zero warnings
```

> **Format convention.** We follow **cargo's formatter** (`cargo fmt`, default
> rustfmt): no `rustfmt.toml`, the tool decides. Every commit must leave
> `cargo fmt --check` clean (and `cargo clippy --all-targets` warning-free). We
> therefore format *before* committing rather than aligning by hand — layout is not
> a review battleground.

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
