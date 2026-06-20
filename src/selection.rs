//! **Sélection** d'une entité pour l'observation — surbrillance + éventail de rayons
//! de vision — partagée par l'aperçu fenêtré et l'enregistreur vidéo.
//!
//! C'est strictement du rendu/observation (tout dans `Update`, jamais `FixedUpdate`).
//! La *cible* de la sélection est pilotée différemment selon le binaire :
//!
//! - **fenêtré** : par le picking souris (cf. `inspector` côté binaire) ;
//! - **enregistreur** : par la **sélection automatique** ([`AutoSelectPlugin`]), pour
//!   qu'une vidéo montre en continu les rayons d'un agent sans intervention — avec un
//!   *mode de roulement* ([`SelectionRoll`]) qui choisit comment l'agent mis en avant
//!   change au fil du temps.
//!
//! Le **rendu** (l'anneau + les rayons) vit dans [`SelectionRenderPlugin`], commun aux
//! deux : il ne fait que lire la ressource [`Selection`], d'où qu'elle vienne.

use crate::components::{Age, Agent, Locomotion, Perception, Radius, Species, Vision};
use bevy::prelude::*;

/// L'entité actuellement mise en avant pour l'observation (surbrillance + rayons), le
/// cas échéant. Écrite par le picking (fenêtré) ou la sélection automatique
/// (enregistreur), lue par le rendu de [`SelectionRenderPlugin`].
#[derive(Resource, Default)]
pub struct Selection(pub Option<Entity>);

/// **Mode de roulement** de la sélection automatique : comment l'agent mis en avant
/// change au fil du temps pendant un enregistrement. Tous les modes « à timer » (cf.
/// [`rolls`](Self::rolls)) **tiennent** leur cible un intervalle entier — jamais de
/// changement par frame — pour rester agréables à regarder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SelectionRoll {
    /// Aucune sélection automatique : l'enregistreur ne met rien en avant.
    Off,
    /// **Fixe** : un seul agent, gardé tant qu'il vit (re-choisi à sa mort).
    Sticky,
    /// **Rotation** : passe à l'agent suivant à intervalle régulier (round-robin).
    Cycle,
    /// **En action** : l'agent dont les rayons détectent le plus (vision+cible+menace) —
    /// réévalué à chaque intervalle. Le meilleur pour *montrer* les raycasts en situation.
    Active,
    /// **Tour des espèces** : à chaque intervalle, l'espèce suivante (un de ses agents, le
    /// plus « en action ») — chaque espèce a ainsi son temps d'écran.
    SpeciesTour,
    /// **Doyen** (défaut) : le plus âgé vivant. Ne change qu'à sa mort (l'âge croît pour
    /// tous au même rythme → le doyen le reste) : pas de timer, donc calme — un suivi
    /// posé du survivant, agréable par défaut.
    #[default]
    Eldest,
}

impl SelectionRoll {
    /// Tous les modes, pour peupler un sélecteur d'UI.
    pub const ALL: [SelectionRoll; 6] = [
        Self::Off,
        Self::Sticky,
        Self::Cycle,
        Self::Active,
        Self::SpeciesTour,
        Self::Eldest,
    ];

    /// Jeton CLI stable (passé au binaire `record` par le menu d'enregistrement).
    pub fn cli(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Sticky => "sticky",
            Self::Cycle => "cycle",
            Self::Active => "active",
            Self::SpeciesTour => "species",
            Self::Eldest => "eldest",
        }
    }

    /// Parse un jeton CLI ; `None` si inconnu.
    pub fn from_cli(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.cli() == s)
    }

    /// Libellé pour l'UI (français, comme le reste de l'éditeur).
    pub fn label(&self) -> &'static str {
        match self {
            Self::Off => "Aucune",
            Self::Sticky => "Fixe",
            Self::Cycle => "Rotation",
            Self::Active => "En action",
            Self::SpeciesTour => "Tour des espèces",
            Self::Eldest => "Doyen",
        }
    }

    /// `true` si ce mode se réévalue **à intervalle régulier** (donc affiche/utilise
    /// l'intervalle). `Off`, `Fixe` et `Doyen` n'ont pas de timer : ils ne changent qu'à
    /// la mort de la cible.
    pub fn rolls(&self) -> bool {
        matches!(self, Self::Cycle | Self::Active | Self::SpeciesTour)
    }
}

/// Rendu de la sélection : un anneau autour de l'agent mis en avant + son éventail de
/// rayons de vision. Commun au fenêtré et à l'enregistreur — il lit seulement
/// [`Selection`]. N'inclut **pas** le pilote de sélection (picking ou auto).
pub struct SelectionRenderPlugin;

impl Plugin for SelectionRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selection>()
            .add_systems(Update, (highlight_selection, draw_selected_vision));
    }
}

/// **Sélection automatique** (enregistreur) : garde toujours un agent **mobile** mis en
/// avant, en le faisant *rouler* selon [`SelectionRoll`]. Cible les agents mobiles car
/// eux seuls lancent des rayons visibles (la flore immobile n'en a pas, cf. `movement`).
/// À ajouter en plus de [`SelectionRenderPlugin`].
pub struct AutoSelectPlugin {
    /// Mode de roulement choisi.
    pub roll: SelectionRoll,
    /// Intervalle entre deux changements, en secondes (modes « à timer », cf.
    /// [`SelectionRoll::rolls`]).
    pub interval: f32,
}

impl Plugin for AutoSelectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selection>()
            .insert_resource(AutoSelect {
                roll: self.roll,
                interval: self.interval.max(0.1),
                elapsed: 0.0,
                cursor: 0,
            })
            .add_systems(Update, drive_selection);
    }
}

/// État du pilote de sélection automatique.
#[derive(Resource)]
struct AutoSelect {
    roll: SelectionRoll,
    interval: f32,
    /// Temps écoulé depuis le dernier changement, en secondes.
    elapsed: f32,
    /// Curseur round-robin : index d'agent (mode `Cycle`) ou d'espèce (`SpeciesTour`).
    cursor: usize,
}

/// Métriques d'un agent candidat à la mise en avant, relevées au moment de choisir.
struct Cand {
    entity: Entity,
    species: u16,
    age: f32,
    /// Somme des canaux de perception (vision + cible + menace) : « à quel point il voit ».
    stim: f32,
}

/// `Update` (enregistreur) : maintient un agent mobile sélectionné selon le mode.
///
/// On ne **rechoisit** que lorsque la cible a disparu (mort) ou, pour les modes à timer
/// ([`SelectionRoll::rolls`]), à l'échéance de l'intervalle — jamais par frame. La cible
/// tient donc tout un intervalle : pas de scintillement, même quand la métrique (énergie,
/// vitesse, « en action »…) fluctue vite.
fn drive_selection(
    time: Res<Time>,
    mut auto: ResMut<AutoSelect>,
    mut selection: ResMut<Selection>,
    agents: Query<(Entity, &Locomotion, &Species, &Age, &Perception), With<Agent>>,
) {
    if auto.roll == SelectionRoll::Off {
        return;
    }
    auto.elapsed += time.delta_secs();
    let due = auto.elapsed >= auto.interval;
    // La cible courante est-elle encore un agent mobile vivant ?
    let valid = selection
        .0
        .is_some_and(|e| agents.get(e).is_ok_and(|(_, loco, ..)| !loco.is_immobile()));
    // Tenir la cible : on ne rechoisit qu'à la mort, ou à l'échéance pour les modes à timer.
    if valid && !(auto.roll.rolls() && due) {
        return;
    }

    // Agents MOBILES vivants (seuls à montrer des rayons) + leurs métriques de choix.
    let mut cands: Vec<Cand> = agents
        .iter()
        .filter(|(_, loco, ..)| !loco.is_immobile())
        .map(|(entity, _, species, age, perception)| Cand {
            entity,
            species: species.0,
            age: age.0,
            stim: perception
                .vision
                .iter()
                .chain(perception.target.iter())
                .chain(perception.threat.iter())
                .copied()
                .sum(),
        })
        .collect();
    if cands.is_empty() {
        selection.0 = None;
        return;
    }
    // Ordre stable (par bits d'entité) pour une rotation reproductible.
    cands.sort_unstable_by_key(|c| c.entity.to_bits());

    auto.elapsed = 0.0;
    let roll = auto.roll;
    selection.0 = Some(choose(roll, &cands, &mut auto));
}

/// Choisit l'agent mis en avant parmi `cands` (non vide, trié) selon le mode.
fn choose(roll: SelectionRoll, cands: &[Cand], auto: &mut AutoSelect) -> Entity {
    // L'agent maximisant une métrique (départage stable : `cands` est trié).
    let best = |key: &dyn Fn(&Cand) -> f32| -> Entity {
        cands
            .iter()
            .max_by(|a, b| key(a).total_cmp(&key(b)))
            .map_or(cands[0].entity, |c| c.entity)
    };
    match roll {
        // `Off` ne parvient jamais ici (filtré) ; `Fixe` garde le premier stable.
        SelectionRoll::Off | SelectionRoll::Sticky => cands[0].entity,
        SelectionRoll::Cycle => {
            auto.cursor = (auto.cursor + 1) % cands.len();
            cands[auto.cursor].entity
        }
        SelectionRoll::Active => best(&|c| c.stim),
        SelectionRoll::Eldest => best(&|c| c.age),
        // Tour des espèces : espèce suivante (round-robin), puis son agent le plus « en action ».
        SelectionRoll::SpeciesTour => {
            let mut species: Vec<u16> = cands.iter().map(|c| c.species).collect();
            species.sort_unstable();
            species.dedup();
            auto.cursor = (auto.cursor + 1) % species.len();
            let target = species[auto.cursor];
            cands
                .iter()
                .filter(|c| c.species == target)
                .max_by(|a, b| a.stim.total_cmp(&b.stim))
                .map_or(cands[0].entity, |c| c.entity)
        }
    }
}

/// Rendu seul : entourer l'agent sélectionné d'un anneau, pour le repérer dans l'aire.
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

/// Rendu seul : l'éventail de rayons de vision de l'agent **sélectionné** uniquement —
/// pour *voir* l'occlusion à l'œuvre sans saturer l'écran. On relit l'état sensoriel déjà
/// calculé par la sim ([`Perception`]) — aucun raycast recalculé ici. Rayon clair = rien
/// vu ; il rougit et raccourcit à mesure qu'un obstacle se rapproche.
///
/// Une entité **immobile** (flore) n'a pas de vision exploitable : `perceive` ne lui lance
/// aucun rayon (perception vide), donc on ne dessine rien pour elle — sélectionner un
/// buisson ne trace pas un éventail trompeur.
pub fn draw_selected_vision(
    mut gizmos: Gizmos,
    selection: Res<Selection>,
    agents: Query<(&Transform, &Vision, &Perception, &Locomotion), With<Agent>>,
) {
    let Some(entity) = selection.0 else {
        return;
    };
    let Ok((transform, vision, perception, loco)) = agents.get(entity) else {
        return;
    };
    if loco.is_immobile() {
        return; // flore : aucun rayon à montrer.
    }
    let origin = transform.translation.truncate();
    let facing = perception.heading;
    for (i, &proximity) in perception.vision.iter().enumerate() {
        let dir = vision.ray_dir(i, facing);
        let length = vision.range * (1.0 - proximity);
        let color = Color::srgb(0.25 + 0.75 * proximity, 0.55 * (1.0 - proximity), 0.15);
        gizmos.line_2d(origin, origin + dir * length, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les jetons CLI font l'aller-retour, libellés/jetons sont uniques et non vides —
    /// garde-fou contre un oubli (ou un doublon) quand on ajoute un mode.
    #[test]
    fn roll_cli_roundtrips() {
        let mut clis = std::collections::HashSet::new();
        for m in SelectionRoll::ALL {
            assert_eq!(SelectionRoll::from_cli(m.cli()), Some(m));
            assert!(!m.label().is_empty());
            assert!(clis.insert(m.cli()), "jeton CLI dupliqué : {}", m.cli());
        }
        assert_eq!(SelectionRoll::from_cli("inconnu"), None);
    }

    /// Les modes **à timer** (réévalués à intervalle) « roulent » ; `Off`/`Fixe`/`Doyen`
    /// ne changent qu'à la mort de la cible.
    #[test]
    fn timer_modes_roll_others_dont() {
        for m in [
            SelectionRoll::Off,
            SelectionRoll::Sticky,
            SelectionRoll::Eldest,
        ] {
            assert!(!m.rolls(), "{m:?} ne devrait pas rouler sur un timer");
        }
        for m in [
            SelectionRoll::Cycle,
            SelectionRoll::Active,
            SelectionRoll::SpeciesTour,
        ] {
            assert!(m.rolls(), "{m:?} devrait rouler sur un timer");
        }
    }
}
