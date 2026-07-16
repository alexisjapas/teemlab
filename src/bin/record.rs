//! **Headless** video recorder (P3, item 14).
//!
//! We *re-render fresh* a run (§7: without bit-for-bit determinism, no replay by
//! seed — we relaunch the run and film it; it is representative, not the exact
//! historical match) and **pipe the raw frames directly to an `ffmpeg`
//! process**: no intermediate PNG on disk.
//!
//! `ffmpeg` is an **external** runtime dependency, never bundled (it stays a
//! separate process, so its GPL terms don't reach the tree). It is resolved via
//! [`ffmpeg_binary`]: `TEEMLAB_FFMPEG` env override → a copy next to this executable
//! → the `PATH`.
//!
//! The rendering is *genuinely* windowless: we disable `WinitPlugin`, remove the
//! primary window, and the camera renders into a **target image**
//! (`RenderTarget::Image`). `ScheduleRunnerPlugin` pumps the loop; each `Update`
//! we capture the target image via the `Screenshot` API (which does the GPU→CPU
//! readback for us), and a dedicated thread writes the raw RGBA pixels to
//! `ffmpeg`'s `stdin`.
//!
//! Time advances by a *fixed* step per frame (`TimeUpdateStrategy::ManualDuration`,
//! = `1/fps`), independently of wall-clock time: the sim's fixed loop plays the
//! right number of ticks per video frame, and the recorded duration is exact.
//!
//! A **single** run, single-threaded (inter-match parallelization is deferred to
//! P5 with the GA). Everything lives in `Update` / at `Startup` — never any sim
//! logic outside `FixedUpdate` (cardinal invariant): we only *observe*.
//!
//! Usage: `record [scenario.ron] [--out f.mp4] [--fps N] [--seconds S]
//! [--width W] [--height H] [--select MODE] [--select-interval S] [--no-hud]
//! [--hud-interval S] [--nutrients] [--stop-when BRAINS] [--stop-after S]`.
//!
//! `--nutrients` overlays the nutrient **heatmap** layer in the arena (the
//! background "calque"); off by default, so existing videos are unchanged.
//!
//! `--stop-when BRAINS` ends the film early — once every living agent of the named
//! **brain families** is gone — instead of always filming the full `--seconds`, so a
//! run that dies out early is not padded with an empty arena. `BRAINS` is a
//! comma-separated list of family names (`wander,hunter,grazer,sessile,mlp`,
//! case-insensitive), a leading `!` inverting it: `--stop-when mlp` waits for the
//! last MLP to die, `--stop-when !sessile` for all non-sessile life to die.
//! `--stop-after S` keeps filming `S` more seconds after that extinction (default
//! `3`); `--seconds` stays the hard upper bound.
//!
//! `--hud` (default) overlays the native visualizer (stats / curves / inspector)
//! in a **9:16** composition — square arena on top, visualizer at the bottom —
//! strictly identical to the windowed "presentation" mode; `--no-hud` renders the
//! arena alone (square, historical behavior). `--hud-interval` sets the section
//! rotation.
//!
//! `--select` keeps a mobile agent **highlighted** during the video (ring + vision
//! rays), to show the raycasts to viewers. MODE ∈ `off`, `cycle`,
//! `active` (the most "active"), `species` (species tour), `vanguard` (a
//! random newest-generation agent, rotating species at each death).
//! `cycle`/`active`/`species` change every `--select-interval` s (default 4);
//! `vanguard` changes only at the target's death.

use bevy::app::{AppExit, ScheduleRunnerPlugin};
use bevy::asset::RenderAssetUsages;
use bevy::camera::{ClearColorConfig, RenderTarget, ScalingMode};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::time::TimeUpdateStrategy;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;
use teemlab::brain::Brain;
use teemlab::components::Agent;
use teemlab::dataviz::DataVizPlugin;
use teemlab::metrics::MetricsPlugin;
use teemlab::selection::{AutoSelectPlugin, SelectionRenderPlugin, SelectionRoll};
use teemlab::visuals::{Layers, NewLayerVisible, VisualsPlugin, srgb3};
use teemlab::{SimConfig, SimPlugin};

/// Recording parameters, read from the command line.
struct Settings {
    scenario: Option<String>,
    out: String,
    fps: f64,
    seconds: f64,
    /// Explicit width/height, otherwise resolved from `hud` (9:16 portrait with
    /// HUD, square without) — cf. [`main`].
    width: Option<u32>,
    height: Option<u32>,
    /// Roll mode of the automatic selection (rays visible in the video).
    select: SelectionRoll,
    /// Interval (s) between two selection changes ("timer" modes).
    select_interval: f32,
    /// Overlay the native visualizer (stats / curves / inspector), 9:16 composition.
    hud: bool,
    /// Interval (s) for rotating the visualizer's sections (curves ↔ inspector).
    hud_interval: f32,
    /// Overlay the nutrient **heatmap** layer(s) in the arena (the background
    /// "calque", cf. [`Layers`]). Off by default → videos unchanged.
    nutrients: bool,
    /// Auto-stop: which brain **families** to watch for extinction (`true` at the
    /// family's index). `None` = no auto-stop (film the full `seconds`). When every
    /// living agent of a watched family is gone, the recording ends after
    /// [`Settings::stop_after`] more seconds — so we don't film an empty arena.
    stop_when: Option<[bool; Brain::FAMILY_COUNT]>,
    /// Seconds to keep filming after the watched families go extinct (`--stop-after`).
    stop_after: f64,
}

/// Parses a `--stop-when` spec into a per-family "watch this for extinction" mask.
/// A comma-separated list of brain family names (case-insensitive, cf.
/// [`Brain::FAMILIES`]); a leading `!` (or `not:`) **inverts** it — watch every
/// family *except* those named. Examples: `mlp` (the last MLP), `!sessile` (all
/// non-sessile life), `hunter,mlp`.
fn parse_brain_filter(spec: &str) -> Result<[bool; Brain::FAMILY_COUNT], String> {
    let (invert, list) = match spec.strip_prefix('!').or_else(|| spec.strip_prefix("not:")) {
        Some(rest) => (true, rest),
        None => (false, spec),
    };
    let mut chosen = [false; Brain::FAMILY_COUNT];
    for name in list.split(',') {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let idx = Brain::FAMILIES
            .iter()
            .position(|f| f.eq_ignore_ascii_case(name))
            .ok_or_else(|| {
                let all: Vec<String> = Brain::FAMILIES.iter().map(|f| f.to_lowercase()).collect();
                format!("unknown brain \"{name}\" ({})", all.join("|"))
            })?;
        chosen[idx] = true;
    }
    if !chosen.iter().any(|&c| c) {
        return Err("no brain named".into());
    }
    if invert {
        for c in &mut chosen {
            *c = !*c;
        }
    }
    Ok(chosen)
}

impl Settings {
    fn parse() -> Self {
        let mut s = Settings {
            scenario: None,
            out: "outputs/out.mp4".into(),
            fps: 30.0,
            seconds: 61.0,
            // Dimensions resolved from `hud` if not provided (cf. `main`).
            width: None,
            height: None,
            // Vanguard by default: we highlight the evolutionary frontier (rays
            // visible in the video); `--select off` disables.
            select: SelectionRoll::Vanguard,
            select_interval: 4.0,
            // Visualizer overlaid **by default** (§ video); `--no-hud` turns it off.
            hud: true,
            hud_interval: 6.0,
            // Nutrient heatmap off by default → existing videos unchanged.
            nutrients: false,
            // Auto-stop off by default (film the full `seconds`); `--stop-when` arms it.
            stop_when: None,
            stop_after: 3.0,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            let mut next = || {
                args.next().unwrap_or_else(|| {
                    eprintln!("record: missing value after \"{arg}\"");
                    std::process::exit(2);
                })
            };
            match arg.as_str() {
                "--out" | "-o" => s.out = next(),
                "--fps" => s.fps = next().parse().expect("--fps: number expected"),
                "--seconds" | "-s" => {
                    s.seconds = next().parse().expect("--seconds: number expected")
                }
                "--width" | "-w" => {
                    s.width = Some(next().parse().expect("--width: integer expected"))
                }
                "--height" | "-h" => {
                    s.height = Some(next().parse().expect("--height: integer expected"))
                }
                // Overlaid visualizer (stats / curves / inspector), 9:16 composition.
                "--hud" => s.hud = true,
                "--no-hud" => s.hud = false,
                // Overlay the nutrient heatmap layer (background "calque").
                "--nutrients" => s.nutrients = true,
                "--hud-interval" => {
                    s.hud_interval = next()
                        .parse()
                        .expect("--hud-interval: number (seconds) expected");
                }
                // Automatic selection of an agent during the video (to show its
                // rays); modes: cf. the module header and `SelectionRoll`.
                "--select" => {
                    let v = next();
                    s.select = SelectionRoll::from_cli(&v).unwrap_or_else(|| {
                        let modes: Vec<&str> = SelectionRoll::ALL.iter().map(|m| m.cli()).collect();
                        eprintln!(
                            "record: unknown selection mode \"{v}\" ({})",
                            modes.join("|")
                        );
                        std::process::exit(2);
                    });
                }
                "--select-interval" => {
                    s.select_interval = next()
                        .parse()
                        .expect("--select-interval: number (seconds) expected");
                }
                // Auto-stop: end the film once a brain-filtered subset goes extinct.
                "--stop-when" => {
                    let v = next();
                    s.stop_when = Some(parse_brain_filter(&v).unwrap_or_else(|err| {
                        eprintln!("record: --stop-when: {err}");
                        std::process::exit(2);
                    }));
                }
                "--stop-after" => {
                    s.stop_after = next()
                        .parse()
                        .expect("--stop-after: number (seconds) expected");
                }
                other if other.starts_with('-') => {
                    eprintln!("record: unknown option \"{other}\"");
                    std::process::exit(2);
                }
                // First positional argument = scenario path (like the rest of the
                // project: scenario = data, 1st argument).
                positional => {
                    if s.scenario.is_none() {
                        s.scenario = Some(positional.to_string());
                    }
                }
            }
        }
        s
    }
}

/// Handle of the image the camera renders into (capture target).
#[derive(Resource)]
struct RecordTarget(Handle<Image>);

/// How many frames to film, and their size.
#[derive(Resource)]
struct RecordPlan {
    width: u32,
    height: u32,
    frames: u32,
}

/// Progress: frames requested (screenshots launched) vs delivered (readback received).
#[derive(Resource, Default)]
struct RecordProgress {
    spawned: u32,
    written: u32,
}

/// Sender of the raw frames to the `ffmpeg` writer thread. Removing it from the
/// `World` closes the channel and cleanly terminates the thread (and thus `ffmpeg`).
#[derive(Resource)]
struct FrameSink(Sender<Vec<u8>>);

/// Auto-stop state (`--stop-when`): watch a brain-filtered subset and cut the film
/// once it goes extinct. Only present when armed.
#[derive(Resource)]
struct AutoStop {
    /// Which brain families to watch (`true` at the family's index, cf. [`Brain::family_index`]).
    watched: [bool; Brain::FAMILY_COUNT],
    /// Frames to keep filming after extinction (`stop_after × fps`).
    grace: u32,
    /// The watched subset has been alive at least once — so a scenario that never
    /// spawns a watched family does not trigger an instant stop.
    seen_alive: bool,
    /// Latched once extinction has capped the plan, so we cap exactly once.
    fired: bool,
}

/// Path of the `ffmpeg` encoder. Resolution order: the `TEEMLAB_FFMPEG` env var
/// (explicit override), then a copy sitting *next to* the current executable (so a
/// release archive — or the user — can drop in its own `ffmpeg` with no system
/// install), then the bare `ffmpeg` on the `PATH`. ffmpeg stays an **external**
/// runtime dependency, never bundled: `record` only spawns it as a separate process
/// (raw frames over its stdin), which keeps the tree clear of ffmpeg's GPL terms.
fn ffmpeg_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("TEEMLAB_FFMPEG") {
        return PathBuf::from(path);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let sibling = dir.join(if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        });
        if sibling.exists() {
            return sibling;
        }
    }
    PathBuf::from("ffmpeg")
}

fn main() -> AppExit {
    let settings = Settings::parse();
    let config = match &settings.scenario {
        Some(path) => SimConfig::from_ron_file(path).unwrap_or_else(|err| {
            eprintln!("record: scenario \"{path}\" unreadable: {err}");
            std::process::exit(1);
        }),
        None => SimConfig::default(),
    };
    let frames = (settings.fps * settings.seconds).round().max(1.0) as u32;

    // Dimensions: 9:16 portrait when the visualizer is overlaid (square arena on
    // top, viz at the bottom), square otherwise. An explicit size wins.
    let (def_w, def_h) = if settings.hud {
        (1080, 1920)
    } else {
        (1080, 1080)
    };
    let width = settings.width.unwrap_or(def_w);
    let height = settings.height.unwrap_or(def_h);

    // We create the output directory if needed (by default `outputs/`, ignored by
    // git) — ffmpeg does not write into a missing tree.
    if let Some(parent) = std::path::Path::new(&settings.out).parent()
        && !parent.as_os_str().is_empty()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        eprintln!(
            "record: output directory \"{}\" cannot be created: {err}",
            parent.display()
        );
        std::process::exit(1);
    }

    // `ffmpeg` reads raw RGBA video on stdin → encodes to H.264/yuv420p. No
    // intermediate file: we wire up the pipe directly.
    let ffmpeg = ffmpeg_binary();
    let mut child: Child = Command::new(&ffmpeg)
        .args([
            "-y",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgba",
            "-video_size",
            &format!("{width}x{height}"),
            "-framerate",
            &format!("{}", settings.fps),
            "-i",
            "-",
            "-pix_fmt",
            "yuv420p",
            "-crf",
            "18",
            &settings.out,
        ])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| {
            eprintln!(
                "record: cannot launch ffmpeg at \"{}\" ({err}). Install ffmpeg (on \
                 PATH), drop it next to this binary, or set TEEMLAB_FFMPEG to its path.",
                ffmpeg.display()
            );
            std::process::exit(1);
        });

    let stdin = child.stdin.take().expect("ffmpeg stdin piped");
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    // Writer thread: the whole pipe to ffmpeg lives outside the Bevy loop, so as
    // not to block rendering on I/O. It runs while a sender exists; its end closes
    // ffmpeg's stdin → file finalization.
    let writer = std::thread::spawn(move || feed_ffmpeg(stdin, rx));

    let frame_dt = Duration::from_secs_f64(1.0 / settings.fps);
    let mut app = App::new();
    app.add_plugins(
        // Real rendering but windowless: no winit (it is ScheduleRunnerPlugin that
        // drives the loop), no primary window — the camera renders into an image,
        // not into a surface.
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                close_when_requested: false,
                ..default()
            })
            .disable::<WinitPlugin>(),
    )
    .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::ZERO))
    .add_plugins(SimPlugin::new(config))
    .add_plugins(VisualsPlugin)
    // View layers ("calques"): the recorder defaults to agents-only (videos
    // unchanged); `--nutrients` overlays the nutrient heatmap layer. Replaces the
    // default `Layers` that `VisualsPlugin` just inserted. T2 has a single nutrient
    // field → a one-element flag vector.
    .insert_resource(Layers {
        agents: true,
        nutrients: vec![settings.nutrients],
    })
    // New nutrient layers (index ≥ 1, appearing when a scenario declares more than one
    // component) stay hidden in the recorder: `--nutrients` shows only the first field,
    // so existing videos are byte-identical. The windowed build defaults this to `true`.
    .insert_resource(NewLayerVisible(false))
    // Curve sampling (shared with the windowed build) + overlaid native visualizer.
    // With HUD, `DataVizPlugin` recomposes the target in 9:16 (arena on top, viz at bottom).
    .add_plugins(MetricsPlugin)
    .add_plugins(DataVizPlugin {
        enabled: settings.hud,
        interval: settings.hud_interval,
    })
    // Driven time: each update advances by exactly 1/fps, so the fixed loop plays
    // the right number of ticks and the video is paced to the wall clock.
    .insert_resource(TimeUpdateStrategy::ManualDuration(frame_dt))
    .insert_resource(RecordPlan {
        width,
        height,
        frames,
    })
    .insert_resource(FrameSink(tx))
    .init_resource::<RecordProgress>()
    .add_systems(Startup, setup_recorder)
    .add_systems(Update, capture_frame);

    // Automatic selection (`--select` option): keeps a mobile agent highlighted
    // throughout the video, to show its rays. The rendering (ring + rays) is shared
    // with the windowed build; here the target rolls on its own per the chosen mode.
    if settings.select != SelectionRoll::Off {
        app.add_plugins(SelectionRenderPlugin)
            .add_plugins(AutoSelectPlugin {
                roll: settings.select,
                interval: settings.select_interval,
            });
    }

    // Auto-stop (`--stop-when`): cap the film once the watched families go extinct,
    // `stop_after` seconds later. `--seconds` stays the hard upper bound.
    if let Some(watched) = settings.stop_when {
        let grace = (settings.stop_after * settings.fps).round().max(0.0) as u32;
        app.insert_resource(AutoStop {
            watched,
            grace,
            seen_alive: false,
            fired: false,
        })
        .add_systems(Update, auto_stop.before(capture_frame));
    }

    eprintln!(
        "record: {} frames at {} fps ({:.1}s), {}×{}{}{}{} → {}",
        frames,
        settings.fps,
        settings.seconds,
        width,
        height,
        if settings.hud { " +HUD" } else { "" },
        if settings.nutrients {
            " +nutrients"
        } else {
            ""
        },
        match settings.stop_when {
            Some(w) => format!(
                " +stop({} @ +{:.1}s)",
                Brain::FAMILIES
                    .iter()
                    .zip(w)
                    .filter(|(_, on)| *on)
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
                    .join(","),
                settings.stop_after,
            ),
            None => String::new(),
        },
        settings.out
    );
    let exit = app.run();

    // End of run: we drop the remaining sender (the resource) to close the
    // channel, wait for the writing to finish, then for ffmpeg's finalization.
    app.world_mut().remove_resource::<FrameSink>();
    let _ = writer.join();
    match child.wait() {
        Ok(status) if status.success() => eprintln!("record: video written."),
        Ok(status) => eprintln!("record: ffmpeg finished with {status}."),
        Err(err) => eprintln!("record: waiting for ffmpeg failed: {err}"),
    }
    exit
}

/// `Startup`: creates the target image and the camera that renders into it, framed on the arena.
fn setup_recorder(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    plan: Res<RecordPlan>,
    config: Res<SimConfig>,
) {
    let size = Extent3d {
        width: plan.width,
        height: plan.height,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    // Render target *and* copy source (for the screenshot readback).
    image.texture_descriptor.usage =
        TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC | TextureUsages::TEXTURE_BINDING;
    let handle = images.add(image);

    // Framing: the arena (± half_extent) always fits, with the shared breathing
    // margin (identical to the windowed Observe view and the 9:16 HUD composition).
    let span = config.arena_half_extent * 2.0 * teemlab::visuals::ARENA_VIEW_MARGIN;
    commands.spawn((
        Camera2d,
        Camera {
            // Off-game (beyond the arena) = the scenario's outer color, like the
            // windowed build; the play area (inside) is painted by `VisualsPlugin`.
            // The image-camera ignores the `ClearColor` resource, so we set the
            // color here.
            clear_color: ClearColorConfig::Custom(srgb3(config.off_game_color)),
            ..default()
        },
        // In 0.18 the render target is a separate component, required by `Camera`.
        RenderTarget::from(handle.clone()),
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::AutoMin {
                min_width: span,
                min_height: span,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));

    commands.insert_resource(RecordTarget(handle));
}

/// `Update`: while there remain frames to film, requests a capture of the target
/// image. The observer (triggered when the GPU→CPU readback is ready) pushes the
/// pixels to the ffmpeg thread and, at the last delivered frame, exits.
fn capture_frame(
    mut commands: Commands,
    target: Res<RecordTarget>,
    sink: Res<FrameSink>,
    plan: Res<RecordPlan>,
    mut progress: ResMut<RecordProgress>,
) {
    if progress.spawned >= plan.frames {
        return;
    }
    progress.spawned += 1;
    let tx = sink.0.clone();
    // One capture per rendered frame: the render pipeline delivers them in
    // submission order and the channel is FIFO → frame order is preserved.
    commands.spawn(Screenshot::image(target.0.clone())).observe(
        move |captured: On<ScreenshotCaptured>,
              plan: Res<RecordPlan>,
              mut progress: ResMut<RecordProgress>,
              mut exit: MessageWriter<AppExit>| {
            if let Some(data) = captured.image.data.clone() {
                // Full/closed channel = ffmpeg thread gone: nothing more to do,
                // the end of run will handle the exit.
                let _ = tx.send(data);
            }
            progress.written += 1;
            if progress.written >= plan.frames {
                exit.write(AppExit::Success);
            }
        },
    );
}

/// `Update` (before [`capture_frame`]): watches the brain-filtered subset and, once
/// it is extinct, **caps the plan** at the current frame plus the grace window — the
/// existing capture loop then films the last `grace` frames and exits on its own, so
/// we never film an empty arena. Idempotent (latched via [`AutoStop::fired`]); a
/// scenario that never spawns a watched family never fires (guarded by `seen_alive`).
fn auto_stop(
    mut stop: ResMut<AutoStop>,
    mut plan: ResMut<RecordPlan>,
    progress: Res<RecordProgress>,
    brains: Query<&Brain, With<Agent>>,
) {
    if stop.fired {
        return;
    }
    let alive = brains
        .iter()
        .filter(|b| stop.watched[b.family_index()])
        .count();
    if alive > 0 {
        stop.seen_alive = true;
        return;
    }
    if !stop.seen_alive {
        return;
    }
    // Extinction of the watched subset: film `grace` more frames from here, then stop.
    let target = progress.spawned.saturating_add(stop.grace);
    plan.frames = plan.frames.min(target);
    stop.fired = true;
    eprintln!(
        "record: watched brains extinct at frame {}; stopping after {} more frame(s).",
        progress.spawned, stop.grace,
    );
}

/// Writer thread: drains the raw frames and pushes them to ffmpeg's stdin. Stops
/// when all senders are dropped (end of run), then closes stdin (via `drop`) so
/// ffmpeg finalizes the file.
fn feed_ffmpeg(mut stdin: std::process::ChildStdin, rx: Receiver<Vec<u8>>) {
    while let Ok(frame) = rx.recv() {
        if stdin.write_all(&frame).is_err() {
            // ffmpeg closed its input (encoding error): no point insisting.
            break;
        }
    }
    let _ = stdin.flush();
    // `stdin` is dropped here → EOF on ffmpeg's side → finalization.
}

#[cfg(test)]
mod tests {
    use super::*;

    // Index of a brain family by name, for readable expectations below.
    fn fam(name: &str) -> usize {
        Brain::FAMILIES
            .iter()
            .position(|f| f.eq_ignore_ascii_case(name))
            .unwrap()
    }

    #[test]
    fn stop_when_names_a_single_family() {
        let mask = parse_brain_filter("mlp").unwrap();
        assert!(mask[fam("MLP")]);
        assert_eq!(mask.iter().filter(|&&b| b).count(), 1);
    }

    #[test]
    fn stop_when_is_case_insensitive_and_lists() {
        let mask = parse_brain_filter("Hunter,mlp").unwrap();
        assert!(mask[fam("Hunter")] && mask[fam("MLP")]);
        assert_eq!(mask.iter().filter(|&&b| b).count(), 2);
    }

    #[test]
    fn stop_when_bang_inverts() {
        // "all non-sessile life" — every family except Sessile.
        let mask = parse_brain_filter("!sessile").unwrap();
        assert!(!mask[fam("Sessile")]);
        assert_eq!(mask.iter().filter(|&&b| b).count(), Brain::FAMILY_COUNT - 1);
    }

    #[test]
    fn stop_when_rejects_unknown_or_empty() {
        assert!(parse_brain_filter("wanderer").is_err());
        assert!(parse_brain_filter("").is_err());
        assert!(parse_brain_filter("!").is_err());
    }
}
