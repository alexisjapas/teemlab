//! Le *scénario* : les paramètres d'une run, chargés depuis un fichier RON.
//!
//! C'est ici que se matérialise l'axe central du projet — **un moteur, des
//! scénarios**. [`SimConfig`] n'est plus un littéral codé en dur mais de la
//! *donnée* : un fichier RON que les deux points d'entrée (fenêtré et headless)
//! chargent à l'identique. Faire varier une expérience = éditer un `.ron`, pas
//! recompiler.
//!
//! La donnée **centrale** est la liste d'[`Archetype`]s : chaque entrée est une
//! *espèce* de premier ordre (corps + décideur), et son **index** dans la liste est
//! son identité ([`crate::components::Species`]) — ce que cible la table de
//! [`Relation`]s. La nourriture est un archétype comme un autre
//! ([`ArchetypeKind::Food`]) : plus de numéro spécial, donc plus de collision.

use crate::brain::BrainKind;
use crate::genotype::Genotype;
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
    /// Les **archétypes** : la donnée centrale du scénario. Chaque entrée est une
    /// *espèce* (nom, couleur, effectif, corps + décideur), et son **index** est son
    /// identité ([`crate::components::Species`]) — ce que cible la table de
    /// [`relations`](Self::relations). La nourriture y est un archétype
    /// ([`ArchetypeKind::Food`]), sans collision de numéros avec les agents. Vide →
    /// monde inerte (rien au spawn).
    pub archetypes: Vec<Archetype>,
    /// Table d'interactions : qui peut agir sur qui (cf. §3, §4). `actor`/`target`
    /// sont des **index d'archétype**. Vide par défaut → aucune interaction (monde
    /// inerte, comme avant l'item 7).
    pub relations: Vec<Relation>,
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
    /// Graine RNG : rejouer une *config d'expérience*, pas le bit-à-bit.
    pub seed: u64,
}

/// Un **archétype** : une espèce de premier ordre. Son index dans
/// [`SimConfig::archetypes`] est son identité ([`crate::components::Species`]).
/// Les propriétés communes (corps, effectif) vivent ici ; ce qui distingue un
/// décideur mobile d'une source sessile est dans [`kind`](Self::kind).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Archetype {
    /// Libellé pour la palette / l'inspecteur.
    pub name: String,
    /// Identité visuelle (sRGB linéaire, `[r, g, b]` dans `[0, 1]`).
    pub color: [f32; 3],
    /// Effectif au spawn (le levier d'une pyramide trophique).
    pub count: usize,
    /// Rayon du corps (et du collider).
    pub radius: f32,
    /// Capacité de réserve (énergie/PV). Pour la nourriture : son énergie pleine.
    pub reserve_max: f32,
    /// Ce qui distingue un agent (décideur mobile) d'une nourriture (source sessile).
    pub kind: ArchetypeKind,
    /// Provenance : le fichier `species/*.ron` d'où cet archétype a été **importé**
    /// (bibliothèque d'espèces). L'import en fait une *copie* (le scénario reste
    /// autonome, §9), mais retient ce lien pour permettre la **resynchronisation** —
    /// recharger la définition à jour depuis le fichier, tout en gardant l'effectif
    /// local. `None` pour un archétype défini directement dans le scénario. Omis du RON
    /// quand absent (`skip_serializing_if`) : les scénarios sans import sont inchangés.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Ce qu'un archétype *est* : un décideur mobile, ou une source sessile. Point
/// d'accroche pour la flore évolutive (Phase 3) — où `Food` gagnera un génotype.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ArchetypeKind {
    /// Décideur mobile : génotype fondateur (corps évolvable), cerveau (auteur de la
    /// décision, §1) et **mutabilité par espèce** (quels gènes ont le droit de muter).
    Agent {
        genotype: Genotype,
        brain: BrainKind,
        mutable: Mutability,
    },
    /// Source de nourriture sessile : `regen` = sources repoussées **par seconde**
    /// (`0` → maintien instantané à `count`). Énergie/rayon viennent des champs
    /// communs de l'archétype (`reserve_max`/`radius`).
    Food { regen: f32 },
}

impl Archetype {
    /// Palette de couleurs par défaut (partagée avec le rendu via les *valeurs*),
    /// pour donner une teinte distincte à un archétype neuf sans dépendre de `visuals`.
    pub const PALETTE: [[f32; 3]; 4] = [
        [0.30, 0.70, 1.00], // bleu
        [1.00, 0.45, 0.35], // corail
        [0.55, 0.90, 0.45], // vert
        [0.95, 0.80, 0.30], // ambre
    ];

    /// Couleur par défaut de l'archétype d'index `i` (cyclique sur la palette).
    pub fn default_color(i: usize) -> [f32; 3] {
        Self::PALETTE[i % Self::PALETTE.len()]
    }

    /// Archétype d'**agent** neuf, à l'index `i` : génotype/cerveau/mutabilité par
    /// défaut, effectif standard, couleur de palette.
    pub fn new_agent(i: usize) -> Self {
        Self {
            name: format!("Espèce {i}"),
            color: Self::default_color(i),
            count: 48,
            radius: 8.0,
            reserve_max: 100.0,
            kind: ArchetypeKind::Agent {
                genotype: Genotype::default(),
                brain: BrainKind::default(),
                mutable: Mutability::default(),
            },
            source: None,
        }
    }

    /// Archétype de **nourriture** neuf, à l'index `i` : sessile, sans repousse.
    pub fn new_food(i: usize) -> Self {
        Self {
            name: "Nourriture".to_string(),
            color: Self::default_color(i),
            count: 0,
            radius: 6.0,
            reserve_max: 50.0,
            kind: ArchetypeKind::Food { regen: 0.0 },
            source: None,
        }
    }

    /// `true` si c'est un agent (décideur mobile).
    pub fn is_agent(&self) -> bool {
        matches!(self.kind, ArchetypeKind::Agent { .. })
    }

    /// `true` si c'est une source de nourriture.
    pub fn is_food(&self) -> bool {
        matches!(self.kind, ArchetypeKind::Food { .. })
    }

    /// Sérialise l'archétype en RON lisible — l'**export** d'une espèce réutilisable
    /// vers la bibliothèque (`species/*.ron`, item 4).
    pub fn to_ron_string(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
    }

    /// Désérialise un archétype (une *espèce*) depuis une chaîne RON.
    pub fn from_ron_str(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    /// Charge une espèce depuis un fichier RON de la bibliothèque.
    pub fn from_ron_file(path: impl AsRef<Path>) -> Result<Self, ScenarioError> {
        let text = std::fs::read_to_string(path)?;
        Ok(Self::from_ron_str(&text)?)
    }

    /// Écrit l'archétype dans un fichier RON (une *espèce* réutilisable).
    pub fn save_ron_file(&self, path: impl AsRef<Path>) -> Result<(), ScenarioError> {
        let text = self
            .to_ron_string()
            .map_err(|e| ScenarioError::Io(std::io::Error::other(e.to_string())))?;
        std::fs::write(path, text)?;
        Ok(())
    }
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

/// Le facet **mutable ?** du §2, par trait, **par espèce** : un gène a-t-il le droit
/// de muter (cf. [`crate::genotype::Genotype::mutate`]) — donc de dériver et de
/// transmettre de la variation sélectionnable — ou reste-t-il cloué à la valeur du
/// fondateur ?
///
/// À noter (et c'est volontairement le mot *mutable*, pas *héritable*) : un gène
/// non mutable est **quand même transmis** à l'enfant (copie du parent) ; ce que
/// cette case gouverne, c'est uniquement la **mutation**. Vit dans
/// [`ArchetypeKind::Agent`], donc une espèce peut figer un gène qu'une autre laisse
/// dériver. `Default` = tout mutable sauf les coûts et le taux de mutation.
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
/// `actor`/`target` sont des **index d'[`Archetype`]**. (La distinction énergie/PV
/// attendra qu'un agent porte *plusieurs* réserves ; v1 n'en a qu'une.)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    /// Index d'archétype de l'acteur.
    pub actor: u16,
    /// Index d'archétype de la cible.
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
            archetypes: vec![Archetype::new_agent(0)],
            relations: Vec::new(),
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
            seed: 0x00C0_FFEE,
        }
    }
}

impl SimConfig {
    /// Scénario *vide* : l'arène et un archétype d'agent du défaut, mais **aucune
    /// entité au spawn** (effectif 0). La toile de l'éditeur — on place tout à la
    /// main (glisser-déposer), puis on lance. C'est le repli sans-argument du build
    /// fenêtré.
    pub fn empty() -> Self {
        let mut agent = Archetype::new_agent(0);
        agent.count = 0;
        Self {
            archetypes: vec![agent],
            ..Self::default()
        }
    }

    /// Nombre d'archétypes (= nombre d'espèces, agents **et** nourriture confondus),
    /// au moins 1. HUD, éditeur et palette s'y réfèrent.
    pub fn species_cardinality(&self) -> u16 {
        (self.archetypes.len() as u16).max(1)
    }

    /// Le **génotype fondateur** de l'archétype `species` (l'« archétype » au sens
    /// génétique). Repli sur le génotype par défaut pour un index hors-liste ou une
    /// nourriture (sans génotype en v1).
    pub fn genotype_of(&self, species: u16) -> Genotype {
        match self.archetypes.get(species as usize).map(|a| &a.kind) {
            Some(ArchetypeKind::Agent { genotype, .. }) => *genotype,
            _ => Genotype::default(),
        }
    }

    /// Le **type de cerveau** fondateur de l'archétype `species` (l'auteur de la
    /// décision, §1). Repli sur l'errance pour un index hors-liste / une nourriture.
    /// Au-delà du fondateur, le cerveau se transmet par héritage à la reproduction
    /// ([`crate::brain::Brain::reproduce`]), sans relire ce champ.
    pub fn brain_of(&self, species: u16) -> BrainKind {
        match self.archetypes.get(species as usize).map(|a| &a.kind) {
            Some(ArchetypeKind::Agent { brain, .. }) => brain.clone(),
            _ => BrainKind::default(),
        }
    }

    /// La **mutabilité** (facet « mutable ? » par gène) de l'archétype `species`.
    /// Repli sur le défaut pour un index hors-liste / une nourriture.
    pub fn mutable_of(&self, species: u16) -> Mutability {
        match self.archetypes.get(species as usize).map(|a| &a.kind) {
            Some(ArchetypeKind::Agent { mutable, .. }) => *mutable,
            _ => Mutability::default(),
        }
    }

    /// La **réserve max** (capacité de corps) de l'archétype `species`. Le **% de
    /// remplissage** ([`crate::components::Reserve::fraction`]) reste normalisé
    /// `[0, 1]` quelle que soit la capacité, donc comparable entre espèces.
    pub fn reserve_max_of(&self, species: u16) -> f32 {
        self.archetypes
            .get(species as usize)
            .map(|a| a.reserve_max)
            .unwrap_or(100.0)
    }

    /// Le **rayon du corps** de l'archétype `species` (corps + collider).
    pub fn agent_radius_of(&self, species: u16) -> f32 {
        self.archetypes
            .get(species as usize)
            .map(|a| a.radius)
            .unwrap_or(8.0)
    }

    /// La **couleur** de l'archétype `species` (repli sur la palette par index).
    pub fn color_of(&self, species: u16) -> [f32; 3] {
        self.archetypes
            .get(species as usize)
            .map(|a| a.color)
            .unwrap_or_else(|| Archetype::default_color(species as usize))
    }

    /// `true` si l'archétype `actor` peut agir sur l'archétype `target` — une
    /// [`Relation`] l'y autorise. C'est le **filtre de cible** de la primitive
    /// d'interaction (§3 : *manger et attaquer sont le même verbe*) : ce qui fait
    /// d'une entité une *cible* dans le canal de perception du `Brain::Hunter` (item 16).
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
    use crate::brain::BrainKind;

    /// Index du premier archétype d'agent (helper de test).
    fn first_agent(cfg: &SimConfig) -> &Archetype {
        cfg.archetypes
            .iter()
            .find(|a| a.is_agent())
            .expect("un agent")
    }

    /// Un scénario partiel parse, et les champs omis retombent sur le défaut.
    #[test]
    fn partial_scenario_falls_back_to_default() {
        let cfg = SimConfig::from_ron_str("(tick_hz: 30.0, arena_half_extent: 200.0, seed: 7)")
            .expect("RON valide");
        assert_eq!(cfg.tick_hz, 30.0);
        assert_eq!(cfg.arena_half_extent, 200.0);
        assert_eq!(cfg.seed, 7);
        assert_eq!(cfg.archetypes, SimConfig::default().archetypes);

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
    /// (`deny_unknown_fields`) : une faute de frappe dans un scénario doit se voir.
    #[test]
    fn unknown_field_is_rejected() {
        assert!(SimConfig::from_ron_str("(seedz: 9)").is_err());
    }

    /// Aller-retour sérialisation : ce que l'éditeur sauve se relit à l'identique.
    #[test]
    fn ron_roundtrip_is_lossless() {
        let mut cfg = SimConfig::default();
        cfg.archetypes.push(Archetype::new_food(1));
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

    /// Une espèce (archétype) fait l'aller-retour RON sans perte, `source` compris —
    /// l'export/import de la bibliothèque (item 4).
    #[test]
    fn archetype_ron_roundtrip_is_lossless() {
        let mut a = Archetype::new_agent(0);
        a.source = Some("species/loup.ron".into());
        let back =
            Archetype::from_ron_str(&a.to_ron_string().expect("sérialisation")).expect("relecture");
        assert_eq!(a, back);
    }

    /// L'espèce versionnée de la bibliothèque parse en un archétype d'agent chasseur,
    /// sans `source` (le fichier *est* la source). Garde-fou : son schéma suit `Archetype`.
    #[test]
    fn bundled_species_parses_as_a_hunter_agent() {
        let text = include_str!("../species/chasseur.ron");
        let a = Archetype::from_ron_str(text).expect("espèce chasseur valide");
        assert!(a.is_agent());
        let ArchetypeKind::Agent { brain, .. } = &a.kind else {
            unreachable!()
        };
        assert_eq!(*brain, BrainKind::Hunter);
        assert_eq!(a.source, None, "un fichier d'espèce n'a pas de source");
    }

    /// Une espèce sans `source` n'émet pas le champ (skip_serializing_if) et se relit en
    /// `None` : les archétypes de scénario non importés restent inchangés (pas de migration).
    #[test]
    fn archetype_without_source_omits_the_field() {
        let a = Archetype::new_food(1);
        assert_eq!(a.source, None);
        let text = a.to_ron_string().expect("sérialisation");
        assert!(
            !text.contains("source"),
            "le champ source doit être omis quand None :\n{text}"
        );
        assert_eq!(Archetype::from_ron_str(&text).expect("relecture"), a);
    }

    /// Le scénario par défaut versionné reste synchronisé avec [`SimConfig::default`].
    #[test]
    fn bundled_default_matches_default() {
        let text = include_str!("../scenarios/default.ron");
        let cfg = SimConfig::from_ron_str(text).expect("scénario par défaut valide");
        assert_eq!(cfg, SimConfig::default());
    }

    /// Le scénario vide versionné reste synchronisé avec [`SimConfig::empty`] et ne
    /// spawne aucune entité.
    #[test]
    fn bundled_empty_matches_empty() {
        let text = include_str!("../scenarios/empty.ron");
        let cfg = SimConfig::from_ron_str(text).expect("scénario vide valide");
        assert_eq!(cfg, SimConfig::empty());
        assert!(cfg.archetypes.iter().all(|a| a.count == 0));
    }

    /// La table de relations parse, et un champ inconnu y est rejeté.
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

    /// `acts_on` reflète la table de relations (le filtre de cible, dirigé).
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

    /// Les résolveurs par archétype lisent l'entrée d'index, avec repli hors-liste.
    #[test]
    fn resolvers_read_archetype_by_index() {
        let mut cfg = SimConfig::default();
        // Espèce 0 : agent. Espèce 1 : nourriture (énergie 50, repousse 0 par défaut).
        cfg.archetypes.push(Archetype::new_food(1));
        if let ArchetypeKind::Agent { brain, .. } = &mut cfg.archetypes[0].kind {
            *brain = BrainKind::Hunter;
        }
        cfg.archetypes[0].reserve_max = 120.0;
        cfg.archetypes[0].radius = 10.0;
        assert_eq!(cfg.brain_of(0), BrainKind::Hunter);
        assert_eq!(cfg.reserve_max_of(0), 120.0);
        assert_eq!(cfg.agent_radius_of(0), 10.0);
        assert!(cfg.archetypes[1].is_food());
        // Index hors-liste → replis.
        assert_eq!(cfg.brain_of(9), BrainKind::default());
        assert_eq!(cfg.reserve_max_of(9), 100.0);
    }

    /// Le scénario de chasse : un agent chasseur **et** une relation qui désigne la
    /// nourriture (autre archétype) comme cible — sinon le canal « cible » reste nul.
    #[test]
    fn bundled_hunt_scenario_uses_hunter_on_a_target() {
        let text = include_str!("../scenarios/chasse.ron");
        let cfg = SimConfig::from_ron_str(text).expect("scénario chasse valide");
        assert_eq!(cfg.brain_of(0), BrainKind::Hunter);
        let food = cfg
            .archetypes
            .iter()
            .position(|a| a.is_food())
            .expect("une nourriture") as u16;
        assert!(
            cfg.relations.iter().any(|r| r.target == food),
            "le chasseur a besoin d'une cible désignée (la nourriture)"
        );
    }

    /// Le scénario proie-prédateur : chaîne trophique à trois niveaux en pure donnée
    /// (pyramide par effectifs, cerveau chasseur, deux relations enchaînées).
    #[test]
    fn bundled_predator_prey_is_a_trophic_chain() {
        let text = include_str!("../scenarios/proie_predateur.ron");
        let cfg = SimConfig::from_ron_str(text).expect("scénario proie-prédateur valide");
        // Pyramide : strictement moins de prédateurs (espèce 0) que de proies (1).
        assert!(
            cfg.archetypes[0].count < cfg.archetypes[1].count,
            "une pyramide veut proies ≫ prédateurs"
        );
        assert_eq!(cfg.brain_of(0), BrainKind::Hunter);
        // Le prédateur mange une espèce qui, elle, mange une nourriture.
        let prey = cfg
            .relations
            .iter()
            .find(|r| r.actor == 0 && r.transfer)
            .expect("le prédateur mange quelqu'un")
            .target;
        let foods: Vec<u16> = cfg
            .archetypes
            .iter()
            .enumerate()
            .filter(|(_, a)| a.is_food())
            .map(|(i, _)| i as u16)
            .collect();
        assert!(
            cfg.relations
                .iter()
                .any(|r| r.actor == prey && foods.contains(&r.target) && r.transfer),
            "la proie du prédateur doit elle-même brouter une nourriture (3 niveaux)"
        );
    }

    /// Le scénario d'évolution active la boucle (reproduction + mutation) et borne la
    /// nourriture (repousse finie → capacité de charge).
    #[test]
    fn bundled_evolution_scenario_closes_the_loop() {
        let text = include_str!("../scenarios/evolution.ron");
        let cfg = SimConfig::from_ron_str(text).expect("scénario évolution valide");
        let agent = first_agent(&cfg);
        let ArchetypeKind::Agent { genotype, .. } = &agent.kind else {
            unreachable!()
        };
        assert!(
            genotype.reproduction_threshold > 0.0,
            "la reproduction doit être active"
        );
        assert!(genotype.mutation_rate > 0.0, "la mutation doit être active");
        assert!(
            cfg.archetypes
                .iter()
                .any(|a| matches!(a.kind, ArchetypeKind::Food { regen } if regen > 0.0)),
            "repousse finie → capacité de charge"
        );
        assert!(
            genotype.reproduction_threshold <= agent.reserve_max,
            "un seuil au-dessus du max serait inatteignable"
        );
    }

    /// Le scénario de cohabitation oppose DEUX cerveaux (chasseur vs errance) à
    /// effectifs égaux sur la même nourriture (driver `tests/cohabitation`).
    #[test]
    fn bundled_cohabitation_pits_two_brains_on_shared_food() {
        let text = include_str!("../scenarios/cohabitation.ron");
        let cfg = SimConfig::from_ron_str(text).expect("scénario cohabitation valide");
        assert_eq!(cfg.archetypes[0].count, cfg.archetypes[1].count);
        assert_eq!(
            cfg.brain_of(0),
            BrainKind::Hunter,
            "espèce 0 = témoin compétent"
        );
        assert!(
            matches!(cfg.brain_of(1), BrainKind::Wander { .. }),
            "espèce 1 = témoin naïf"
        );
        let food = cfg
            .archetypes
            .iter()
            .position(|a| a.is_food())
            .expect("une nourriture") as u16;
        for s in [0u16, 1] {
            assert!(
                cfg.relations
                    .iter()
                    .any(|r| r.actor == s && r.target == food && r.transfer),
                "l'espèce {s} doit pouvoir manger la nourriture"
            );
        }
    }

    /// Le scénario MLP oppose un cerveau APPRIS (espèce 0) au témoin d'errance
    /// (espèce 1) sur la même nourriture (driver `tests/mlp`).
    #[test]
    fn bundled_cerveau_mlp_pits_a_learned_brain_against_wander() {
        let text = include_str!("../scenarios/cerveau_mlp.ron");
        let cfg = SimConfig::from_ron_str(text).expect("scénario MLP valide");
        assert!(
            matches!(cfg.brain_of(0), BrainKind::Mlp { ref hidden } if !hidden.is_empty()),
            "espèce 0 = cerveau appris (MLP)"
        );
        assert!(
            matches!(cfg.brain_of(1), BrainKind::Wander { .. }),
            "espèce 1 = témoin d'errance"
        );
    }
}
