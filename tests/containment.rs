//! Confinement : aucun agent ne doit sortir de l'arène.
//!
//! Garde-fou des murs en **demi-espaces** (plans infinis) : on fait tourner un
//! vrai monde de sim — déplacements *et* reproduction (qui pose des nouveau-nés
//! près des bords) — et on vérifie qu'aucun agent ne franchit la frontière. Ce
//! test attrape les deux régressions possibles : un retour à des murs fins (où
//! un agent peut tunneler ou naître dehors) **et** une normale inversée (qui
//! éjecterait au contraire tout le monde *hors* de l'arène).

use bevy::prelude::*;
use teemlab::components::Agent;
use teemlab::SimConfig;

mod common;

#[test]
fn agents_stay_within_arena() {
    // Le scénario d'évolution : ça bouge à pleine vitesse et ça se reproduit, donc
    // ça presse les bords de toutes les façons utiles.
    let config = SimConfig::from_ron_file("scenarios/evolution.ron")
        .expect("scénario evolution.ron chargeable");

    // Chaque `update()` avance d'un tick fixe pile (cf. `common::stepping_app`).
    let mut app = common::stepping_app(&config);

    // ~30 s de temps simulé : largement de quoi atteindre les murs et enchaîner
    // des générations près des bords.
    for _ in 0..2000 {
        app.update();
    }

    let h = config.arena_half_extent;
    // Tolérance : un agent peut s'enfoncer de quelques pixels dans le demi-espace
    // avant que le solveur ne le repousse. Une évasion réelle, elle, se chiffre en
    // dizaines/centaines d'unités — donc cette marge serrée suffit à la détecter.
    let margin = config.agent_radius + 2.0;

    let world = app.world_mut();
    let mut query = world.query_filtered::<(Entity, &Transform), With<Agent>>();
    let mut escaped = Vec::new();
    for (entity, transform) in query.iter(world) {
        let p = transform.translation.truncate();
        if p.x.abs() > h + margin || p.y.abs() > h + margin {
            escaped.push((entity, p));
        }
    }
    let population = query.iter(world).count();
    assert!(population > 0, "la population s'est éteinte — test non concluant");
    assert!(
        escaped.is_empty(),
        "{} agent(s) hors de l'arène (|x|,|y| ≤ {:.0} attendu) : {:?}",
        escaped.len(),
        h + margin,
        escaped,
    );
}
