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
/// scénario. Sépare *quel* cerveau (et ses **paramètres d'archétype**, propres à
/// chaque variant — `turn_rate` pour l'errance, aucun pour le chasseur) de son
/// *état vivant* : un `BrainKind` (RON : `Wander(turn_rate: …)` / `Hunter`) se
/// compile en un [`Brain`] frais au spawn, comme un génotype en phénotype (§2).
/// Édité par le sélecteur de cerveau de l'éditeur (item 15) ; chaque variant y
/// expose ses propres paramètres. Substitution par scénario (§4) faite ; la
/// substitution *par espèce* reste à venir.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum BrainKind {
    /// [`Brain::Wander`] — errance. `turn_rate` : amplitude de la dérive de cap par
    /// tick (le paramètre propre à ce variant). Défaut rétro-compatible (item 16).
    Wander { turn_rate: f32 },
    /// [`Brain::Hunter`] — réflexe déterministe, sans paramètre.
    Hunter,
}

impl Default for BrainKind {
    /// Errance au taux d'archétype : le défaut d'avant l'item 16 (scénarios qui
    /// ne parlent pas de `brain` restent des mondes d'errance).
    fn default() -> Self {
        BrainKind::Wander {
            turn_rate: WanderBrain::DEFAULT_TURN_RATE,
        }
    }
}

impl BrainKind {
    /// Compile le choix en un cerveau frais. La graine n'alimente que les cerveaux
    /// à état stochastique (l'errance) ; le chasseur, déterministe, l'ignore.
    pub fn build(&self, seed: u64, heading: f32) -> Brain {
        match self {
            BrainKind::Wander { turn_rate } => {
                Brain::Wander(WanderBrain::new(seed, heading, *turn_rate))
            }
            BrainKind::Hunter => Brain::Hunter(HunterBrain),
        }
    }

    /// Libellé court du type, pour le sélecteur d'éditeur (item 15).
    pub fn name(&self) -> &'static str {
        match self {
            BrainKind::Wander { .. } => "Errance",
            BrainKind::Hunter => "Chasseur",
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
    /// Amplitude par défaut de la dérive de cap (rad/tick) — la valeur d'archétype
    /// du variant `Wander` quand le scénario n'en précise pas.
    pub const DEFAULT_TURN_RATE: f32 = 0.25;

    pub fn new(seed: u64, initial_heading: f32, turn_rate: f32) -> Self {
        Self {
            rng: Rng::new(seed),
            heading: initial_heading,
            turn_rate,
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
/// perception donne toujours la même action.
///
/// Un **seul champ de pilotage**, où chaque rayon « pousse » vers sa direction
/// d'un poids `ATTRACTION · cible + dégagement(1 − obstacle)` :
///
/// - un rayon dont le hit le plus proche est une **cible** (`vision == target`)
///   pèse `ATTRACTION · t + (1 − t)` ≈ **attire** (avec `ATTRACTION > 1`, plus que
///   l'espace dégagé) → il *s'en approche* ;
/// - un rayon bouché par un **non-cible** (mur, autre entité : `target = 0`) pèse
///   `1 − occlusion` ≈ 0 → il ne pousse pas vers lui → il s'en *détourne*, sans le
///   fuir activement ;
/// - un rayon **dégagé** pèse 1 → en terrain vide, les poussées symétriques se
///   résolvent vers l'avant (il balaie le terrain en ligne quasi droite).
///
/// D'où le correctif : il n'évite plus la nourriture comme un obstacle — la cible
/// attire, le reste se contourne. C'est le **groupe témoin compétent** (§4) : même
/// dépense d'énergie qu'un errant, mais il *trouve* sa nourriture. Il ne sait
/// toutefois pas mémoriser : hors de portée, il ne fait qu'explorer (un MLP, lui,
/// pourra apprendre mieux — c'est l'enjeu). « Cible » se définit par la table de
/// relations (§3) : sans relation, rien n'attire et tout n'est qu'obstacle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HunterBrain;

impl HunterBrain {
    /// Sur-pondération d'une cible face à l'espace dégagé (`> 1` pour qu'un rayon
    /// pointant une cible l'emporte sur un rayon simplement libre).
    const ATTRACTION: f32 = 2.5;

    fn think(&self, perception: &Perception) -> Action {
        let mut steer = Vec2::ZERO;
        for i in 0..perception.ray_dirs.len() {
            let openness = 1.0 - perception.vision[i];
            let weight = Self::ATTRACTION * perception.target[i] + openness;
            steer += perception.ray_dirs[i] * weight;
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

    /// Deux cibles visibles → le pilotage penche vers la plus proche (-Y, proximité
    /// 0.9) plutôt que vers la lointaine (+Y, 0.3) : l'attraction est graduée par la
    /// proximité.
    #[test]
    fn hunter_favors_the_nearer_target() {
        let p = perception([0.3, 0.0, 0.9], [0.3, 0.0, 0.9]);
        let action = HunterBrain.think(&p);
        assert!(action.dir.y < 0.0, "penche vers la cible la plus proche (-Y), dir={:?}", action.dir);
        assert_eq!(action.throttle, 1.0);
    }

    /// Une **cible** d'un côté (+Y) et un **obstacle non-cible** (mur) de l'autre
    /// (-Y), à proximité égale → le chasseur va vers la cible et s'écarte du mur.
    /// C'est le correctif : il ne fuit plus la nourriture comme un obstacle.
    #[test]
    fn hunter_approaches_target_not_plain_obstacle() {
        // +Y : nourriture (vision == target) ; -Y : mur (vision sans target).
        let action = HunterBrain.think(&perception([0.6, 0.0, 0.6], [0.6, 0.0, 0.0]));
        assert!(
            action.dir.y > 0.0,
            "doit aller vers la cible (+Y), pas vers le mur (-Y), dir={:?}",
            action.dir
        );
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

    /// Le paramètre propre au variant `Wander` (turn_rate) est bien transmis au
    /// cerveau compilé : c'est ce que le sélecteur d'éditeur (item 15) fait varier.
    #[test]
    fn brainkind_wander_carries_its_turn_rate() {
        match (BrainKind::Wander { turn_rate: 0.4 }).build(1, 0.0) {
            Brain::Wander(w) => assert_eq!(w.turn_rate, 0.4),
            other => panic!("attendu Wander, obtenu {other:?}"),
        }
        assert!(matches!(BrainKind::default(), BrainKind::Wander { .. }));
        assert!(matches!(
            (BrainKind::Hunter).build(1, 0.0),
            Brain::Hunter(_)
        ));
    }
}
