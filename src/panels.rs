//! The windowed build's whole layout, assembled by a **single** system ([`dock`]):
//! the persistent **screen router** rail ([`nav_rail`]) on the far left, then the
//! panels of the current screen (Observe · Library · Studio · Lab · Analyze,
//! [`crate::screen`]) `show_inside` the one root `Ui` (redesign Phase B,
//! `docs/ui-redesign.md`). A module of the windowed *binary* only; it invents nothing —
//! each panel calls a reusable `*_section(ui, …)` already exposed by its tool module
//! (`controls`, `editor`, `runs`, `hud`, `recorder`, `inspector`, `dashboard`). The
//! role of this system is purely **layout** — the rail, and reserving the edges of
//! each screen.
//!
//! **The five screens.** Only [`Screen::Observe`] renders the live arena: transport
//! strip on top, live stats + view layers on the left, the agent inspector on the
//! right, evolution curves at the bottom — each foldable to a thin rail (`1` `2` `3`)
//! so the arena leads. **Studio** is world + cast + archetype editor with static
//! food-web validation; **Lab** is the headless breeding dashboard; **Library** is the
//! compose hub (Worlds / Species galleries + tray); **Analyze** is a deferred
//! placeholder. Help is **hover-first**
//! (tooltips), the nav rail's Help opening the one remaining surface — the shortcuts
//! cheatsheet.
//!
//! **One root viewport `Ui`, `show_inside`.** Following bevy_egui 0.40
//! (`examples/ui.rs`): we build a single background-layer `Ui` covering
//! `ctx.viewport_rect()`, then add every panel into it with
//! `Panel::show_inside(&mut root, …)` — no deprecated top-level `Panel::show(ctx, …)`.
//!
//! **The one-camera discipline** (`docs/ui-redesign.md` §1). On **Observe** the centre
//! stays "transparent" (no `CentralPanel`) and the Bevy rendering shows through, so the
//! arena is centred and fully visible; the free region is read with
//! `available_rect_before_wrap()` and stashed in [`CentralRect`] for
//! `main::set_sim_camera`. Every **other** screen fills its content area with an opaque
//! `CentralPanel` and records an **empty** rect, so the arena is hidden and the camera /
//! picking systems idle — switching away from Observe releases the arena cleanly,
//! without ever spawning a second camera.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use teemlab::SimConfig;
use teemlab::brain::Brain;
use teemlab::components::{Action, Age, Agent, Generation, Perception, Reserve, Species, Vision};
use teemlab::genotype::Genotype;
use teemlab::metrics::History;
use teemlab::nutrients::Nutrients;
use teemlab::selection::{AutoSelect, BrainFilter, Selection};
use teemlab::visuals::Layers;

use crate::controls::{self, SimControls};
use crate::dashboard::{self, BreedingSession};
use crate::editor::{self, Palette};
use crate::experiment::{EXPERIMENTS_DIR, LabMode, LabSetup};
use crate::fonts::{self, icons};
use crate::hud;
use crate::inspector;
use crate::library::{CatalogSource, Library, LibraryTab};
use crate::recorder::RecorderPanel;
use crate::runs::{self, RunsPanel};
use crate::screen::{Router, Screen};
use crate::status::UiStatus;

/// Last frame's measured transport width — the input the top-bar centering needs this
/// frame (a one-frame lag, harmless). A single [`Local`] so [`dock`] adds no system
/// parameter. (Observe's panels are fixed-width now, so no side-width bookkeeping.)
#[derive(Default)]
pub struct DockLayout {
    /// Measured width of the centered transport controls (for centering — see below).
    ctrl_width: f32,
}

/// Visibility of the **user-toggleable floating surfaces** — the one convention for
/// "what's open": a bool per surface on a single resource. (egui memory holds only
/// per-widget presentation state like collapsing headers; `Option`-presence stays
/// reserved for genuine *selection*, e.g. `palette.selected`, never for visibility;
/// a scenario's `batch` being set is a data *precondition* for the Lab dashboard,
/// not its toggle.)
#[derive(Resource, Default)]
pub struct UiWindows {
    /// The keyboard-shortcuts **cheatsheet** (`?` / the nav rail's Help).
    pub shortcuts: bool,
    /// The **dynamic trophic-graph overlay** on the arena (ui-redesign §7): the derived
    /// web annotated with live population (node size) and dependency (edge colour). A
    /// view layer, off by default.
    pub trophic_overlay: bool,
}

/// Cross-panel resources [`dock`] writes, bundled into one [`SystemParam`] so the
/// system stays within Bevy's 16-parameter limit (like [`ObsParams`]): the scenario
/// document model, the recorder settings, the unified status line and the
/// window/region toggles.
#[derive(SystemParam)]
pub struct DockState<'w> {
    /// The top-level screen router (Observe · Library · Studio · Lab · Analyze — cf.
    /// [`crate::screen`]). Bundled here so `dock` stays within Bevy's 16-parameter
    /// limit; the nav rail writes it and the layout dispatches on it.
    pub router: ResMut<'w, Router>,
    pub runs_panel: ResMut<'w, RunsPanel>,
    pub recorder_panel: ResMut<'w, RecorderPanel>,
    pub ui_status: ResMut<'w, UiStatus>,
    pub windows: ResMut<'w, UiWindows>,
    /// The Library catalog + compose tray (Worlds / Species — cf. [`crate::library`]).
    pub library: ResMut<'w, Library>,
    /// The Lab screen's experiment-setup state (mode + sweep + run-record — [`LabSetup`]).
    pub lab: ResMut<'w, LabSetup>,
    /// The breeding session (P5) — the **Lab** screen's dashboard reads/drives it (the
    /// generational `run → score → breed` loop over isolated worlds). Bundled here so
    /// `dock` stays within Bevy's 16-parameter limit.
    pub breeding: ResMut<'w, BreedingSession>,
    /// The config the running world was built from — the transport's Reset accents
    /// itself when the live config diverges from it (cf. `controls::world_diverged`).
    pub world_baseline: Res<'w, controls::WorldBaseline>,
}

/// **Observation** state Observe reads, bundled into one [`SystemParam`] so [`dock`]
/// stays within Bevy's 16-parameter limit: the current [`Selection`] (read, by the
/// inspector), the auto-follow mode ([`AutoSelect`]), the sim view's pan/zoom
/// ([`crate::ViewControl`] — written by [`arena_controls`]) and a read-only lookup of
/// agent positions (so the Fit menu can centre the view on the selected entity).
#[derive(SystemParam)]
pub struct ObsParams<'w, 's> {
    pub selection: Res<'w, Selection>,
    pub auto_select: ResMut<'w, AutoSelect>,
    pub view: ResMut<'w, crate::ViewControl>,
    pub bodies: Query<'w, 's, &'static Transform, With<Agent>>,
}

/// The central region left free by the docked panels (egui points), computed by
/// [`dock`] from the root `Ui` and consumed by `main::set_sim_camera` to frame the
/// simulation. Replaces the deprecated `ctx.available_rect()`.
#[derive(Resource)]
pub struct CentralRect(pub egui::Rect);

impl Default for CentralRect {
    fn default() -> Self {
        Self(egui::Rect::ZERO)
    }
}

/// True if the pointer is over the egui UI (a docked panel or a floating window), as
/// opposed to the central simulation area — the gate the interaction systems use to
/// avoid acting on the sim through the UI.
///
/// Replaces `ctx.is_pointer_over_egui()`, which egui 0.34 only wires up for its own
/// closure-based `run_ui` flow: it relies on `root_ui_available_rect`, left **unset**
/// under bevy_egui + `show_inside` (and unsettable — `pass_state_mut` is crate-private),
/// so the built-in falls back to a legacy `unused_rect` that `show_inside` never
/// shrinks → it would report "not over UI" everywhere, and clicks on a panel would
/// fall through to the sim. We reproduce its **modern** logic against our
/// [`CentralRect`] (which *is* what `root_ui_available_rect` would hold): a floating
/// window (non-background layer) always counts as UI; on the background layer, the
/// pointer is over a panel iff it falls **outside** the central rect.
pub fn pointer_over_ui(ctx: &egui::Context, central: egui::Rect) -> bool {
    ctx.input(|i| i.pointer.interact_pos())
        .is_some_and(|pos| pointer_over_ui_at(ctx, pos, central))
}

/// [`pointer_over_ui`] at an arbitrary position — the **gesture-origin** variant: a
/// drag belongs to where it *started*, so `camera_navigation` tests the press origin
/// here rather than the pointer's current position (a pan begun on the sim survives
/// crossing a panel; one begun on a panel never pans the view).
pub fn pointer_over_ui_at(ctx: &egui::Context, pos: egui::Pos2, central: egui::Rect) -> bool {
    match ctx.layer_id_at(pos) {
        // A window / menu / popup floating over the sim: always UI.
        Some(layer) if layer.order != egui::Order::Background => true,
        // Background layer (panels live here): UI iff outside the sim's central rect.
        _ => !central.contains(pos),
    }
}

/// The Studio archetype-editor **detail** view: a header (the selected species' name,
/// its colour dot, and a close `✕`) then the editor itself in its own scroll. Returns
/// `true` if the user asked to close (deselect the archetype). Rendered in Studio's
/// central region beside the cast master (`docs/ui-redesign.md` §5).
fn archetype_detail(ui: &mut egui::Ui, palette: &mut Palette, config: &mut SimConfig) -> bool {
    let mut deselect = false;
    let name = palette
        .selected
        .and_then(|i| config.archetypes.get(i))
        .map(|a| a.name.clone())
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.heading(if name.is_empty() { "Archetype" } else { &name });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(fonts::icon(icons::X))
                .on_hover_text("Close (deselect the archetype)")
                .clicked()
            {
                deselect = true;
            }
        });
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("archetype_detail_scroll")
        .show(ui, |ui| editor::editor_section(ui, palette, config));
    deselect
}

/// The keyboard-shortcuts cheatsheet body: the keyboard [`crate::keymap::BINDINGS`]
/// then the [`crate::keymap::MOUSE`] gestures, as two-column grids. Reads the same
/// tables the tooltips do, so it can never drift from the real controls.
fn shortcuts_cheatsheet(ui: &mut egui::Ui) {
    use crate::keymap::{BINDINGS, MOUSE};
    egui::Grid::new("keyboard_shortcuts")
        .num_columns(2)
        .spacing([16.0, 6.0])
        .show(ui, |ui| {
            for b in BINDINGS {
                ui.strong(b.keys_text);
                if b.when.is_empty() {
                    ui.label(b.label);
                } else {
                    ui.label(format!("{}  ({})", b.label, b.when));
                }
                ui.end_row();
            }
        });
    ui.separator();
    ui.weak("Mouse");
    egui::Grid::new("mouse_gestures")
        .num_columns(2)
        .spacing([16.0, 6.0])
        .show(ui, |ui| {
            for (gesture, label) in MOUSE {
                ui.strong(*gesture);
                ui.label(*label);
                ui.end_row();
            }
        });
}

/// Width of the persistent **nav rail** (egui points): the router's left strip, on
/// every screen (`docs/ui-redesign.md` §9; the comp's 76 px).
const NAV_W: f32 = 76.0;

/// The persistent **screen router** rail on the far left: the five destinations
/// (Observe · Library · Studio · Lab · Analyze, [`Screen::ALL`]) in fixed order, plus
/// a Help affordance at the bottom. Present on **every** screen — only the content to
/// its right changes. Rendered **first** in [`dock`], so it is unconditional and its
/// ids never shift (§2.5 of `docs/ui-spec.md`). Clicking a destination writes
/// [`Router::current`]; the active one is accented.
/// The Phosphor glyph for a nav destination (comp iconography).
fn nav_icon(screen: Screen) -> char {
    match screen {
        Screen::Observe => icons::EYE,
        Screen::Library => icons::SQUARES,
        Screen::Studio => icons::PUZZLE_PIECE,
        Screen::Lab => icons::FLASK,
        Screen::Analyze => icons::CHART,
    }
}

/// One nav-rail entry: a Phosphor icon **over** a small label, custom-painted (egui
/// buttons are single-line, so they can't stack). **Active** = accent ink + a left
/// accent strip + a neutral card fill (the comp keeps the wash neutral — the gold is
/// the ink and the strip); hovered = the same card wash. `dimmed` marks a deferred
/// destination (the comp's "soon" state).
fn nav_entry(
    ui: &mut egui::Ui,
    glyph: char,
    label: &str,
    active: bool,
    dimmed: bool,
) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 58.0), egui::Sense::click());
    let painter = ui.painter();
    let pill = rect.shrink2(egui::vec2(4.0, 2.0));
    if active {
        painter.rect_filled(pill, 11.0, crate::theme::CARD);
        let strip = egui::Rect::from_min_size(
            egui::pos2(rect.left() - 4.0, rect.center().y - 13.0),
            egui::vec2(3.0, 26.0),
        );
        painter.rect_filled(
            strip,
            egui::CornerRadius {
                nw: 0,
                ne: 3,
                sw: 0,
                se: 3,
            },
            crate::theme::ACCENT,
        );
    } else if resp.hovered() {
        painter.rect_filled(pill, 11.0, crate::theme::CARD);
    }
    let color = if active {
        crate::theme::ACCENT
    } else if dimmed {
        crate::theme::INK_FAINT
    } else {
        crate::theme::INK_MUTED
    };
    painter.text(
        egui::pos2(rect.center().x, rect.top() + 19.0),
        egui::Align2::CENTER_CENTER,
        glyph.to_string(),
        egui::FontId::new(23.0, fonts::phosphor()),
        color,
    );
    painter.text(
        egui::pos2(rect.center().x, rect.bottom() - 11.0),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(10.5),
        color,
    );
    resp
}

fn nav_rail(root: &mut egui::Ui, router: &mut Router, windows: &mut UiWindows) {
    egui::Panel::left("nav_rail")
        .resizable(false)
        .default_size(NAV_W)
        .size_range(NAV_W..=NAV_W)
        .frame(
            egui::Frame::default()
                .fill(crate::theme::SURFACE)
                .inner_margin(egui::Margin::symmetric(8, 12)),
        )
        .show_inside(root, |ui| {
            // The app mark: the atom glyph in an accent-soft rounded square.
            ui.vertical_centered(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, 11.0, crate::theme::soft(crate::theme::ACCENT));
                ui.painter().rect_stroke(
                    rect,
                    11.0,
                    egui::Stroke::new(1.0, crate::theme::line(crate::theme::ACCENT)),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    icons::ATOM.to_string(),
                    egui::FontId::new(22.0, fonts::phosphor_fill()),
                    crate::theme::ACCENT,
                );
            });
            ui.add_space(14.0);
            for screen in Screen::ALL {
                if nav_entry(
                    ui,
                    nav_icon(screen),
                    screen.label(),
                    router.current == screen,
                    // Analyze is deferred (a placeholder screen) — the comp dims it.
                    screen == Screen::Analyze,
                )
                .clicked()
                {
                    router.current = screen;
                }
                ui.add_space(2.0);
            }
            // Help pinned to the bottom.
            let rem = ui.available_height() - 58.0;
            if rem > 0.0 {
                ui.add_space(rem);
            }
            if nav_entry(ui, icons::QUESTION, "Help", windows.shortcuts, false)
                .on_hover_text(crate::keymap::tooltip(
                    "Keyboard shortcuts & mouse gestures",
                    crate::keymap::UiAction::ToggleShortcuts,
                ))
                .clicked()
            {
                windows.shortcuts = !windows.shortcuts;
            }
        });
}

/// The static **food-web validation** strip (Studio), from the A5 reachability
/// analyzer ([`SimConfig::broken_chains`], `docs/emergent-trophics.md` §6.1): green
/// when every archetype's needs are reachable, else one red line per broken chain
/// (`‹species› needs ‹component›`). Recomputed each frame, so it updates live as the
/// user edits — the payoff of emergent (checkable) trophic interactions.
fn studio_validation(ui: &mut egui::Ui, graph: &crate::trophic::TrophicGraph, config: &SimConfig) {
    if graph.is_viable() {
        ui.colored_label(
            crate::theme::SUCCESS,
            "Food web viable — every need is reachable.",
        );
    } else {
        for &(species, component) in &graph.broken {
            let sp = config
                .archetypes
                .get(species)
                .map(|a| a.name.as_str())
                .unwrap_or("?");
            let comp = config
                .components
                .get(component)
                .map(|c| c.name.as_str())
                .unwrap_or("?");
            ui.colored_label(
                crate::theme::ERROR,
                format!("{sp} needs {comp} — chain broken"),
            );
        }
    }
    // Structural fragility (§6.4): the worst predator's diet concentration — a specialist
    // (→1) is one collapse away from losing its only prey.
    let frag = crate::trophic::web_fragility(config);
    if !frag.per_predator.is_empty() {
        ui.weak(format!(
            "web fragility · worst {:.2} · mean {:.2} (1 = specialist, →0 = generalist)",
            frag.worst, frag.mean
        ));
    }
}

/// A centred **placeholder** screen (Analyze deferred): an icon medallion, the title,
/// an optional *deferred* chip, a wrapped description, and a row of feature chips. Its
/// own `CentralPanel` fills the content area, so the arena stays hidden (the one-camera
/// discipline — `docs/ui-redesign.md` §1).
fn placeholder_screen(
    root: &mut egui::Ui,
    icon: char,
    title: &str,
    deferred: bool,
    body: &str,
    features: &[&str],
) {
    egui::CentralPanel::default().show_inside(root, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.5 - 110.0).max(16.0));
            // Icon medallion.
            let (rect, _) = ui.allocate_exact_size(egui::vec2(66.0, 66.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 18.0, crate::theme::CARD);
            ui.painter().rect_stroke(
                rect,
                18.0,
                egui::Stroke::new(1.0, crate::theme::LINE),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                icon.to_string(),
                egui::FontId::new(30.0, fonts::phosphor()),
                crate::theme::INK_MUTED,
            );
            ui.add_space(16.0);
            if deferred {
                crate::theme::chip(ui, crate::theme::ACCENT, "Deferred · placeholder");
                ui.add_space(12.0);
            }
            ui.heading(title);
            ui.add_space(10.0);
            ui.allocate_ui_with_layout(
                egui::vec2(440.0, 96.0),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ui.colored_label(crate::theme::INK_MUTED, body);
                },
            );
            if !features.is_empty() {
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    for f in features {
                        crate::theme::chip(ui, crate::theme::INK_MUTED, *f);
                    }
                });
            }
        });
    });
}

/// Observe's **live-stats** block (comp §3): a big-number population card (total +
/// delta) then one coloured row per species. Reads the metrics [`History`] — the same
/// source the population curve plots, so the two agree.
fn observe_population(ui: &mut egui::Ui, history: &History, config: &SimConfig) {
    let (total, delta) = history.population_delta();
    editor::card(ui, |ui| {
        crate::theme::caption(ui, "Population");
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(total.to_string())
                    .monospace()
                    .size(26.0)
                    .color(crate::theme::INK),
            );
            if delta != 0 {
                let (col, txt) = if delta > 0 {
                    (crate::theme::SUCCESS, format!("+{delta}"))
                } else {
                    (crate::theme::ERROR, delta.to_string())
                };
                ui.label(egui::RichText::new(txt).color(col).size(12.0));
            }
        });
    });
    let pop = history.latest_population();
    if !pop.is_empty() {
        editor::card(ui, |ui| {
            for (i, arch) in config.archetypes.iter().enumerate() {
                ui.horizontal(|ui| {
                    let (dot, _) =
                        ui.allocate_exact_size(egui::vec2(11.0, 11.0), egui::Sense::hover());
                    ui.painter()
                        .circle_filled(dot.center(), 4.5, crate::theme::rgb(arch.color));
                    ui.label(&arch.name);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.monospace(pop.get(i).copied().unwrap_or(0).to_string());
                    });
                });
            }
        });
    }
}

/// A **dot caption**: a small filled dot followed by a [`crate::theme::caption`] — the
/// comp's `● OUTER · SWEEP` block headers.
fn dot_caption(ui: &mut egui::Ui, color: egui::Color32, text: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
        ui.painter().circle_filled(r.center(), 4.0, color);
        crate::theme::caption(ui, text);
    });
}

/// A small **metric tile** (Lab results header): a caption over a large mono value in
/// its own colour, in a card. Used for the species / trophic-links / web-fragility
/// read-outs.
fn metric_tile(ui: &mut egui::Ui, label: &str, value: &str, color: egui::Color32) {
    editor::card(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.vertical(|ui| {
            crate::theme::caption(ui, label);
            ui.label(
                egui::RichText::new(value)
                    .monospace()
                    .size(22.0)
                    .color(color),
            );
        });
    });
}

/// HSL → `Color32` (`h` in degrees, `s`/`l` in `[0, 1]`) — for the seed-derived hue of a
/// World thumbnail.
fn hsl(h: f32, s: f32, l: f32) -> egui::Color32 {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let q = |v: f32| ((v + m) * 255.0) as u8;
    egui::Color32::from_rgb(q(r), q(g), q(b))
}

/// Paint a **World thumbnail** (the comp's gallery preview): a seed-derived radial glow
/// over a near-black base, with a few species motes. egui has no radial gradient, so
/// stacked translucent circles (large-faint → small-bright) approximate it.
fn world_thumbnail(painter: &egui::Painter, rect: egui::Rect, seed: u64) {
    let painter = painter.with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(13, 15, 19));
    let base = hsl((seed % 360) as f32, 0.45, 0.32);
    let center = egui::pos2(
        rect.left() + rect.width() * 0.38,
        rect.top() + rect.height() * 0.40,
    );
    for i in (1..=6).rev() {
        let r = rect.width() * 0.11 * i as f32;
        let a = (46 / i) as u8;
        painter.circle_filled(
            center,
            r,
            egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), a),
        );
    }
    for (fx, fy, col) in [
        (0.20, 0.20, crate::theme::FLORA),
        (0.42, 0.32, crate::theme::FLORA),
        (0.66, 0.54, crate::theme::FLORA),
        (0.30, 0.68, crate::theme::FLORA),
        (0.78, 0.24, crate::theme::TARGET),
    ] {
        painter.circle_filled(
            egui::pos2(
                rect.left() + rect.width() * fx,
                rect.top() + rect.height() * fy,
            ),
            3.0,
            col,
        );
    }
}

/// The **Record** menu (top bar): the recording's **components** to capture — Video
/// (wired, with its render sub-options when on), Sound / Metrics (planned, shown off +
/// disabled) — then a single **Run record** button, the *only* entry point to launching
/// a (headless) recording. A recording in flight swaps the launcher for a spinner +
/// Cancel. Stays open while you edit it (closes only on a click outside).
fn record_menu(ui: &mut egui::Ui, panel: &mut RecorderPanel) {
    // Cap the width: egui menus lay out **justified** (they stretch to fill the popup's
    // width, which defaults very wide), so a `min_width` can't tighten them — only a
    // `max_width` does.
    ui.set_max_width(190.0);
    crate::theme::caption(ui, "Add to recording");
    crate::theme::toggle_row(ui, "Video", &mut panel.video)
        .on_hover_text("Render the run to video.mp4 (headless).");
    // Video's render sub-options appear (editable) when Video is on.
    if panel.video {
        panel.video_options_ui(ui);
    }
    // Sound / Metrics: planned — shown off and non-interactive for now (their disabled
    // state carries the "not yet" — no wide caption needed).
    ui.add_enabled_ui(false, |ui| {
        let mut off = false;
        crate::theme::toggle_row(ui, "Sound", &mut off);
        crate::theme::toggle_row(ui, "Metrics", &mut off);
    });
    ui.separator();
    if panel.is_recording() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Recording…");
            if ui.button(fonts::icon_label(icons::X, "Cancel")).clicked() {
                panel.cancel();
                ui.close();
            }
        });
    } else if crate::theme::primary_button(
        ui,
        fonts::icon_label_tinted(icons::RECORD, "Run record", crate::theme::ON_ACCENT2),
    )
    .on_hover_text(
        "Create a recording folder (outputs/run-NN/) with the scenario parameters and the \
         selected components, then render it headless.",
    )
    .clicked()
    {
        panel.request_launch();
        ui.close();
    }
}

/// A translucent dark **overlay frame** for the arena's floating controls (the comp's
/// blurred pills — egui has no backdrop-blur, so a dark wash + hairline approximates it).
fn overlay_frame() -> egui::Frame {
    egui::Frame::default()
        .fill(egui::Color32::from_black_alpha(180))
        .stroke(egui::Stroke::new(1.0, crate::theme::LINE))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(9, 6))
}

/// A framing-mode button (arena controls): **accent-filled** while its mode is `active`,
/// `disabled` when `!enabled` (e.g. Follow with nothing selected). Returns the `Response`.
///
/// Both states carry a **1 px stroke** (accent when active, transparent otherwise): a
/// button's frame grows by its stroke width, so a strokeless inactive button would jump
/// bigger the moment it accents — the invisible stroke keeps the geometry identical.
fn mode_button(
    ui: &mut egui::Ui,
    glyph: char,
    label: &str,
    active: bool,
    enabled: bool,
) -> egui::Response {
    let button = if active {
        egui::Button::new(fonts::icon_label_tinted(glyph, label, crate::theme::ACCENT))
            .fill(crate::theme::soft(crate::theme::ACCENT))
            .stroke(egui::Stroke::new(
                1.0,
                crate::theme::line(crate::theme::ACCENT),
            ))
    } else {
        // No explicit stroke — the theme reserves a 1 px (transparent) border on the
        // inactive state, matching the active button's accent border, so the two are the
        // same size.
        egui::Button::new(fonts::icon_label(glyph, label))
    };
    ui.add_enabled(enabled, button)
}

/// The Observe **arena overlays** (interactive, floating over the live arena): the
/// *follow* selector (bottom-left) and the *zoom / fit* controls (bottom-right), as
/// small `Area`s (ui-redesign §3). They write the auto-follow mode and the
/// [`crate::ViewControl`] — rendering only, never the sim. On a non-Background layer,
/// so `pointer_over_ui` counts a click on them as UI (not a sim click / deselect).
fn arena_controls(
    ctx: &egui::Context,
    rect: egui::Rect,
    auto: &mut AutoSelect,
    view: &mut crate::ViewControl,
    selected_pos: Option<Vec2>,
) {
    if rect.width() < 60.0 || rect.height() < 60.0 {
        return;
    }
    // Follow — bottom-left.
    egui::Area::new(egui::Id::new("arena_follow"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(rect.left() + 12.0, rect.bottom() - 12.0))
        .pivot(egui::Align2::LEFT_BOTTOM)
        .show(ctx, |ui| {
            overlay_frame().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Follow").on_hover_text(
                        "What the view auto-follows (same modes as the video). Manual = \
                         click an agent; a manual click always switches back to Manual.",
                    );
                    inspector::follow_combo(ui, "arena_follow_combo", &mut auto.roll);
                    if auto.roll.rolls() {
                        ui.add(egui::Slider::new(&mut auto.interval, 0.5..=20.0).text("s"));
                    }
                    // Brain filter — restrict the auto-follow to chosen brain families
                    // (e.g. hunters + MLPs only). Every family is offered, flora
                    // (Sessile) included — plants are entities like any other. The label
                    // goes accent while a filter is in effect.
                    let filtered = !auto.brains.is_all();
                    let brains_label = egui::RichText::new("Brains").color(if filtered {
                        crate::theme::ACCENT
                    } else {
                        crate::theme::INK_MUTED
                    });
                    // A sticky menu: toggling a family keeps it open (it closes only on a
                    // click outside).
                    crate::theme::sticky_menu(ui, egui::Button::new(brains_label), |ui| {
                        for (fi, name) in Brain::FAMILIES.iter().enumerate() {
                            crate::theme::toggle_row(ui, name, &mut auto.brains.0[fi]);
                        }
                        ui.separator();
                        if ui.button("Include all").clicked() {
                            auto.brains = BrainFilter::default();
                        }
                    })
                    .on_hover_text(
                        "Restrict the auto-follow to these brain families — a manual click \
                         still selects any agent.",
                    );
                });
            });
        });
    // Zoom / fit — bottom-right.
    egui::Area::new(egui::Id::new("arena_view"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(rect.right() - 12.0, rect.bottom() - 12.0))
        .pivot(egui::Align2::RIGHT_BOTTOM)
        .show(ctx, |ui| {
            overlay_frame().show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("+").on_hover_text("Zoom in").clicked() {
                        view.zoom_by(1.25);
                    }
                    if ui.button("-").on_hover_text("Zoom out").clicked() {
                        view.zoom_by(0.8);
                    }
                    // Two separate framing buttons — the active one is accented; any
                    // manual camera move de-accents both (cf. `ViewControl::set_free`).
                    let mode = view.mode();
                    // Fit arena.
                    if mode_button(
                        ui,
                        icons::RESET,
                        "Fit arena",
                        mode == crate::ViewMode::FitArena,
                        true,
                    )
                    .on_hover_text(crate::keymap::tooltip(
                        "Frame the whole arena",
                        crate::keymap::UiAction::ResetView,
                    ))
                    .clicked()
                    {
                        view.fit_arena();
                    }
                    // Follow entity — only meaningful with a selection.
                    if mode_button(
                        ui,
                        icons::EYE,
                        "Follow entity",
                        mode == crate::ViewMode::Follow,
                        selected_pos.is_some(),
                    )
                    .on_hover_text("Keep the view centered on the selected entity")
                    .clicked()
                    {
                        view.follow_selection();
                    }
                });
            });
        });
}

/// Builds the whole windowed layout in one pass: one background-layer root `Ui`, the
/// persistent [`nav_rail`], then the panels of the **current screen**
/// ([`Router::current`]) `show_inside` it. Chained **before** the interaction systems
/// (`pick_agent`, `resolve_drag`, …) and `set_sim_camera`, all of which read the
/// central rect it records in [`CentralRect`]. **Only [`Screen::Observe`]** leaves the
/// arena visible (a live central rect); every other screen fully covers the viewport
/// and records an **empty** rect, so the camera / picking systems idle — the
/// one-camera discipline (`docs/ui-redesign.md` §1).
#[allow(clippy::too_many_arguments)]
pub fn dock(
    mut contexts: EguiContexts,
    mut central: ResMut<CentralRect>,
    mut state: DockState,
    mut config: ResMut<SimConfig>,
    mut layers: ResMut<Layers>,
    mut palette: ResMut<Palette>,
    mut sim_controls: ResMut<SimControls>,
    mut vtime: ResMut<Time<Virtual>>,
    mut history: ResMut<History>,
    // Gate: don't render until the UI fonts are live (cf. `fonts`), so an icon is never
    // drawn before its Phosphor family is bound (egui binds fonts only next-pass).
    fonts_ready: Res<crate::fonts::FontsReady>,
    // Real (unpausable) time — stamps the status line so info/success messages expire
    // (presentation only, never the sim clock).
    time: Res<Time<Real>>,
    // Last frame's panel widths + left-region mode + transport width (cf. [`DockLayout`]).
    mut layout: Local<DockLayout>,
    // Observation: selection + auto-follow mode + the sim view's pan/zoom (bundled to
    // keep `dock` within the 16-param system limit — cf. [`ObsParams`]).
    mut obs: ObsParams,
    stats_agents: Query<(&Reserve, &Genotype, &Species), With<Agent>>,
    inspector_agents: Query<
        (
            &Species,
            &Reserve,
            &Genotype,
            &Vision,
            &Perception,
            &Action,
            &Brain,
            &Generation,
            &Age,
            &Nutrients,
        ),
        With<Agent>,
    >,
) -> Result {
    // Skip the first pass (before the fonts are bound): the icons would panic, and a
    // blank first frame on the paused startup screen is imperceptible.
    if !fonts_ready.0 {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    // Stamp a freshly-set status message with the current real time so it can expire.
    let now = time.elapsed_secs_f64();
    state.ui_status.stamp(now);
    // A single root viewport `Ui` on the background layer, shared by every panel
    // (bevy_egui 0.40 `examples/ui.rs`). `show_inside` then docks each panel into it.
    let mut root = egui::Ui::new(
        ctx.clone(),
        egui::Id::new("teemlab_dock"),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );

    // The persistent router rail — **always first** (unconditional → the later panels'
    // ids never shift, §2.5 of `docs/ui-spec.md`).
    nav_rail(&mut root, &mut state.router, &mut state.windows);

    // A colour-by-kind status line (the transient feedback sink, cf. `status`), reused by
    // the screens that surface it (Observe's bottom strip, Lab).
    let status_line = |ui: &mut egui::Ui, status: &crate::status::UiStatus| {
        let color = match status.kind {
            crate::status::StatusKind::Success => crate::theme::SUCCESS,
            crate::status::StatusKind::Error => crate::theme::ERROR,
            crate::status::StatusKind::Info => crate::theme::INK_MUTED,
        };
        ui.colored_label(color, &status.message);
    };

    // Dispatch on the current screen. Each arm builds that screen's panels; **only
    // Observe** returns a live arena rect (the transparent centre the sim shows through),
    // the others fully cover the content area (a `CentralPanel`) and return an empty rect,
    // so the camera / picking systems idle — the one-camera discipline (ui-redesign §1).
    let screen = state.router.current;
    let arena_rect = match screen {
        Screen::Observe => {
            // TOP STRIP: scenario IO (left) · transport (centred) · Export (right). View
            // layers move to the left panel, Help to the nav rail, Breeding to the Lab
            // screen — so the Observe strip is purely watch-a-run controls.
            egui::Panel::top("observe_top")
                .resizable(false)
                .default_size(56.0)
                .size_range(56.0..=56.0)
                .show_inside(&mut root, |ui| {
                    // The comp's 56 px strip; the row spans it so content centres.
                    let row_h = ui.available_height();
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), row_h),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            let full_w = ui.available_width();
                            ui.push_id("scenario_bar", |ui| {
                                runs::scenario_section(
                                    ui,
                                    &mut state.runs_panel,
                                    &mut config,
                                    &mut state.ui_status,
                                );
                            });
                            // Centre the transport on the whole bar, padding by last
                            // frame's measured width (a 1-frame lag; `scope` measures this
                            // frame's — same trick the old single strip used).
                            let left_w = full_w - ui.available_width();
                            let pad = (full_w * 0.5 - layout.ctrl_width * 0.5 - left_w).max(8.0);
                            ui.add_space(pad);
                            let measured = ui
                                .scope(|ui| {
                                    controls::controls_section(
                                        ui,
                                        &mut sim_controls,
                                        &mut vtime,
                                        &config,
                                        &state.world_baseline,
                                    )
                                })
                                .response
                                .rect
                                .width();
                            layout.ctrl_width = measured;
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // Record cluster (review): the top-bar affordance,
                                    // styled like the comp's accented button (accent-soft
                                    // fill, accent hairline, accent text). Its (sticky)
                                    // menu lists the recording's **components** then a
                                    // single *Run record* launcher — the only entry point
                                    // to a recording. A stronger wash signals a recording
                                    // is in flight.
                                    let recording = state.recorder_panel.is_recording();
                                    let rec_label = fonts::icon_label_tinted(
                                        icons::RECORD,
                                        if recording { "Recording" } else { "Record" },
                                        crate::theme::ACCENT,
                                    );
                                    let rest = if recording {
                                        crate::theme::ACCENT.gamma_multiply(0.28)
                                    } else {
                                        crate::theme::soft(crate::theme::ACCENT)
                                    };
                                    let rec_button = egui::Button::new(rec_label)
                                        .fill(rest)
                                        .stroke(egui::Stroke::new(
                                            1.0,
                                            crate::theme::line(crate::theme::ACCENT),
                                        ));
                                    crate::theme::sticky_menu(ui, rec_button, |ui| {
                                        record_menu(ui, &mut state.recorder_panel);
                                    });
                                },
                            );
                        },
                    );
                });

            // BOTTOM — the evolution curves, full width: docked **before** the side
            // panels so it spans the whole content width (the nav rail to the right edge,
            // the comp's full-bleed footer), the side columns sitting above it. Fixed and
            // tall enough to actually read the two plots — not resizable, not foldable.
            egui::Panel::bottom("observe_bottom")
                .resizable(false)
                .default_size(crate::layout::BOTTOM_DEFAULT)
                .size_range(crate::layout::BOTTOM_DEFAULT..=crate::layout::BOTTOM_DEFAULT)
                .show_inside(&mut root, |ui| {
                    hud::hud_section(ui, &mut history, &config);
                });

            // RIGHT — the agent inspector. Fixed width, always open (no fold, no drag): a
            // Capture routes the derived archetype into Studio to edit.
            egui::Panel::right("observe_right")
                .resizable(false)
                .default_size(crate::layout::INSPECTOR_DEFAULT)
                .size_range(crate::layout::INSPECTOR_DEFAULT..=crate::layout::INSPECTOR_DEFAULT)
                .show_inside(&mut root, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("observe_inspector_scroll")
                        .show(ui, |ui| {
                            // The header (title + the Capture menu, right-aligned) is drawn
                            // by `inspector_section` itself — the Capture menu needs the
                            // inspected agent's data. It returns a capture request, applied
                            // *after* the call so its shared `config` borrow has ended
                            // before the mutable one.
                            let inspector_action = inspector::inspector_section(
                                ui,
                                &obs.selection,
                                &config,
                                &mut palette.variant_name,
                                &inspector_agents,
                            );
                            match inspector_action {
                                Some(inspector::InspectorAction::Capture(arch)) => {
                                    let from = arch.captured_from.clone().unwrap_or_default();
                                    config.archetypes.push(arch);
                                    palette.selected = Some(config.archetypes.len() - 1);
                                    // Editing lives in Studio now — hand it there.
                                    state.router.current = Screen::Studio;
                                    state.ui_status.set(format!(
                                        "Captured to scenario (from {from}). Opened in Studio."
                                    ));
                                }
                                Some(inspector::InspectorAction::SaveVariant {
                                    species,
                                    variant,
                                }) => {
                                    let scenario = state.runs_panel.origin_label();
                                    let msg = editor::save_variant(
                                        &mut palette,
                                        &config,
                                        species as usize,
                                        variant,
                                        &scenario,
                                    );
                                    palette.variant_name.clear();
                                    state.ui_status.set_result(msg);
                                }
                                None => {}
                            }
                        });
                });

            // LEFT — live stats + view layers (ui-redesign §3). Fixed width, always open.
            egui::Panel::left("observe_left")
                .resizable(false)
                .default_size(crate::layout::STATS_DEFAULT)
                .size_range(crate::layout::STATS_DEFAULT..=crate::layout::STATS_DEFAULT)
                .show_inside(&mut root, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("observe_left_scroll")
                        .show(ui, |ui| {
                            crate::theme::caption(ui, "Live stats");
                            observe_population(ui, &history, &config);
                            ui.add_space(12.0);
                            crate::theme::caption(ui, "Layers");
                            editor::layers_section(ui, &mut layers, &config);
                            crate::theme::toggle_row(
                                ui,
                                "Trophic graph",
                                &mut state.windows.trophic_overlay,
                            )
                            .on_hover_text(
                                "Overlay the derived food web on the arena: node size = \
                                 population, edge colour = dependency \
                                 (docs/emergent-trophics.md §6).",
                            );
                            // Pinned last (review): a removal candidate — cf.
                            // docs/review-2026-07-15.md.
                            ui.add_space(12.0);
                            egui::CollapsingHeader::new("Per-gene means")
                                .default_open(false)
                                .show(ui, |ui| editor::stats_section(ui, &stats_agents, &config));
                        });
                });

            // The transparent centre: where `set_sim_camera` frames the live arena.
            // The comp frames the arena with a strong hairline (`--line-2`); its
            // rounded corners + shadow don't survive the camera compositing (the sim
            // renders beneath egui), so only the stroke ports.
            let arena = root.available_rect_before_wrap();
            root.painter().rect_stroke(
                arena.shrink(0.5),
                0.0,
                egui::Stroke::new(1.0, crate::theme::LINE_2),
                egui::StrokeKind::Inside,
            );
            arena
        }

        Screen::Studio => {
            // TOP STRIP: the document model (file name, dirty `*`, Save / Save As / Open /
            // Revert with the committed-example guardrails). The redesign's explicit
            // Overwrite/Save-as-new buttons are a later polish; the scenario menu already
            // carries the save model.
            egui::Panel::top("studio_top")
                .resizable(false)
                .show_inside(&mut root, |ui| {
                    ui.horizontal(|ui| {
                        ui.heading("Studio");
                        // The scenario menu chip carries the document name + dirty
                        // marker (cf. `runs::scenario_section`) — nothing repeated here.
                        runs::scenario_section(
                            ui,
                            &mut state.runs_panel,
                            &mut config,
                            &mut state.ui_status,
                        );
                    });
                });

            // LEFT — the World stage (arena, sources, components, gene bounds, allometric
            // costs, appearance). The old interaction-relations card is **gone**
            // (interactions are emergent, Phase A) — a large simplification (ui-redesign §5).
            egui::Panel::left("studio_world")
                .default_size(320.0)
                .resizable(true)
                .size_range(280.0..=460.0)
                .show_inside(&mut root, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("studio_world_scroll")
                        .show(ui, |ui| {
                            ui.strong("World");
                            editor::world_section(ui, &mut config);
                        });
                });

            // MIDDLE — the cast (master list): add / duplicate / reorder / delete; click a
            // species to edit it in the detail on the right.
            egui::Panel::left("studio_cast")
                .default_size(240.0)
                .resizable(true)
                .size_range(200.0..=360.0)
                .show_inside(&mut root, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("studio_cast_scroll")
                        .show(ui, |ui| {
                            ui.strong("Cast");
                            editor::selector_section(
                                ui,
                                &mut palette,
                                &mut config,
                                &mut state.ui_status,
                            );
                        });
                });

            // CENTRE (a `CentralPanel`, added last) — the static food-web validation
            // (A5 / the derived graph) above the selected archetype's editor; fills the
            // remaining width and hides the arena.
            egui::CentralPanel::default().show_inside(&mut root, |ui| {
                egui::CollapsingHeader::new("Food web · static validation")
                    .default_open(true)
                    .show(ui, |ui| {
                        // The derived graph (nodes = components ∪ archetypes; edges =
                        // edibility / absorption / emission) — built once, skinned three
                        // ways (`trophic`); here the static reachability surface.
                        let graph = crate::trophic::TrophicGraph::derive(&config);
                        graph.paint(ui, 150.0);
                        studio_validation(ui, &graph, &config);
                    });
                ui.separator();
                if palette
                    .selected
                    .is_some_and(|i| i < config.archetypes.len())
                {
                    if archetype_detail(ui, &mut palette, &mut config) {
                        palette.selected = None;
                    }
                } else {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.colored_label(
                            crate::theme::INK_MUTED,
                            "Select a species in the cast to edit it.",
                        );
                    });
                }
            });
            egui::Rect::ZERO
        }

        Screen::Lab => {
            // Headless breeding & sweeps — no live arena (ui-redesign §6). Experiment
            // **setup** on the left; **results** (dashboard + web-fragility) on the right.

            // LEFT — experiment setup: mode, sweep, run-record, save Experiment (params).
            egui::Panel::left("lab_setup")
                .default_size(320.0)
                .resizable(true)
                .size_range(280.0..=420.0)
                .show_inside(&mut root, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("lab_setup_scroll")
                        .show(ui, |ui| {
                            ui.heading("Experiment");
                            ui.weak("Run headless cohorts — breed, sweep, or nest both.");
                            ui.add_space(6.0);
                            crate::theme::caption(ui, "Scenario");
                            editor::card(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.label(
                                    egui::RichText::new(state.runs_panel.origin_label())
                                        .monospace(),
                                );
                            });
                            ui.add_space(8.0);

                            // The comp's mode selector: a card-filled segmented control,
                            // equal thirds, the active segment on the raised tone.
                            egui::Frame::new()
                                .fill(crate::theme::CARD)
                                .corner_radius(egui::CornerRadius::same(10))
                                .inner_margin(egui::Margin::same(4))
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    let seg_w = ((ui.available_width() - 8.0) / 3.0).max(40.0);
                                    for (m, label) in [
                                        (LabMode::Breed, "Breed"),
                                        (LabMode::Sweep, "Sweep"),
                                        (LabMode::Both, "Both (nested)"),
                                    ] {
                                        let active = state.lab.mode == m;
                                        let (fill, ink) = if active {
                                            (crate::theme::RAISED, crate::theme::INK)
                                        } else {
                                            (egui::Color32::TRANSPARENT, crate::theme::INK_MUTED)
                                        };
                                        if ui
                                            .add_sized(
                                                egui::vec2(seg_w, 28.0),
                                                egui::Button::new(
                                                    egui::RichText::new(label)
                                                        .size(12.5)
                                                        .color(ink),
                                                )
                                                .fill(fill)
                                                .corner_radius(egui::CornerRadius::same(7)),
                                            )
                                            .clicked()
                                        {
                                            state.lab.mode = m;
                                        }
                                    }
                                });
                            ui.add_space(6.0);

                            if state.lab.mode.has_sweep() {
                                editor::card(ui, |ui| {
                                    dot_caption(ui, crate::theme::ACCENT, "Outer · sweep");
                                    egui::Grid::new("sweep_grid").num_columns(2).show(ui, |ui| {
                                        ui.label("parameter");
                                        ui.add(
                                            egui::TextEdit::singleline(
                                                &mut state.lab.sweep.parameter,
                                            )
                                            .desired_width(150.0),
                                        );
                                        ui.end_row();
                                        ui.label("min");
                                        fonts::value(ui, |ui| {
                                            ui.add(
                                                egui::DragValue::new(&mut state.lab.sweep.min)
                                                    .speed(0.01),
                                            )
                                        });
                                        ui.end_row();
                                        ui.label("max");
                                        fonts::value(ui, |ui| {
                                            ui.add(
                                                egui::DragValue::new(&mut state.lab.sweep.max)
                                                    .speed(0.01),
                                            )
                                        });
                                        ui.end_row();
                                        ui.label("steps");
                                        fonts::value(ui, |ui| {
                                            ui.add(
                                                egui::DragValue::new(&mut state.lab.sweep.steps)
                                                    .range(1..=64),
                                            )
                                        });
                                        ui.end_row();
                                    });
                                    ui.weak(
                                        "Sweeps run headless via the `sweep` bin; saved here as \
                                         an Experiment.",
                                    );
                                });
                            }
                            // The nesting cue: an outer sweep over an inner breed.
                            if state.lab.mode == LabMode::Both {
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        fonts::icon(icons::ARROW_DOWN)
                                            .color(crate::theme::INK_FAINT),
                                    );
                                });
                            }
                            if state.lab.mode.has_breed() {
                                editor::card(ui, |ui| {
                                    dot_caption(ui, crate::theme::AMBER, "Inner · breed");
                                    if config.batch.is_some() {
                                        ui.weak(
                                            "Config in Studio's World editor; run it in the \
                                             results panel →",
                                        );
                                    } else {
                                        ui.colored_label(
                                            crate::theme::ACCENT,
                                            "No batch regime — add one in Studio's World editor.",
                                        );
                                    }
                                });
                            }

                            editor::card(ui, |ui| {
                                crate::theme::toggle_row(
                                    ui,
                                    "Save run record",
                                    &mut state.lab.run_record,
                                )
                                .on_hover_text(
                                    "Persist this run's metrics for Analyze (default on for the \
                                     Lab; inert until records land).",
                                );
                                ui.separator();
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut state.lab.experiment_name)
                                            .hint_text("experiment name")
                                            .desired_width(130.0),
                                    );
                                    if ui.button("Save Experiment").clicked() {
                                        let name = state.lab.experiment_name.trim().to_string();
                                        if name.is_empty() {
                                            state
                                                .ui_status
                                                .error("Name the experiment first.".to_string());
                                        } else {
                                            let exp = state.lab.to_experiment(
                                                state.runs_panel.origin_label(),
                                                config.seed,
                                            );
                                            let path = format!("{EXPERIMENTS_DIR}/{name}.ron");
                                            match exp.save_ron_file(&path) {
                                                Ok(()) => state
                                                    .ui_status
                                                    .set(format!("Experiment saved → {path}")),
                                                Err(e) => state
                                                    .ui_status
                                                    .error(format!("Save failed: {e}")),
                                            }
                                        }
                                    }
                                });
                            });
                        });
                });

            // CENTRE — results: fragility tiles, then the breeding dashboard.
            egui::CentralPanel::default().show_inside(&mut root, |ui| {
                if state.ui_status.visible(now) {
                    status_line(ui, &state.ui_status);
                    ui.separator();
                }
                let frag = crate::trophic::web_fragility(&config);
                let graph = crate::trophic::TrophicGraph::derive(&config);
                // Comp: three equal tiles across the results width.
                ui.columns(3, |cols| {
                    metric_tile(
                        &mut cols[0],
                        "SPECIES",
                        &config.archetypes.len().to_string(),
                        crate::theme::INK,
                    );
                    metric_tile(
                        &mut cols[1],
                        "TROPHIC LINKS",
                        &graph.edge_count().to_string(),
                        crate::theme::INK,
                    );
                    metric_tile(
                        &mut cols[2],
                        "WEB FRAGILITY",
                        &format!("{:.2}", frag.worst),
                        crate::theme::ACCENT,
                    );
                });
                ui.separator();
                if config.batch.is_some() {
                    egui::ScrollArea::vertical()
                        .id_salt("lab_results_scroll")
                        .show(ui, |ui| {
                            editor::card(ui, |ui| {
                                ui.strong("Breeding (generational)");
                                if let Some(act) = dashboard::breeding_panel(
                                    ui,
                                    &mut state.breeding,
                                    &config,
                                    &mut vtime,
                                    &mut state.ui_status,
                                ) {
                                    dashboard::apply_action(
                                        act,
                                        &mut config,
                                        &mut palette,
                                        &state.runs_panel,
                                        &mut state.ui_status,
                                        &mut sim_controls,
                                        &mut vtime,
                                    );
                                }
                            });
                        });
                } else if state.lab.mode.has_sweep() {
                    ui.add_space(20.0);
                    ui.colored_label(
                        crate::theme::INK_MUTED,
                        "Configure the sweep on the left and save it as an Experiment; run it \
                         headless with the `sweep` bin.",
                    );
                } else {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.colored_label(
                            crate::theme::INK_MUTED,
                            "This scenario has no batch regime. Add a `batch` block in Studio's \
                             World editor to breed a cohort here.",
                        );
                    });
                }
            });
            egui::Rect::ZERO
        }

        Screen::Library => {
            // The low-friction hub (ui-redesign §4): browse Worlds / Species, drop a cast
            // into a World, launch. Catalogs scanned once on the first visit.
            if !state.library.loaded {
                state.library.reload();
            }

            // TOP — title + search + the gallery / library tabs.
            egui::Panel::top("library_top")
                .resizable(false)
                .show_inside(&mut root, |ui| {
                    ui.horizontal(|ui| {
                        ui.heading("Library");
                        ui.label(
                            egui::RichText::new("pick a world, drop in species, launch")
                                .color(crate::theme::INK_MUTED),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut state.library.search)
                                    .hint_text("search name…")
                                    .desired_width(160.0),
                            );
                            if !state.library.search.is_empty() && ui.button("clear").clicked() {
                                state.library.search.clear();
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        let src = state.library.source;
                        let nw = state
                            .library
                            .worlds
                            .iter()
                            .filter(|w| w.source == src)
                            .count();
                        let ns = state
                            .library
                            .species
                            .iter()
                            .filter(|s| s.source == src)
                            .count();
                        for (tab, label, n) in [
                            (LibraryTab::Worlds, "Worlds", nw),
                            (LibraryTab::Species, "Species", ns),
                        ] {
                            if ui
                                .selectable_label(state.library.tab == tab, format!("{label}  {n}"))
                                .clicked()
                            {
                                state.library.tab = tab;
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            for (source, label) in [
                                (CatalogSource::Saved, "saved"),
                                (CatalogSource::Examples, "examples"),
                            ] {
                                if ui
                                    .selectable_label(state.library.source == source, label)
                                    .clicked()
                                {
                                    state.library.source = source;
                                }
                            }
                        });
                    });
                });

            // RIGHT — the compose tray: chosen World + cast + validator + launch.
            egui::Panel::right("library_tray")
                .default_size(300.0)
                .resizable(true)
                .size_range(260.0..=380.0)
                .show_inside(&mut root, |ui| {
                    crate::theme::caption(ui, "Compose");
                    ui.separator();
                    match state
                        .library
                        .chosen_world
                        .and_then(|i| state.library.worlds.get(i))
                    {
                        Some(w) => {
                            let (name, seed) = (w.name.clone(), w.world.seed);
                            editor::card(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let (thumb, _) = ui.allocate_exact_size(
                                        egui::vec2(46.0, 46.0),
                                        egui::Sense::hover(),
                                    );
                                    world_thumbnail(ui.painter(), thumb, seed);
                                    ui.vertical(|ui| {
                                        ui.strong(&name);
                                        ui.label(
                                            egui::RichText::new(format!("world · #{seed}"))
                                                .color(crate::theme::INK_MUTED)
                                                .size(11.5),
                                        );
                                    });
                                });
                            });
                        }
                        None => {
                            ui.weak("No world chosen — pick one in the Worlds gallery.");
                        }
                    }
                    ui.add_space(6.0);
                    crate::theme::caption(ui, "Cast");
                    let mut remove = None;
                    for (i, item) in state.library.cast.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(&item.entry.archetype.name);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button(fonts::icon(icons::X)).clicked() {
                                        remove = Some(i);
                                    }
                                    if ui.small_button("+").clicked() {
                                        item.count += 1;
                                    }
                                    ui.monospace(item.count.to_string());
                                    if ui.small_button("-").clicked() {
                                        item.count = item.count.saturating_sub(1);
                                    }
                                },
                            );
                        });
                    }
                    if let Some(i) = remove {
                        state.library.cast.remove(i);
                    }
                    if state.library.cast.is_empty() {
                        ui.weak("Add species from the Species gallery.");
                    }
                    ui.separator();

                    // Validator + launch (drop-only composition, §8).
                    let composed = state.library.compose();
                    if let Some(cfg) = &composed {
                        let g = crate::trophic::TrophicGraph::derive(cfg);
                        if g.is_viable() {
                            crate::theme::chip(ui, crate::theme::SUCCESS, "Food web is viable");
                        } else {
                            crate::theme::chip(
                                ui,
                                crate::theme::ERROR,
                                format!("{} broken chain(s) — refine in Studio", g.broken.len()),
                            );
                        }
                    }
                    ui.add_space(6.0);
                    ui.add_enabled_ui(composed.is_some(), |ui| {
                        let launch =
                            |target: Screen,
                             state: &mut DockState,
                             composed: &Option<SimConfig>| {
                                if let Some(cfg) = composed.clone() {
                                    state.runs_panel.load_config(cfg);
                                    state.router.current = target;
                                }
                            };
                        if crate::theme::primary_button(
                            ui,
                            fonts::icon_label_tinted(
                                icons::PLAY,
                                "Observe",
                                crate::theme::ON_ACCENT2,
                            ),
                        )
                        .clicked()
                        {
                            launch(Screen::Observe, &mut state, &composed);
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Open in Studio").clicked() {
                                launch(Screen::Studio, &mut state, &composed);
                            }
                            if ui.button("Send to Lab").clicked() {
                                launch(Screen::Lab, &mut state, &composed);
                            }
                        });
                    });
                });

            // CENTRE — the gallery (a scrollable list of cards, filtered by tab / source
            // / search).
            // The gallery sits on the page tone (comp: surface cards on `--bg`).
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .fill(crate::theme::BG)
                        .inner_margin(egui::Margin::same(18)),
                )
                .show_inside(&mut root, |ui| {
                    let search = state.library.search.trim().to_lowercase();
                    let source = state.library.source;
                    egui::ScrollArea::vertical()
                        .id_salt("library_gallery")
                        .show(ui, |ui| match state.library.tab {
                            LibraryTab::Worlds => {
                                let mut choose = None;
                                let mut delete = None;
                                let ids: Vec<usize> = (0..state.library.worlds.len())
                                    .filter(|&i| {
                                        let w = &state.library.worlds[i];
                                        w.source == source
                                            && (search.is_empty()
                                                || w.name.to_lowercase().contains(&search))
                                    })
                                    .collect();
                                ui.horizontal_wrapped(|ui| {
                                    // The comp's 16 px grid gap.
                                    ui.spacing_mut().item_spacing = egui::vec2(14.0, 14.0);
                                    for i in ids {
                                        let w = &state.library.worlds[i];
                                        let (name, seed, ncomp, nsrc, derived, path, selected) = (
                                            w.name.clone(),
                                            w.world.seed,
                                            w.world.components.len(),
                                            w.world.sources.len(),
                                            w.derived_from,
                                            w.path.clone(),
                                            state.library.chosen_world == Some(i),
                                        );
                                        // Force a top-down card: inside `horizontal_wrapped`
                                        // a child ui inherits the wrapping horizontal layout,
                                        // which would lay the thumbnail and body side by side.
                                        ui.allocate_ui_with_layout(
                                            egui::vec2(224.0, 208.0),
                                            egui::Layout::top_down(egui::Align::Min),
                                            |ui| {
                                                egui::Frame::default()
                                                    .fill(crate::theme::SURFACE)
                                                    .stroke(egui::Stroke::new(
                                                        1.0,
                                                        if selected {
                                                            crate::theme::line(crate::theme::ACCENT)
                                                        } else {
                                                            crate::theme::LINE
                                                        },
                                                    ))
                                                    .corner_radius(egui::CornerRadius::same(14))
                                                    .show(ui, |ui| {
                                                        ui.set_width(224.0);
                                                        let (thumb, _) = ui.allocate_exact_size(
                                                            egui::vec2(224.0, 130.0),
                                                            egui::Sense::hover(),
                                                        );
                                                        world_thumbnail(ui.painter(), thumb, seed);
                                                        egui::Frame::default()
                                                            .inner_margin(egui::Margin::same(12))
                                                            .show(ui, |ui| {
                                                                ui.horizontal(|ui| {
                                                                    ui.strong(&name);
                                                                    ui.with_layout(
                                                                        egui::Layout::right_to_left(
                                                                            egui::Align::Center,
                                                                        ),
                                                                        |ui| {
                                                                            ui.label(
                                                                        egui::RichText::new(
                                                                            format!(
                                                                                "#{}",
                                                                                seed % 100000
                                                                            ),
                                                                        )
                                                                        .monospace()
                                                                        .size(11.0)
                                                                        .color(
                                                                            crate::theme::INK_FAINT,
                                                                        ),
                                                                    );
                                                                        },
                                                                    );
                                                                });
                                                                ui.add(
                                                            egui::Label::new(
                                                                egui::RichText::new(format!(
                                                                    "{ncomp} components · \
                                                                     {nsrc} sources"
                                                                ))
                                                                .color(crate::theme::INK_MUTED)
                                                                .size(12.0),
                                                            )
                                                            .truncate(),
                                                        );
                                                                ui.horizontal(|ui| {
                                                                    if derived > 0 {
                                                                        ui.label(
                                                                    egui::RichText::new(format!(
                                                                        "{derived} scenario(s)"
                                                                    ))
                                                                    .color(crate::theme::INK_FAINT)
                                                                    .size(11.0),
                                                                );
                                                                    }
                                                                    ui.with_layout(
                                                                        egui::Layout::right_to_left(
                                                                            egui::Align::Center,
                                                                        ),
                                                                        |ui| {
                                                                            if ui
                                                                                .button("Use")
                                                                                .clicked()
                                                                            {
                                                                                choose = Some(i);
                                                                            }
                                                                            if path.is_some()
                                                                        && ui
                                                                            .small_button(
                                                                                fonts::icon(
                                                                                    icons::TRASH,
                                                                                ),
                                                                            )
                                                                            .clicked()
                                                                    {
                                                                        delete = path.clone();
                                                                    }
                                                                        },
                                                                    );
                                                                });
                                                            });
                                                    });
                                            },
                                        );
                                    }
                                });
                                if let Some(i) = choose {
                                    state.library.chosen_world = Some(i);
                                }
                                if let Some(path) = delete {
                                    let _ = std::fs::remove_file(&path);
                                    state.library.chosen_world = None; // indices change on rescan
                                    state.library.reload();
                                }
                            }
                            LibraryTab::Species => {
                                let mut add = None;
                                let mut delete = None;
                                let ids: Vec<usize> = (0..state.library.species.len())
                                    .filter(|&i| {
                                        let s = &state.library.species[i];
                                        s.source == source
                                            && (search.is_empty()
                                                || s.name.to_lowercase().contains(&search))
                                    })
                                    .collect();
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(14.0, 14.0);
                                    for i in ids {
                                        let s = &state.library.species[i];
                                        let (name, brain, color, saved, path, entry) = (
                                            s.name.clone(),
                                            s.entry.archetype.brain.name().to_string(),
                                            s.entry.archetype.color,
                                            s.source == CatalogSource::Saved,
                                            s.path.clone(),
                                            s.entry.clone(),
                                        );
                                        ui.allocate_ui_with_layout(
                                            egui::vec2(224.0, 78.0),
                                            egui::Layout::top_down(egui::Align::Min),
                                            |ui| {
                                                egui::Frame::default()
                                                    .fill(crate::theme::SURFACE)
                                                    .stroke(egui::Stroke::new(
                                                        1.0,
                                                        crate::theme::LINE,
                                                    ))
                                                    .corner_radius(egui::CornerRadius::same(14))
                                                    .inner_margin(egui::Margin::same(13))
                                                    .show(ui, |ui| {
                                                        ui.set_width(224.0);
                                                        ui.horizontal(|ui| {
                                                            let (dot, _) = ui.allocate_exact_size(
                                                                egui::vec2(14.0, 14.0),
                                                                egui::Sense::hover(),
                                                            );
                                                            ui.painter().circle_filled(
                                                                dot.center(),
                                                                6.0,
                                                                crate::theme::rgb(color),
                                                            );
                                                            ui.vertical(|ui| {
                                                                ui.strong(&name);
                                                                ui.label(
                                                                    egui::RichText::new(&brain)
                                                                        .color(
                                                                            crate::theme::INK_MUTED,
                                                                        )
                                                                        .size(11.5),
                                                                );
                                                            });
                                                            ui.with_layout(
                                                                egui::Layout::right_to_left(
                                                                    egui::Align::Center,
                                                                ),
                                                                |ui| {
                                                                    if ui.button("Add").clicked() {
                                                                        add = Some(entry.clone());
                                                                    }
                                                                    if saved
                                                                        && ui
                                                                            .small_button(
                                                                                fonts::icon(
                                                                                    icons::TRASH,
                                                                                ),
                                                                            )
                                                                            .clicked()
                                                                    {
                                                                        delete = Some(path.clone());
                                                                    }
                                                                },
                                                            );
                                                        });
                                                    });
                                            },
                                        );
                                    }
                                });
                                if let Some(entry) = add {
                                    state.library.add_to_cast(entry);
                                }
                                if let Some(path) = delete {
                                    let _ = std::fs::remove_file(&path);
                                    state.library.reload();
                                }
                            }
                        });
                });
            egui::Rect::ZERO
        }

        Screen::Analyze => {
            // LEFT — the record selector (ui-redesign §7). The **Experiment** params
            // saved from the Lab are the MVP persistence unit; the multi-select +
            // comparison over full **run records** (time series) are deferred, so the
            // list is shown dimmed.
            egui::Panel::left("analyze_records")
                .resizable(false)
                .default_size(250.0)
                .size_range(250.0..=250.0)
                .show_inside(&mut root, |ui| {
                    ui.strong("Records");
                    ui.weak("Select saved runs to compare.");
                    ui.separator();
                    let experiments = crate::files::ron_files(EXPERIMENTS_DIR);
                    if experiments.is_empty() {
                        ui.weak("No saved experiments yet — save one from the Lab.");
                    } else {
                        ui.add_enabled_ui(false, |ui| {
                            for path in &experiments {
                                let stem = std::path::Path::new(path)
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or(path);
                                let mut selected = false;
                                ui.checkbox(&mut selected, stem);
                            }
                        });
                    }
                });
            // CENTRE — the deferred comparison placeholder.
            placeholder_screen(
                &mut root,
                icons::CHART,
                "Post-hoc comparison",
                true,
                "Overlay populations, gene trajectories and component quantities across \
                 saved runs, compare species side by side, and export the data (CSV / PNG) \
                 for the falsifiable-knowledge deliverable. Deferred until run records \
                 (persisted time series) exist.",
                &[
                    "overlaid time series",
                    "small multiples",
                    "CSV / PNG export",
                ],
            );
            egui::Rect::ZERO
        }
    };
    central.0 = arena_rect;

    // Keyboard-shortcuts cheatsheet — **global** (any screen), toggled by `?` / F1 / the
    // nav rail's Help. A floating window over the same keymap tables the tooltips read.
    if state.windows.shortcuts {
        let mut open = true;
        egui::Window::new(fonts::icon_label(icons::CARET_DOWN, "Keyboard shortcuts"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .open(&mut open)
            .show(root.ctx(), shortcuts_cheatsheet);
        if !open {
            state.windows.shortcuts = false;
        }
    }

    // Sim-state overlay over the arena — **the arena screen only**: the run time (+ speed
    // when not ×1), a paused chip, and a first-steps hint on an empty arena; then the
    // interactive follow / zoom controls floating over the arena's corners.
    if screen.shows_arena() {
        let painter = root.painter().with_clip_rect(central.0);
        // The **dynamic trophic-graph overlay** (§7): the derived web over a translucent
        // backdrop, node size ∝ live population, edge colour ∝ dependency. Drawn first, so
        // the run-time read-out and the arena controls stay on top.
        if state.windows.trophic_overlay {
            painter.rect_filled(central.0, 0.0, egui::Color32::from_black_alpha(190));
            let graph = crate::trophic::TrophicGraph::derive(&config);
            let mut population = vec![0usize; config.archetypes.len()];
            for (_, _, species) in &stats_agents {
                if let Some(p) = population.get_mut(species.0 as usize) {
                    *p += 1;
                }
            }
            graph.paint_into(
                &painter,
                central.0,
                Some(crate::trophic::Live {
                    population: &population,
                    config: &config,
                }),
            );
        }
        central_overlay(
            &painter,
            central.0,
            // The **live** virtual-clock time, not the last metrics sample: the read-out
            // then refreshes every frame down to its smallest shown digit (0.1 s) instead
            // of stepping at the (coarser) sampling interval.
            vtime.elapsed_secs(),
            sim_controls.speed,
            vtime.is_paused(),
            stats_agents.is_empty(),
            !config.archetypes.is_empty(),
        );
        // World position of the selected agent (if any), for the Fit menu's
        // "Center on selection". Computed before the `&mut` borrows below (owned `Vec2`).
        let selected_pos = obs
            .selection
            .0
            .and_then(|e| obs.bodies.get(e).ok())
            .map(|tf| tf.translation.truncate());
        arena_controls(
            root.ctx(),
            central.0,
            &mut obs.auto_select,
            &mut obs.view,
            selected_pos,
        );
        // Follow tracking: while locked to Follow, keep the view centred on the selected
        // entity each frame; losing the target drops back to Free (de-accents the button).
        if obs.view.mode() == crate::ViewMode::Follow {
            match selected_pos {
                Some(p) => obs.view.center_on(p),
                None => obs.view.set_free(),
            }
        }
    }
    Ok(())
}

/// The run-time / speed read-out shown at the top of the sim area. The speed suffix is
/// dropped at ×1 (the default is noise). Pure, so it is unit-tested.
fn overlay_label(t: f32, speed: f32) -> String {
    if (speed - 1.0).abs() < 1e-3 {
        format!("t = {t:.1} s")
    } else {
        format!("t = {t:.1} s   ·   ×{speed:.1}")
    }
}

/// Paints the sim-area overlay: the [`overlay_label`] read-out, an accent **paused
/// chip** (which doubles as a "Space to run" affordance), and — on an empty arena — a
/// discreet hint on how to begin.
fn central_overlay(
    painter: &egui::Painter,
    rect: egui::Rect,
    run_time: f32,
    speed: f32,
    paused: bool,
    agents_empty: bool,
    has_archetypes: bool,
) {
    let cx = rect.center().x;
    // Run-time read-out in a translucent pill (the comp's blurred chip).
    let galley = painter.layout_no_wrap(
        overlay_label(run_time, speed),
        egui::FontId::monospace(11.0),
        crate::theme::INK_MUTED,
    );
    let pill = egui::Rect::from_center_size(
        egui::pos2(cx, rect.top() + 6.0 + galley.size().y * 0.5),
        galley.size() + egui::vec2(16.0, 6.0),
    );
    painter.rect_filled(pill, 8.0, egui::Color32::from_black_alpha(150));
    painter.galley(
        egui::pos2(cx - galley.size().x * 0.5, rect.top() + 6.0),
        galley,
        crate::theme::INK_MUTED,
    );
    if paused {
        let text = "Paused — Space to run";
        let font = egui::FontId::proportional(14.0);
        let galley = painter.layout_no_wrap(text.to_owned(), font.clone(), crate::theme::ACCENT);
        let top = rect.top() + 26.0;
        let chip = egui::Rect::from_center_size(
            egui::pos2(cx, top + galley.size().y * 0.5),
            galley.size() + egui::vec2(20.0, 8.0),
        );
        painter.rect_filled(chip, 6.0, crate::theme::ACCENT.gamma_multiply(0.15));
        painter.rect_stroke(
            chip,
            6.0,
            egui::Stroke::new(1.0, crate::theme::ACCENT),
            egui::StrokeKind::Inside,
        );
        painter.text(
            egui::pos2(cx, top),
            egui::Align2::CENTER_TOP,
            text,
            font,
            crate::theme::ACCENT,
        );
    }
    if agents_empty {
        let hint = if has_archetypes {
            "Press ▶ to run, or ⟲ Reset to (re)spawn the cast"
        } else {
            "Open a scenario, or add a species in Studio to begin"
        };
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            hint,
            egui::FontId::proportional(13.0),
            crate::theme::INK_FAINT,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_windows_defaults() {
        // The floating surfaces start closed; Observe's docked panels are fixed (no
        // open/fold state to hold anymore).
        let w = UiWindows::default();
        assert!(!w.shortcuts, "the cheatsheet starts closed");
        assert!(!w.trophic_overlay, "the trophic overlay starts off");
    }

    #[test]
    fn overlay_label_hides_unit_speed() {
        // At ×1 (the default) the speed suffix is dropped as noise.
        let at_one = overlay_label(12.34, 1.0);
        assert_eq!(at_one, "t = 12.3 s");
        assert!(!at_one.contains('×'));
        // Off-default speed is shown.
        assert!(overlay_label(12.0, 2.0).contains("×2.0"));
    }
}
