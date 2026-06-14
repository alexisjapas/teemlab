//! Point d'entrée **fenêtré** (direct).
//!
//! `DefaultPlugins` pilote la fenêtre, le rendu et la présentation. Tout ce
//! qu'on ajoute ici vit dans `Update` et ne touche QUE le rendu / l'UI — jamais
//! l'état de simulation, qui appartient à [`teemlab::SimPlugin`].

mod editor;

use bevy::prelude::*;
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};
use teemlab::components::{Agent, Food, Perception, Radius, Reserve, Species, Vision};
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
        .add_plugins(SimPlugin::new(SimConfig::from_cli()))
        .add_systems(Startup, (setup_camera, editor::build_palette))
        // RENDU UNIQUEMENT — jamais de logique de sim ici.
        .add_systems(
            Update,
            (
                attach_visuals,
                attach_food_visuals,
                shade_by_reserve,
                draw_arena,
                draw_vision,
            ),
        )
        // UI egui : panneau d'archétypes + placement manuel (item 4).
        .add_systems(EguiPrimaryContextPass, editor::editor_ui)
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

/// Couleur de base d'une espèce (palette cyclique). Le rendu seul donne un sens
/// visuel à l'entier d'espèce ; la sim, elle, n'a pas de couleur.
fn species_color(species: Species) -> Srgba {
    const PALETTE: [Srgba; 4] = [
        Srgba::new(0.30, 0.70, 1.00, 1.0), // bleu
        Srgba::new(1.00, 0.45, 0.35, 1.0), // corail
        Srgba::new(0.55, 0.90, 0.45, 1.0), // vert
        Srgba::new(0.95, 0.80, 0.30, 1.0), // ambre
    ];
    PALETTE[species.0 as usize % PALETTE.len()]
}

/// Rendu seul : donner un mesh visible aux agents fraîchement spawnés, teinté
/// par espèce. Tourne dans `Update` car ça manipule des assets de rendu.
fn attach_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    new_agents: Query<(Entity, &Radius, &Species), (Added<Agent>, Without<Mesh2d>)>,
) {
    for (entity, radius, species) in &new_agents {
        commands.entity(entity).insert((
            Mesh2d(meshes.add(Circle::new(radius.0))),
            MeshMaterial2d(materials.add(Color::from(species_color(*species)))),
        ));
    }
}

/// Rendu seul : donner un mesh aux sources de nourriture fraîchement semées,
/// teintées par leur espèce. Elles s'assombriront ensuite via `shade_by_reserve`
/// (elles portent `Species` + `Reserve`) à mesure qu'on les mange.
fn attach_food_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    new_food: Query<(Entity, &Radius, &Species), (Added<Food>, Without<Mesh2d>)>,
) {
    for (entity, radius, species) in &new_food {
        commands.entity(entity).insert((
            Mesh2d(meshes.add(Circle::new(radius.0))),
            MeshMaterial2d(materials.add(Color::from(species_color(*species)))),
        ));
    }
}

/// Rendu seul : assombrir un agent à mesure que sa réserve baisse, pour *voir*
/// la prédation vider ses proies. Chaque agent possède son propre matériau (créé
/// dans `attach_visuals`), qu'on module ici par la fraction de réserve.
fn shade_by_reserve(
    mut materials: ResMut<Assets<ColorMaterial>>,
    agents: Query<(&MeshMaterial2d<ColorMaterial>, &Species, &Reserve)>,
) {
    for (handle, species, reserve) in &agents {
        if let Some(material) = materials.get_mut(&handle.0) {
            let dim = 0.25 + 0.75 * reserve.fraction();
            let base = species_color(*species);
            material.color =
                Color::srgb(base.red * dim, base.green * dim, base.blue * dim);
        }
    }
}

/// Rendu seul : visualiser les rayons de vision pour *voir* l'occlusion à
/// l'œuvre. On relit l'état sensoriel calculé par la sim (`Perception`) — on ne
/// recalcule aucun raycast ici. Rayon clair = rien vu ; il rougit et raccourcit
/// à mesure qu'un obstacle se rapproche.
fn draw_vision(mut gizmos: Gizmos, agents: Query<(&Transform, &Vision, &Perception)>) {
    for (transform, vision, perception) in &agents {
        let origin = transform.translation.truncate();
        let facing = perception.heading;
        for (i, &proximity) in perception.vision.iter().enumerate() {
            let dir = vision.ray_dir(i, facing);
            let length = vision.range * (1.0 - proximity);
            let color = Color::srgb(0.25 + 0.75 * proximity, 0.55 * (1.0 - proximity), 0.15);
            gizmos.line_2d(origin, origin + dir * length, color);
        }
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
