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
//! Rendered as a **docked panel** — the **left half of the bottom panel**, side by side with
//! the evolution curves, when the top-bar Breeding toggle is on and the scenario carries a
//! `batch` regime — **not** a floating popup over the sim: [`breeding_panel`] is called from
//! [`crate::panels::dock`] within the shared root `Ui`, so it reserves real layout space and
//! the sim stays centred and fully visible (a Replay then plays out in it). The panel holds
//! the controls (Run/Stop + progress), a
//! **generation navigator** (**click the fitness graph** to inspect any completed generation,
//! or *follow the latest* live — the whole history is retained) with a **Replay** button
//! (re-seed the live world's founders from that generation's cohort — [`seed_founders`]), a
//! per-faction readout,
//! a **fitness-vs-generation curve** (**best + mean per faction**, an accent marker at the
//! inspected generation — the shared [`crate::hud::plot`]), a **per-match metrics table**
//! (every match scored under every metric, the selection-driving one accented — "several
//! metrics between simulations") and a **leaderboard** (a faction **selector** under
//! co-evolution; inspect an MLP genome's network, **Save to library**, or **Save best of
//! run**). The `batch` **config** lives in the World panel's editor ([`crate::editor`], every
//! field). Side effects ([`BreedingAction`]) are applied by [`apply_action`]. See
//! `docs/p5-breeding-plan.md`.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use bevy::prelude::*;
use bevy_egui::egui;

use teemlab::SimConfig;
use teemlab::brain::Brain;
use teemlab::breeding::{GenerationReport, Individual, MatchMetrics, Orchestrator, seed_founders};
use teemlab::config::Fitness;
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
/// **writes** (status + each generation's report) and only locks **between**
/// generations; the UI **reads** through brief per-frame locks, never held across egui.
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
    /// Which **generation** the readout / metrics / leaderboard inspect. `None` = *follow
    /// the latest* (live, the default); `Some(g)` pins a past generation to browse it while
    /// the run keeps going or after it ends. The whole history is retained in `reports`.
    selected_generation: Option<usize>,
    /// The status the UI saw last frame — detects the Running→Done/Stopped **edge** to
    /// bridge the end of a run to the next step with one status-line message.
    last_status: BreedingStatus,
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

/// A side-effecting request the docked breeding panel raises for the caller ([`crate::panels`])
/// to apply where the catalog + live-world resources are in hand — the panel itself only reads.
pub(crate) enum BreedingAction {
    /// Save this genome as a library variant (`species/saved/`).
    Save(Individual),
    /// **Replay** a generation in the live world: re-seed each bred faction's founders from
    /// its cohort (`(species, ranked elites)`) and reset — a fresh re-render (Law 10).
    Replay(Vec<(u16, Vec<Individual>)>),
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
        self.selected_generation = None;
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

    /// Forgets the whole session — history, selection, and any in-flight worker (asked
    /// to stop, then **detached** onto a fresh `Arc`: its late writes land in the
    /// orphaned state and are never displayed). Called on a scenario (re)load: a
    /// report's species indices are only meaningful in the scenario that bred them, so
    /// browsing — or replaying — them against another config would cross wires.
    pub(crate) fn reset(&mut self) {
        if let Ok(mut s) = self.shared.lock() {
            s.stop = true; // the detached worker exits after its in-flight generation
        }
        self.shared = Arc::default();
        self.worker = None; // dropping the handle detaches the thread, never blocks the UI
        self.selected_faction = 0;
        self.selected = None;
        self.selected_generation = None;
        self.last_status = BreedingStatus::Idle;
    }

    /// Number of bred factions (1 for foraging / single-faction battle, more under
    /// co-evolution). `0` before the first generation completes.
    fn faction_count(&self) -> usize {
        let s = self.shared.lock().expect("breeding mutex");
        s.reports.first().map_or(0, |r| r.factions.len())
    }

    /// Number of generations completed so far (the whole retained history).
    fn generation_count(&self) -> usize {
        self.shared.lock().expect("breeding mutex").reports.len()
    }

    /// The **fitness-vs-generation** curves — **two lines per bred faction**: its *best* (the
    /// faction's colour) and its *mean* over the cohort (a dimmed shade), so the progress and
    /// the cohort's central tendency are both legible. X = generation index. (`Dominance`
    /// goes negative; the caller's `YAxis::Auto` handles the range.)
    fn fitness_curves(&self, config: &SimConfig) -> Vec<Curve> {
        let s = self.shared.lock().expect("breeding mutex");
        let n = s.reports.first().map_or(0, |r| r.factions.len());
        let mut curves = Vec::with_capacity(n * 2);
        for f in 0..n {
            let species = s.reports[0].factions[f].species;
            let color = config.color_of(species);
            let name = config
                .archetypes
                .get(species as usize)
                .map_or_else(|| format!("#{species}"), |a| a.name.clone());
            let mut best = Vec::with_capacity(s.reports.len());
            let mut mean = Vec::with_capacity(s.reports.len());
            for (i, r) in s.reports.iter().enumerate() {
                if let Some(fr) = r.factions.get(f) {
                    best.push([i as f32, fr.best_fitness as f32]);
                    mean.push([i as f32, fr.mean_fitness as f32]);
                }
            }
            curves.push(Curve {
                name: format!("{name} best"),
                color,
                pts: best,
            });
            curves.push(Curve {
                name: format!("{name} mean"),
                color: dim(color),
                pts: mean,
            });
        }
        curves
    }

    /// Generation `gen_idx`'s per-faction summaries (species + best/mean) for the readout.
    fn faction_summaries_at(&self, gen_idx: usize) -> Vec<FactionSummary> {
        let s = self.shared.lock().expect("breeding mutex");
        s.reports
            .get(gen_idx)
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

    /// Generation `gen_idx`'s **per-match diagnostics** for `faction` (every metric for each
    /// match of the cohort) — the "several metrics between simulations" table.
    fn match_metrics_at(&self, faction: usize, gen_idx: usize) -> Vec<MatchMetrics> {
        let s = self.shared.lock().expect("breeding mutex");
        s.reports
            .get(gen_idx)
            .and_then(|r| r.factions.get(faction))
            .map(|fr| fr.match_metrics.clone())
            .unwrap_or_default()
    }

    /// Generation `gen_idx`'s leaderboard rows for `faction` (lightweight — no brain clone), its
    /// ranked per-match elites. Empty when that generation / faction has no living member.
    fn leaderboard_at(&self, faction: usize, gen_idx: usize) -> Vec<LeaderRow> {
        let s = self.shared.lock().expect("breeding mutex");
        s.reports
            .get(gen_idx)
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

    /// The `idx`-th elite of `faction` in generation `gen_idx`, **cloned** (genotype + brain)
    /// for the graph / Save-as-variant. Only called for the selected row.
    fn elite_at(&self, faction: usize, idx: usize, gen_idx: usize) -> Option<Individual> {
        let s = self.shared.lock().expect("breeding mutex");
        s.reports
            .get(gen_idx)
            .and_then(|r| r.factions.get(faction))
            .and_then(|fr| fr.elites.get(idx).cloned())
    }

    /// Generation `gen_idx`'s **whole cohort** — every bred faction's ranked elites, keyed by
    /// species — for a **replay** (each faction's live founders are re-seeded from its list).
    fn cohort_of(&self, gen_idx: usize) -> Vec<(u16, Vec<Individual>)> {
        let s = self.shared.lock().expect("breeding mutex");
        s.reports
            .get(gen_idx)
            .map(|r| {
                r.factions
                    .iter()
                    .map(|fr| (fr.species, fr.elites.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The **best genome of the whole run** for `faction`: the champion of the
    /// highest-scoring generation (max `best_fitness` across all generations, ties by the
    /// most recent). Feeds the "★ best of run" one-click save — the "save an entity from the
    /// training" tool, aimed at the strongest lineage rather than the currently-browsed one.
    fn best_of_run(&self, faction: usize) -> Option<Individual> {
        let s = self.shared.lock().expect("breeding mutex");
        s.reports
            .iter()
            .filter_map(|r| r.factions.get(faction))
            .filter_map(|fr| fr.best().map(|i| (fr.best_fitness, i.clone())))
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(_, i)| i)
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

/// Applies a [`BreedingAction`] the docked panel raised, with the catalog + live-world
/// resources in hand (called by [`crate::panels::dock`]). **Save** captures the genome as a
/// library variant (the `breed`-bin / inspector path — `Archetype::capture` + `save_variant`);
/// **Replay** re-seeds each bred faction's live founders from its cohort ([`seed_founders`]),
/// then resets + un-pauses so the generation plays out in the live world.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_action(
    action: BreedingAction,
    config: &mut SimConfig,
    palette: &mut Palette,
    runs_panel: &RunsPanel,
    ui_status: &mut UiStatus,
    sim_controls: &mut crate::controls::SimControls,
    vtime: &mut Time<Virtual>,
) {
    match action {
        BreedingAction::Save(genome) => {
            // The genome carries its own faction (`species`), so it is captured under the
            // right base archetype whichever faction's leaderboard it came from.
            let species = genome.species as usize;
            if let Some(base) = config.archetypes.get(species) {
                let variant = base.capture(genome.genotype, genome.brain, genome.generation);
                let scenario = runs_panel.origin_label();
                let msg = editor::save_variant(palette, config, species, variant, &scenario);
                ui_status.set_result(msg);
            }
        }
        BreedingAction::Replay(cohorts) => {
            // Seed each bred faction's founders from its cohort, then rebuild the world (the
            // reset path reads `config.founder_pools`) and un-pause — a fresh re-render. The
            // pools stay on the live config until the next scenario load, so a manual Reset
            // replays the same generation; they are never saved nor exported (`serde(skip)`,
            // so they don't dirty the document either — cf. `runs`), and a later Run starts
            // clean (`Orchestrator::new` clears them).
            let seed = config.seed;
            let mut seeded = false;
            for (species, elites) in &cohorts {
                if !elites.is_empty() {
                    seed_founders(config, *species, elites, seed);
                    seeded = true;
                }
            }
            if seeded {
                sim_controls.reset_requested = true;
                vtime.unpause();
                ui_status.set("Replaying generation in the live world");
            }
        }
    }
}

/// The docked breeding panel's contents: config hint + status/progress + Run/Stop + the
/// generation navigator (Replay any generation) + the fitness curve + per-match metrics +
/// leaderboard + save tools. Returns a [`BreedingAction`] for [`crate::panels::dock`] to apply.
pub(crate) fn breeding_panel(
    ui: &mut egui::Ui,
    session: &mut BreedingSession,
    config: &SimConfig,
    vtime: &mut Time<Virtual>,
    ui_status: &mut UiStatus,
) -> Option<BreedingAction> {
    let mut action = None;
    let view = session.view();
    let running = view.status == BreedingStatus::Running;

    // Bridge the END of a run to the next step: the live world stays paused and the
    // central chip only says "Space to run", so say what just happened and what the
    // natural follow-up is — once, at the status edge.
    if view.status != session.last_status {
        match (session.last_status, view.status) {
            (BreedingStatus::Running, BreedingStatus::Done) => ui_status.ok(
                "Breeding done — click the fitness curve to browse generations, \
                 or Replay one in the live world.",
            ),
            (BreedingStatus::Running, BreedingStatus::Stopped) => {
                ui_status.set("Breeding stopped — the completed generations stay browsable below.")
            }
            _ => {}
        }
        session.last_status = view.status;
    }

    // Status + progress.
    let (label, color) = match view.status {
        BreedingStatus::Idle => ("Idle", crate::theme::INK_MUTED),
        BreedingStatus::Running => ("Running…", crate::theme::ACCENT),
        BreedingStatus::Done => ("Done", crate::theme::SUCCESS),
        BreedingStatus::Stopped => ("Stopped", crate::theme::INK_MUTED),
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

    // Nothing bred yet → no history to browse.
    let done = session.generation_count();
    if done == 0 {
        return None;
    }

    // Generation navigator — inspect ANY completed generation (its readout, per-match
    // metrics and leaderboard) by **clicking the fitness graph below** (the marker shows the
    // pinned generation), or **follow the latest** (live, the default). The whole history is
    // retained, so browsing never blocks or races the running worker.
    let latest = done - 1;
    let mut view_gen = session.selected_generation.unwrap_or(latest).min(latest);
    ui.add_space(6.0);
    ui.separator();
    ui.horizontal(|ui| {
        let mut follow = session.selected_generation.is_none();
        if ui
            .checkbox(&mut follow, "follow latest (live)")
            .on_hover_text("Track the newest generation as it completes, instead of a pinned one")
            .changed()
        {
            session.selected_generation = if follow { None } else { Some(view_gen) };
            session.selected = None;
        }
        fonts::value(ui, |ui| ui.label(format!("gen {view_gen}/{latest}")));
    });
    if session.selected_generation.is_none() {
        view_gen = latest; // following live: always show the newest.
    }

    // Replay this generation in the LIVE world — re-seed the founders from its whole cohort
    // and run it. A fresh re-render (Law 10 forbids exact replay); the live sim shows the
    // generation's genomes foraging / evolving.
    if ui
        .add_enabled(
            !running,
            egui::Button::new(fonts::icon_label(icons::PLAY, "Replay this generation")),
        )
        .on_hover_text(
            "Seed the live world's founders from this generation's cohort and run it — a \
             fresh re-render (exact seed replay is impossible, Law 10).",
        )
        .clicked()
    {
        action = Some(BreedingAction::Replay(session.cohort_of(view_gen)));
    }

    // Per-faction readout at the viewed generation.
    for fs in session.faction_summaries_at(view_gen) {
        let name = config
            .archetypes
            .get(fs.species as usize)
            .map_or_else(|| format!("#{}", fs.species), |a| a.name.clone());
        fonts::value(ui, |ui| {
            ui.label(format!("{name}: best {:.1} · mean {:.1}", fs.best, fs.mean))
        });
    }

    // Fitness vs generation — **best + mean per faction** (the shared plot widget,
    // X = generation index), with an accent marker at the generation being inspected.
    // **Clicking the graph pins that generation** (replaces the old slider). Drawn once at
    // least two generations give a line; the Y range auto-scales without forcing zero, so a
    // `Dominance` run that goes negative still fills the plot.
    let curves = session.fitness_curves(config);
    if curves.iter().any(|c| c.pts.len() >= 2) {
        ui.add_space(4.0);
        ui.weak("fitness / generation — click to inspect a generation");
        let cfg = crate::plot::PlotConfig {
            height: 90.0,
            y: crate::plot::YAxis::Auto {
                include_zero: false,
                pad: 0.1,
            },
            x_unit: "",
            marker_x: Some(view_gen as f32),
        };
        if let Some(x) = crate::plot::plot(ui, &cfg, &curves) {
            // Snap the clicked X to the nearest generation index and pin it.
            let g = (x.round().max(0.0) as usize).min(latest);
            session.selected_generation = Some(g);
            session.selected = None; // the row index is generation-local.
            view_gen = g; // reflect the pick in the metrics + leaderboard below, same frame.
        }
        crate::plot::legend(ui, &curves);
    }

    // Cohort inspection at the viewed generation: the per-match metrics table + the
    // leaderboard (may raise a Save action). A Replay clicked above takes precedence.
    action.or(cohort_section(ui, session, config, view_gen))
}

/// Dims a colour toward the background — the *mean* line's shade against the *best* line's
/// full-strength faction colour.
fn dim(color: [f32; 3]) -> [f32; 3] {
    color.map(|c| c * 0.55)
}

/// One header cell of the metrics table: a hover tooltip spells out the abbreviation,
/// and the cell is **accented** when that metric is the one driving selection (the
/// scenario's `Fitness`) so the reader sees which column the breeding actually
/// optimises.
fn metric_header(ui: &mut egui::Ui, label: &str, tip: &str, driving: bool) {
    let response = if driving {
        ui.colored_label(crate::theme::ACCENT, label)
    } else {
        ui.weak(label)
    };
    let tip = if driving {
        format!("{tip}\nAccented: this metric drives the selection (the scenario's fitness).")
    } else {
        tip.to_owned()
    };
    response.on_hover_text(tip);
}

/// The **cohort inspection** for generation `gen_idx`: a faction selector (under co-evolution),
/// the **per-match metrics table** ("several metrics between simulations"), the ranked
/// leaderboard with the selected genome's network preview (MLP), and the **save tools** —
/// Save-to-library for the picked genome plus a one-click "best of run". Returns a
/// [`BreedingAction::Save`] when a save button is clicked (the side-effecting write is done by
/// [`apply_action`], which holds the catalog resources — the genome carries its own `species`,
/// so it is captured under the right archetype whatever the faction).
fn cohort_section(
    ui: &mut egui::Ui,
    session: &mut BreedingSession,
    config: &SimConfig,
    gen_idx: usize,
) -> Option<BreedingAction> {
    let n_factions = session.faction_count();
    if n_factions == 0 {
        return None;
    }
    ui.add_space(6.0);
    ui.separator();

    // Pick the faction to inspect (only when several co-evolve).
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

    // Per-match metrics table — every match of the cohort scored under EVERY metric, with
    // the column driving selection (the scenario's `Fitness`) accented. Only the selected
    // metric drove selection; the rest are diagnostics to read the cohort several ways.
    let metrics = session.match_metrics_at(faction, gen_idx);
    let driving = config.batch.as_ref().map(|b| b.fitness);
    if !metrics.is_empty() {
        ui.add_space(4.0);
        ui.strong("Match metrics");
        egui::Grid::new(("breed_metrics", gen_idx, faction))
            .striped(true)
            .num_columns(7)
            .spacing([7.0, 3.0])
            .show(ui, |ui| {
                ui.weak("match")
                    .on_hover_text("One row per headless match of the cohort.");
                metric_header(
                    ui,
                    "pop",
                    "Mean standing population over the match — sustained biomass \
                     (fitness: Population).",
                    driving == Some(Fitness::Population),
                );
                metric_header(
                    ui,
                    "peak",
                    "Peak standing population over the match — the strongest bloom \
                     (fitness: Peak).",
                    driving == Some(Fitness::Peak),
                );
                metric_header(
                    ui,
                    "surv",
                    "Fraction of the match the faction stayed alive (fitness: Survival).",
                    driving == Some(Fitness::Survival),
                );
                metric_header(
                    ui,
                    "lin",
                    "Deepest lineage (in-match generations) ever reached \
                     (fitness: BestEvolved).",
                    driving == Some(Fitness::BestEvolved),
                );
                metric_header(
                    ui,
                    "dom",
                    "Terminal dominance — own minus living rivals at the last sample \
                     (a diagnostic; no longer a selectable fitness).",
                    false,
                );
                metric_header(
                    ui,
                    "rsv",
                    "Mean energy reserve of survivors — a foraging-health diagnostic \
                     (never drives selection).",
                    false,
                );
                ui.end_row();
                for (m, mm) in metrics.iter().enumerate() {
                    fonts::value(ui, |ui| ui.label(format!("#{}", m + 1)));
                    fonts::value(ui, |ui| ui.label(format!("{:.0}", mm.mean_population)));
                    fonts::value(ui, |ui| ui.label(format!("{:.0}", mm.peak_population)));
                    fonts::value(ui, |ui| ui.label(format!("{:.0}%", mm.survival * 100.0)));
                    fonts::value(ui, |ui| ui.label(format!("{:.0}", mm.best_evolved)));
                    fonts::value(ui, |ui| ui.label(format!("{:.0}", mm.dominance)));
                    fonts::value(ui, |ui| ui.label(format!("{:.0}", mm.mean_reserve)));
                    ui.end_row();
                }
            });
    }

    // Leaderboard — the generation's ranked per-match elites (pick one to inspect / save).
    let rows = session.leaderboard_at(faction, gen_idx);
    let mut save = None;
    if rows.is_empty() {
        ui.weak("(this faction died out this generation)");
    } else {
        ui.add_space(4.0);
        ui.strong("Leaderboard");
        // Explicit column header (monospace, so the rows align beneath it).
        fonts::value(ui, |ui| {
            ui.weak(format!(
                "{:<6}{:<7}{:<10}{}",
                "rank", "gen", "reserve", "brain"
            ))
        })
        .on_hover_text(
            "rank in this generation's cohort · in-match lineage depth (generations) · \
             terminal energy reserve · brain type",
        );
        for (i, row) in rows.iter().enumerate() {
            let selected = session.selected == Some(i);
            let kind = if row.is_mlp { "MLP" } else { "—" };
            let text = format!(
                "{:<6}{:<7}{:<10}{kind}",
                format!("#{}", i + 1),
                format!("G{}", row.generation),
                format!("r{:.0}", row.reserve),
            );
            let clicked = fonts::value(ui, |ui| ui.selectable_label(selected, text)).clicked();
            if clicked {
                session.selected = (!selected).then_some(i);
            }
        }

        // The selected genome: its network (MLP only — a structural graph, no live
        // activations) and the Save-to-library action.
        if let Some(idx) = session.selected
            && let Some(elite) = session.elite_at(faction, idx, gen_idx)
        {
            if let Brain::Mlp(m) = &elite.brain {
                editor::draw_mlp_graph(ui, &m.layer_sizes(), Some(m), None);
            }
            if ui
                .button(fonts::icon_label(icons::FLOPPY, "Save to library"))
                .on_hover_text("Save this genome as a variant in the library (species/saved/).")
                .clicked()
            {
                save = Some(elite);
            }
        }
    }

    // Best of the whole run — a one-click save of the strongest lineage seen across ALL
    // generations, independent of which generation is being browsed (the "save an entity
    // from the training" tool for the end of a run).
    if ui
        .button(fonts::icon_label(icons::SPARKLE, "Save best of run"))
        .on_hover_text("Save the strongest genome across every generation to the library.")
        .clicked()
        && let Some(best) = session.best_of_run(faction)
    {
        save = Some(best);
    }
    save.map(BreedingAction::Save)
}
