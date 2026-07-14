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
//! food-web validation; **Lab** is the headless breeding dashboard; **Library** and
//! **Analyze** are placeholders (built out in later stages). Help is **hover-first**
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
use teemlab::selection::{AutoSelect, Selection};
use teemlab::visuals::Layers;

use crate::controls::{self, SimControls};
use crate::dashboard::{self, BreedingSession};
use crate::editor::{self, Palette};
use crate::fonts::{self, icons};
use crate::hud;
use crate::inspector;
use crate::recorder::{self, RecorderPanel};
use crate::runs::{self, RunsPanel};
use crate::screen::{Router, Screen};
use crate::status::UiStatus;

/// Last frame's measured panel widths — the inputs the [`crate::layout`] range rule
/// needs this frame (a one-frame lag, harmless for sizing). A single [`Local`] so
/// [`dock`] adds no system parameter.
#[derive(Default)]
pub struct DockLayout {
    /// Left panel width, feeding the right panel's range (each side reserves the sim's
    /// minimum against the *other* side's width).
    left_w: f32,
    /// Right panel width, feeding the left panel's range.
    right_w: f32,
    /// Measured width of the centered transport controls (for centering — see below).
    ctrl_width: f32,
}

/// Visibility of the **user-toggleable floating surfaces** — the one convention for
/// "what's open": a bool per surface on a single resource. (egui memory holds only
/// per-widget presentation state like collapsing headers; `Option`-presence stays
/// reserved for genuine *selection*, e.g. `palette.selected`, never for visibility;
/// a scenario's `batch` being set is a data *precondition* for the Lab dashboard,
/// not its toggle.)
#[derive(Resource)]
pub struct UiWindows {
    /// The video **Export** window (Observe's Export button).
    pub export: bool,
    /// The keyboard-shortcuts **cheatsheet** (`?` / the nav rail's Help).
    pub shortcuts: bool,
    /// Observe's foldable regions, each collapsing to a thin **rail** so the arena can
    /// take the space back: the left (live stats + layers) column…
    pub left_open: bool,
    /// …the right (inspector) column…
    pub right_open: bool,
    /// …and the bottom strip (status + curves).
    pub bottom_open: bool,
}

impl Default for UiWindows {
    fn default() -> Self {
        Self {
            export: false,
            shortcuts: false,
            left_open: true,
            right_open: true,
            bottom_open: true,
        }
    }
}

impl UiWindows {
    /// The launch layout: **composing** (the empty canvas — every tool deployed) vs
    /// **observing** (a scenario passed on the CLI — the side columns folded, the
    /// arena and the curves lead; the tooling is one key / rail-click away).
    pub fn at_launch(observing: bool) -> Self {
        Self {
            left_open: !observing,
            right_open: !observing,
            ..Self::default()
        }
    }
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
    /// The breeding session (P5) — the **Lab** screen's dashboard reads/drives it (the
    /// generational `run → score → breed` loop over isolated worlds). Bundled here so
    /// `dock` stays within Bevy's 16-parameter limit.
    pub breeding: ResMut<'w, BreedingSession>,
    /// The config the running world was built from — the transport's Reset accents
    /// itself when the live config diverges from it (cf. `controls::world_diverged`).
    pub world_baseline: Res<'w, controls::WorldBaseline>,
}

/// **Observation** state of the right panel, bundled into one [`SystemParam`] so
/// [`dock`] stays within Bevy's 16-parameter limit: the current [`Selection`]
/// (read), the auto-follow mode ([`AutoSelect`]) and the sim view's pan/zoom
/// ([`crate::ViewControl`]) — all written/read by `inspector::observation_section`
/// and the inspector.
#[derive(SystemParam)]
pub struct ObsParams<'w> {
    pub selection: Res<'w, Selection>,
    pub auto_select: ResMut<'w, AutoSelect>,
    pub view: ResMut<'w, crate::ViewControl>,
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

/// Width of a folded region's **rail** (egui points): just enough for its chevron.
const RAIL_W: f32 = 26.0;

/// Overlays a small frameless chevron in an open region's **top-right corner** —
/// `ui.put` consumes no layout space, so the content keeps its full height — that
/// folds the region to its rail. The rail's chevron (cf. the `dock` rails) is the
/// mirror affordance, in the same spot the region folded from.
fn collapse_overlay(
    ui: &mut egui::Ui,
    glyph: char,
    action: crate::keymap::UiAction,
    open: &mut bool,
) {
    let r = ui.max_rect();
    let rect = egui::Rect::from_min_size(
        egui::pos2(r.right() - 20.0, r.top() + 2.0),
        egui::vec2(18.0, 18.0),
    );
    if ui
        .put(
            rect,
            egui::Button::new(fonts::icon(glyph)).small().frame(false),
        )
        .on_hover_text(crate::keymap::tooltip("Fold this panel", action))
        .clicked()
    {
        *open = false;
    }
}

/// A rail's reopen chevron (frameless, quiet). Returns `true` when clicked.
fn rail_chevron(
    ui: &mut egui::Ui,
    glyph: char,
    tip: &str,
    action: crate::keymap::UiAction,
) -> bool {
    ui.add(egui::Button::new(fonts::icon(glyph)).frame(false))
        .on_hover_text(crate::keymap::tooltip(tip, action))
        .clicked()
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
/// every screen (`docs/ui-redesign.md` §9).
const NAV_W: f32 = 78.0;

/// The persistent **screen router** rail on the far left: the five destinations
/// (Observe · Library · Studio · Lab · Analyze, [`Screen::ALL`]) in fixed order, plus
/// a Help affordance at the bottom. Present on **every** screen — only the content to
/// its right changes. Rendered **first** in [`dock`], so it is unconditional and its
/// ids never shift (§2.5 of `docs/ui-spec.md`). Clicking a destination writes
/// [`Router::current`]; the active one is accented.
fn nav_rail(root: &mut egui::Ui, router: &mut Router, windows: &mut UiWindows) {
    egui::Panel::left("nav_rail")
        .resizable(false)
        .default_size(NAV_W)
        .size_range(NAV_W..=NAV_W)
        .show_inside(root, |ui| {
            ui.add_space(8.0);
            // Plain-text wordmark (not a glyph — the embedded Inter subset renders some
            // PUA symbols as tofu, cf. the `*` dirty marker in `runs`).
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("teem")
                        .strong()
                        .size(13.0)
                        .color(crate::theme::ACCENT),
                );
            });
            ui.add_space(12.0);
            let w = ui.available_width();
            for screen in Screen::ALL {
                let active = router.current == screen;
                let mut text = egui::RichText::new(screen.label())
                    .size(11.5)
                    .color(if active {
                        crate::theme::ACCENT
                    } else {
                        crate::theme::INK_MUTED
                    });
                if active {
                    text = text.strong();
                }
                if ui
                    .add_sized([w, 34.0], egui::Button::selectable(active, text))
                    .clicked()
                {
                    router.current = screen;
                }
                ui.add_space(2.0);
            }
            // Push Help to the bottom of the rail.
            let rem = ui.available_height() - 36.0;
            if rem > 0.0 {
                ui.add_space(rem);
            }
            if ui
                .add_sized(
                    [w, 30.0],
                    egui::Button::new(
                        egui::RichText::new("Help")
                            .size(11.5)
                            .color(crate::theme::INK_MUTED),
                    )
                    .frame(false),
                )
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
fn studio_validation(ui: &mut egui::Ui, config: &SimConfig) {
    let broken = config.broken_chains();
    if broken.is_empty() {
        ui.colored_label(
            crate::theme::SUCCESS,
            "Food web viable — every need is reachable.",
        );
    } else {
        for (species, component) in broken {
            let sp = config
                .archetypes
                .get(species as usize)
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
}

/// A centred **placeholder** screen (Library not-yet-built, Analyze deferred): a
/// heading, an optional *deferred* chip, and a wrapped one-paragraph description.
/// Its own `CentralPanel` fills the whole content area, so the arena stays hidden
/// (the one-camera discipline — `docs/ui-redesign.md` §1).
fn placeholder_screen(root: &mut egui::Ui, title: &str, deferred: bool, body: &str) {
    egui::CentralPanel::default().show_inside(root, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.5 - 70.0).max(16.0));
            ui.heading(title);
            if deferred {
                ui.add_space(6.0);
                ui.colored_label(crate::theme::ACCENT, "Deferred · placeholder");
            }
            ui.add_space(10.0);
            ui.allocate_ui_with_layout(
                egui::vec2(440.0, 96.0),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ui.colored_label(crate::theme::INK_MUTED, body);
                },
            );
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

    // Width available to a *screen's* own panels once the rail has taken its strip: the
    // side-panel ranges reserve the sim's minimum against this, not the whole window.
    let viewport_w = root.ctx().viewport_rect().width();
    let content_w = (viewport_w - NAV_W).max(1.0);

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
                .show_inside(&mut root, |ui| {
                    let row_h = ui.spacing().interact_size.y;
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
                                    if ui
                                        .button(fonts::icon_label(icons::RECORD, "Export…"))
                                        .on_hover_text(
                                            "Render the current scenario to a video \
                                             (opens the export panel).",
                                        )
                                        .clicked()
                                    {
                                        state.windows.export = !state.windows.export;
                                    }
                                },
                            );
                        },
                    );
                });

            // RIGHT — the agent inspector (foldable). Follow selector pinned above the
            // scroll; a Capture routes the derived archetype into Studio to edit.
            let right_w = if !state.windows.right_open {
                egui::Panel::right("observe_right_rail")
                    .resizable(false)
                    .default_size(RAIL_W)
                    .size_range(RAIL_W..=RAIL_W)
                    .show_inside(&mut root, |ui| {
                        if rail_chevron(
                            ui,
                            icons::CARET_LEFT,
                            "Show the inspector",
                            crate::keymap::UiAction::ToggleRightPanel,
                        ) {
                            state.windows.right_open = true;
                        }
                    })
                    .response
                    .rect
                    .width()
            } else {
                egui::Panel::right("observe_right")
                    .default_size(crate::layout::SIDE_DEFAULT)
                    .resizable(true)
                    .size_range(crate::layout::side_range(content_w, layout.left_w))
                    .show_inside(&mut root, |ui| {
                        collapse_overlay(
                            ui,
                            icons::CARET_RIGHT,
                            crate::keymap::UiAction::ToggleRightPanel,
                            &mut state.windows.right_open,
                        );
                        inspector::observation_section(ui, &mut obs.auto_select, &mut obs.view);
                        egui::ScrollArea::vertical()
                            .id_salt("observe_inspector_scroll")
                            .show(ui, |ui| {
                                // `inspector_section` returns a capture request; it is
                                // `Some` only while the header is expanded (`flatten`), and
                                // applied *after* the call so its shared `config` borrow has
                                // ended before the mutable one.
                                let inspector_action =
                                    egui::CollapsingHeader::new("Agent inspector")
                                        .default_open(true)
                                        .show(ui, |ui| {
                                            inspector::inspector_section(
                                                ui,
                                                &obs.selection,
                                                &config,
                                                &mut palette.variant_name,
                                                &inspector_agents,
                                            )
                                        })
                                        .body_returned
                                        .flatten();
                                match inspector_action {
                                    Some(inspector::InspectorAction::Capture(arch)) => {
                                        let from = arch.captured_from.clone().unwrap_or_default();
                                        config.archetypes.push(arch);
                                        palette.selected = Some(config.archetypes.len() - 1);
                                        // Editing lives in Studio now — hand it there.
                                        state.router.current = Screen::Studio;
                                        state.ui_status.set(format!(
                                            "Captured to scenario (from {from}). \
                                             Opened in Studio."
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
                    })
                    .response
                    .rect
                    .width()
            };

            // LEFT — live stats + view layers (foldable): both live here (ui-redesign §3).
            let left_w = if !state.windows.left_open {
                egui::Panel::left("observe_left_rail")
                    .resizable(false)
                    .default_size(RAIL_W)
                    .size_range(RAIL_W..=RAIL_W)
                    .show_inside(&mut root, |ui| {
                        if rail_chevron(
                            ui,
                            icons::CARET_RIGHT,
                            "Show live stats & layers",
                            crate::keymap::UiAction::ToggleLeftPanel,
                        ) {
                            state.windows.left_open = true;
                        }
                    })
                    .response
                    .rect
                    .width()
            } else {
                egui::Panel::left("observe_left")
                    .default_size(crate::layout::SIDE_DEFAULT)
                    .resizable(true)
                    .size_range(crate::layout::side_range(content_w, right_w))
                    .show_inside(&mut root, |ui| {
                        collapse_overlay(
                            ui,
                            icons::CARET_LEFT,
                            crate::keymap::UiAction::ToggleLeftPanel,
                            &mut state.windows.left_open,
                        );
                        egui::ScrollArea::vertical()
                            .id_salt("observe_left_scroll")
                            .show(ui, |ui| {
                                egui::CollapsingHeader::new("Live stats")
                                    .default_open(true)
                                    .show(ui, |ui| {
                                        editor::stats_section(ui, &stats_agents, &config)
                                    });
                                egui::CollapsingHeader::new("Layers")
                                    .default_open(true)
                                    .show(ui, |ui| {
                                        editor::layers_section(ui, &mut layers, &config)
                                    });
                            });
                    })
                    .response
                    .rect
                    .width()
            };

            // BOTTOM — status line + evolution curves (foldable), spanning the central
            // width the side panels leave free.
            if !state.windows.bottom_open {
                egui::Panel::bottom("observe_bottom_rail")
                    .resizable(false)
                    .default_size(RAIL_W)
                    .size_range(RAIL_W..=RAIL_W)
                    .show_inside(&mut root, |ui| {
                        ui.horizontal(|ui| {
                            if rail_chevron(
                                ui,
                                icons::CARET_UP,
                                "Show the curves strip",
                                crate::keymap::UiAction::ToggleBottomPanel,
                            ) {
                                state.windows.bottom_open = true;
                            }
                            if state.ui_status.visible(now) {
                                status_line(ui, &state.ui_status);
                            }
                        });
                    });
            } else {
                egui::Panel::bottom("observe_bottom")
                    .resizable(true)
                    .default_size(300.0)
                    .size_range(260.0..=520.0)
                    .show_inside(&mut root, |ui| {
                        collapse_overlay(
                            ui,
                            icons::CARET_DOWN,
                            crate::keymap::UiAction::ToggleBottomPanel,
                            &mut state.windows.bottom_open,
                        );
                        if state.ui_status.visible(now) {
                            status_line(ui, &state.ui_status);
                            ui.add_space(2.0);
                        }
                        hud::hud_section(ui, &mut history, &config);
                    });
            }

            layout.left_w = left_w;
            layout.right_w = right_w;
            // The transparent centre: where `set_sim_camera` frames the live arena.
            root.available_rect_before_wrap()
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
                        ui.separator();
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
            // (A5) above the selected archetype's editor; fills the remaining width and
            // hides the arena.
            egui::CentralPanel::default().show_inside(&mut root, |ui| {
                studio_validation(ui, &config);
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
            // Headless breeding & sweeps — no live arena (ui-redesign §6). The breeding
            // dashboard runs the generational loop over isolated worlds on a worker.
            egui::CentralPanel::default().show_inside(&mut root, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Lab");
                    ui.label(
                        egui::RichText::new("headless breeding & sweeps")
                            .color(crate::theme::INK_MUTED),
                    );
                });
                if state.ui_status.visible(now) {
                    status_line(ui, &state.ui_status);
                }
                ui.separator();
                if config.batch.is_some() {
                    egui::ScrollArea::vertical()
                        .id_salt("lab_scroll")
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
                            ui.add_space(8.0);
                            ui.colored_label(
                                crate::theme::INK_MUTED,
                                "Sweeps (seed / parameter, and breed×sweep nesting) run via \
                                 the `sweep` bin for now; an in-app setup form lands in a \
                                 later stage.",
                            );
                        });
                } else {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.colored_label(
                            crate::theme::INK_MUTED,
                            "This scenario has no batch regime. Add a `batch` block in \
                             Studio's World editor to breed a cohort here.",
                        );
                    });
                }
            });
            egui::Rect::ZERO
        }

        Screen::Library => {
            placeholder_screen(
                &mut root,
                "Library",
                false,
                "Browse Worlds and Species, compose a Scenario in a few clicks, and manage \
                 the catalog. The catalog and drop-only composition land in a later stage; \
                 for now, compose and edit scenarios in Studio.",
            );
            egui::Rect::ZERO
        }

        Screen::Analyze => {
            placeholder_screen(
                &mut root,
                "Post-hoc comparison",
                true,
                "Overlay populations, gene trajectories and component quantities across \
                 saved runs, compare species side by side, and export the data (CSV / PNG) \
                 for the falsifiable-knowledge deliverable. Deferred until run records exist.",
            );
            egui::Rect::ZERO
        }
    };
    central.0 = arena_rect;

    // Floating "Export video" window — **Observe only** (it renders the current run).
    // Driven through a local `open` (the window's [x]) so it does not alias the
    // `&mut recorder_panel` the section needs.
    if screen.shows_arena() && state.windows.export {
        let mut open = true;
        egui::Window::new("Export video")
            .collapsible(true)
            .resizable(false)
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 36.0))
            .open(&mut open)
            .show(root.ctx(), |ui| {
                recorder::recorder_section(ui, &mut state.recorder_panel);
            });
        if !open {
            state.windows.export = false;
        }
    }

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
    // when not ×1), a paused chip, and a first-steps hint on an empty arena.
    if screen.shows_arena() {
        let painter = root.painter().with_clip_rect(central.0);
        central_overlay(
            &painter,
            central.0,
            history.latest_time(),
            sim_controls.speed,
            vtime.is_paused(),
            stats_agents.is_empty(),
            !config.archetypes.is_empty(),
        );
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
    painter.text(
        egui::pos2(cx, rect.top() + 6.0),
        egui::Align2::CENTER_TOP,
        overlay_label(run_time, speed),
        egui::FontId::monospace(11.0),
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
        let w = UiWindows::default();
        assert!(!w.export, "Export starts closed");
        assert!(!w.shortcuts, "the cheatsheet starts closed");
        assert!(
            w.left_open && w.right_open && w.bottom_open,
            "composing: every region deployed"
        );
    }

    #[test]
    fn launch_layout_folds_side_tooling_when_observing() {
        // A CLI scenario → observation: the side columns fold to rails, the arena and
        // the curves lead; the empty canvas keeps everything open (composing).
        let observing = UiWindows::at_launch(true);
        assert!(!observing.left_open && !observing.right_open);
        assert!(observing.bottom_open, "the curves are the observation tool");
        let composing = UiWindows::at_launch(false);
        assert!(composing.left_open && composing.right_open && composing.bottom_open);
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
