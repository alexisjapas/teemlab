//! Recording orchestration of the windowed build.
//!
//! A module of the windowed *binary* only (like [`crate::editor`], …). It does
//! **not** render anything itself: it **drives the headless `record` binary** (P3,
//! item 14) as a subprocess. A recording is a **folder** under `outputs/` holding the
//! simulation's parameters and its artefacts:
//!
//! ```text
//! outputs/run-NN/
//!   scenario.ron   the exact SimConfig the run re-renders from (editor edits included)
//!   video.mp4      the rendered video (when the Video component is enabled)
//!   …              sound / metrics later
//! ```
//!
//! The recording is a **clean fresh re-render** (without this egui overlay), in line
//! with §7. The entry point is the top-bar **Record menu**'s *Run record* button (cf.
//! [`crate::panels::dock`]); this module only launches and monitors — an `Update`
//! system watches the process, the status line carries the outcome.
//!
//! Cardinal invariant: no sim logic here, just tool orchestration — like the editor,
//! it is manual action outside `FixedUpdate`.

use bevy::prelude::*;
use bevy_egui::egui;
use std::path::PathBuf;
use std::process::{Child, Command};
use teemlab::SimConfig;
use teemlab::selection::SelectionRoll;

use crate::status::UiStatus;

/// State of the recording (the components to capture + the running `record` process).
/// The render settings (size, fps, duration, follow, HUD) are fixed sensible defaults
/// for now — the menu exposes only *what to record*, not *how*.
#[derive(Resource)]
pub struct RecorderPanel {
    fps: f64,
    seconds: f64,
    width: u32,
    height: u32,
    /// **Automatic selection** mode for an agent during the video (to show its rays to
    /// viewers). `Off` = video unchanged.
    select: SelectionRoll,
    /// Interval (s) between two selection changes ("timer" modes).
    select_interval: f32,
    /// Overlay the **native visualizer** (stats / curves / inspector) in a 9:16
    /// composition.
    hud: bool,
    /// Interval (s) for rotating the visualizer's sections (curves ↔ inspector).
    hud_interval: f32,
    /// **Video** component — render the run to `video.mp4`. On by default; the only
    /// component wired for now (Sound / Metrics are shown off + disabled in the menu,
    /// added here when they land).
    pub video: bool,
    /// The `record` subprocess while it runs (otherwise `None`).
    child: Option<Child>,
    /// Launch requested by the menu, handled at the next `Update`.
    launch_requested: bool,
    /// Cancel requested by the menu (kill the subprocess, discard the partial folder).
    cancel_requested: bool,
    /// The folder of the recording **in flight** — for the completion message and, on a
    /// cancel, the partial-folder cleanup.
    active_dir: Option<PathBuf>,
}

impl Default for RecorderPanel {
    fn default() -> Self {
        Self {
            fps: 30.0,
            seconds: 61.0,
            // Portrait 9:16 by default: the visualizer is overlaid (square arena on top,
            // stats/curves/inspector at the bottom).
            width: 1080,
            height: 1920,
            // Vanguard by default: we follow the evolutionary frontier (calm — it changes
            // only at the target's death) → the rays are visible in the video without
            // tuning.
            select: SelectionRoll::Vanguard,
            select_interval: 4.0,
            // Visualizer overlaid by default (cf. `record --hud`).
            hud: true,
            hud_interval: 6.0,
            video: true,
            child: None,
            launch_requested: false,
            cancel_requested: false,
            active_dir: None,
        }
    }
}

impl RecorderPanel {
    /// `true` while a `record` subprocess is running.
    pub fn is_recording(&self) -> bool {
        self.child.is_some()
    }

    /// Ask [`drive_recorder`] to start a recording at the next `Update` (ignored while
    /// one is already running). The **sole** launch entry point (the menu's *Run record*).
    pub fn request_launch(&mut self) {
        self.launch_requested = true;
    }

    /// Ask [`drive_recorder`] to stop the running recording and discard its folder.
    pub fn cancel(&mut self) {
        self.cancel_requested = true;
    }

    /// The **video sub-options** (shown under the Video toggle when it is on): the
    /// render settings that used to live in the Export window — duration, frame rate,
    /// size, the followed agent, and the 9:16 HUD overlay. Editable in place.
    pub fn video_options_ui(&mut self, ui: &mut egui::Ui) {
        ui.indent("rec_video_opts", |ui| {
            // Keep the whole block compact so the menu can stay narrow: a small combo and
            // tight grid spacing.
            ui.spacing_mut().combo_width = 96.0;
            egui::Grid::new("rec_video_grid")
                .num_columns(2)
                .spacing([8.0, 5.0])
                .show(ui, |ui| {
                    ui.label("Length");
                    ui.add(
                        egui::DragValue::new(&mut self.seconds)
                            .range(1.0..=120.0)
                            .suffix(" s"),
                    );
                    ui.end_row();

                    ui.label("FPS");
                    ui.add(egui::DragValue::new(&mut self.fps).range(24.0..=60.0));
                    ui.end_row();

                    ui.label("Size");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut self.width).range(320..=3840));
                        ui.label("×");
                        ui.add(egui::DragValue::new(&mut self.height).range(240..=2160));
                    });
                    ui.end_row();

                    ui.label("Follow");
                    crate::inspector::follow_combo(ui, "rec_select", &mut self.select);
                    ui.end_row();

                    if self.select.rolls() {
                        ui.label("Interval");
                        ui.add(
                            egui::DragValue::new(&mut self.select_interval)
                                .range(0.5..=30.0)
                                .suffix(" s"),
                        )
                        .on_hover_text("Interval between followed-agent changes");
                        ui.end_row();
                    }
                });
            crate::theme::toggle_row(ui, "HUD (9:16)", &mut self.hud).on_hover_text(
                "Compose the video in 9:16: arena on top, native visualizer (stats / curves / \
                 inspector) at the bottom. Off: the arena alone (choose 1080×1080).",
            );
            if self.hud {
                ui.horizontal(|ui| {
                    ui.label("Rotate");
                    ui.add(
                        egui::DragValue::new(&mut self.hud_interval)
                            .range(1.0..=30.0)
                            .suffix(" s"),
                    )
                    .on_hover_text(
                        "Interval to rotate the visualizer's sections (curves ↔ inspector)",
                    );
                });
            }
        });
    }
}

/// The first free `outputs/run-NN` folder — the recording's home: readable, ordered,
/// and never a previous take. Falls back to the bare name past 999.
fn free_run_dir() -> PathBuf {
    for n in 1..=999u32 {
        let candidate = PathBuf::from(format!("outputs/run-{n:02}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from("outputs/run")
}

/// Path of the `record` binary: next to the current executable (`cargo run` case →
/// `target/debug/record`), otherwise we fall back to the `PATH`.
fn record_binary() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let sibling = dir.join(if cfg!(windows) {
            "record.exe"
        } else {
            "record"
        });
        if sibling.exists() {
            return sibling;
        }
    }
    PathBuf::from("record")
}

/// `Update`: watches for the `record` process to finish and, when the menu requested
/// it, builds the recording folder (writes the current `SimConfig` as `scenario.ron`)
/// then launches `record` on it, headless. No sim logic — process orchestration.
pub fn drive_recorder(
    mut panel: ResMut<RecorderPanel>,
    mut status: ResMut<UiStatus>,
    config: Res<SimConfig>,
) {
    // A cancel kills the subprocess and discards the partial folder. (ffmpeg, fed by the
    // dying `record`'s pipe, exits on its own once the pipe closes.)
    if panel.cancel_requested {
        panel.cancel_requested = false;
        if let Some(mut child) = panel.child.take() {
            let _ = child.kill();
            let _ = child.wait(); // reap, no zombie
            if let Some(dir) = panel.active_dir.take() {
                let _ = std::fs::remove_dir_all(&dir); // best-effort cleanup
            }
            status.set("Recording cancelled.");
        }
        return;
    }

    // Monitoring the running process: we detect its end without blocking (`try_wait`).
    if let Some(child) = panel.child.as_mut() {
        match child.try_wait() {
            Ok(Some(exit)) => {
                panel.child = None;
                let dir = panel.active_dir.take();
                if exit.success() {
                    match dir {
                        Some(d) => status.ok(format!("Recording written → {}", d.display())),
                        None => status.ok("Recording written."),
                    }
                } else {
                    status.error(format!("record failed ({exit}). See the console."));
                }
            }
            Ok(None) => {} // still running
            Err(e) => {
                panel.child = None;
                panel.active_dir = None;
                status.error(format!("Cannot monitor the process: {e}"));
            }
        }
    }

    // Launch requested and nothing running: build the folder then launch `record`. Only
    // one recording at a time.
    if !panel.launch_requested || panel.child.is_some() {
        return;
    }
    panel.launch_requested = false;

    // The recording folder + the current scenario (editor edits included) frozen into it
    // as `scenario.ron`, so the recording is self-describing and `record` re-renders
    // exactly what is configured.
    let dir = free_run_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        status.error(format!("Cannot create the recording folder: {e}"));
        return;
    }
    let scenario = dir.join("scenario.ron");
    if let Err(e) = config.save_ron_file(&scenario) {
        status.error(format!("Failed to write the scenario parameters: {e}"));
        return;
    }

    // Video is the only wired component for now. With it off, the folder is created with
    // just the parameters (a placeholder for the sound/metrics-only recordings to come).
    if !panel.video {
        status.ok(format!("Recording folder created → {}", dir.display()));
        return;
    }

    let video = dir.join("video.mp4");
    let (fps, seconds, width, height) = (panel.fps, panel.seconds, panel.width, panel.height);
    // Automatic selection + HUD passed as arguments (render settings, not the scenario)
    // → `record` drives them without touching the saved RON.
    let (select, select_interval) = (panel.select.cli(), panel.select_interval.to_string());
    let hud_interval = panel.hud_interval.to_string();
    let mut cmd = Command::new(record_binary());
    cmd.arg(&scenario).args([
        "--out",
        &video.to_string_lossy(),
        "--fps",
        &fps.to_string(),
        "--seconds",
        &seconds.to_string(),
        "--width",
        &width.to_string(),
        "--height",
        &height.to_string(),
        "--select",
        select,
        "--select-interval",
        &select_interval,
        "--hud-interval",
        &hud_interval,
    ]);
    // HUD enabled by default on the `record` side: pass `--no-hud` only if it is off.
    if !panel.hud {
        cmd.arg("--no-hud");
    }
    match cmd.spawn() {
        Ok(child) => {
            panel.child = Some(child);
            status.set(format!("Recording in progress → {}", dir.display()));
            panel.active_dir = Some(dir);
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            status.error(format!(
                "Cannot launch ({e}). Are `record` and `ffmpeg` present?"
            ));
        }
    }
}
