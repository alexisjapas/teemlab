//! **Windowed** entry point (direct).
//!
//! `DefaultPlugins` drives the window, rendering and presentation. Everything we
//! add here lives in `Update` and touches ONLY rendering / UI — never the
//! simulation state, which belongs to [`teemlab::SimPlugin`].

// Cf. `lib.rs`: Bevy queries trigger `type_complexity` by their very nature.
#![allow(clippy::type_complexity)]

mod controls;
mod editor;
mod fonts;
mod hud;
mod inspector;
mod panels;
mod recorder;
mod runs;
mod status;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use teemlab::metrics::MetricsPlugin;
use teemlab::selection::SelectionRenderPlugin;
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
                set_sim_camera,
            )
                .chain(),
        )
        .run();
}

/// Keyboard shortcuts mirroring the transport controls: **Space** play/pause, **→**
/// single-step (when paused), **R** reset. They only set `Time<Virtual>` / the
/// `SimControls` flags (the same paths as the buttons). We respect egui's keyboard
/// focus: when a text input (a RON path, a name…) has focus we let it keep the keys.
fn keyboard_shortcuts(
    mut contexts: EguiContexts,
    keys: Res<ButtonInput<KeyCode>>,
    mut vtime: ResMut<Time<Virtual>>,
    mut controls: ResMut<controls::SimControls>,
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
    Ok(())
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

/// Frames the simulation in the central area left free by the **docked panels**
/// (cf. `panels`): the **whole** arena is visible and centered ("see everything"
/// framing, small margin), whatever the window. The panels reserve the edges, so
/// the central rect shrinks to that center and the sim fits within it. The off-game
/// area around the arena (on the longer side) is grayed by `ClearColor` +
/// `draw_play_area`, so it does not look empty.
///
/// We **zoom and move the camera** rather than resizing its viewport: under
/// bevy_egui the egui surface is keyed to the camera's viewport, so shrinking it
/// would relaunch a layout → vibration. By keeping the viewport full-screen, the
/// egui surface is stable. Rendering only — never touches the sim state. Runs last
/// in the egui pass and reads [`panels::CentralRect`] (the region the panels leave
/// free, recorded by `panels::dock` via `available_rect_before_wrap` — the
/// non-deprecated successor of `ctx.available_rect()`). Picking stays correct
/// (`viewport_to_world_2d` reads the scale and the translation).
fn set_sim_camera(
    central: Res<panels::CentralRect>,
    config: Res<SimConfig>,
    windows: Query<&Window>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
) -> Result {
    /// Breathing margin around the arena (1.0 = flush with the edges).
    const VIEW_MARGIN: f32 = 1.06;

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

    // Scale = world units per pixel. Taking the SMALLEST side of the area gives
    // the largest scale that makes **the whole arena fit** (the long side keeps
    // margins, grayed as off-game); the margin adds a little air around it.
    // Default Camera2d projection: ScalingMode::WindowSize, origin at the center.
    let s = arena / wc.min(hc) * VIEW_MARGIN;
    if let Projection::Orthographic(ortho) = &mut *projection {
        ortho.scale = s;
    }

    // Move: the world origin (arena center) projects to the area's center `c`
    // (screen Y down ↔ world Y up), at scale `s`.
    let c = rect.center();
    transform.translation.x = (window.width() * 0.5 - c.x) * s;
    transform.translation.y = (c.y - window.height() * 0.5) * s;
    Ok(())
}
