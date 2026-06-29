# Recording videos

teemlab can render any scenario to a video **headless** — no window required — by
re-running the simulation and piping frames to `ffmpeg`. Because the headless and
windowed builds run the identical sim, the recording is a faithful re-render of what you
would see live, just without the editor UI.

## From the command line

```sh
cargo run --bin record -- scenarios/examples/04_evolution.ron --out outputs/run.mp4
```

| Option        | Default     | Meaning                                            |
| ------------- | ----------- | -------------------------------------------------- |
| `--out F`     | `outputs/…` | Output file path.                                  |
| `--fps N`     | `30`        | Frames per second.                                 |
| `--seconds S` | `61`        | Length of simulated time to render.                |
| `--width W`   | `1080`      | Frame width (the arena is square).                 |
| `--height H`  | `1080`      | Frame height.                                      |
| `--nutrients` | off         | Overlay the nutrient **heatmap** layer.            |

The `--nutrients` overlay is especially good on the resource scenarios — try it on
[`02_nutrients.ron`](./scenarios.md#02--nutrients) to film the oases blooming, or on
[`12_nutrient_web.ron`](./scenarios.md#12--nutrient-web) to watch recycling light up the
field.

```sh
cargo run --bin record -- scenarios/examples/02_nutrients.ron \
    --out outputs/nutrients.mp4 --seconds 90 --nutrients
```

## From the editor

The **⏺ Export…** button in the top strip opens a floating window that configures and
launches the same `record` binary as a subprocess (a clean re-render, without the UI).
You set the output file, duration, fps, size, which agent to follow, and an optional 9:16
HUD overlay for vertical video.

> **Why `play` builds everything.** The recording menu looks for `record` *next to* the
> running `teemlab` executable. `cargo run --bin teemlab` compiles only `teemlab`, so the
> menu would fail to find `record`. The `play` command does a full `cargo build` first
> (all binaries) and then launches — so `record` is always there, debug or release.

## Output location

Videos land in `outputs/`, which is git-ignored (only the directory is kept). `ffmpeg`
must be on the path — the Nix dev shell provides it.
