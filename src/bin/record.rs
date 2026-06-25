//! **Headless** video recorder (P3, item 14).
//!
//! We *re-render fresh* a run (§7: without bit-for-bit determinism, no replay by
//! seed — we relaunch the run and film it; it is representative, not the exact
//! historical match) and **pipe the raw frames directly to an `ffmpeg`
//! process**: no intermediate PNG on disk.
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
//! [--hud-interval S] [--nutrients]`.
//!
//! `--nutrients` overlays the nutrient **heatmap** layer in the arena (the
//! background "calque"); off by default, so existing videos are unchanged.
//!
//! `--hud` (default) overlays the native visualizer (stats / curves / inspector)
//! in a **9:16** composition — square arena on top, visualizer at the bottom —
//! strictly identical to the windowed "presentation" mode; `--no-hud` renders the
//! arena alone (square, historical behavior). `--hud-interval` sets the section
//! rotation.
//!
//! `--select` keeps a mobile agent **highlighted** during the video (ring + vision
//! rays), to show the raycasts to viewers. MODE ∈ `off`, `sticky`, `cycle`,
//! `active` (the most "active"), `species` (species tour), `eldest`.
//! `cycle`/`active`/`species` change every `--select-interval` s (default 4);
//! `sticky`/`eldest` change only at the target's death.

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
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;
use teemlab::dataviz::DataVizPlugin;
use teemlab::metrics::MetricsPlugin;
use teemlab::selection::{AutoSelectPlugin, SelectionRenderPlugin, SelectionRoll};
use teemlab::visuals::{Layers, VisualsPlugin, srgb3};
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
            // Eldest by default: we highlight the survivor (rays visible in the
            // video); `--select off` disables.
            select: SelectionRoll::Eldest,
            select_interval: 4.0,
            // Visualizer overlaid **by default** (§ video); `--no-hud` turns it off.
            hud: true,
            hud_interval: 6.0,
            // Nutrient heatmap off by default → existing videos unchanged.
            nutrients: false,
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
    let mut child: Child = Command::new("ffmpeg")
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
            eprintln!("record: cannot launch ffmpeg ({err}). Is it installed?");
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

    eprintln!(
        "record: {} frames at {} fps ({:.1}s), {}×{}{}{} → {}",
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

    // Framing: the arena (± half_extent) always fits, with a margin.
    let span = config.arena_half_extent * 2.0 * 1.1;
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
