//! Le cerveau.
//!
//! Contrat : `&Perception` (flottants normalisés) → `Action` (flottants).
//! L'intérieur est interchangeable. On stocke en **`enum`**, pas en
//! `Box<dyn>` : dispatch statique, `serde` propre, et le compilateur liste les
//! `match` à compléter quand on ajoute un type de cerveau. Le crossover est de
//! toute façon intra-type (on ne croise pas un NN avec une FSM).

use crate::components::{Action, Perception};
use crate::rng::Rng;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Le cerveau d'un agent. Un variant par implémentation.
///
/// `serde` propre (§2) : un cerveau est sérialisable, donc capturable dans un
/// snapshot de run (item 13) — et le sera pour un futur MLP sans changer le
/// contrat.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Brain {
    /// Échafaudage déterministe trivial : marche aléatoire (errance). Dérisque
    /// toute la chaîne percevoir→décider→agir avant qu'aucun cerveau appris
    /// n'existe, et sert de groupe témoin « naïf ».
    Wander(WanderBrain),
    /// Réflexe déterministe (item 16) : fonce vers la cible perçue la plus proche.
    /// Le **groupe témoin compétent** — un cerveau appris qui ne le bat pas n'a
    /// rien appris (§4) — et le 2ᵉ variant qui rend le sélecteur de cerveau
    /// falsifiable.
    Hunter(HunterBrain),
}

impl Brain {
    /// Le contrat. `match` exhaustif → ajout d'un variant = erreur de compile
    /// ici, exactement ce qu'on veut.
    pub fn think(&mut self, perception: &Perception) -> Action {
        match self {
            Brain::Wander(b) => b.think(perception),
            Brain::Hunter(b) => b.think(perception),
        }
    }

    /// Libellé court du type de cerveau, pour l'inspecteur (item 12).
    pub fn name(&self) -> &'static str {
        match self {
            Brain::Wander(_) => "Errance",
            Brain::Hunter(_) => "Chasseur",
        }
    }
}

/// Le **type** de cerveau — le choix de l'auteur de la décision (§1), donnée de
/// scénario. Sépare *quel* cerveau de son *état* : un `BrainKind` (RON :
/// `Wander` / `Hunter`) se compile en un [`Brain`] frais au spawn, comme un
/// génotype se compile en phénotype (§2). Substitution par scénario (§4) ; la
/// substitution par espèce et le sélecteur d'éditeur (item 15) viendront dessus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrainKind {
    /// [`Brain::Wander`] — l'échafaudage d'avant l'item 16, défaut rétro-compatible.
    #[default]
    Wander,
    /// [`Brain::Hunter`].
    Hunter,
}

impl BrainKind {
    /// Compile le choix en un cerveau frais. La graine n'alimente que les cerveaux
    /// à état stochastique (l'errance) ; le chasseur, déterministe, l'ignore.
    pub fn build(&self, seed: u64, heading: f32) -> Brain {
        match self {
            BrainKind::Wander => Brain::Wander(WanderBrain::new(seed, heading)),
            BrainKind::Hunter => Brain::Hunter(HunterBrain),
        }
    }
}

/// Errance par braquage : le cap dérive d'un petit incrément aléatoire à chaque
/// tick, produisant des trajectoires courbes plausibles.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WanderBrain {
    rng: Rng,
    /// Cap courant, en radians.
    heading: f32,
    /// Amplitude maximale de la dérive de cap par tick, en radians.
    turn_rate: f32,
}

impl WanderBrain {
    pub fn new(seed: u64, initial_heading: f32) -> Self {
        Self {
            rng: Rng::new(seed),
            heading: initial_heading,
            turn_rate: 0.25,
        }
    }

    fn think(&mut self, _perception: &Perception) -> Action {
        self.heading += self.rng.next_signed() * self.turn_rate;
        Action {
            dir: Vec2::new(self.heading.cos(), self.heading.sin()),
            throttle: 1.0,
        }
    }
}

/// Chasseur **déterministe** (item 16) : pas d'état, pas de RNG — une même
/// perception donne toujours la même action. Un réflexe à deux régimes :
///
/// 1. **Chasse** : s'il voit une cible (canal [`Perception::target`]), il fonce
///    vers le rayon de plus forte proximité — la cible perçue la plus proche.
/// 2. **Exploration / évitement** : sinon, il glisse vers l'espace le plus dégagé
///    (somme des directions de rayon pondérée par leur *dégagement* `1 - vision`),
///    ce qui le détourne des murs et balaie le terrain quasi en ligne droite
///    quand tout est dégagé (poussées symétriques → résultante vers l'avant).
///
/// C'est le **groupe témoin compétent** (§4) : il dépense la même énergie qu'un
/// errant — il *trouve* simplement sa nourriture au lieu de tomber dessus par
/// hasard. Il ne sait toutefois pas mémoriser : hors de portée de vision, il ne
/// fait qu'explorer (un MLP, lui, pourra apprendre mieux — c'est l'enjeu).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HunterBrain;

impl HunterBrain {
    fn think(&self, perception: &Perception) -> Action {
        // 1. Chasse : le rayon « cible » de plus forte proximité (= la plus proche).
        let mut best: Option<(f32, usize)> = None;
        for (i, &p) in perception.target.iter().enumerate() {
            if p > 0.0 && best.is_none_or(|(bp, _)| p > bp) {
                best = Some((p, i));
            }
        }
        if let Some((_, i)) = best {
            return Action {
                dir: perception.ray_dirs[i],
                throttle: 1.0,
            };
        }

        // 2. Rien à chasser : viser le plus dégagé. Chaque rayon « pousse » vers sa
        // direction d'autant plus qu'il est libre ; la résultante évite les
        // obstacles, et part droit devant en terrain dégagé (poussées symétriques).
        let mut steer = Vec2::ZERO;
        for (i, &occlusion) in perception.vision.iter().enumerate() {
            steer += perception.ray_dirs[i] * (1.0 - occlusion);
        }
        let dir = steer.normalize_or_zero();
        // Encerclé (tout occlus) ou aveugle (zéro rayon) : on garde le cap.
        let dir = if dir == Vec2::ZERO { perception.heading } else { dir };
        Action { dir, throttle: 1.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trois rayons : gauche (+Y), avant (+X), droite (-Y) — un éventail symétrique
    /// autour du cap +X, comme `perceive` les produirait.
    fn perception(vision: [f32; 3], target: [f32; 3]) -> Perception {
        Perception {
            heading: Vec2::X,
            vision: vision.into(),
            target: target.into(),
            ray_dirs: vec![Vec2::Y, Vec2::X, Vec2::NEG_Y].into_boxed_slice(),
        }
    }

    /// Cibles visibles → le chasseur fonce vers le rayon de plus forte proximité
    /// (la cible la plus proche), pas vers la plus lointaine.
    #[test]
    fn hunter_steers_toward_nearest_target() {
        let p = perception([0.3, 0.0, 0.9], [0.3, 0.0, 0.9]);
        let action = HunterBrain.think(&p);
        assert_eq!(action.dir, Vec2::NEG_Y, "rayon de droite = cible la plus proche");
        assert_eq!(action.throttle, 1.0);
    }

    /// Pas de cible mais un obstacle à gauche (+Y) → la résultante penche à
    /// l'opposé (vers -Y) : le chasseur s'écarte du mur.
    #[test]
    fn hunter_steers_toward_open_space_when_no_target() {
        let action = HunterBrain.think(&perception([0.9, 0.0, 0.0], [0.0, 0.0, 0.0]));
        assert!(action.dir.y < 0.0, "doit s'écarter de l'obstacle en +Y, dir={:?}", action.dir);
    }

    /// Terrain entièrement dégagé : poussées symétriques → cap maintenu vers l'avant.
    #[test]
    fn hunter_cruises_forward_in_the_open() {
        let action = HunterBrain.think(&perception([0.0, 0.0, 0.0], [0.0, 0.0, 0.0]));
        assert!(action.dir.x > 0.9, "doit filer droit devant, dir={:?}", action.dir);
        assert!(action.dir.y.abs() < 1e-6);
    }

    /// La cible prime sur l'évitement : même cerné d'obstacles, un chasseur qui voit
    /// une cible va dessus (le réflexe de chasse court-circuite l'exploration).
    #[test]
    fn target_takes_priority_over_avoidance() {
        let action = HunterBrain.think(&perception([0.8, 0.8, 0.8], [0.0, 0.8, 0.0]));
        assert_eq!(action.dir, Vec2::X, "la cible au centre l'emporte");
    }

    /// Déterminisme : deux évaluations de la même perception donnent la même action
    /// (pas d'état, pas de RNG — c'est ce qui en fait un groupe témoin reproductible).
    #[test]
    fn hunter_is_deterministic() {
        let p = perception([0.1, 0.4, 0.2], [0.0, 0.4, 0.0]);
        assert_eq!(HunterBrain.think(&p).dir, HunterBrain.think(&p).dir);
    }
}
