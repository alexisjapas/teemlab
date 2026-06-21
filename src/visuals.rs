//! Rendering layer **shared** by the binaries that *display* the sim: the
//! windowed build (`main.rs`) and the headless video recorder (`bin/record.rs`).
//!
//! This is strictly rendering/observation — everything lives in `Update`,
//! **never** in `FixedUpdate` (cardinal invariant). Deliberately outside
//! [`crate::SimPlugin`], which stays render-agnostic: the "pure" headless
//! (`bin/headless.rs`) does not include it. Centralizing here avoids duplicating
//! the rendering between the live preview and the recording (item 14, §7: *fresh
//! re-render* of a run).

use crate::components::{Agent, Locomotion, Perception, Radius, Reserve, Species};
use crate::config::SimConfig;
use bevy::prelude::*;

/// Adds the sim's rendering systems (entity meshes, reserve-based shading,
/// arena, heading indicator, inner/outer **backgrounds**). To be combined with a
/// camera provided by the binary (window for `main`, image target for `record`).
/// The detailed fan of vision rays is not part of it: it is reserved for the
/// inspected agent, on the windowed side.
///
/// The backgrounds ([`draw_play_area`]) live here — therefore **shared** by the
/// live preview and the video recording — so that a video renders exactly the
/// colors set in the editor (background-colors item), and not a frozen
/// background.
pub struct VisualsPlugin;

impl Plugin for VisualsPlugin {
    fn build(&self, app: &mut App) {
        // `ClearColor` drives the off-game area on the windowed side (the `main`
        // camera uses it); we ensure its presence so `draw_play_area` can write it
        // in both binaries (the recorder, for its part, sets the off-game area on
        // its image-camera).
        app.init_resource::<ClearColor>().add_systems(
            Update,
            (
                attach_visuals,
                shade_by_reserve,
                draw_arena,
                draw_heading,
                draw_play_area,
            ),
        );
    }
}

/// Marker of the background quad materializing the play area (inside of the arena).
#[derive(Component)]
pub struct PlayAreaBg;

/// Opaque `Color` from an sRGB triplet `[r, g, b]` of the scenario (background settings).
pub fn srgb3([r, g, b]: [f32; 3]) -> Color {
    Color::srgb(r, g, b)
}

/// Base color of a species (cyclic palette). Rendering alone gives a visual
/// meaning to the species integer; the sim itself has no color.
pub fn species_color(species: Species) -> Srgba {
    const PALETTE: [Srgba; 4] = [
        Srgba::new(0.30, 0.70, 1.00, 1.0), // blue
        Srgba::new(1.00, 0.45, 0.35, 1.0), // coral
        Srgba::new(0.55, 0.90, 0.45, 1.0), // green
        Srgba::new(0.95, 0.80, 0.30, 1.0), // amber
    ];
    PALETTE[species.0 as usize % PALETTE.len()]
}

/// Display color of an entity: that of **its archetype** (the index carried by
/// [`Species`]), falling back to the palette for an out-of-list index. This is
/// how the color chosen in the archetype editor shows on screen.
fn entity_color(config: &SimConfig, species: Species) -> Srgba {
    let [r, g, b] = config.color_of(species.0);
    Srgba::new(r, g, b, 1.0)
}

/// Rendering only: give a visible mesh to freshly spawned agents, tinted by their
/// archetype's color. Runs in `Update` because it manipulates render assets.
fn attach_visuals(
    mut commands: Commands,
    config: Res<SimConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    new_agents: Query<(Entity, &Radius, &Species), (Added<Agent>, Without<Mesh2d>)>,
) {
    for (entity, radius, species) in &new_agents {
        commands.entity(entity).insert((
            Mesh2d(meshes.add(Circle::new(radius.0))),
            MeshMaterial2d(materials.add(Color::from(entity_color(&config, *species)))),
        ));
    }
}

/// Rendering only: darken an agent as its reserve drops, to *see* predation drain
/// its prey. Each agent owns its own material (created in `attach_visuals`),
/// which we modulate here by the reserve fraction.
fn shade_by_reserve(
    config: Res<SimConfig>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    agents: Query<(&MeshMaterial2d<ColorMaterial>, &Species, &Reserve)>,
) {
    for (handle, species, reserve) in &agents {
        if let Some(material) = materials.get_mut(&handle.0) {
            let dim = 0.25 + 0.75 * reserve.fraction();
            let base = entity_color(&config, *species);
            material.color = Color::srgb(base.red * dim, base.green * dim, base.blue * dim);
        }
    }
}

/// Rendering only: a short **heading indicator** for **mobile** agents — a line
/// from the center to the body's edge, along the heading, to read a moving
/// entity's orientation at a glance. It stops at the radius (`Radius`): it never
/// overflows the body. We re-read the heading already computed by the sim
/// (`Perception::heading`), recomputing nothing.
///
/// An **immobile** entity (flora / sessile source, [`Locomotion::is_immobile`])
/// gets **none**: its "heading" is only a fixed fallback (`+X`), not a gaze
/// direction — showing it would draw a misleading line over a bush.
///
/// The vision **detail** (the full fan of rays, occlusion at work) is NOT drawn
/// here: on every agent it would saturate the screen. It is the windowed binary
/// that draws it, for the single **inspected** agent (cf. `inspector`).
fn draw_heading(
    mut gizmos: Gizmos,
    agents: Query<(&Transform, &Radius, &Perception, &Locomotion), With<Agent>>,
) {
    for (transform, radius, perception, loco) in &agents {
        if loco.is_immobile() {
            continue; // flora: no useful heading to show.
        }
        let facing = perception.heading;
        if facing == Vec2::ZERO {
            continue; // no heading yet (1st tick): nothing to show.
        }
        let origin = transform.translation.truncate();
        gizmos.line_2d(
            origin,
            origin + facing * radius.0,
            Color::srgb(0.95, 0.95, 0.98),
        );
    }
}

/// Rendering only: draw the arena outline with gizmos.
fn draw_arena(mut gizmos: Gizmos, config: Res<crate::SimConfig>) {
    let h = config.arena_half_extent;
    let color = Color::srgb(0.40, 0.40, 0.46);
    gizmos.linestrip_2d(
        [
            Vec2::new(-h, -h),
            Vec2::new(h, -h),
            Vec2::new(h, h),
            Vec2::new(-h, h),
            Vec2::new(-h, -h),
        ],
        color,
    );
}

/// Rendering only: materializes the **two backgrounds** set by the scenario
/// ([`SimConfig::play_area_color`] / [`SimConfig::off_game_color`]).
///
/// - The **play area** (inside of the arena) is a quad painted **under** the
///   agents (negative z), following the arena's size (`arena_half_extent`, which
///   may change when a scenario is reloaded) and the inner color.
/// - The **off-game area** (beyond the walls) is driven by `ClearColor`, written
///   here from the outer color. The windowed camera (`main`) uses it as-is; the
///   recorder (`record`) sets the same color on its image-camera (which ignores
///   `ClearColor`).
///
/// Shared by the live preview and the video recording: a video therefore renders
/// exactly the chosen colors. Both tints are re-read **continuously** → a change
/// in the editor (or when loading a scenario) shows immediately, without a reset.
fn draw_play_area(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut clear_color: ResMut<ClearColor>,
    config: Res<SimConfig>,
    mut existing: Query<(&mut Transform, &MeshMaterial2d<ColorMaterial>), With<PlayAreaBg>>,
) {
    let side = 2.0 * config.arena_half_extent;
    clear_color.0 = srgb3(config.off_game_color);
    let play_color = srgb3(config.play_area_color);
    if let Ok((mut tf, material)) = existing.single_mut() {
        tf.scale = Vec3::new(side, side, 1.0);
        if let Some(mat) = materials.get_mut(&material.0) {
            mat.color = play_color;
        }
    } else {
        commands.spawn((
            PlayAreaBg,
            Mesh2d(meshes.add(Rectangle::new(1.0, 1.0))),
            MeshMaterial2d(materials.add(play_color)),
            Transform::from_xyz(0.0, 0.0, -10.0).with_scale(Vec3::new(side, side, 1.0)),
        ));
    }
}
