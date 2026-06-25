//! **Native Bevy visualizer** of the stats / curves / inspector — the *rendered*
//! (non-egui) counterpart of the observation panels, so they appear **in the
//! video** (the egui overlay, for its part, is never filmed: §7).
//!
//! A shared rendering layer (like [`crate::visuals`]): everything lives in
//! `Update`, never any sim logic. The *data* (stats, curves) comes from
//! [`crate::metrics`] — therefore **exactly** the same numbers/polylines as the
//! egui versions; here we only *plot* them in Bevy (Text2d + Sprite + gizmos).
//!
//! ## 9:16 composition (fixed)
//! When the visualizer is **active**, the render target is recomposed into 9:16
//! portrait: the **square arena** occupies the top square, the **visualizer** the
//! bottom strip (width × 7/9). Two extra cameras (one background/letterbox, one for
//! the visualizer) frame the sim camera, whose `viewport` we set. The windowed
//! "presentation" mode and the video rendering take **the same** path → the editor
//! preview is strictly identical to the video.
//!
//! ## Section rotation
//! The bottom strip is of fixed size: we show the **stats** permanently (at the
//! top) and **rotate** the rest — curves then inspector — every `interval` seconds
//! (configurable).

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, ScalingMode, Viewport};
use bevy::prelude::*;
use bevy::sprite::{Anchor, Text2d};

use crate::brain::{Brain, MlpBrain};
use crate::components::{Action, Age, Agent, Generation, Perception, Reserve, Species, Vision};
use crate::config::SimConfig;
use crate::genotype::{Genotype, TRAITS};
use crate::metrics::{Curve, History, live_stats, population_curves, trait_curves};
use crate::selection::Selection;

/// Render layer of the visualizer (Text2d / Sprite / `VizGizmos` gizmos).
const VIZ_LAYER: usize = 1;
/// Layer of the background (letterbox) camera: no entity, it only *clears*.
const LETTERBOX_LAYER: usize = 2;
/// Logical canvas of the visualizer (9:7 ratio = the bottom strip of a 9:16 frame).
/// We draw in this virtual pixel frame (origin at the top-left corner), independent
/// of the real resolution → identical layout between the editor preview and the
/// video.
const VIZ_W: f32 = 900.0;
const VIZ_H: f32 = 700.0;
/// Breathing margin around the arena in the top square.
const ARENA_MARGIN: f32 = 1.08;
/// Number of rotating pages (0 = curves, 1 = inspector).
const PAGES: usize = 2;

/// Gizmo group dedicated to the visualizer, restricted to [`VIZ_LAYER`] (cf. `build`).
#[derive(Default, Reflect, GizmoConfigGroup)]
struct VizGizmos;

/// State of the visualizer: active or not, and the section rotation.
#[derive(Resource)]
pub struct DataViz {
    /// Does the visualizer recompose the view (9:16) and draw?
    pub active: bool,
    /// Section rotation interval (simulated seconds).
    pub interval: f32,
    elapsed: f32,
    page: usize,
    was_active: bool,
}

/// Adds the native visualizer. `enabled` = initial state (video: `true`; editor:
/// `false`, toggled by a key). To be combined with [`crate::metrics::MetricsPlugin`]
/// (the curve data) and a sim camera provided by the binary.
pub struct DataVizPlugin {
    pub enabled: bool,
    pub interval: f32,
}

impl Plugin for DataVizPlugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<VizGizmos>()
            .insert_resource(DataViz {
                active: self.enabled,
                interval: self.interval.max(0.5),
                elapsed: 0.0,
                page: 0,
                was_active: false,
            })
            .add_systems(Startup, load_viz_font)
            .add_systems(
                Update,
                (
                    manage_viz_cameras,
                    advance_page,
                    compose_viewports,
                    draw_viz,
                )
                    .chain(),
            );

        // The visualizer's gizmos must appear only in its camera (its strip), not on
        // top of the sim: we restrict them to its layer.
        let mut store = app.world_mut().resource_mut::<GizmoConfigStore>();
        let (config, _) = store.config_mut::<VizGizmos>();
        config.render_layers = RenderLayers::layer(VIZ_LAYER);
    }
}

/// The visualizer's font. The default Bevy font is ASCII-only; we embed DejaVu Sans
/// (free) for full glyph coverage (degree signs, bullets, …) — as in the egui
/// preview.
#[derive(Resource)]
struct VizFont(Handle<Font>);

/// `Startup`: loads the visualizer's font from `assets/fonts/`.
fn load_viz_font(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(VizFont(assets.load("fonts/DejaVuSans.ttf")));
}

/// Marker of the visualizer's camera (bottom strip).
#[derive(Component)]
struct VizCamera;
/// Marker of the background/letterbox camera (full frame, only clears).
#[derive(Component)]
struct LetterboxCamera;
/// Marker of the display entities recreated every frame (text, bars).
#[derive(Component)]
struct VizEntity;

/// `Update`: creates the two framing cameras **on activation** and destroys them on
/// return to normal mode. Crucial for the windowed build: in normal mode, only a
/// single `Camera2d` must remain, otherwise bevy_egui no longer resolves its
/// primary context (no panels left) and the camera `single()` queries fail. We
/// therefore create them only in presentation/video, targeting the same render as
/// the sim camera.
fn manage_viz_cameras(
    mut commands: Commands,
    viz: Res<DataViz>,
    existing: Query<Entity, Or<(With<VizCamera>, With<LetterboxCamera>)>>,
    sim: Query<
        Option<&RenderTarget>,
        (With<Camera2d>, Without<VizCamera>, Without<LetterboxCamera>),
    >,
) {
    let present = !existing.is_empty();
    if !viz.active {
        // Return to normal mode: we remove the framing cameras (egui comes back).
        if present {
            for e in &existing {
                commands.entity(e).despawn();
            }
        }
        return;
    }
    if present {
        return; // already in place; `compose_viewports` sets their viewports.
    }

    // Activation: we create the cameras (inactive; `compose_viewports` will turn
    // them on once their viewports are set, the next frame — no full-frame flash).
    let target = sim.single().ok().flatten().cloned();

    // Background / letterbox: full frame (no viewport), clears to black, renders nothing.
    let mut letterbox = commands.spawn((
        Camera2d,
        Camera {
            order: -1,
            is_active: false,
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        Projection::from(OrthographicProjection::default_2d()),
        RenderLayers::layer(LETTERBOX_LAYER),
        LetterboxCamera,
    ));
    if let Some(t) = &target {
        letterbox.insert(t.clone());
    }

    // Visualizer: fixed logical canvas 900×700 (origin at center), above the sim.
    let mut viz = commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            is_active: false,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.06, 0.06, 0.09)),
            ..default()
        },
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: VIZ_W,
                height: VIZ_H,
            },
            ..OrthographicProjection::default_2d()
        }),
        RenderLayers::layer(VIZ_LAYER),
        VizCamera,
    ));
    if let Some(t) = &target {
        viz.insert(t.clone());
    }
}

/// `Update`: rotates the displayed page every `interval` seconds (simulated time,
/// hence frozen while paused). Inactive → touches nothing.
fn advance_page(time: Res<Time<Virtual>>, mut viz: ResMut<DataViz>) {
    if !viz.active {
        return;
    }
    viz.elapsed += time.delta_secs();
    if viz.elapsed >= viz.interval {
        viz.elapsed = 0.0;
        viz.page = (viz.page + 1) % PAGES;
    }
}

/// `Update`: recomposes the viewports into **full-frame** 9:16 when active, and
/// restores the normal framing on deactivation. The top square receives the sim
/// (framed arena), the bottom strip the visualizer; the margins (non-9:16 target)
/// are blackened by the background camera.
fn compose_viewports(
    mut viz: ResMut<DataViz>,
    config: Res<SimConfig>,
    mut sim: Query<
        (&mut Camera, &mut Projection, &mut Transform),
        (With<Camera2d>, Without<VizCamera>, Without<LetterboxCamera>),
    >,
    mut viz_cam: Query<&mut Camera, (With<VizCamera>, Without<LetterboxCamera>)>,
    mut letterbox: Query<&mut Camera, With<LetterboxCamera>>,
) {
    let active = viz.active;
    if active {
        let Ok((mut sim_cam, mut sim_proj, mut sim_tf)) = sim.single_mut() else {
            return;
        };
        // Base region: the whole render target (window or video image).
        let Some(target) = sim_cam.physical_target_size() else {
            return;
        };
        let (bx, by, bw, bh) = (0.0, 0.0, target.x as f32, target.y as f32);

        // Largest 9:16 rectangle **inscribed** in the target (never an overflow → no
        // off-target viewport). On a 9:16 target (1080×1920 video) it fills exactly;
        // on a wider/taller target, letterbox (bands blackened by the background camera).
        const ASPECT: f32 = 9.0 / 16.0;
        let (rw, rh) = if bw / bh > ASPECT {
            (bh * ASPECT, bh)
        } else {
            (bw, bw / ASPECT)
        };
        let ox = bx + (bw - rw) * 0.5;
        let oy = by + (bh - rh) * 0.5;
        let square = rw; // top square = full width of the 9:16 frame

        // Sim camera: viewport = top square, framed arena (AutoMin), centered.
        sim_cam.viewport = Some(Viewport {
            physical_position: UVec2::new(ox.max(0.0) as u32, oy.max(0.0) as u32),
            physical_size: UVec2::new(square.max(1.0) as u32, square.max(1.0) as u32),
            ..default()
        });
        sim_cam.is_active = true;
        let span = 2.0 * config.arena_half_extent * ARENA_MARGIN;
        match &mut *sim_proj {
            Projection::Orthographic(o) => {
                o.scaling_mode = ScalingMode::AutoMin {
                    min_width: span,
                    min_height: span,
                };
            }
            other => {
                *other = Projection::from(OrthographicProjection {
                    scaling_mode: ScalingMode::AutoMin {
                        min_width: span,
                        min_height: span,
                    },
                    ..OrthographicProjection::default_2d()
                });
            }
        }
        sim_tf.translation.x = 0.0;
        sim_tf.translation.y = 0.0;

        if let Ok(mut vc) = viz_cam.single_mut() {
            vc.viewport = Some(Viewport {
                physical_position: UVec2::new(ox.max(0.0) as u32, (oy + square).max(0.0) as u32),
                physical_size: UVec2::new(rw.max(1.0) as u32, (rh - square).max(1.0) as u32),
                ..default()
            });
            vc.is_active = true;
        }
        // Background camera: clears **the region** (not the whole window, otherwise
        // the egui panels would be blackened); its margins (non-9:16 region) stay black.
        if let Ok(mut lc) = letterbox.single_mut() {
            lc.viewport = Some(Viewport {
                physical_position: UVec2::new(bx.max(0.0) as u32, by.max(0.0) as u32),
                physical_size: UVec2::new(bw.max(1.0) as u32, bh.max(1.0) as u32),
                ..default()
            });
            lc.is_active = true;
        }
    } else if viz.was_active {
        // Deactivation: we give the full frame back to the sim (the windowed build
        // resumes its egui framing via `set_sim_camera`) and turn off the framing cameras.
        if let Ok((mut sim_cam, mut sim_proj, _)) = sim.single_mut() {
            sim_cam.viewport = None;
            *sim_proj = Projection::from(OrthographicProjection::default_2d());
        }
        if let Ok(mut vc) = viz_cam.single_mut() {
            vc.is_active = false;
        }
        if let Ok(mut lc) = letterbox.single_mut() {
            lc.is_active = false;
        }
    }
    viz.was_active = active;
}

// ---------------------------------------------------------------------------
// Drawing (immediate: we recreate text/bars every frame; the gizmos already are)
// ---------------------------------------------------------------------------

/// Top-left px (0..VIZ_W, 0..VIZ_H, y downward) → canvas world (origin at center,
/// y upward).
fn p(x: f32, y: f32) -> Vec2 {
    Vec2::new(x - VIZ_W * 0.5, VIZ_H * 0.5 - y)
}

fn srgb([r, g, b]: [f32; 3]) -> Color {
    Color::srgb(r, g, b)
}

/// Spawns text aligned top-left at the canvas's px position (DejaVu font for the
/// non-ASCII glyphs).
fn text(
    commands: &mut Commands,
    font: &Handle<Font>,
    s: impl Into<String>,
    x: f32,
    y: f32,
    size: f32,
    color: Color,
) {
    let pos = p(x, y);
    commands.spawn((
        Text2d::new(s.into()),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
        Anchor::TOP_LEFT,
        Transform::from_xyz(pos.x, pos.y, 0.3),
        RenderLayers::layer(VIZ_LAYER),
        VizEntity,
    ));
}

/// Spawns a filled rectangle, top-left corner at (x, y) px, size (w, h) px.
fn rect(commands: &mut Commands, x: f32, y: f32, w: f32, h: f32, color: Color, z: f32) {
    let pos = p(x, y);
    commands.spawn((
        Sprite::from_color(color, Vec2::new(w.max(0.0), h)),
        Anchor::TOP_LEFT,
        Transform::from_xyz(pos.x, pos.y, z),
        RenderLayers::layer(VIZ_LAYER),
        VizEntity,
    ));
}

/// A progress bar (background + fill), `frac` ∈ [0, 1].
fn bar(commands: &mut Commands, x: f32, y: f32, w: f32, h: f32, frac: f32, fill: Color) {
    rect(commands, x, y, w, h, Color::srgb(0.16, 0.16, 0.20), 0.1);
    rect(commands, x, y, w * frac.clamp(0.0, 1.0), h, fill, 0.2);
}

/// `Update`: redraws the whole visualizer. Recreates the text/bar entities (the
/// previous ones are removed); the gizmos are already immediate.
#[allow(clippy::too_many_arguments)]
fn draw_viz(
    mut commands: Commands,
    viz: Res<DataViz>,
    old: Query<Entity, With<VizEntity>>,
    mut gizmos: Gizmos<VizGizmos>,
    font: Res<VizFont>,
    config: Res<SimConfig>,
    history: Res<History>,
    selection: Res<Selection>,
    stats_q: Query<(&Reserve, &Genotype, &Brain), With<Agent>>,
    inspect_q: Query<
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
        ),
        With<Agent>,
    >,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    if !viz.active {
        return;
    }
    let font = &font.0;

    draw_stats(&mut commands, font, &stats_q);

    match viz.page {
        0 => draw_curves(&mut commands, &mut gizmos, font, &history, &config),
        _ => draw_inspector(&mut commands, &mut gizmos, font, &selection, &inspect_q),
    }

    // Page indicator (bottom-right corner).
    let label = if viz.page == 0 { "curves" } else { "inspector" };
    text(
        &mut commands,
        font,
        format!("● {label}"),
        VIZ_W - 150.0,
        VIZ_H - 24.0,
        15.0,
        Color::srgb(0.5, 0.5, 0.55),
    );
}

/// Global stats, always at the top of the strip. Same numbers as `editor::stats_section`.
fn draw_stats(
    commands: &mut Commands,
    font: &Handle<Font>,
    agents: &Query<(&Reserve, &Genotype, &Brain), With<Agent>>,
) {
    let s = live_stats(agents);
    let ink = Color::srgb(0.92, 0.92, 0.95);
    text(
        commands,
        font,
        format!(
            "Pop {}    Food {}    Reserve {:.0}",
            s.population, s.food, s.mean_reserve
        ),
        24.0,
        16.0,
        26.0,
        ink,
    );
    let genes: Vec<String> = TRAITS
        .iter()
        .zip(&s.mean_traits)
        .map(|(t, m)| format!("{} {:.*}", t.name, t.decimals as usize, m))
        .collect();
    text(
        commands,
        font,
        format!("mean genes — {}", genes.join("   ")),
        24.0,
        52.0,
        15.0,
        Color::srgb(0.65, 0.65, 0.7),
    );
}

/// "Curves" page: population (top) and gene drift (bottom), via [`crate::metrics`].
fn draw_curves(
    commands: &mut Commands,
    gizmos: &mut Gizmos<VizGizmos>,
    font: &Handle<Font>,
    history: &History,
    config: &SimConfig,
) {
    if history.is_empty() {
        text(
            commands,
            font,
            "(waiting for data…)",
            24.0,
            120.0,
            18.0,
            Color::srgb(0.6, 0.6, 0.65),
        );
        return;
    }

    let (pop, y_max) = population_curves(history, config);
    text(
        commands,
        font,
        "Population per species",
        24.0,
        100.0,
        18.0,
        Color::WHITE,
    );
    plot(
        commands,
        gizmos,
        font,
        &pop,
        0.0,
        y_max * 1.1,
        (24.0, 126.0, 852.0, 198.0),
    );

    text(
        commands,
        font,
        "Gene drift (0–1)",
        24.0,
        356.0,
        18.0,
        Color::WHITE,
    );
    let traits = trait_curves(history);
    plot(
        commands,
        gizmos,
        font,
        &traits,
        0.0,
        1.0,
        (24.0, 382.0, 852.0, 198.0),
    );
}

/// Draws a frame, its polylines and a legend, in `(x, y, w, h)` px of the canvas.
fn plot(
    commands: &mut Commands,
    gizmos: &mut Gizmos<VizGizmos>,
    font: &Handle<Font>,
    curves: &[Curve],
    y_min: f32,
    y_max: f32,
    (x, y, w, h): (f32, f32, f32, f32),
) {
    // Frame.
    let center = p(x + w * 0.5, y + h * 0.5);
    gizmos.rect_2d(center, Vec2::new(w, h), Color::srgb(0.3, 0.3, 0.35));

    if curves.is_empty() {
        return;
    }
    let (mut x_min, mut x_max) = (f32::MAX, f32::MIN);
    for c in curves {
        for q in &c.pts {
            x_min = x_min.min(q[0]);
            x_max = x_max.max(q[0]);
        }
    }
    if !x_max.is_finite() || x_max <= x_min {
        return;
    }
    let y_span = (y_max - y_min).max(1e-6);
    // Data point → canvas px (top = high value), then world.
    let map = |t: f32, v: f32| {
        let px = x + (t - x_min) / (x_max - x_min) * w;
        let py = y + h - (v - y_min) / y_span * h;
        p(px, py)
    };
    for c in curves {
        let col = srgb(c.color);
        for win in c.pts.windows(2) {
            gizmos.line_2d(map(win[0][0], win[0][1]), map(win[1][0], win[1][1]), col);
        }
    }

    // Legend: dot + name, in a row under the frame (no overlap beyond 6).
    let mut lx = x;
    let ly = y + h + 4.0;
    for c in curves {
        rect(commands, lx, ly + 4.0, 10.0, 10.0, srgb(c.color), 0.2);
        text(
            commands,
            font,
            &c.name,
            lx + 14.0,
            ly,
            13.0,
            Color::srgb(0.8, 0.8, 0.85),
        );
        lx += 130.0;
    }
}

/// "Inspector" page: the selected agent (same information as `inspector_section`,
/// minus the "Capture" button which is an interaction, not data).
fn draw_inspector(
    commands: &mut Commands,
    gizmos: &mut Gizmos<VizGizmos>,
    font: &Handle<Font>,
    selection: &Selection,
    agents: &Query<
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
        ),
        With<Agent>,
    >,
) {
    let dim = Color::srgb(0.6, 0.6, 0.65);
    let Some(entity) = selection.0 else {
        text(commands, font, "No agent selected.", 24.0, 120.0, 18.0, dim);
        return;
    };
    let Ok((species, reserve, genotype, vision, perception, action, brain, generation, age)) =
        agents.get(entity)
    else {
        text(
            commands,
            font,
            "The inspected agent no longer exists.",
            24.0,
            120.0,
            18.0,
            dim,
        );
        return;
    };

    let immobile = genotype.locomotion().is_immobile();
    let ink = Color::srgb(0.9, 0.9, 0.93);
    let key = Color::srgb(0.7, 0.7, 0.75);

    // --- Left column: identity, energy, genes, action ---
    text(
        commands,
        font,
        format!("Species {}", species.0),
        24.0,
        100.0,
        18.0,
        ink,
    );
    text(
        commands,
        font,
        format!("Brain: {}", brain.name()),
        24.0,
        124.0,
        16.0,
        key,
    );
    text(
        commands,
        font,
        format!("Generation {} · age {:.1} s", generation.0, age.0),
        24.0,
        146.0,
        16.0,
        key,
    );

    text(commands, font, "Energy / reserve", 24.0, 176.0, 16.0, ink);
    bar(
        commands,
        24.0,
        198.0,
        410.0,
        20.0,
        reserve.fraction(),
        Color::srgb(0.35, 0.75, 0.45),
    );
    text(
        commands,
        font,
        format!("{:.0} / {:.0}", reserve.current, reserve.max),
        30.0,
        200.0,
        14.0,
        Color::WHITE,
    );

    text(
        commands,
        font,
        "Genotype (inherited genes)",
        24.0,
        232.0,
        16.0,
        ink,
    );
    let mut gy = 256.0;
    for t in &TRAITS {
        if immobile && t.inert_when_immobile {
            continue;
        }
        text(
            commands,
            font,
            format!("{}: {:.*}", t.name, t.decimals as usize, (t.get)(genotype)),
            30.0,
            gy,
            14.0,
            key,
        );
        gy += 18.0;
    }
    if !immobile {
        text(
            commands,
            font,
            format!("vision cost/s: {:.3}", vision.metabolic_cost()),
            30.0,
            gy,
            14.0,
            key,
        );
        gy += 18.0;
    }

    // Action (brain output).
    let heading_deg = if action.dir.length_squared() > 1e-6 {
        action.dir.to_angle().to_degrees()
    } else {
        0.0
    };
    text(
        commands,
        font,
        "Action (brain output)",
        24.0,
        gy + 8.0,
        16.0,
        ink,
    );
    text(
        commands,
        font,
        format!("desired heading {heading_deg:+.0}°"),
        30.0,
        gy + 30.0,
        14.0,
        key,
    );
    bar(
        commands,
        24.0,
        gy + 50.0,
        410.0,
        16.0,
        action.throttle,
        Color::srgb(0.4, 0.6, 0.9),
    );
    text(
        commands,
        font,
        format!("throttle {:.2}", action.throttle),
        30.0,
        gy + 50.0,
        13.0,
        Color::WHITE,
    );

    // --- Right column: MLP brain + perception ---
    if let Brain::Mlp(mlp) = brain {
        text(
            commands,
            font,
            "MLP brain (activations)",
            470.0,
            100.0,
            16.0,
            ink,
        );
        let acts = mlp.forward_activations(perception);
        draw_mlp(
            commands,
            gizmos,
            font,
            mlp,
            &acts,
            (470.0, 124.0, 406.0, 280.0),
        );
    } else {
        text(commands, font, brain.name(), 470.0, 100.0, 16.0, ink);
    }

    if !immobile {
        text(
            commands,
            font,
            format!("Perception — {} rays", vision.ray_count),
            470.0,
            424.0,
            16.0,
            ink,
        );
        text(
            commands,
            font,
            "obstacle · target · threat",
            470.0,
            446.0,
            13.0,
            dim,
        );
        // One row per ray (capped to what fits in the strip).
        let row_h = 14.0;
        let max_rows = (((VIZ_H - 30.0) - 468.0) / row_h).floor() as usize;
        for (i, &prox) in perception.vision.iter().take(max_rows).enumerate() {
            let target = perception.target.get(i).copied().unwrap_or(0.0);
            let threat = perception.threat.get(i).copied().unwrap_or(0.0);
            let ry = 468.0 + i as f32 * row_h;
            bar(
                commands,
                470.0,
                ry,
                120.0,
                row_h - 3.0,
                prox,
                Color::srgb(0.6, 0.6, 0.62),
            );
            bar(
                commands,
                596.0,
                ry,
                120.0,
                row_h - 3.0,
                target,
                Color::srgb(0.86, 0.51, 0.16),
            );
            bar(
                commands,
                722.0,
                ry,
                120.0,
                row_h - 3.0,
                threat,
                Color::srgb(0.82, 0.24, 0.24),
            );
        }
    }
}

/// Color of a node by its activation (cold<0<warm), same tints as the editor.
fn activation_color(v: f32) -> Color {
    let t = v.clamp(-1.0, 1.0).abs();
    let base = 0.24;
    let lerp = |to: f32| base + (to - base) * t;
    if v >= 0.0 {
        Color::srgb(lerp(0.94), lerp(0.59), lerp(0.16)) // warm
    } else {
        Color::srgb(lerp(0.24), lerp(0.55), lerp(0.94)) // cold
    }
}

/// Draws the MLP (edges tinted by weight, nodes by activation, size ∝ |bias|), in
/// `(x, y, w, h)` px. Reuses the editor's geometry but in gizmos.
fn draw_mlp(
    commands: &mut Commands,
    gizmos: &mut Gizmos<VizGizmos>,
    font: &Handle<Font>,
    mlp: &MlpBrain,
    acts: &[Vec<f32>],
    (x, y, w, h): (f32, f32, f32, f32),
) {
    let sizes = mlp.layer_sizes();
    if sizes.len() < 2 {
        return;
    }
    let cols = sizes.len();
    let widest = *sizes.iter().max().unwrap_or(&1);
    const LABEL_W: f32 = 34.0;
    let (ix, iw) = (x + LABEL_W, w - 2.0 * LABEL_W);

    // Px position (canvas) of node `node` of column `col`.
    let node_px = |col: usize, node: usize, n: usize| -> (f32, f32) {
        let px = if cols == 1 {
            ix + iw * 0.5
        } else {
            ix + iw * col as f32 / (cols - 1) as f32
        };
        let py = if n == 1 {
            y + h * 0.5
        } else {
            y + h * (node as f32 + 0.5) / n as f32
        };
        (px, py)
    };

    // Edges first (under the nodes, the gizmo group's draw order).
    for col in 0..cols - 1 {
        let (from_n, to_n) = (sizes[col], sizes[col + 1]);
        let weights = (col < mlp.weight_layers()).then(|| mlp.layer_weights(col));
        for o in 0..to_n {
            for i in 0..from_n {
                let col_c = match weights {
                    Some((wts, fan_in, _)) if o * fan_in + i < wts.len() => {
                        let wt = wts[o * fan_in + i];
                        let a = (wt.abs() * 0.9).clamp(0.05, 0.9);
                        if wt >= 0.0 {
                            Color::srgba(0.9, 0.59, 0.24, a)
                        } else {
                            Color::srgba(0.27, 0.55, 0.9, a)
                        }
                    }
                    _ => Color::srgba(0.3, 0.3, 0.3, 0.3),
                };
                let (ax, ay) = node_px(col, i, from_n);
                let (bx, by) = node_px(col + 1, o, to_n);
                gizmos.line_2d(p(ax, ay), p(bx, by), col_c);
            }
        }
    }

    // Nodes: reference radius shrunk by |bias| (normalized to the largest bias).
    let base_r = (h / (widest as f32 * 2.2)).clamp(2.5, 9.0);
    let max_bias = (0..mlp.weight_layers())
        .flat_map(|l| mlp.layer_biases(l).iter().copied())
        .fold(0.0_f32, |acc, b| acc.max(b.abs()));
    for (col, &n) in sizes.iter().enumerate() {
        let biases = (col >= 1 && col - 1 < mlp.weight_layers()).then(|| mlp.layer_biases(col - 1));
        for node in 0..n {
            let act = acts
                .get(col)
                .and_then(|l| l.get(node))
                .copied()
                .unwrap_or(0.0);
            let radius = match (biases, max_bias > 1e-6) {
                (Some(b), true) => {
                    let t = (b.get(node).copied().unwrap_or(0.0).abs() / max_bias).clamp(0.0, 1.0);
                    base_r * (0.35 + 0.65 * t)
                }
                _ => base_r,
            };
            let (px, py) = node_px(col, node, n);
            gizmos.circle_2d(p(px, py), radius, activation_color(act));
        }
    }

    // Labels of the input groups (vis/tgt/thr) and the outputs (fwd/side).
    let n_in = sizes[0];
    if n_in.is_multiple_of(3) {
        let rays = n_in / 3;
        for (g, name) in ["vis", "tgt", "thr"].iter().enumerate() {
            let (_, py) = node_px(0, g * rays + rays / 2, n_in);
            text(
                commands,
                font,
                *name,
                x - 2.0,
                py - 6.0,
                11.0,
                Color::srgb(0.6, 0.6, 0.65),
            );
        }
    }
    let last = cols - 1;
    if sizes[last] == MlpBrain::OUTPUTS {
        for (o, name) in ["fwd", "side"].iter().enumerate() {
            let (px, py) = node_px(last, o, MlpBrain::OUTPUTS);
            text(
                commands,
                font,
                *name,
                px + 8.0,
                py - 6.0,
                11.0,
                Color::srgb(0.6, 0.6, 0.65),
            );
        }
    }
}
