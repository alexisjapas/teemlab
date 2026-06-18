//! Couche de rendu **partagée** par les binaires qui *affichent* la sim : le
//! build fenêtré (`main.rs`) et l'enregistreur vidéo headless (`bin/record.rs`).
//!
//! C'est strictement du rendu/observation — tout vit dans `Update`, **jamais**
//! dans `FixedUpdate` (invariant cardinal). Volontairement hors de [`crate::SimPlugin`],
//! qui reste agnostique au rendu : le headless « pur » (`bin/headless.rs`) ne
//! l'inclut pas. Centraliser ici évite de dupliquer le rendu entre l'aperçu live
//! et l'enregistrement (item 14, §7 : *re-render frais* d'une run).

use crate::components::{Agent, Food, Perception, Radius, Reserve, Species};
use bevy::prelude::*;

/// Ajoute les systèmes de rendu de la sim (mesh des entités, teinte par réserve,
/// arène, indicateur de cap). À combiner avec une caméra fournie par le binaire
/// (fenêtre pour `main`, cible image pour `record`). L'éventail détaillé des rayons
/// de vision n'en fait pas partie : il est réservé à l'agent inspecté, côté fenêtré.
pub struct VisualsPlugin;

impl Plugin for VisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                attach_visuals,
                attach_food_visuals,
                shade_by_reserve,
                draw_arena,
                draw_heading,
            ),
        );
    }
}

/// Couleur de base d'une espèce (palette cyclique). Le rendu seul donne un sens
/// visuel à l'entier d'espèce ; la sim, elle, n'a pas de couleur.
pub fn species_color(species: Species) -> Srgba {
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
            material.color = Color::srgb(base.red * dim, base.green * dim, base.blue * dim);
        }
    }
}

/// Rendu seul : un court **indicateur de cap** pour *tous* les agents — un trait du
/// centre au bord du corps, le long du cap, pour lire d'un coup d'œil l'orientation
/// d'une entité mouvante. Il s'arrête au rayon (`Radius`) : il ne déborde jamais du
/// corps. On relit le cap déjà calculé par la sim (`Perception::heading`), sans rien
/// recalculer.
///
/// Le **détail** de la vision (l'éventail complet des rayons, l'occlusion à l'œuvre)
/// n'est PAS dessiné ici : à tous les agents il saturerait l'écran. C'est le binaire
/// fenêtré qui le trace, pour le seul agent **inspecté** (cf. `inspector`).
fn draw_heading(
    mut gizmos: Gizmos,
    agents: Query<(&Transform, &Radius, &Perception), With<Agent>>,
) {
    for (transform, radius, perception) in &agents {
        let facing = perception.heading;
        if facing == Vec2::ZERO {
            continue; // pas encore de cap (1er tick) : rien à montrer.
        }
        let origin = transform.translation.truncate();
        gizmos.line_2d(
            origin,
            origin + facing * radius.0,
            Color::srgb(0.95, 0.95, 0.98),
        );
    }
}

/// Rendu seul : tracer le contour de l'arène avec des gizmos.
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
