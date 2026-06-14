//! Point d'entrée **fenêtré** (direct).
//!
//! `DefaultPlugins` pilote la fenêtre, le rendu et la présentation. Tout ce
//! qu'on ajoute ici vit dans `Update` et ne touche QUE le rendu / l'UI — jamais
//! l'état de simulation, qui appartient à [`teemlab::SimPlugin`].

use bevy::prelude::*;
use teemlab::components::{Agent, Radius};
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
        .add_plugins(SimPlugin::new(SimConfig::from_cli()))
        .add_systems(Startup, setup_camera)
        // RENDU UNIQUEMENT — jamais de logique de sim ici.
        .add_systems(Update, (attach_visuals, draw_arena))
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

/// Rendu seul : donner un mesh visible aux agents fraîchement spawnés. Tourne
/// dans `Update` car ça manipule des assets de rendu, pas l'état de sim.
fn attach_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    new_agents: Query<(Entity, &Radius), (Added<Agent>, Without<Mesh2d>)>,
) {
    for (entity, radius) in &new_agents {
        commands.entity(entity).insert((
            Mesh2d(meshes.add(Circle::new(radius.0))),
            MeshMaterial2d(materials.add(Color::srgb(0.30, 0.70, 1.0))),
        ));
    }
}

/// Rendu seul : tracer le contour de l'arène avec des gizmos.
fn draw_arena(mut gizmos: Gizmos, config: Res<SimConfig>) {
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
