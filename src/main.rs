//! **Windowed** entry point (direct).
//!
//! `DefaultPlugins` drives the window, rendering and presentation. Everything we
//! add here lives in `Update` and touches ONLY rendering / UI — never the
//! simulation state, which belongs to [`teemlab::SimPlugin`].

// Cf. `lib.rs`: Bevy queries trigger `type_complexity` by their very nature.
#![allow(clippy::type_complexity)]

mod controls;
mod editor;
mod files;
mod fonts;
mod help;
mod hud;
mod inspector;
mod panels;
mod recorder;
mod runs;
mod status;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use teemlab::metrics::MetricsPlugin;
use teemlab::selection::{AutoSelectPlugin, SelectionRenderPlugin, SelectionRoll};
use teemlab::visuals::VisualsPlugin;
use teemlab::{SimConfig, SimPlugin};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "teemlab".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        // With no argument, the windowed build starts on an **empty arena** (the
        // editor's canvas); an explicit scenario wins. The headless build keeps the
        // populated default (`from_cli`).
        .add_plugins(SimPlugin::new(SimConfig::from_cli_or(SimConfig::empty())))
        // Sim rendering shared with the video recorder (item 14) — including the
        // **backgrounds** (play area + off-game): `VisualsPlugin::draw_play_area`
        // reads their colors in the scenario and drives `ClearColor`, so the live
        // preview and the video render the same tints (cf. that function).
        .add_plugins(VisualsPlugin)
        // Highlight + rays of the selected agent — rendering **shared** with the
        // recorder (which drives the selection automatically). Here the target
        // comes from mouse picking (cf. `inspector`). Also provides the `Selection`
        // resource.
        .add_plugins(SelectionRenderPlugin)
        // **Auto-follow** — the same observation modes as the video recorder
        // (`SelectionRoll`), driven from the UI's "Follow" selector. Mounted **Off**
        // so the default stays manual mouse picking; the driver then *holds* whatever
        // the user clicks until it dies (cf. `selection::drive_selection`).
        .add_plugins(AutoSelectPlugin {
            roll: SelectionRoll::Off,
            interval: 4.0,
        })
        // Curve sampling (`History` resource + `sample_history`), shared with the
        // video recorder: the live preview and the video plot the same data.
        .add_plugins(MetricsPlugin)
        // NB: the **native Bevy visualizer** ([`teemlab::dataviz`]) is NOT mounted
        // here. It exists only for **video** rendering (`bin/record`): in the
        // windowed build, bevy_egui renders egui through the sim camera, so
        // recomposing the view would break the UI (cf. memory).
        .init_resource::<controls::SimControls>()
        .init_resource::<recorder::RecorderPanel>()
        // Central region left free by the docked panels (cf. `panels::dock`), read by
        // `set_sim_camera` to frame the sim — the non-deprecated `available_rect`.
        .init_resource::<panels::CentralRect>()
        // User pan/zoom of the sim view, layered on top of the fit-the-arena framing
        // (cf. `camera_navigation` / `set_sim_camera`). Default = the framed arena.
        .init_resource::<ViewControl>()
        // Single status line shown in the bottom bar (scenario / species / capture /
        // recording feedback), written from across the UI (cf. `status`).
        .init_resource::<status::UiStatus>()
        // Flips true once the UI fonts are live (cf. `fonts`), gating the first panel
        // render so an icon is never drawn before its font family is bound.
        .init_resource::<fonts::FontsReady>()
        // The sim starts **paused** (we prepare the run before launching it).
        .add_systems(
            Startup,
            (
                setup_camera,
                editor::build_palette,
                runs::build_runs_panel,
                controls::pause_at_launch,
            ),
        )
        // TIME CONTROL / RESET / RUNS (items 11, 13) — no sim logic: we set the
        // clock, reload a scenario, save/restore a run, or rebuild the world, all
        // before the frame's fixed loop. `apply_scenario_load` precedes
        // `apply_reset`: it sets the flag the latter consumes to rebuild the world
        // with the new scenario.
        .add_systems(
            PreUpdate,
            (
                controls::drive_steps,
                runs::apply_scenario_load,
                controls::apply_reset,
            )
                .chain(),
        )
        // RENDERING / OBSERVATION ONLY — never any sim logic here.
        // Sim rendering (mesh, arena, vision) lives in `VisualsPlugin`; curve
        // sampling in `MetricsPlugin` (lib, shared); here, the only observer
        // specific to the binary is the video-recording driver.
        .add_systems(Update, recorder::drive_recorder)
        // egui UI — **fixed docked panels** around the central simulation area, all
        // assembled by the single `panels::dock` system (one root `Ui`,
        // `show_inside`). The order is **chained** and matters: `dock` first (it
        // reserves the edges and records the free central rect in `panels::CentralRect`),
        // then the interactions AFTER — they read that rect via `panels::pointer_over_ui`
        // to tell a click on the sim from one on a panel; were they to run first, a
        // stale rect would let a click on a panel deselect the agent or a drop above it
        // place a hidden entity. `set_sim_camera` closes the pass: it reads the same
        // `CentralRect`, so the sim is framed exactly within the central area.
        .add_systems(
            EguiPrimaryContextPass,
            (
                // Installs the UI fonts (Inter / Departure Mono / Phosphor) on the egui
                // context once, before any panel renders (cf. `fonts`).
                fonts::setup_ui_fonts,
                panels::dock,
                inspector::pick_agent,
                inspector::delete_under_cursor,
                editor::resolve_drag,
                keyboard_shortcuts,
                camera_navigation,
                set_sim_camera,
            )
                .chain(),
        )
        .run();
}

/// Keyboard shortcuts mirroring the transport controls: **Space** play/pause, **→**
/// single-step (when paused), **R** reset (world), **Home** reset *view* (pan/zoom).
/// They only set `Time<Virtual>` / the `SimControls` flags / `ViewControl` (the same
/// paths as the buttons). We respect egui's keyboard focus: when a text input (a RON
/// path, a name…) has focus we let it keep the keys.
fn keyboard_shortcuts(
    mut contexts: EguiContexts,
    keys: Res<ButtonInput<KeyCode>>,
    mut vtime: ResMut<Time<Virtual>>,
    mut controls: ResMut<controls::SimControls>,
    mut view: ResMut<ViewControl>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    if ctx.egui_wants_keyboard_input() {
        return Ok(());
    }
    if keys.just_pressed(KeyCode::Space) {
        if vtime.is_paused() {
            vtime.unpause();
        } else {
            vtime.pause();
        }
    }
    // Single-step only makes sense while paused (mirrors the disabled Step button).
    if keys.just_pressed(KeyCode::ArrowRight) && vtime.is_paused() {
        controls.steps_pending += 1;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        controls.reset_requested = true;
    }
    // Recenter the view on the whole arena (mirrors the "Reset view" button).
    if keys.just_pressed(KeyCode::Home) {
        *view = ViewControl::default();
    }
    Ok(())
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

/// Breathing margin around the arena for the **base** framing (1.0 = flush with the
/// edges). Shared by `camera_navigation` (cursor-anchored zoom math) and
/// `set_sim_camera` (the actual framing), so the two agree on the base scale.
const VIEW_MARGIN: f32 = 1.06;

/// User pan/zoom of the sim view, layered on top of the automatic fit-the-arena
/// framing. **Rendering only** (a windowed-build resource, read in the egui pass);
/// it never touches the sim. The defaults reproduce the historical framing exactly
/// (whole arena, centered), so an untouched view is byte-for-byte the old behavior.
#[derive(Resource)]
struct ViewControl {
    /// World point shown at the **center** of the view (the pan target). The arena
    /// center `(0,0)` by default.
    look_at: Vec2,
    /// Zoom factor: `1` = the fit-the-arena framing, `>1` zooms in, `<1` out.
    zoom: f32,
}

impl Default for ViewControl {
    fn default() -> Self {
        Self {
            look_at: Vec2::ZERO,
            zoom: 1.0,
        }
    }
}

impl ViewControl {
    /// Zoom bounds: a little out (see beyond the arena) up to a deep close-up.
    const ZOOM_MIN: f32 = 0.4;
    const ZOOM_MAX: f32 = 40.0;
}

/// World units per egui point at the base (fit-the-arena, zoom = 1) framing — the
/// smallest side of the central rect makes the **whole** arena fit, plus the margin.
/// The single definition shared by zoom math and framing (cf. [`set_sim_camera`]).
fn base_scale(rect: bevy_egui::egui::Rect, arena: f32) -> f32 {
    arena / rect.width().min(rect.height()) * VIEW_MARGIN
}

/// Pan/zoom of the sim view (windowed only) — **scroll** to zoom toward the cursor,
/// **middle/right-drag** to pan, **Home** / the "Reset view" button to recenter. It
/// only writes [`ViewControl`]; [`set_sim_camera`] (next in the chain) turns that
/// into the camera transform. Rendering only, never the sim.
///
/// Runs **after** `panels::dock` (it needs the up-to-date [`panels::CentralRect`])
/// and **before** `set_sim_camera`, so the same frame's input feeds the framing. We
/// ignore input over a panel (via [`panels::pointer_over_ui`]) so scrolling a panel's
/// list doesn't zoom the world. All math is in egui points — the same space as
/// `CentralRect` and the cursor — matching `set_sim_camera`'s logical-pixel framing.
fn camera_navigation(
    mut contexts: EguiContexts,
    central: Res<panels::CentralRect>,
    config: Res<SimConfig>,
    mut view: ResMut<ViewControl>,
) -> Result {
    use bevy_egui::egui;
    let ctx = contexts.ctx_mut()?;

    let rect = central.0;
    let arena = 2.0 * config.arena_half_extent;
    if rect.width() < 1.0 || rect.height() < 1.0 || arena <= 0.0 {
        return Ok(());
    }
    // Only act when the pointer is over the sim, not over a docked panel/window.
    if panels::pointer_over_ui(ctx, rect) {
        return Ok(());
    }

    let s_eff = base_scale(rect, arena) / view.zoom; // world units per egui point now
    let c = rect.center();

    // SCROLL → zoom, anchored on the cursor (the world point under it stays put).
    // The mapping (derivation in `set_sim_camera`): a viewport point maps to
    // `look_at + k * s_eff`, with `k = (cursor.x - c.x, c.y - cursor.y)`. Keeping
    // that world point fixed across a scale change s0→s1 means
    // `look_at += k * (s0 - s1)`.
    let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
    if scroll.abs() > f32::EPSILON
        && let Some(p) = ctx.input(|i| i.pointer.hover_pos())
    {
        let new_zoom = (view.zoom * (scroll * 0.0015).exp())
            .clamp(ViewControl::ZOOM_MIN, ViewControl::ZOOM_MAX);
        let s_new = base_scale(rect, arena) / new_zoom;
        let k = Vec2::new(p.x - c.x, c.y - p.y);
        view.look_at += k * (s_eff - s_new);
        view.zoom = new_zoom;
    }

    // MIDDLE / RIGHT drag → pan. The world point under the cursor follows it:
    // `look_at += (-Δx, Δy) * s_eff` (screen Y is down, world Y up).
    let (panning, delta) = ctx.input(|i| {
        (
            i.pointer.middle_down() || i.pointer.secondary_down(),
            i.pointer.delta(),
        )
    });
    if panning && delta != egui::Vec2::ZERO {
        view.look_at += Vec2::new(-delta.x, delta.y) * s_eff;
    }
    Ok(())
}

/// Frames the simulation in the central area left free by the **docked panels**
/// (cf. `panels`): by default the **whole** arena is visible and centered ("see
/// everything" framing, small margin), whatever the window — then the user's
/// [`ViewControl`] (pan/zoom, written by `camera_navigation`) is layered on top. The
/// panels reserve the edges, so the central rect shrinks to that center and the sim
/// fits within it. The off-game area around the arena (on the longer side) is grayed
/// by `ClearColor` + `draw_play_area`, so it does not look empty.
///
/// We **zoom and move the camera** rather than resizing its viewport: under
/// bevy_egui the egui surface is keyed to the camera's viewport, so shrinking it
/// would relaunch a layout → vibration. By keeping the viewport full-screen, the
/// egui surface is stable. Rendering only — never touches the sim state. Runs last
/// in the egui pass and reads [`panels::CentralRect`] (the region the panels leave
/// free, recorded by `panels::dock` via `available_rect_before_wrap` — the
/// non-deprecated successor of `ctx.available_rect()`). Picking stays correct
/// (`viewport_to_world_2d` reads the scale and the translation).
///
/// View model (zoom = 1, look_at = 0 ⇒ the historical framing): a viewport point
/// maps to `look_at + ((px - c.x), (c.y - py)) * s_eff`, where `s_eff` is the
/// world-units-per-point scale (base scale ÷ zoom), `c` the central rect's center
/// and `(px, py)` the point in egui-point space. The camera translation that
/// realizes this puts `look_at` at the rect center: `T = look_at + centering(s_eff)`.
fn set_sim_camera(
    central: Res<panels::CentralRect>,
    config: Res<SimConfig>,
    view: Res<ViewControl>,
    windows: Query<&Window>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
) -> Result {
    let rect = central.0;
    let (Ok(window), Ok((mut transform, mut projection))) =
        (windows.single(), cameras.single_mut())
    else {
        return Ok(());
    };

    let (wc, hc) = (rect.width(), rect.height());
    let arena = 2.0 * config.arena_half_extent; // side of the square arena, in world units
    if wc < 1.0 || hc < 1.0 || arena <= 0.0 {
        return Ok(());
    }

    // Scale = world units per point. The base makes the WHOLE arena fit (smallest
    // side, the long side grayed as off-game); the user's zoom divides it (zoom in →
    // fewer world units per point). Default Camera2d projection: ScalingMode::WindowSize.
    let s = base_scale(rect, arena) / view.zoom;
    if let Projection::Orthographic(ortho) = &mut *projection {
        ortho.scale = s;
    }

    // Move so that `look_at` (arena center by default) projects to the area's center
    // `c` (screen Y down ↔ world Y up), at scale `s`; the user's pan rides along.
    let c = rect.center();
    transform.translation.x = view.look_at.x + (window.width() * 0.5 - c.x) * s;
    transform.translation.y = view.look_at.y + (c.y - window.height() * 0.5) * s;
    Ok(())
}
