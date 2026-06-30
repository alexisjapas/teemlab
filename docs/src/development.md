# Building & development

teemlab is a Rust project (edition 2024) built on Bevy 0.19 and Avian 0.7. The
[Getting started](./getting-started.md) page covers the everyday commands; this page is
the contributor's view — the module map, the testing philosophy, and the release flow.

## The workshop

`cargo build` produces several binaries:

| Binary     | What it is |
| ---------- | ---------- |
| `teemlab`  | the windowed build — the [editor](./editor.md) + the simulation. |
| `headless` | the same sim with no window or rendering (smoke test, profiling). |
| `record`   | headless render → `ffmpeg` ([video capture](./recording.md)). |
| `sweep`    | a multi-seed parameter sweeper for tuning scenarios. |
| `train`    | a one-off generator: evolves an MLP and writes the `mlp_*` artifacts. |
| `breed`    | the **generational regime** (P5): `run → score → breed` over a scenario's `batch`, capturing the best genome into the catalog. |

## Source map

The engine core (`SimPlugin`) is render-agnostic and shared by every binary:

| Module           | Responsibility |
| ---------------- | -------------- |
| `config.rs`      | `SimConfig` (the scenario) + loading; the `Archetype`, `Relation`, `Bounds` types. |
| `components.rs`  | the agent body; `Vision`, `Perception`, `Action` — the brain's contract; genealogy. |
| `brain.rs`       | the `Brain` enum: Wander · Hunter · Sessile · Mlp. |
| `genotype.rs`    | the heritable `Genotype` + the `TRAITS` table + mutation. |
| `movement.rs`    | the `perceive → decide → act` systems (FixedUpdate, chained). |
| `interaction.rs` | the single interaction primitive + the relation table. |
| `ecology.rs`     | the economy: metabolize, die, age, reproduce. |
| `nutrients.rs`   | the nutrient field, emission, diffusion, absorption. |
| `spawn.rs`       | compiling a genotype into a living phenotype. |
| `breeding.rs`    | the generational `Orchestrator` (P5): cohorts of headless matches, `Fitness` scoring, per-faction selection — the `breed` bin's engine. |

The windowed build adds `main.rs`, `panels.rs`, `editor.rs`, `hud.rs`, `inspector.rs`,
`visuals.rs`, the breeding `dashboard.rs` and friends — all of it strictly in `Update`
(rendering / UI), never touching the fixed simulation schedule.

## Testing: properties across seeds

The test suite has two layers:

- **Unit tests** per module for pure logic (mutation stays in bounds, RON round-trips,
  the scenario-sync guards…).
- **Multi-seed integration drivers** (`tests/`), one per scenario. Each runs the *real*
  sim world across several seeds and asserts a *property that holds across seeds* —
  `flora` self-regulates, `cohabitation`'s hunter out-forages the wanderer,
  `predator_prey` coexists, the trained `mlp` beats the naive one. A single seed's
  success would be anecdotal; the property *is* the claim.

```sh
cargo test                     # everything
cargo test --test cohabitation -- --nocapture   # one driver, with its trajectory printout
```

## The dev rules in brief

The full set lives in `CONSTITUTION-DEV.md`; the ones easiest to trip over:

- **No simulation logic in `Update`.** Agency lives in `FixedUpdate`/`FixedPostUpdate`;
  `Update` is rendering and UI only. This is what keeps headless ⇄ windowed parity.
- **`cargo fmt` is authoritative; the tree stays clippy-clean.** Format before
  committing.
- **Keep the sim byte-identical** unless a change is *meant* to alter it: append new
  genes at the *end* of the genotype, non-mutable and defaulted to `0.0`, so the RNG draw
  stream of existing scenarios is preserved. `tests/mlp.rs` is the chaos-sensitive
  tripwire.
- **Extend the data, not the drivers.** A new gene is one `TRAITS` entry + one struct
  field; no editor/HUD/inspector code to touch.

## Releases

Pushing a `vX.Y.Z` tag that matches `Cargo.toml`'s version triggers the release CI
(`.github/workflows/release.yml`): it builds the workshop under the `dist` profile (fat
LTO, runtime-perf tuned) for Linux x86_64, Windows x86_64 and macOS arm64, archives each
with the runtime data (`assets/`, `scenarios/`, `species/`), the dual
[license](./laws.md#license) files and a generated `THIRD-PARTY-LICENSES.html`, and
publishes them to a GitHub Release. A tag is cut **on explicit request** (any version,
a patch included) or to capture the **outgoing** version before a minor/major bump; the
annotated tag's message *is* the changelog.

## Performance

```sh
cargo bench        # throughput (ticks/sec) — the version-to-version comparator
flame [scenario]   # flamegraph of the headless sim → outputs/flamegraph.svg
```

Because the sim is deterministic (seed + tick count ⇒ identical work), a Criterion
baseline diff is a *real* perf delta, not run-to-run noise — the honest way to confirm an
optimization paid off.
