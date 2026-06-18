//! Le *scénario* : les paramètres d'une run, chargés depuis un fichier RON.
//!
//! C'est ici que se matérialise l'axe central du projet — **un moteur, des
//! scénarios**. [`SimConfig`] n'est plus un littéral codé en dur mais de la
//! *donnée* : un fichier RON que les deux points d'entrée (fenêtré et headless)
//! chargent à l'identique. Faire varier une expérience = éditer un `.ron`, pas
//! recompiler.

use crate::brain::BrainKind;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Paramètres d'une run, désérialisés depuis un scénario RON.
///
/// `#[serde(default)]` : un scénario n'a besoin de mentionner que les champs
/// qu'il veut changer ; tout le reste retombe sur [`SimConfig::default`]. Un
/// fichier `()` vide est donc un scénario valide (= les défauts).
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SimConfig {
    /// Cadence du timestep fixe, en Hz (stabilité du solveur, pas le rendu).
    pub tick_hz: f64,
    /// Demi-côté de l'arène carrée, en unités monde.
    pub arena_half_extent: f32,
    /// Nombre d'agents au spawn.
    pub agent_count: usize,
    /// Rayon d'un agent.
    pub agent_radius: f32,
    /// Vitesse maximale d'un agent.
    pub max_speed: f32,
    /// Nombre de rayons de vision du **fondateur** (la précision visuelle de
    /// l'archétype). N'est plus verrouillé : c'est le gène `vision_rays` du
    /// [`crate::genotype::Genotype`] qui le porte ensuite par individu, mutable et
    /// hérité (cf. [`crate::genotype::TRAITS`]). Ce champ ne fait que graîner la
    /// valeur fondatrice (`Genotype::base`).
    pub vision_rays: usize,
    /// Champ de vision *total*, en degrés, réparti symétriquement autour du cap.
    pub vision_fov_deg: f32,
    /// Portée de vision, en unités monde (= longueur max d'un rayon).
    pub vision_range: f32,
    /// Nombre d'espèces. Les agents sont répartis en round-robin (`i % count`)
    /// — sauf si [`agents_per_species`](Self::agents_per_species) impose un partage
    /// explicite.
    pub species_count: u16,
    /// Effectifs **par espèce** au spawn : `agents_per_species[s]` agents de
    /// l'espèce `s`. Vide par défaut → repli sur le partage *uniforme*
    /// (`agent_count` en round-robin sur `species_count`, le comportement d'avant
    /// cet ajout). Non vide, il **prime** : il fixe à la fois l'effectif de chaque
    /// espèce **et**, par sa longueur, le nombre d'espèces ; `agent_count` est alors
    /// ignoré. C'est le levier d'une **pyramide trophique** (peu de prédateurs,
    /// beaucoup de proies) qu'un ratio uniforme 50/50 interdit — l'archétype reste
    /// partagé, seul l'effectif diffère.
    pub agents_per_species: Vec<usize>,
    /// Réserve initiale (= max) de chaque agent.
    pub reserve_max: f32,
    /// Le **type de cerveau** des agents (l'auteur de la décision, §1) : choix de
    /// scénario, compilé en un [`crate::brain::Brain`] frais à chaque spawn
    /// (peuplement initial **et** reproduction). `Wander` par défaut (errance,
    /// l'échafaudage d'avant l'item 16). La substitution par espèce et le
    /// sélecteur d'éditeur (item 15) viendront se poser dessus.
    pub brain: BrainKind,
    /// Cerveau **par espèce** au fondateur (item 18a) : `brains_per_species[s]`
    /// fonde l’espèce `s`. Vide par défaut → le [`brain`](Self::brain) uniforme
    /// s’applique partout (rétro-compatible, aucun `.ron` à migrer) ; une espèce
    /// au-delà de la longueur retombe aussi sur `brain`. C’est le levier de la
    /// **cohabitation** témoin/appris (§4) — corps partagé, cerveau distinct : la
    /// substitution par espèce que les items 16 et 17 réservaient. Calqué sur
    /// [`agents_per_species`](Self::agents_per_species), additif. Au-delà du
    /// fondateur, le cerveau se transmet par **héritage** à la reproduction
    /// ([`crate::brain::Brain::reproduce`]), sans relire ce champ.
    pub brains_per_species: Vec<BrainKind>,
    /// Table d'interactions : qui peut agir sur qui (cf. §3, §4). Vide par
    /// défaut → aucune interaction (monde inerte, comme avant l'item 7).
    pub relations: Vec<Relation>,
    /// Métabolisme de base : énergie drainée **par seconde**, au repos. `0` →
    /// pas de drain (monde inerte, comme avant l'item 8).
    pub base_metabolism: f32,
    /// Surcoût de locomotion : énergie/seconde supplémentaire à pleine vitesse
    /// (proportionnel à la fraction de vitesse). La vitesse devient un coût.
    pub move_cost: f32,
    /// Nombre de sources de nourriture maintenues dans l'arène. `0` → aucune.
    pub food_count: usize,
    /// Rayon d'une source de nourriture.
    pub food_radius: f32,
    /// Énergie contenue dans une source de nourriture (= sa réserve pleine).
    pub food_energy: f32,
    /// Vitesse de repousse de la nourriture, en sources **par seconde**. `0` →
    /// maintien instantané à `food_count` (régime item 8). Une valeur finie crée
    /// une **capacité de charge** : à population élevée, la nourriture est mangée
    /// plus vite qu'elle ne repousse → la faim borne la croissance (item 9).
    pub food_regen: f32,
    /// Espèce assignée à la nourriture, pour que la table de relations puisse la
    /// désigner comme cible (`(actor: <agent>, target: <food_species>, …)`).
    pub food_species: u16,
    /// Valeur initiale du gène d'agilité (vivacité du braquage, `[0, 1]`).
    pub agility: f32,
    /// Amplitude de mutation : écart-type d'une mutation de gène, en *fraction*
    /// de l'amplitude (`max - min`) de ce gène. `0` → pas de mutation.
    pub mutation_rate: f32,
    /// Énergie nécessaire pour se reproduire. `0` → pas de reproduction (régime
    /// pré-item-9 : la population ne fait que décliner).
    pub reproduction_threshold: f32,
    /// Énergie transmise à l'enfant, déduite du parent (conservation : aucune
    /// énergie créée à la reproduction).
    pub offspring_energy: f32,
    /// Bornes du gène de vitesse maximale.
    pub speed_bounds: Bounds,
    /// Bornes du gène d'agilité.
    pub agility_bounds: Bounds,
    /// Bornes du gène de portée de vision.
    pub vision_range_bounds: Bounds,
    /// Bornes du gène de champ de vision, **en degrés**.
    pub vision_fov_bounds: Bounds,
    /// Bornes du gène de seuil de reproduction.
    pub reproduction_threshold_bounds: Bounds,
    /// Bornes du gène d'énergie passée à l'enfant.
    pub offspring_energy_bounds: Bounds,
    /// Bornes du gène de taux de mutation.
    pub mutation_rate_bounds: Bounds,
    /// Bornes du gène de métabolisme de base.
    pub base_metabolism_bounds: Bounds,
    /// Bornes du gène de surcoût de locomotion.
    pub move_cost_bounds: Bounds,
    /// Bornes du gène de nombre de rayons de vision (la précision visuelle). Bornes
    /// entières en pratique (le gène est arrondi à la compilation du phénotype).
    pub vision_rays_bounds: Bounds,
    /// Facet « mutable ? » par trait (§2) : un gène mute (il dérive et se transmet
    /// avec variation), ou reste figé à la valeur du fondateur. `Default` = tout
    /// mutable sauf les coûts et le taux de mutation.
    pub mutable: Mutability,
    /// Graine RNG : rejouer une *config d'expérience*, pas le bit-à-bit.
    pub seed: u64,
}

/// Bornes `[min, max]` d'un gène. Matérialise, avec la valeur (dans le
/// [`crate::genotype::Genotype`]) et le couplage de coût (dans l'économie), le
/// triplet du §2 : *une caractéristique n'est pas un nombre*.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    pub min: f32,
    pub max: f32,
}

impl Bounds {
    /// Étendue (`max - min`), l'échelle naturelle d'une mutation.
    pub fn span(&self) -> f32 {
        self.max - self.min
    }

    /// Ramène une valeur dans `[min, max]`.
    pub fn clamp(&self, v: f32) -> f32 {
        v.clamp(self.min, self.max)
    }
}

/// Le facet **mutable ?** du §2, par trait : un gène a-t-il le droit de muter
/// (cf. [`crate::genotype::Genotype::mutate`]) — donc de dériver et de transmettre
/// de la variation sélectionnable — ou reste-t-il cloué à la valeur du fondateur ?
///
/// À noter (et c'est volontairement le mot *mutable*, pas *héritable*) : un gène
/// non mutable est **quand même transmis** à l'enfant (copie du parent) ; ce que
/// cette case gouverne, c'est uniquement la **mutation**. Un gène gelé reste donc à
/// la valeur du fondateur pour toute la lignée (variance héritable nulle → rien à
/// sélectionner). Donnée de scénario, modifiable à l'édition. `Default` = tout
/// mutable sauf les coûts et le taux de mutation. Un champ par caractéristique de
/// [`crate::genotype::TRAITS`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Mutability {
    pub max_speed: bool,
    pub agility: bool,
    pub vision_range: bool,
    pub vision_fov: bool,
    pub reproduction_threshold: bool,
    pub offspring_energy: bool,
    pub mutation_rate: bool,
    pub base_metabolism: bool,
    pub move_cost: bool,
    pub vision_rays: bool,
}

impl Default for Mutability {
    fn default() -> Self {
        Self {
            max_speed: true,
            agility: true,
            vision_range: true,
            vision_fov: true,
            reproduction_threshold: true,
            offspring_energy: true,
            // Précision visuelle (nombre de rayons) : mutable — c'est l'objet du
            // gène, et son coût métabolique (cf. `Vision::metabolic_cost`) borne sa
            // dérive.
            vision_rays: true,
            // Non mutables par défaut : taux de mutation (méta-évolution
            // instable) et les coûts (métabolisme, locomotion) qui *sont* la
            // pression de sélection — évolvables, ils se raboteraient à 0.
            mutation_rate: false,
            base_metabolism: false,
            move_cost: false,
        }
    }
}

/// Une entrée de la table d'interactions. Matérialise l'insight du §3 — *manger
/// et attaquer sont le même verbe* : une interaction dirigée où l'acteur réduit
/// la réserve de la cible, à portée. Le seul axe sémantique en v1 est `transfer` :
///
/// - `transfer: true`  → **prédation** : ce qui est retiré à la cible est gagné
///   par l'acteur.
/// - `transfer: false` → **combat** : la réserve est détruite, sans transfert.
///
/// (La distinction énergie/PV attendra qu'un agent porte *plusieurs* réserves ;
/// v1 n'en a qu'une.)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    /// Espèce de l'acteur.
    pub actor: u16,
    /// Espèce de la cible.
    pub target: u16,
    /// Transfert (prédation) ou simple destruction (combat).
    pub transfer: bool,
    /// Quantité de réserve transférée/détruite **par seconde** de temps simulé.
    pub rate: f32,
    /// Portée d'action, en unités monde (distance centre-à-centre).
    pub range: f32,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            tick_hz: 64.0,
            arena_half_extent: 400.0,
            agent_count: 48,
            agent_radius: 8.0,
            max_speed: 140.0,
            vision_rays: 7,
            vision_fov_deg: 120.0,
            vision_range: 160.0,
            species_count: 1,
            agents_per_species: Vec::new(),
            reserve_max: 100.0,
            brain: BrainKind::default(),
            brains_per_species: Vec::new(),
            relations: Vec::new(),
            base_metabolism: 0.0,
            move_cost: 0.0,
            food_count: 0,
            food_radius: 6.0,
            food_energy: 50.0,
            food_regen: 0.0,
            food_species: 1,
            agility: 0.12,
            mutation_rate: 0.0,
            reproduction_threshold: 0.0,
            offspring_energy: 30.0,
            speed_bounds: Bounds {
                min: 40.0,
                max: 260.0,
            },
            agility_bounds: Bounds {
                min: 0.02,
                max: 0.5,
            },
            vision_range_bounds: Bounds {
                min: 40.0,
                max: 300.0,
            },
            vision_fov_bounds: Bounds {
                min: 40.0,
                max: 280.0,
            },
            reproduction_threshold_bounds: Bounds {
                min: 0.0,
                max: 200.0,
            },
            offspring_energy_bounds: Bounds {
                min: 10.0,
                max: 120.0,
            },
            mutation_rate_bounds: Bounds { min: 0.0, max: 0.5 },
            base_metabolism_bounds: Bounds {
                min: 0.0,
                max: 20.0,
            },
            move_cost_bounds: Bounds {
                min: 0.0,
                max: 20.0,
            },
            vision_rays_bounds: Bounds {
                min: 1.0,
                max: 21.0,
            },
            mutable: Mutability::default(),
            seed: 0x00C0_FFEE,
        }
    }
}

impl SimConfig {
    /// Scénario *vide* : l'arène et les archétypes du défaut, mais **aucune entité
    /// au spawn** (ni agent, ni nourriture). La toile de l'éditeur — on place tout
    /// à la main (glisser-déposer), puis on lance. C'est le repli sans-argument du
    /// build fenêtré.
    pub fn empty() -> Self {
        Self {
            agent_count: 0,
            food_count: 0,
            ..Self::default()
        }
    }

    /// Nombre d'espèces d'agents *effectif* : le max entre `species_count` (le knob
    /// uniforme) et la longueur d'[`agents_per_species`](Self::agents_per_species)
    /// (effectifs explicites). HUD, éditeur et palette s'y réfèrent pour rester
    /// cohérents même si un scénario ne renseigne que l'un des deux (au moins 1).
    pub fn species_cardinality(&self) -> u16 {
        self.species_count
            .max(self.agents_per_species.len() as u16)
            .max(1)
    }

    /// Le **type de cerveau** fondateur de l’espèce `species` : l’entrée de
    /// [`brains_per_species`](Self::brains_per_species) si elle existe, sinon le
    /// [`brain`](Self::brain) uniforme (champ vide, ou espèce au-delà de sa
    /// longueur). C’est le résolveur unique du peuplement (item 18a) ; au-delà du
    /// fondateur, le cerveau se transmet par héritage et non par ce champ.
    pub fn brain_of(&self, species: u16) -> BrainKind {
        self.brains_per_species
            .get(species as usize)
            .cloned()
            .unwrap_or_else(|| self.brain.clone())
    }

    /// `true` si l'espèce `actor` peut agir sur l'espèce `target` — une
    /// [`Relation`] l'y autorise. C'est le **filtre de cible** de la primitive
    /// d'interaction (§3 : *manger et attaquer sont le même verbe*), sans
    /// distinction transfert/destruction : ce qui fait d'une entité une *cible*
    /// dans le canal de perception du `Brain::Hunter` (item 16).
    pub fn acts_on(&self, actor: u16, target: u16) -> bool {
        self.relations
            .iter()
            .any(|r| r.actor == actor && r.target == target)
    }

    /// Construit le scénario depuis le 1er argument positionnel (chemin RON), avec
    /// `fallback` quand aucun argument n'est donné.
    ///
    /// - Aucun argument → `fallback`.
    /// - Fichier illisible / invalide → on échoue **bruyamment** (sortie 1).
    ///   Faire tourner silencieusement le mauvais monde est pire que s'arrêter.
    ///
    /// Avec un argument, les deux binaires chargent **exactement le même scénario,
    /// de la même façon** ; ils ne diffèrent que par leur repli sans-argument (cf.
    /// [`SimConfig::from_cli`], peuplé, et [`SimConfig::empty`], vide).
    pub fn from_cli_or(fallback: Self) -> Self {
        match std::env::args().nth(1) {
            None => fallback,
            Some(path) => Self::from_ron_file(&path).unwrap_or_else(|err| {
                eprintln!("teemlab: scénario « {path} » illisible : {err}");
                std::process::exit(1);
            }),
        }
    }

    /// [`from_cli_or`](SimConfig::from_cli_or) avec le scénario par défaut (peuplé)
    /// en repli — le headless, dont le smoke test a besoin d'agents.
    pub fn from_cli() -> Self {
        Self::from_cli_or(Self::default())
    }

    /// Charge et désérialise un scénario depuis un fichier RON.
    pub fn from_ron_file(path: impl AsRef<Path>) -> Result<Self, ScenarioError> {
        let text = std::fs::read_to_string(path)?;
        Ok(Self::from_ron_str(&text)?)
    }

    /// Désérialise un scénario depuis une chaîne RON.
    pub fn from_ron_str(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    /// Sérialise le scénario en RON lisible (export depuis l'éditeur, item 5).
    pub fn to_ron_string(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
    }

    /// Écrit le scénario dans un fichier RON.
    pub fn save_ron_file(&self, path: impl AsRef<Path>) -> Result<(), ScenarioError> {
        let text = self
            .to_ron_string()
            .map_err(|e| ScenarioError::Io(std::io::Error::other(e.to_string())))?;
        std::fs::write(path, text)?;
        Ok(())
    }
}

/// Échec de chargement d'un scénario : I/O ou parsing RON.
#[derive(Debug)]
pub enum ScenarioError {
    /// Le fichier n'a pas pu être lu (absent, droits, …).
    Io(std::io::Error),
    /// Le contenu n'est pas du RON valide pour [`SimConfig`].
    Parse(ron::error::SpannedError),
}

impl std::fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScenarioError::Io(e) => write!(f, "lecture impossible : {e}"),
            ScenarioError::Parse(e) => write!(f, "RON invalide : {e}"),
        }
    }
}

impl std::error::Error for ScenarioError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ScenarioError::Io(e) => Some(e),
            ScenarioError::Parse(e) => Some(e),
        }
    }
}

impl From<std::io::Error> for ScenarioError {
    fn from(e: std::io::Error) -> Self {
        ScenarioError::Io(e)
    }
}

impl From<ron::error::SpannedError> for ScenarioError {
    fn from(e: ron::error::SpannedError) -> Self {
        ScenarioError::Parse(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un scénario complet parse vers exactement ses valeurs.
    #[test]
    fn parses_full_scenario() {
        let cfg = SimConfig::from_ron_str(
            "(tick_hz: 30.0, arena_half_extent: 200.0, agent_count: 12, \
             agent_radius: 4.0, max_speed: 90.0, seed: 7)",
        )
        .expect("RON valide");
        assert_eq!(cfg.tick_hz, 30.0);
        assert_eq!(cfg.agent_count, 12);
        assert_eq!(cfg.seed, 7);
    }

    /// `#[serde(default)]` : les champs omis retombent sur le défaut, donc un
    /// scénario partiel — voire vide — reste valide.
    #[test]
    fn partial_scenario_falls_back_to_default() {
        let cfg = SimConfig::from_ron_str("(agent_count: 100)").expect("RON valide");
        assert_eq!(cfg.agent_count, 100);
        assert_eq!(cfg.max_speed, SimConfig::default().max_speed);

        let empty = SimConfig::from_ron_str("()").expect("RON vide valide");
        assert_eq!(empty, SimConfig::default());
    }

    /// Un littéral hexadécimal RON donne bien la graine attendue.
    #[test]
    fn hex_seed_literal() {
        let cfg = SimConfig::from_ron_str("(seed: 0x00C0FFEE)").expect("RON valide");
        assert_eq!(cfg.seed, 0x00C0_FFEE);
    }

    /// Un champ inconnu est rejeté plutôt qu'ignoré silencieusement
    /// (`deny_unknown_fields`) : une faute de frappe dans un scénario doit se
    /// voir, pas se traduire par un monde subtilement faux.
    #[test]
    fn unknown_field_is_rejected() {
        assert!(SimConfig::from_ron_str("(agent_kount: 9)").is_err());
    }

    /// Aller-retour sérialisation : ce que l'éditeur sauve se relit à l'identique.
    #[test]
    fn ron_roundtrip_is_lossless() {
        let mut cfg = SimConfig {
            agent_count: 17,
            ..SimConfig::default()
        };
        cfg.relations.push(Relation {
            actor: 0,
            target: 1,
            transfer: true,
            rate: 12.0,
            range: 9.0,
        });
        let text = cfg.to_ron_string().expect("sérialisation RON");
        let back = SimConfig::from_ron_str(&text).expect("relecture RON");
        assert_eq!(cfg, back);
    }

    /// Le scénario par défaut versionné dans le dépôt reste synchronisé avec
    /// [`SimConfig::default`] : garde-fou contre la dérive des deux sources.
    #[test]
    fn bundled_default_matches_default() {
        let text = include_str!("../scenarios/default.ron");
        let cfg = SimConfig::from_ron_str(text).expect("scénario par défaut valide");
        assert_eq!(cfg, SimConfig::default());
    }

    /// Le scénario vide versionné (repli sans-argument du fenêtré) reste
    /// synchronisé avec [`SimConfig::empty`] et ne spawne aucune entité.
    #[test]
    fn bundled_empty_matches_empty() {
        let text = include_str!("../scenarios/empty.ron");
        let cfg = SimConfig::from_ron_str(text).expect("scénario vide valide");
        assert_eq!(cfg, SimConfig::empty());
        assert_eq!(cfg.agent_count, 0);
        assert_eq!(cfg.food_count, 0);
    }

    /// La table de relations parse depuis le RON et un champ inconnu y est aussi
    /// rejeté (`deny_unknown_fields` sur `Relation`).
    #[test]
    fn relations_parse_from_ron() {
        let cfg = SimConfig::from_ron_str(
            "(relations: [(actor: 0, target: 1, transfer: true, rate: 40.0, range: 28.0)])",
        )
        .expect("RON valide");
        assert_eq!(cfg.relations.len(), 1);
        assert_eq!(cfg.relations[0].actor, 0);
        assert_eq!(cfg.relations[0].target, 1);
        assert!(cfg.relations[0].transfer);

        assert!(
            SimConfig::from_ron_str(
                "(relations: [(actor: 0, target: 1, transfer: true, rate: 1.0, range: 1.0, oops: 2)])"
            )
            .is_err()
        );
    }

    /// Le champ `brain` parse le type de cerveau **et ses paramètres de variant** ;
    /// absent, il retombe sur l'errance — rétro-compatibilité d'avant l'item 16 (les
    /// scénarios existants n'en parlent pas et restent des mondes d'errance).
    #[test]
    fn brain_kind_parses_and_defaults_to_wander() {
        use crate::brain::BrainKind;
        assert_eq!(
            SimConfig::from_ron_str("(brain: Hunter)").unwrap().brain,
            BrainKind::Hunter
        );
        // Le variant `Wander` porte son propre paramètre (turn_rate), lu depuis le RON.
        assert_eq!(
            SimConfig::from_ron_str("(brain: Wander(turn_rate: 0.4))")
                .unwrap()
                .brain,
            BrainKind::Wander { turn_rate: 0.4 }
        );
        // Absent → errance au taux par défaut.
        assert!(matches!(
            SimConfig::from_ron_str("()").unwrap().brain,
            BrainKind::Wander { .. }
        ));
        assert!(matches!(
            SimConfig::default().brain,
            BrainKind::Wander { .. }
        ));
    }

    /// `acts_on` reflète la table de relations : c'est le filtre de cible du
    /// chasseur (une espèce est « cible » ssi une relation l'autorise comme tel).
    #[test]
    fn acts_on_follows_relations() {
        let cfg = SimConfig::from_ron_str(
            "(relations: [(actor: 0, target: 1, transfer: true, rate: 1.0, range: 1.0)])",
        )
        .unwrap();
        assert!(cfg.acts_on(0, 1));
        assert!(!cfg.acts_on(1, 0), "la relation est dirigée");
        assert!(!cfg.acts_on(0, 2), "espèce non visée");
    }

    /// Le scénario de chasse versionné est valide : cerveau chasseur **et** une
    /// relation qui désigne la nourriture comme cible — sans elle, le canal
    /// « cible » resterait nul et le chasseur ne ferait qu'errer.
    #[test]
    fn bundled_hunt_scenario_uses_hunter_on_a_target() {
        use crate::brain::BrainKind;
        let text = include_str!("../scenarios/chasse.ron");
        let cfg = SimConfig::from_ron_str(text).expect("scénario chasse valide");
        assert_eq!(cfg.brain, BrainKind::Hunter);
        assert!(
            cfg.relations.iter().any(|r| r.target == cfg.food_species),
            "le chasseur a besoin d'une cible désignée (la nourriture)"
        );
    }

    /// Le scénario de prédication versionné reste valide (garde-fou contre une
    /// dérive du format de la table de relations).
    #[test]
    fn bundled_predation_scenario_is_valid() {
        let text = include_str!("../scenarios/predation.ron");
        let cfg = SimConfig::from_ron_str(text).expect("scénario prédation valide");
        assert_eq!(cfg.species_count, 2);
        assert_eq!(cfg.relations.len(), 1);
    }

    /// Le scénario de sélection naturelle versionné reste valide et constitue
    /// bien une économie : métabolisme non nul, nourriture présente, et une
    /// relation qui désigne cette nourriture comme cible.
    #[test]
    fn bundled_selection_scenario_is_an_economy() {
        let text = include_str!("../scenarios/selection.ron");
        let cfg = SimConfig::from_ron_str(text).expect("scénario sélection valide");
        assert!(cfg.base_metabolism > 0.0, "il faut un coût de survie");
        assert!(cfg.food_count > 0, "il faut une source d'énergie");
        assert!(
            cfg.relations
                .iter()
                .any(|r| r.target == cfg.food_species && r.transfer),
            "une relation doit permettre de manger la nourriture"
        );
    }

    /// Les effectifs par espèce parsent depuis le RON, et `species_cardinality`
    /// reflète bien le nombre d'espèces effectif (le max du knob uniforme et de la
    /// longueur des effectifs explicites). Vide → repli sur `species_count`.
    #[test]
    fn agents_per_species_parses_and_drives_cardinality() {
        let cfg = SimConfig::from_ron_str("(species_count: 2, agents_per_species: [10, 60])")
            .expect("RON valide");
        assert_eq!(cfg.agents_per_species, vec![10, 60]);
        assert_eq!(cfg.species_cardinality(), 2);

        // La longueur prime si elle dépasse species_count (scénario qui n'a renseigné
        // que les effectifs).
        let cfg = SimConfig::from_ron_str("(agents_per_species: [4, 4, 4])").expect("RON valide");
        assert_eq!(cfg.species_cardinality(), 3);

        // Vide (défaut) → repli sur species_count, au moins 1.
        assert_eq!(SimConfig::default().agents_per_species, Vec::<usize>::new());
        assert_eq!(SimConfig::default().species_cardinality(), 1);
    }

    /// Le scénario proie-prédateur versionné est une chaîne trophique à trois
    /// niveaux exprimée en pure donnée : pyramide par effectifs (proies ≫
    /// prédateurs), cerveau chasseur, et DEUX relations enchaînées
    /// (prédateur→proie, proie→plante) — la dynamique elle-même est vérifiée par le
    /// driver `tests/predator_prey`.
    #[test]
    fn bundled_predator_prey_is_a_trophic_chain() {
        use crate::brain::BrainKind;
        let text = include_str!("../scenarios/proie_predateur.ron");
        let cfg = SimConfig::from_ron_str(text).expect("scénario proie-prédateur valide");
        // Pyramide : strictement moins de prédateurs (espèce 0) que de proies (1).
        assert_eq!(cfg.agents_per_species.len(), 2);
        assert!(
            cfg.agents_per_species[0] < cfg.agents_per_species[1],
            "une pyramide veut proies ≫ prédateurs"
        );
        assert_eq!(cfg.brain, BrainKind::Hunter);
        // La chaîne : le prédateur mange une espèce qui, elle, mange la nourriture.
        let prey = cfg
            .relations
            .iter()
            .find(|r| r.actor == 0 && r.transfer)
            .expect("le prédateur mange quelqu'un")
            .target;
        assert!(
            cfg.relations
                .iter()
                .any(|r| r.actor == prey && r.target == cfg.food_species && r.transfer),
            "la proie du prédateur doit elle-même brouter la nourriture (3 niveaux)"
        );
    }

    /// Le scénario d'évolution versionné active bien la boucle (reproduction +
    /// mutation) et borne la nourriture (capacité de charge), sinon la
    /// population exploserait.
    #[test]
    fn bundled_evolution_scenario_closes_the_loop() {
        let text = include_str!("../scenarios/evolution.ron");
        let cfg = SimConfig::from_ron_str(text).expect("scénario évolution valide");
        assert!(
            cfg.reproduction_threshold > 0.0,
            "la reproduction doit être active"
        );
        assert!(cfg.mutation_rate > 0.0, "la mutation doit être active");
        assert!(cfg.food_regen > 0.0, "repousse finie → capacité de charge");
        assert!(
            cfg.reproduction_threshold <= cfg.reserve_max,
            "un seuil au-dessus du max serait inatteignable"
        );
    }

    /// Le cerveau par espèce (item 18a) parse depuis le RON, et `brain_of` résout :
    /// l'entrée explicite si elle existe, sinon le `brain` uniforme (champ vide, ou
    /// espèce au-delà de la longueur).
    #[test]
    fn brains_per_species_parses_and_brain_of_resolves() {
        use crate::brain::BrainKind;
        let cfg = SimConfig::from_ron_str("(brains_per_species: [Hunter, Wander(turn_rate: 0.3)])")
            .expect("RON valide");
        assert_eq!(cfg.brain_of(0), BrainKind::Hunter);
        assert_eq!(cfg.brain_of(1), BrainKind::Wander { turn_rate: 0.3 });
        // Espèce au-delà de la longueur → repli sur le `brain` uniforme (défaut Wander).
        assert_eq!(cfg.brain_of(2), cfg.brain);

        // Champ vide (défaut) → toutes les espèces sur le `brain` uniforme.
        let cfg = SimConfig::default();
        assert!(cfg.brains_per_species.is_empty());
        assert_eq!(cfg.brain_of(0), cfg.brain);
        assert_eq!(cfg.brain_of(5), cfg.brain);
    }

    /// Le scénario de cohabitation versionné oppose DEUX cerveaux sur la MÊME
    /// nourriture : effectifs égaux, un cerveau par espèce (chasseur vs errance), et
    /// les deux espèces mangent la nourriture. La dynamique (domination du chasseur)
    /// est vérifiée par le driver `tests/cohabitation`.
    #[test]
    fn bundled_cohabitation_pits_two_brains_on_shared_food() {
        use crate::brain::BrainKind;
        let text = include_str!("../scenarios/cohabitation.ron");
        let cfg = SimConfig::from_ron_str(text).expect("scénario cohabitation valide");
        // Départ équitable : mêmes effectifs, seul le cerveau diffère.
        assert_eq!(cfg.agents_per_species, vec![30, 30]);
        assert_eq!(
            cfg.brain_of(0),
            BrainKind::Hunter,
            "espèce 0 = témoin compétent"
        );
        assert!(
            matches!(cfg.brain_of(1), BrainKind::Wander { .. }),
            "espèce 1 = témoin naïf"
        );
        // Les deux broutent la même nourriture (canal d'énergie commun).
        for s in [0u16, 1] {
            assert!(
                cfg.relations
                    .iter()
                    .any(|r| r.actor == s && r.target == cfg.food_species && r.transfer),
                "l'espèce {s} doit pouvoir manger la nourriture"
            );
        }
    }

    /// Le scénario MLP versionné oppose un cerveau APPRIS (espèce 0) au témoin
    /// d'errance (espèce 1) sur la même nourriture : le cadre de falsification de
    /// l'item 18b. La dynamique (le MLP domine) est vérifiée par le driver `tests/mlp`.
    #[test]
    fn bundled_cerveau_mlp_pits_a_learned_brain_against_wander() {
        use crate::brain::BrainKind;
        let text = include_str!("../scenarios/cerveau_mlp.ron");
        let cfg = SimConfig::from_ron_str(text).expect("scénario MLP valide");
        assert!(
            matches!(cfg.brain_of(0), BrainKind::Mlp { ref hidden } if !hidden.is_empty()),
            "espèce 0 = MLP avec au moins une couche cachée"
        );
        assert!(
            matches!(cfg.brain_of(1), BrainKind::Wander { .. }),
            "espèce 1 = témoin d'errance"
        );
        // Départ équitable et nourriture commune.
        assert_eq!(cfg.agents_per_species.len(), 2);
        assert_eq!(cfg.agents_per_species[0], cfg.agents_per_species[1]);
        for s in [0u16, 1] {
            assert!(
                cfg.relations
                    .iter()
                    .any(|r| r.actor == s && r.target == cfg.food_species && r.transfer),
                "l'espèce {s} doit pouvoir manger la nourriture"
            );
        }
    }
}
