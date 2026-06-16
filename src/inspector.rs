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
use teemlab::components::{Action, Agent, Perception, Radius, Reserve, Species, Vision};
use teemlab::genotype::Genotype;

use crate::editor::Palette;

/// L'agent actuellement inspecté, le cas échéant. Vit dans le binaire fenêtré.
#[derive(Resource, Default)]
pub struct Selection(pub Option<Entity>);

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

    let Ok((camera, cam_tf)) = cameras.single() else {
        return Ok(());
    };
    let Ok(window) = windows.single() else {
        return Ok(());
    };
    let Some(cursor) = window.cursor_position() else {
        return Ok(());
    };
    let Ok(world) = camera.viewport_to_world_2d(cam_tf, cursor) else {
        return Ok(());
    };

    // L'agent le plus proche dont le corps contient le clic (rayon). Sinon, clic
    // dans le vide → désélection.
    let mut best: Option<(Entity, f32)> = None;
    for (entity, transform, radius) in &agents {
        let d = transform.translation.truncate().distance(world);
        if d <= radius.0 && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((entity, d));
        }
    }
    selection.0 = best.map(|(entity, _)| entity);
    Ok(())
}

/// La fenêtre flottante de l'inspecteur : génotype, énergie, perception, action de
/// l'agent sélectionné. Tourne dans `EguiPrimaryContextPass`. Lecture seule.
pub fn inspector_ui(
    mut contexts: EguiContexts,
    selection: Res<Selection>,
    mut vis: ResMut<crate::controls::PanelVisibility>,
    agents: Query<(&Species, &Reserve, &Genotype, &Vision, &Perception, &Action), With<Agent>>,
) -> Result {
    if !vis.inspector {
        return Ok(());
    }
    let tidy = vis.tidy_windows;
    let ctx = contexts.ctx_mut()?;
    let screen = ctx.content_rect();
    // Pas de `vscroll` ici : `inspector_section` a déjà sa propre `ScrollArea`
    // (la liste des rayons de vision peut être longue).
    let mut window = egui::Window::new("Inspecteur d'agent")
        .open(&mut vis.inspector)
        .default_pos([560.0, 820.0])
        .default_width(260.0)
        .resizable(true);
    if tidy {
        window = window
            .current_pos(crate::controls::tidy_pos(screen, crate::controls::WindowSlot::Inspector));
    }
    window.show(ctx, |ui| inspector_section(ui, &selection, &agents));
    Ok(())
}

/// Le contenu de l'inspecteur (sans le cadre de fenêtre). Si l'agent sélectionné a
/// disparu (mort), on le signale. Lecture seule du monde.
pub fn inspector_section(
    ui: &mut egui::Ui,
    selection: &Selection,
    agents: &Query<(&Species, &Reserve, &Genotype, &Vision, &Perception, &Action), With<Agent>>,
) {
    let Some(entity) = selection.0 else {
        ui.weak("Clique un agent dans l'aire pour l'inspecter.");
        return;
    };
    let Ok((species, reserve, genotype, vision, perception, action)) = agents.get(entity) else {
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

        ui.separator();
        ui.strong("Énergie / réserve");
        ui.add(
            egui::ProgressBar::new(reserve.fraction())
                .text(format!("{:.1} / {:.0}", reserve.current, reserve.max)),
        );

        ui.separator();
        ui.strong("Génotype (gènes hérités)");
        egui::Grid::new("genes").num_columns(2).show(ui, |ui| {
            ui.label("vitesse max");
            ui.label(format!("{:.1}", genotype.max_speed));
            ui.end_row();
            ui.label("agilité");
            ui.label(format!("{:.3}", genotype.agility));
            ui.end_row();
            ui.label("portée vision");
            ui.label(format!("{:.1}", genotype.vision_range));
            ui.end_row();
            ui.label("champ vision");
            ui.label(format!("{:.0}°", genotype.vision_fov.to_degrees()));
            ui.end_row();
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

        ui.separator();
        ui.strong(format!("Perception — vision ({} rayons)", vision.ray_count));
        ui.weak("proximité par rayon (0 = rien, 1 = au contact)");
        for (i, &proximity) in perception.vision.iter().enumerate() {
            ui.add(egui::ProgressBar::new(proximity).text(format!("r{i} · {proximity:.2}")));
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
