//! Inspecteur d'agent du build fenêtré : **cliquer un agent → voir son état**
//! (item 12).
//!
//! Module du *binaire* fenêtré uniquement (comme [`crate::editor`],
//! [`crate::hud`], [`crate::controls`]). C'est l'outil de débogage du
//! comportement — le garde-fou du groupe témoin déterministe : on lit le
//! génotype, l'énergie, la perception et l'action courante d'un agent vivant.
//!
//! Lecture seule : on n'écrit jamais dans la sim. La sélection (un `Entity`) et
//! son rendu (un anneau en gizmo) vivent dans le binaire fenêtré ; l'invariant
//! cardinal tient.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use teemlab::brain::Brain;
use teemlab::components::{
    Action, Age, Agent, Generation, Perception, Radius, Reserve, Species, Vision,
};
use teemlab::genotype::{Genotype, TRAITS};

use crate::editor::{Palette, draw_mlp_graph};

/// L'agent actuellement inspecté, le cas échéant. Vit dans le binaire fenêtré.
#[derive(Resource, Default)]
pub struct Selection(pub Option<Entity>);

/// Position **monde** du curseur dans l'aire de jeu (caméra et fenêtre uniques),
/// si elle existe. Partagée par le picking de l'inspecteur et la suppression : le
/// `viewport_to_world_2d` tient compte de l'offset de la sim centrée (cf.
/// `main::set_sim_camera`), donc le curseur fenêtre reste la bonne entrée.
fn pointer_world(
    cameras: &Query<(&Camera, &GlobalTransform)>,
    windows: &Query<&Window>,
) -> Option<Vec2> {
    let (camera, cam_tf) = cameras.single().ok()?;
    let window = windows.single().ok()?;
    let cursor = window.cursor_position()?;
    camera.viewport_to_world_2d(cam_tf, cursor).ok()
}

/// L'entité (corps) la plus proche dont le rayon **contient** `world`, le cas
/// échéant. Même critère pour sélectionner (inspecteur) et supprimer — d'où le
/// partage. `None` = curseur dans le vide.
fn body_at<'a>(
    world: Vec2,
    bodies: impl IntoIterator<Item = (Entity, &'a Transform, &'a Radius)>,
) -> Option<Entity> {
    let mut best: Option<(Entity, f32)> = None;
    for (entity, transform, radius) in bodies {
        let d = transform.translation.truncate().distance(world);
        if d <= radius.0 && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((entity, d));
        }
    }
    best.map(|(entity, _)| entity)
}

/// Sélectionne l'agent sous le curseur au clic dans l'aire de jeu. Un clic dans
/// le vide désélectionne ; un clic sur un panneau egui ou pendant un glisser
/// d'archétype est ignoré (c'est l'éditeur qui gère ce dernier).
pub fn pick_agent(
    mut contexts: EguiContexts,
    mut selection: ResMut<Selection>,
    palette: Res<Palette>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    windows: Query<&Window>,
    agents: Query<(Entity, &Transform, &Radius), With<Agent>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    // On ne pioche pas pendant un glisser-déposer d'archétype (éditeur), ni quand
    // le clic vise un panneau egui, ni s'il n'y a pas de clic du tout.
    if palette.dragging.is_some()
        || !ctx.input(|i| i.pointer.any_click())
        || ctx.is_pointer_over_area()
    {
        return Ok(());
    }
    let Some(world) = pointer_world(&cameras, &windows) else {
        return Ok(());
    };
    // L'agent le plus proche dont le corps contient le clic ; sinon (vide) → None.
    selection.0 = body_at(world, agents);
    Ok(())
}

/// Suppression manuelle (Suppr / Retour arrière) : retire l'entité **sous le
/// curseur** — agent OU nourriture (tout corps à [`Radius`] ; les murs, sans
/// `Radius`, sont épargnés). Édition manuelle déclenchée par l'utilisateur, comme
/// le placement de l'éditeur → vit hors `FixedUpdate`, et reste autorisée même
/// hors pause, par cohérence avec le placement. Pas d'annulation en v1 : une
/// entité se repose depuis la palette (le monde est un bac à sable d'expérience,
/// pas une donnée précieuse).
///
/// Comme [`pick_agent`] et `resolve_drag`, doit tourner **après** les fenêtres egui
/// pour que `is_pointer_over_area` soit à jour (sinon un Suppr au-dessus d'un
/// panneau viserait l'entité cachée dessous).
#[allow(clippy::too_many_arguments)]
pub fn delete_under_cursor(
    mut contexts: EguiContexts,
    keys: Res<ButtonInput<KeyCode>>,
    palette: Res<Palette>,
    mut selection: ResMut<Selection>,
    mut commands: Commands,
    cameras: Query<(&Camera, &GlobalTransform)>,
    windows: Query<&Window>,
    bodies: Query<(Entity, &Transform, &Radius)>,
) -> Result {
    if !(keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace)) {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    // Pas pendant un glisser d'archétype, ni quand le curseur vise un panneau egui.
    if palette.dragging.is_some() || ctx.is_pointer_over_area() {
        return Ok(());
    }
    let Some(world) = pointer_world(&cameras, &windows) else {
        return Ok(());
    };
    // Le corps le plus proche dont le rayon contient le curseur (même critère que
    // le picking de l'inspecteur).
    if let Some(entity) = body_at(world, bodies) {
        commands.entity(entity).despawn();
        if selection.0 == Some(entity) {
            selection.0 = None; // ne pas garder une sélection fantôme.
        }
    }
    Ok(())
}

/// L'inspecteur d'agent — génotype, énergie, perception, action (+ graphe MLP) de
/// l'agent sélectionné. Rendu dans le panneau du bas (à droite, item dock). Si
/// l'agent sélectionné a disparu (mort), on le signale. Lecture seule du monde.
pub(crate) fn inspector_section(
    ui: &mut egui::Ui,
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
    let Some(entity) = selection.0 else {
        ui.weak("Clique un agent dans l'aire pour l'inspecter.");
        return;
    };
    let Ok((species, reserve, genotype, vision, perception, action, brain, generation, age)) =
        agents.get(entity)
    else {
        ui.colored_label(
            egui::Color32::from_rgb(255, 140, 120),
            "L'agent sélectionné n'existe plus (mort ?).",
        );
        ui.weak("Clique un autre agent, ou dans le vide pour désélectionner.");
        return;
    };

    // Un défilement évite de rogner la liste des rayons de vision quand le panneau
    // est réduit.
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.label(format!("Espèce : {}", species.0));
        ui.label(format!("Cerveau : {}", brain.name()));
        ui.label(format!("Génération : {}", generation.0));
        ui.label(format!("Âge : {:.1} s", age.0));

        ui.separator();
        ui.strong("Énergie / réserve");
        ui.add(
            egui::ProgressBar::new(reserve.fraction())
                .text(format!("{:.1} / {:.0}", reserve.current, reserve.max)),
        );

        ui.separator();
        ui.strong("Génotype (gènes hérités)");
        egui::Grid::new("genes").num_columns(2).show(ui, |ui| {
            // Une ligne par caractéristique de TRAITS : ajouter un trait l'affiche
            // ici sans toucher l'inspecteur.
            for t in &TRAITS {
                ui.label(t.name);
                ui.label(format!("{:.*}", t.decimals as usize, (t.get)(genotype)));
                ui.end_row();
            }
            ui.label("coût vision/s");
            ui.label(format!("{:.3}", vision.metabolic_cost()));
            ui.end_row();
        });

        ui.separator();
        ui.strong("Action (sortie du cerveau)");
        let throttle = action.throttle;
        let heading_deg = if action.dir.length_squared() > 1e-6 {
            action.dir.to_angle().to_degrees()
        } else {
            0.0
        };
        ui.label(format!("cap désiré : {heading_deg:+.0}°"));
        ui.add(egui::ProgressBar::new(throttle).text(format!("accélérateur {throttle:.2}")));

        // Cerveau MLP : le réseau en action (item 18b-viz). Nœuds colorés par leur
        // activation courante (le dernier `think`), arêtes par signe/poids — la
        // décision apprise rendue lisible. Les autres cerveaux n'ont pas de graphe.
        if let Brain::Mlp(mlp) = brain {
            ui.separator();
            ui.strong("Cerveau MLP (activations)");
            ui.weak("entrée (vision/cible) → couches cachées → pilotage · froid<0<chaud");
            // Les activations sont recalculées ici, à la demande, pour le seul agent
            // inspecté (le `think` du cœur de sim ne les mémorise plus).
            let activations = mlp.forward_activations(perception);
            draw_mlp_graph(ui, &mlp.layer_sizes(), Some(mlp), Some(&activations));
        }

        ui.separator();
        ui.strong(format!("Perception — vision ({} rayons)", vision.ray_count));
        ui.weak("obstacle (gris) · cible comestible (orange) — 0 = rien, 1 = au contact");
        for (i, &proximity) in perception.vision.iter().enumerate() {
            let target = perception.target.get(i).copied().unwrap_or(0.0);
            ui.horizontal(|ui| {
                ui.add(
                    egui::ProgressBar::new(proximity)
                        .desired_width(110.0)
                        .text(format!("r{i} · {proximity:.2}")),
                );
                ui.add(
                    egui::ProgressBar::new(target)
                        .desired_width(110.0)
                        .fill(egui::Color32::from_rgb(220, 130, 40))
                        .text(format!("{target:.2}")),
                );
            });
        }
    });
}

/// Rendu seul : entourer l'agent sélectionné d'un anneau, pour le retrouver dans
/// l'aire. Tourne dans `Update` (gizmos = rendu).
pub fn highlight_selection(
    mut gizmos: Gizmos,
    selection: Res<Selection>,
    agents: Query<(&Transform, &Radius), With<Agent>>,
) {
    if let Some(entity) = selection.0
        && let Ok((transform, radius)) = agents.get(entity)
    {
        gizmos.circle_2d(
            transform.translation.truncate(),
            radius.0 + 5.0,
            Color::srgb(1.0, 1.0, 1.0),
        );
    }
}

/// Rendu seul : l'éventail de rayons de vision de l'agent **sélectionné**
/// uniquement — pour *voir* l'occlusion à l'œuvre sans saturer l'écran en le
/// traçant pour tous les agents (le cap de chacun reste lisible via l'indicateur
/// de `visuals::draw_heading`). On relit l'état sensoriel déjà calculé par la sim
/// (`Perception`) — aucun raycast recalculé ici. Rayon clair = rien vu ; il rougit
/// et raccourcit à mesure qu'un obstacle se rapproche.
pub fn draw_selected_vision(
    mut gizmos: Gizmos,
    selection: Res<Selection>,
    agents: Query<(&Transform, &Vision, &Perception), With<Agent>>,
) {
    let Some(entity) = selection.0 else {
        return;
    };
    let Ok((transform, vision, perception)) = agents.get(entity) else {
        return;
    };
    let origin = transform.translation.truncate();
    let facing = perception.heading;
    for (i, &proximity) in perception.vision.iter().enumerate() {
        let dir = vision.ray_dir(i, facing);
        let length = vision.range * (1.0 - proximity);
        let color = Color::srgb(0.25 + 0.75 * proximity, 0.55 * (1.0 - proximity), 0.15);
        gizmos.line_2d(origin, origin + dir * length, color);
    }
}
