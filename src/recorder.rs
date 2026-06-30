//! Video recording menu of the windowed build.
//!
//! A module of the windowed *binary* only (like [`crate::editor`], …). It does
//! **not** do the video rendering itself: it **drives the headless `record`
//! binary** (P3, item 14) as a subprocess. We write the current `SimConfig`
//! (editor edits included) to a temporary file, then launch `record` on it → a
//! **clean** *fresh re-render* (without the egui overlay), in line with §7. The
//! UI only configures and launches; an `Update` system watches for the process
//! to finish.
//!
//! Cardinal invariant: no sim logic here, just tool orchestration — like the
//! editor, it is manual action outside `FixedUpdate`.

use bevy::prelude::*;
use bevy_egui::egui;
use std::path::PathBuf;
use std::process::{Child, Command};
use teemlab::SimConfig;
use teemlab::selection::SelectionRoll;

use crate::fonts::{self, icons};
use crate::status::UiStatus;

/// State of the "Recording" panel + the running `record` process, if any.
#[derive(Resource)]
pub struct RecorderPanel {
    out: String,
    fps: f64,
    seconds: f64,
    width: u32,
    height: u32,
    /// **Automatic selection** mode for an agent during the video (to show its
    /// rays to viewers). `Off` = video unchanged.
    select: SelectionRoll,
    /// Interval (s) between two selection changes ("timer" modes).
    select_interval: f32,
    /// Overlay the **native visualizer** (stats / curves / inspector) in a 9:16
    /// composition.
    hud: bool,
    /// Interval (s) for rotating the visualizer's sections (curves ↔ inspector).
    hud_interval: f32,
    /// Whether the floating "Export video" window is open (toggled from the top bar).
    pub open: bool,
    /// The `record` subprocess while it runs (otherwise `None`).
    child: Option<Child>,
    /// Launch requested by the UI, handled at the next `Update`.
    launch_requested: bool,
}

impl Default for RecorderPanel {
    fn default() -> Self {
        Self {
            out: "outputs/run.mp4".into(),
            fps: 30.0,
            seconds: 61.0,
            // Portrait 9:16 by default: the visualizer is overlaid (square arena
            // on top, stats/curves/inspector at the bottom). Uncheck "HUD" and
            // choose 1080×1080 for the old square video of the arena alone.
            width: 1080,
            height: 1920,
            // Eldest by default: we follow the survivor (calm, changes little) →
            // the rays are visible in the video without tuning. "None" disables.
            select: SelectionRoll::Eldest,
            select_interval: 4.0,
            // Visualizer overlaid by default (cf. `record --hud`).
            hud: true,
            hud_interval: 6.0,
            open: false,
            child: None,
            launch_requested: false,
        }
    }
}

/// Path of the `record` binary: next to the current executable (`cargo run` case
/// → `target/debug/record`), otherwise we fall back to the `PATH`.
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

/// The "Export video" section, shown in a **floating window** toggled from the top
/// bar (cf. [`crate::panels::dock`]). A natural top-to-bottom layout — a labelled
/// grid of settings then the Record button — now that it no longer has to align
/// itself to the right of the top bar (the old `right_to_left` reverse-order hack is
/// gone). Only reads/writes its state and sets `launch_requested`; the launch and the
/// monitoring (and the status feedback) live in [`drive_recorder`].
pub(crate) fn recorder_section(ui: &mut egui::Ui, panel: &mut RecorderPanel) {
    let recording = panel.child.is_some();

    ui.label(
        "Re-runs the current scenario fresh (clean headless render, without this \
         interface) and encodes it via ffmpeg.",
    );
    ui.separator();

    egui::Grid::new("rec_grid")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Output file");
            ui.add(egui::TextEdit::singleline(&mut panel.out).desired_width(200.0));
            ui.end_row();

            ui.label("Duration");
            ui.add(
                egui::DragValue::new(&mut panel.seconds)
                    .range(1.0..=120.0)
                    .suffix(" s"),
            );
            ui.end_row();

            ui.label("Frame rate");
            ui.add(
                egui::DragValue::new(&mut panel.fps)
                    .range(24.0..=60.0)
                    .suffix(" fps"),
            );
            ui.end_row();

            ui.label("Size (px)");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut panel.width).range(320..=3840));
                ui.label("×");
                ui.add(egui::DragValue::new(&mut panel.height).range(240..=2160));
            });
            ui.end_row();

            ui.label("Follow agent");
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("rec_select")
                    .selected_text(panel.select.label())
                    .show_ui(ui, |ui| {
                        for m in SelectionRoll::ALL {
                            ui.selectable_value(&mut panel.select, m, m.label());
                        }
                    });
                // Keeps an agent highlighted (ring + rays) in the video.
                if panel.select.rolls() {
                    ui.add(
                        egui::DragValue::new(&mut panel.select_interval)
                            .range(0.5..=30.0)
                            .suffix(" s"),
                    )
                    .on_hover_text("Interval between followed-agent changes");
                }
            });
            ui.end_row();
        });

    // Overlaid native visualizer: composes the video in 9:16.
    ui.checkbox(&mut panel.hud, "HUD overlay (9:16 composition)")
        .on_hover_text(
            "Composes the video in 9:16: arena on top, native visualizer (stats / curves / \
         inspector) at the bottom. Unchecked: video of the arena alone (then choose \
         1080×1080).",
        );
    if panel.hud {
        ui.horizontal(|ui| {
            ui.label("Section rotation");
            ui.add(
                egui::DragValue::new(&mut panel.hud_interval)
                    .range(1.0..=30.0)
                    .suffix(" s"),
            )
            .on_hover_text("Interval to rotate the visualizer's sections (curves ↔ inspector)");
        });
    }

    ui.separator();
    if recording {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.add_enabled(
                false,
                egui::Button::new(fonts::icon_label(icons::RECORD, "Recording…")),
            );
        });
    } else if ui
        .button(fonts::icon_label(icons::RECORD, "Record"))
        .on_hover_text("Launches the headless `record` binary as a subprocess.")
        .clicked()
    {
        panel.launch_requested = true;
    }
}

/// `Update`: watches for the `record` process to finish and, if the UI requested
/// it, writes the current `SimConfig` to a temporary file then launches `record`
/// on it. No sim logic — process orchestration.
pub fn drive_recorder(
    mut panel: ResMut<RecorderPanel>,
    mut status: ResMut<UiStatus>,
    config: Res<SimConfig>,
) {
    // Monitoring the running process: we detect its end without blocking (`try_wait`).
    if let Some(child) = panel.child.as_mut() {
        match child.try_wait() {
            Ok(Some(exit)) => {
                panel.child = None;
                status.set(if exit.success() {
                    format!("Video written → {}", panel.out)
                } else {
                    format!("record failed ({exit}). See the console.")
                });
            }
            Ok(None) => {} // still running
            Err(e) => {
                panel.child = None;
                status.set(format!("Cannot monitor the process: {e}"));
            }
        }
    }

    // Launch requested and nothing running: we write the current scenario then
    // launch `record`. We allow only one recording at a time.
    if !panel.launch_requested || panel.child.is_some() {
        return;
    }
    panel.launch_requested = false;

    // The current scenario (editor edits included), captured in a temporary RON
    // so that `record` re-renders exactly what is seen configured.
    let scenario = std::env::temp_dir().join("teemlab_record_scenario.ron");
    if let Err(e) = config.save_ron_file(&scenario) {
        status.set(format!("Failed to write the temporary scenario: {e}"));
        return;
    }

    let (out, fps, seconds, width, height) = (
        panel.out.clone(),
        panel.fps,
        panel.seconds,
        panel.width,
        panel.height,
    );
    // Automatic selection + HUD passed as arguments (render settings, not the
    // scenario) → `record` drives them without touching the temporary RON.
    let (select, select_interval) = (panel.select.cli(), panel.select_interval.to_string());
    let hud_interval = panel.hud_interval.to_string();
    let mut cmd = Command::new(record_binary());
    cmd.arg(&scenario).args([
        "--out",
        &out,
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
    // HUD enabled by default on the `record` side: we pass `--no-hud` only if it is unchecked.
    if !panel.hud {
        cmd.arg("--no-hud");
    }
    match cmd.spawn() {
        Ok(child) => {
            panel.child = Some(child);
            status.set(format!("Recording in progress → {out}"));
        }
        Err(e) => {
            status.set(format!(
                "Cannot launch ({e}). Are `record` and `ffmpeg` present?"
            ));
        }
    }
}
