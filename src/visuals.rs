//! Couche de rendu **partagée** par les binaires qui *affichent* la sim : le
//! build fenêtré (`main.rs`) et l'enregistreur vidéo headless (`bin/record.rs`).
//!
//! C'est strictement du rendu/observation — tout vit dans `Update`, **jamais**
//! dans `FixedUpdate` (invariant cardinal). Volontairement hors de [`crate::SimPlugin`],
//! qui reste agnostique au rendu : le headless « pur » (`bin/headless.rs`) ne
//! l'inclut pas. Centraliser ici évite de dupliquer le rendu entre l'aperçu live
//! et l'enregistrement (item 14, §7 : *re-render frais* d'une run).

use crate::components::{Agent, Locomotion, Perception, Radius, Reserve, Species};
use crate::config::SimConfig;
use bevy::prelude::*;

/// Ajoute les systèmes de rendu de la sim (mesh des entités, teinte par réserve,
/// arène, indicateur de cap, **fonds** intérieur/extérieur). À combiner avec une caméra
/// fournie par le binaire (fenêtre pour `main`, cible image pour `record`). L'éventail
/// détaillé des rayons de vision n'en fait pas partie : il est réservé à l'agent inspecté,
/// côté fenêtré.
///
/// Les fonds ([`draw_play_area`]) vivent ici — donc **partagés** par l'aperçu live et
/// l'enregistrement vidéo — pour qu'une vidéo rende exactement les couleurs réglées dans
/// l'éditeur (item couleurs de fond), et pas un fond figé.
pub struct VisualsPlugin;

impl Plugin for VisualsPlugin {
    fn build(&self, app: &mut App) {
        // `ClearColor` pilote le hors-jeu côté fenêtré (la caméra de `main` l'utilise) ;
        // on garantit sa présence pour que `draw_play_area` puisse l'écrire dans les deux
        // binaires (l'enregistreur, lui, fixe le hors-jeu sur sa caméra-image).
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

/// Marqueur du quad de fond matérialisant l'aire de jeu (intérieur de l'arène).
#[derive(Component)]
pub struct PlayAreaBg;

/// `Color` opaque depuis un triplet sRGB `[r, g, b]` du scénario (réglages de fond).
pub fn srgb3([r, g, b]: [f32; 3]) -> Color {
    Color::srgb(r, g, b)
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

/// Couleur d'affichage d'une entité : celle de **son archétype** (l'index porté par
/// [`Species`]), avec repli sur la palette pour un index hors-liste. C'est ainsi que
/// la couleur choisie dans l'éditeur d'archétype se voit à l'écran.
fn entity_color(config: &SimConfig, species: Species) -> Srgba {
    let [r, g, b] = config.color_of(species.0);
    Srgba::new(r, g, b, 1.0)
}

/// Rendu seul : donner un mesh visible aux agents fraîchement spawnés, teinté par la
/// couleur de leur archétype. Tourne dans `Update` car ça manipule des assets de rendu.
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

/// Rendu seul : assombrir un agent à mesure que sa réserve baisse, pour *voir*
/// la prédation vider ses proies. Chaque agent possède son propre matériau (créé
/// dans `attach_visuals`), qu'on module ici par la fraction de réserve.
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

/// Rendu seul : un court **indicateur de cap** pour les agents **mobiles** — un trait
/// du centre au bord du corps, le long du cap, pour lire d'un coup d'œil l'orientation
/// d'une entité mouvante. Il s'arrête au rayon (`Radius`) : il ne déborde jamais du
/// corps. On relit le cap déjà calculé par la sim (`Perception::heading`), sans rien
/// recalculer.
///
/// Une entité **immobile** (flore / source sessile, [`Locomotion::is_immobile`]) n'en
/// reçoit **pas** : son « cap » n'est qu'un repli fixe (`+X`), pas une direction de
/// regard — l'afficher tracerait un trait trompeur sur un buisson.
///
/// Le **détail** de la vision (l'éventail complet des rayons, l'occlusion à l'œuvre)
/// n'est PAS dessiné ici : à tous les agents il saturerait l'écran. C'est le binaire
/// fenêtré qui le trace, pour le seul agent **inspecté** (cf. `inspector`).
fn draw_heading(
    mut gizmos: Gizmos,
    agents: Query<(&Transform, &Radius, &Perception, &Locomotion), With<Agent>>,
) {
    for (transform, radius, perception, loco) in &agents {
        if loco.is_immobile() {
            continue; // flore : aucun cap utile à montrer.
        }
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

/// Rendu seul : matérialise les **deux fonds** réglés par le scénario
/// ([`SimConfig::play_area_color`] / [`SimConfig::off_game_color`]).
///
/// - L'**aire de jeu** (intérieur de l'arène) est un quad peint **sous** les agents
///   (z négatif), suivant la taille de l'arène (`arena_half_extent`, qui peut changer au
///   rechargement d'un scénario) et la couleur intérieure.
/// - Le **hors-jeu** (au-delà des murs) est piloté par `ClearColor`, écrit ici depuis la
///   couleur extérieure. La caméra fenêtrée (`main`) l'utilise telle quelle ; l'enregistreur
///   (`record`) fixe la même couleur sur sa caméra-image (qui ignore `ClearColor`).
///
/// Partagé par l'aperçu live et l'enregistrement vidéo : une vidéo rend donc exactement
/// les couleurs choisies. Les deux teintes sont relues **en continu** → un changement dans
/// l'éditeur (ou au chargement d'un scénario) se voit immédiatement, sans reset.
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
