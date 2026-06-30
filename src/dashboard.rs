//! Windowed **breeding dashboard** (P5, §4 axis A) — the UI face of the generational
//! `run → score → breed` loop.
//!
//! A binary module (like [`crate::controls`] / [`crate::editor`]): **observation +
//! control only**, never sim logic (DEV Rule 1). The heavy breeding runs on a
//! **background thread** — the [`Orchestrator`] drives isolated headless `World`s (§6) —
//! so the windowed `App` stays responsive; this module owns the thread and surfaces its
//! progress in egui. The live `SimPlugin` world is **paused** while a run is on (it is
//! unused — the matches run in their own worlds off-thread).
//!
//! Rendered as a **floating window** (a separate egui system, like the Export window),
//! not a docked panel: it sidesteps the dock's single-root layout and 16-param limit, and
//! a floating window already counts as UI for [`crate::panels::pointer_over_ui`] (so a
//! click on it never falls through to the sim). Shown only when the scenario carries a
//! `batch` regime. The window holds the controls (Run/Stop + progress), a **per-faction**
//! readout, a **fitness-vs-generation curve** (**one line per bred faction** — the shared
//! [`crate::hud::plot`]) and a **leaderboard** (with a faction **selector** under
//! co-evolution; inspect an MLP genome's network + Save-as-variant to the catalog); the
//! `batch` *editor* lives in the World panel ([`crate::editor`]). See
//! `docs/p5-breeding-plan.md`.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use teemlab::SimConfig;
use teemlab::brain::Brain;
use teemlab::breeding::{GenerationReport, Individual, Orchestrator};
use teemlab::metrics::Curve;

use crate::editor::{self, Palette};
use crate::fonts::{self, icons};
use crate::runs::RunsPanel;
use crate::status::UiStatus;

/// Lifecycle of a breeding session.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum BreedingStatus {
    /// No run started yet.
    #[default]
    Idle,
    /// The worker is breeding generations.
    Running,
    /// Every generation ran to completion.
    Done,
    /// The user stopped it early (after the in-flight generation).
    Stopped,
}

/// State shared between the worker thread and the UI (behind a `Mutex`). The worker
/// **writes** (status + each generation's report); the UI **reads** once per frame.
#[derive(Default)]
struct BreedingShared {
    status: BreedingStatus,
    /// One report per completed generation, in order.
    reports: Vec<GenerationReport>,
    /// Total generations the run will execute (for the progress bar).
    total_generations: usize,
    /// UI → worker request: stop after the current generation.
    stop: bool,
}

/// The dashboard's session handle (a windowed-binary resource). Owns the worker thread
/// and the `Arc<Mutex<…>>` it writes; the UI reads a [`BreedingView`] each frame.
#[derive(Resource, Default)]
pub struct BreedingSession {
    shared: Arc<Mutex<BreedingShared>>,
    /// The breeding worker (detached on a new run / at exit — it observes `stop`).
    worker: Option<JoinHandle<()>>,
    /// Which bred **faction** the leaderboard shows (UI state; 0 unless several factions).
    selected_faction: usize,
    /// Leaderboard row the user picked to inspect / save (UI state, not shared).
    selected: Option<usize>,
}

/// A leaderboard row — the lightweight per-elite stats shown in the list (no brain
/// clone). The brain is fetched separately for the selected row only.
struct LeaderRow {
    generation: u32,
    reserve: f32,
    is_mlp: bool,
}

/// A cheap, lock-free-for-the-caller snapshot of the session for one UI frame (the lock
/// is taken and released inside [`BreedingSession::view`], never held across egui).
pub struct BreedingView {
    pub status: BreedingStatus,
    /// Generations completed so far.
    pub done: usize,
    pub total: usize,
}

/// The latest generation's summary for one bred faction (the readout line).
struct FactionSummary {
    species: u16,
    best: f64,
    mean: f64,
}

impl BreedingSession {
    /// Starts a breeding run on `config` (which must carry a `batch`). Spawns the
    /// orchestrator on a background thread; a **no-op** if a run is already in flight.
    fn start(&mut self, config: SimConfig) {
        if self.view().status == BreedingStatus::Running {
            return;
        }
        self.selected = None;
        self.selected_faction = 0;
        let total = config.batch.as_ref().map_or(0, |b| b.generations);
        // Reset the shared state for the new run.
        if let Ok(mut s) = self.shared.lock() {
            *s = BreedingShared {
                status: BreedingStatus::Running,
                total_generations: total,
                ..Default::default()
            };
        }
        let shared = Arc::clone(&self.shared);
        self.worker = Some(std::thread::spawn(move || run_session(shared, config)));
    }

    /// Asks the worker to stop after the current generation.
    fn request_stop(&mut self) {
        if let Ok(mut s) = self.shared.lock() {
            s.stop = true;
        }
    }

    /// Number of bred factions (1 for foraging / single-faction battle, more under
    /// co-evolution). `0` before the first generation completes.
    fn faction_count(&self) -> usize {
        let s = self.shared.lock().expect("breeding mutex");
        s.reports.first().map_or(0, |r| r.factions.len())
    }

    /// The **fitness-vs-generation** curves — one line **per bred faction** (its best
    /// fitness, coloured by the faction's archetype) — plus the `[y_min, y_max]` range
    /// (`Dominance` goes negative, so `y_min` may be < 0). X = generation index.
    fn fitness_curves(&self, config: &SimConfig) -> (Vec<Curve>, f32, f32) {
        let s = self.shared.lock().expect("breeding mutex");
        let n = s.reports.first().map_or(0, |r| r.factions.len());
        let mut curves = Vec::with_capacity(n);
        let (mut y_min, mut y_max) = (0.0_f32, 1.0_f32);
        for f in 0..n {
            let species = s.reports[0].factions[f].species;
            let mut pts = Vec::with_capacity(s.reports.len());
            for (i, r) in s.reports.iter().enumerate() {
                if let Some(fr) = r.factions.get(f) {
                    let y = fr.best_fitness as f32;
                    pts.push([i as f32, y]);
                    y_min = y_min.min(y);
                    y_max = y_max.max(y);
                }
            }
            let name = config
                .archetypes
                .get(species as usize)
                .map_or_else(|| format!("#{species}"), |a| a.name.clone());
            curves.push(Curve {
                name,
                color: config.color_of(species),
                pts,
            });
        }
        (curves, y_min, y_max)
    }

    /// The latest generation's per-faction summaries (species + best/mean) for the readout.
    fn faction_summaries(&self) -> Vec<FactionSummary> {
        let s = self.shared.lock().expect("breeding mutex");
        s.reports
            .last()
            .map(|r| {
                r.factions
                    .iter()
                    .map(|fr| FactionSummary {
                        species: fr.species,
                        best: fr.best_fitness,
                        mean: fr.mean_fitness,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The latest generation's leaderboard rows for `faction` (lightweight — no brain
    /// clone), its ranked per-match elites. Empty before the first generation completes.
    fn leaderboard(&self, faction: usize) -> Vec<LeaderRow> {
        let s = self.shared.lock().expect("breeding mutex");
        s.reports
            .last()
            .and_then(|r| r.factions.get(faction))
            .map(|fr| {
                fr.elites
                    .iter()
                    .map(|i| LeaderRow {
                        generation: i.generation,
                        reserve: i.reserve,
                        is_mlp: matches!(i.brain, Brain::Mlp(_)),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The `idx`-th elite of `faction` in the latest generation, **cloned** (genotype +
    /// brain) for the graph / Save-as-variant. Only called for the selected row.
    fn elite(&self, faction: usize, idx: usize) -> Option<Individual> {
        let s = self.shared.lock().expect("breeding mutex");
        s.reports
            .last()
            .and_then(|r| r.factions.get(faction))
            .and_then(|fr| fr.elites.get(idx).cloned())
    }

    /// A snapshot of the shared progress state for this frame.
    fn view(&self) -> BreedingView {
        let s = self.shared.lock().expect("breeding mutex");
        BreedingView {
            status: s.status,
            done: s.reports.len(),
            total: s.total_generations,
        }
    }
}

/// The worker thread: drive the orchestrator generation by generation, pushing each
/// report into the shared state and honouring the stop flag (checked **between**
/// generations, so a stop never interrupts a match mid-flight).
fn run_session(shared: Arc<Mutex<BreedingShared>>, config: SimConfig) {
    let Some(mut orch) = Orchestrator::new(config) else {
        set_status(&shared, BreedingStatus::Done);
        return;
    };
    while !orch.is_done() {
        let stop = shared.lock().map(|s| s.stop).unwrap_or(true);
        if stop {
            set_status(&shared, BreedingStatus::Stopped);
            return;
        }
        let report = orch.step();
        if let Ok(mut s) = shared.lock() {
            s.reports.push(report);
        }
    }
    set_status(&shared, BreedingStatus::Done);
}

/// Sets the shared status (a one-line lock, kept out of [`run_session`] for clarity).
fn set_status(shared: &Arc<Mutex<BreedingShared>>, status: BreedingStatus) {
    if let Ok(mut s) = shared.lock() {
        s.status = status;
    }
}

/// Draws the breeding dashboard as a floating window — shown only when the scenario
/// carries a `batch` regime. Runs in the egui pass **after** `panels::dock` (it reuses the
/// same context); a floating window does not affect the dock's central rect, so the sim
/// framing is untouched.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut contexts: EguiContexts,
    mut session: ResMut<BreedingSession>,
    config: Res<SimConfig>,
    mut vtime: ResMut<Time<Virtual>>,
    fonts_ready: Res<crate::fonts::FontsReady>,
    mut palette: ResMut<Palette>,
    runs_panel: Res<RunsPanel>,
    mut ui_status: ResMut<UiStatus>,
) -> Result {
    // Gate on the fonts (an icon would panic before its family is bound) and on a batch
    // regime being present (the dashboard is meaningless for a continuous scenario).
    if !fonts_ready.0 || config.batch.is_none() {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let mut to_save = None;
    egui::Window::new(fonts::icon_label(icons::SPARKLE, "Breeding"))
        .collapsible(true)
        .resizable(false)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 44.0))
        .show(ctx, |ui| {
            to_save = dashboard_section(ui, &mut session, &config, &mut vtime);
        });

    // Save-as-variant (outside the closure, where the catalog resources are free): capture
    // the genome under the scored species' base archetype and write it to the catalog —
    // the `breed`-bin / inspector path, reused (`Archetype::capture` + `save_variant`).
    if let Some(genome) = to_save {
        // The genome carries its own faction (`species`), so it is captured under the right
        // base archetype whichever faction's leaderboard it came from.
        let species = genome.species as usize;
        if let Some(base) = config.archetypes.get(species) {
            let variant = base.capture(genome.genotype, genome.brain, genome.generation);
            let scenario = runs_panel.origin_label();
            let msg = editor::save_variant(&mut palette, &config, species, variant, &scenario);
            ui_status.set(msg);
        }
    }
    Ok(())
}

/// The dashboard's contents: status + progress + Run/Stop + a fitness readout. Factored
/// out of [`draw`] so the docked-panel integration (a later step) can reuse it verbatim.
fn dashboard_section(
    ui: &mut egui::Ui,
    session: &mut BreedingSession,
    config: &SimConfig,
    vtime: &mut Time<Virtual>,
) -> Option<Individual> {
    // A min width so the fitness plot (below) has room in this content-sized window.
    ui.set_min_width(220.0);
    let view = session.view();
    let running = view.status == BreedingStatus::Running;

    // Status + progress.
    let (label, color) = match view.status {
        BreedingStatus::Idle => ("Idle", egui::Color32::GRAY),
        BreedingStatus::Running => ("Running…", egui::Color32::from_rgb(240, 180, 80)),
        BreedingStatus::Done => ("Done", egui::Color32::from_rgb(120, 200, 120)),
        BreedingStatus::Stopped => ("Stopped", egui::Color32::GRAY),
    };
    ui.colored_label(color, label);
    if view.total > 0 {
        let frac = (view.done as f32 / view.total as f32).clamp(0.0, 1.0);
        ui.add(egui::ProgressBar::new(frac).text(format!("gen {}/{}", view.done, view.total)));
    }

    // Run / Stop.
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                !running,
                egui::Button::new(fonts::icon_label(icons::PLAY, "Run")),
            )
            .on_hover_text("Start the breeding run (headless, off the render thread)")
            .clicked()
        {
            session.start(config.clone());
            // The live world is unused while breeding (matches run in their own worlds).
            vtime.pause();
        }
        if ui
            .add_enabled(
                running,
                egui::Button::new(fonts::icon_label(icons::X, "Stop")),
            )
            .on_hover_text("Stop after the current generation")
            .clicked()
        {
            session.request_stop();
        }
    });

    // Per-faction fitness readout (one line per bred faction; the curve below colours them).
    for fs in session.faction_summaries() {
        let name = config
            .archetypes
            .get(fs.species as usize)
            .map_or_else(|| format!("#{}", fs.species), |a| a.name.clone());
        fonts::value(ui, |ui| {
            ui.label(format!("{name}: best {:.1} · mean {:.1}", fs.best, fs.mean))
        });
    }

    // Fitness vs generation — **one line per bred faction** (the shared homemade plotter,
    // X = generation index). Drawn once at least two generations give a line; the Y range
    // follows the data (a `Dominance` run goes negative — the Red Queen reads as the lines
    // crossing / tracking near 0).
    let (curves, y_min, y_max) = session.fitness_curves(config);
    if curves.iter().any(|c| c.pts.len() >= 2) {
        ui.add_space(4.0);
        ui.weak("fitness / generation");
        let pad = (y_max - y_min).max(1.0) * 0.1;
        crate::hud::plot(ui, 90.0, &curves, y_min - pad, y_max + pad, "");
        crate::hud::legend(ui, &curves);
    }

    // Leaderboard — the selected faction's ranked cohort (returns a genome to save).
    leaderboard_section(ui, session, config)
}

/// A **faction selector** (when several factions co-evolve) + the selected faction's
/// leaderboard, the selected genome's network preview (MLP) and a Save-as-variant button.
/// Returns the genome to save when the button is clicked (the side-effecting save is done
/// by [`draw`], which holds the catalog resources — the genome carries its own `species`,
/// so it is captured under the right archetype whatever the faction).
fn leaderboard_section(
    ui: &mut egui::Ui,
    session: &mut BreedingSession,
    config: &SimConfig,
) -> Option<Individual> {
    let n_factions = session.faction_count();
    if n_factions == 0 {
        return None;
    }
    ui.add_space(6.0);
    ui.separator();

    // Pick the faction whose leaderboard to show (only when several co-evolve).
    if n_factions > 1 {
        ui.horizontal(|ui| {
            ui.label("faction:");
            for f in 0..n_factions {
                let species = config
                    .batch
                    .as_ref()
                    .and_then(|b| b.scored_species.get(f).copied())
                    .unwrap_or(f as u16);
                let name = config
                    .archetypes
                    .get(species as usize)
                    .map_or_else(|| format!("#{species}"), |a| a.name.clone());
                if ui
                    .selectable_label(session.selected_faction == f, name)
                    .clicked()
                {
                    session.selected_faction = f;
                    session.selected = None; // the row index is faction-local.
                }
            }
        });
    }
    let faction = session.selected_faction.min(n_factions - 1);

    let rows = session.leaderboard(faction);
    if rows.is_empty() {
        ui.weak("(this faction died out)");
        return None;
    }
    ui.strong("Leaderboard");
    for (i, row) in rows.iter().enumerate() {
        let selected = session.selected == Some(i);
        let kind = if row.is_mlp { "MLP" } else { "—" };
        let text = format!(
            "#{}  G{}  r{:.0}  {kind}",
            i + 1,
            row.generation,
            row.reserve
        );
        if ui.selectable_label(selected, text).clicked() {
            session.selected = (!selected).then_some(i);
        }
    }

    // The selected genome: its network (MLP only — a structural graph, no live
    // activations) and the Save-as-variant action.
    let mut save = None;
    if let Some(idx) = session.selected
        && let Some(elite) = session.elite(faction, idx)
    {
        if let Brain::Mlp(m) = &elite.brain {
            editor::draw_mlp_graph(ui, &m.layer_sizes(), Some(m), None);
        }
        if ui
            .button(fonts::icon_label(icons::FLOPPY, "Save as variant"))
            .on_hover_text("Capture this genome into the species catalog (species/saved/)")
            .clicked()
        {
            save = Some(elite);
        }
    }
    save
}
