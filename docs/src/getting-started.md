# Getting started

teemlab is a Rust project. Its toolchain and the system libraries Bevy needs (Vulkan,
wayland/X11, ALSA, ffmpeg…) are all provided by a **Nix flake**, so a one-line setup
gets you a reproducible environment.

## Prerequisites

- [Nix](https://nixos.org/download) with flakes enabled (and, optionally,
  [direnv](https://direnv.net/) for automatic shell loading).
- A GPU with a Vulkan driver for the windowed build (the headless tools need none).

## Enter the dev shell

```sh
nix develop          # or: direnv allow   (then it loads automatically)
```

This drops you into a shell with `cargo`, `clippy`, `rustfmt`, `ffmpeg`, profiling
tools, `mdbook`, and two convenience commands: **`play`** and **`flame`**.

## Launch the windowed build

`play` builds the whole workshop (the `teemlab`, `record` and `headless` binaries) and
launches the windowed editor. Building *all* binaries matters because the in-app
recording menu runs `record` as a subprocess.

```sh
play                                            # debug, empty arena — the editor's canvas
play scenarios/examples/04_evolution.ron        # debug, an explicit scenario
play --release                                  # release build (faster sim)
play --release scenarios/examples/10_predator_prey.ron
```

The window opens **paused** so you can place, edit and inspect before pressing
**Space** to run. See [The editor](./editor.md) for the full UI tour.

## Run it headless

The headless binary runs the *exact same simulation* with no window or rendering — the
basis of teemlab's headless ⇄ windowed parity. It is a smoke test by default:

```sh
cargo run --bin headless                                       # default scenario
cargo run --bin headless scenarios/examples/01_default.ron     # explicit scenario
TEEMLAB_TICKS=20000 cargo run --bin headless scenarios/examples/03_flora.ron
```

## Record a video

`record` re-renders a scenario headless and pipes frames to `ffmpeg`. Outputs land in
`outputs/`.

```sh
cargo run --bin record -- scenarios/examples/04_evolution.ron --out outputs/run.mp4
#   options: --out F  --fps N  --seconds S  --width W  --height H  --nutrients
#   defaults: 30 fps, 61 s, 1080×1080 (the arena is square)
#   --nutrients overlays the nutrient heatmap (great for 02_nutrients.ron)
```

See [Recording videos](./recording.md) for the menu-driven workflow.

## Tests, formatting, benchmarks

```sh
cargo test                     # unit tests + multi-seed scenario drivers + guardrails
cargo fmt                      # the default rustfmt is authoritative (no rustfmt.toml)
cargo clippy --all-targets     # the tree is kept at zero warnings
cargo bench                    # throughput benchmark — ticks/sec, for version A/B
flame [scenario.ron]           # flamegraph of the headless sim → outputs/flamegraph.svg
```

The multi-seed drivers (in `tests/`) are the heart of the test suite: each runs a
real scenario across several seeds and asserts a *property that holds across seeds* —
"the hunter out-forages the wanderer", "predator and prey coexist", "the trained MLP
beats the naive one". A single seed's success would be anecdotal; a property across
seeds is the actual claim.

## Preview these docs locally

```sh
mdbook serve docs --open       # live-reloading preview of this site
mdbook build docs              # one-off render to docs/book/
```

## Next

Now that it runs, learn what you are watching: [The agent loop](./model/the-loop.md).
