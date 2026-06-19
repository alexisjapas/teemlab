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
    /// Perceptron multicouche fait maison (item 18b), le **cerveau appris** : ses
    /// poids muent à la reproduction (neuroévolution). C'est ce que le chasseur et
    /// l'errance servaient à jauger (§4) — le seul variant dont la décision n'est
    /// pas écrite à la main mais découverte par la sélection.
    Mlp(MlpBrain),
}

impl Brain {
    /// Le contrat. `match` exhaustif → ajout d'un variant = erreur de compile
    /// ici, exactement ce qu'on veut.
    pub fn think(&mut self, perception: &Perception) -> Action {
        match self {
            Brain::Wander(b) => b.think(perception),
            Brain::Hunter(b) => b.think(perception),
            Brain::Mlp(b) => b.think(perception),
        }
    }

    /// Libellé court du type de cerveau, pour l'inspecteur (item 12).
    pub fn name(&self) -> &'static str {
        match self {
            Brain::Wander(_) => "Errance",
            Brain::Hunter(_) => "Chasseur",
            Brain::Mlp(_) => "MLP",
        }
    }

    /// Cerveau d'un enfant à partir de **celui du parent** (et non d'un
    /// [`BrainKind`] global) : la couture par laquelle un comportement *appris* se
    /// transmet (item 18a). C'est ici que vit la **neuroévolution** (item 18b) :
    ///
    /// - `Wander` hérite le `turn_rate` du parent (paramètre d'archétype, non mué),
    ///   avec un état RNG **frais** (`seed`/`heading`) pour décorréler la lignée ;
    /// - `Hunter`, déterministe et sans état, est simplement cloné ;
    /// - `Mlp` hérite la **topologie cachée** du parent, **adapte sa couche d'entrée**
    ///   à `n_inputs` (la précision visuelle de l'enfant peut différer de celle du
    ///   parent, gène `vision_rays`) et **mute ses poids** par perturbation gaussienne
    ///   d'écart `rate · WEIGHT_STEP` (cf. [`MlpBrain::reproduced`]).
    ///
    /// `n_inputs` (= `2 × rayons` de l'enfant) ne sert qu'au MLP ; Wander et Hunter
    /// l'ignorent. `seed`/`heading` n'alimentent que les cerveaux à état (l'errance) ;
    /// `rng`/`rate` la mutation/adaptation du MLP. Wander et Hunter **ne tirent pas**
    /// dans `rng` → le flux RNG reste **identique** aux scénarios non-MLP, `rate`
    /// venant du génotype (`mutation_rate`, le gène par lignée, §2).
    pub fn reproduce(
        &self,
        seed: u64,
        heading: f32,
        rng: &mut Rng,
        rate: f32,
        n_inputs: usize,
    ) -> Brain {
        match self {
            Brain::Wander(w) => Brain::Wander(WanderBrain::new(seed, heading, w.turn_rate)),
            Brain::Hunter(_) => Brain::Hunter(HunterBrain),
            Brain::Mlp(m) => Brain::Mlp(m.reproduced(rng, rate, n_inputs)),
        }
    }
}

/// Le **type** de cerveau — le choix de l'auteur de la décision (§1), donnée de
/// scénario. Sépare *quel* cerveau (et ses **paramètres d'archétype**, propres à
/// chaque variant — `turn_rate` pour l'errance, aucun pour le chasseur) de son
/// *état vivant* : un `BrainKind` (RON : `Wander(turn_rate: …)` / `Hunter`) se
/// compile en un [`Brain`] frais au spawn, comme un génotype en phénotype (§2).
/// Édité par le sélecteur de cerveau de l'éditeur (item 15) ; chaque variant y
/// expose ses propres paramètres. Substitution par scénario (§4) **et** par espèce
/// (item 18a) faites.
///
/// Plus `Copy` depuis l'item 18b : le MLP porte sa topologie en `Vec`. On le
/// `clone()` donc explicitement (peu fréquent : spawn, repli de `brain_of`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BrainKind {
    /// [`Brain::Wander`] — errance. `turn_rate` : amplitude de la dérive de cap par
    /// tick (le paramètre propre à ce variant). Défaut rétro-compatible (item 16).
    Wander { turn_rate: f32 },
    /// [`Brain::Hunter`] — réflexe déterministe, sans paramètre.
    Hunter,
    /// [`Brain::Mlp`] — perceptron multicouche évolué (item 18b). `hidden` : la
    /// largeur de chaque **couche cachée** (donnée de designer, éditable). L'entrée
    /// (canaux de perception) et la sortie (2) sont fixées par le contrat → seule la
    /// topologie cachée est libre (la topologie variable/NEAT reste repoussée, §2).
    Mlp { hidden: Vec<usize> },
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
    /// Compile le choix en un cerveau frais. `seed` graine les cerveaux à état
    /// (l'errance ; les poids initiaux *aléatoires* du MLP) ; `heading` l'errance ;
    /// `n_inputs` (= nombre de canaux de perception, `2 × vision_rays`) dimensionne la
    /// couche d'entrée du MLP. Le chasseur ignore tout, l'errance ignore `n_inputs`.
    pub fn build(&self, seed: u64, heading: f32, n_inputs: usize) -> Brain {
        match self {
            BrainKind::Wander { turn_rate } => {
                Brain::Wander(WanderBrain::new(seed, heading, *turn_rate))
            }
            BrainKind::Hunter => Brain::Hunter(HunterBrain),
            BrainKind::Mlp { hidden } => Brain::Mlp(MlpBrain::random(seed, n_inputs, hidden)),
        }
    }

    /// Libellé court du type, pour le sélecteur d'éditeur (item 15).
    pub fn name(&self) -> &'static str {
        match self {
            BrainKind::Wander { .. } => "Errance",
            BrainKind::Hunter => "Chasseur",
            BrainKind::Mlp { .. } => "Réseau (MLP)",
        }
    }

    /// Description *fonctionnelle* du cerveau — comment il décide, pas seulement son
    /// nom — affichée par le sélecteur d'éditeur. Contrepartie **hétérogène** de
    /// [`name`](Self::name) : le `match` exhaustif force tout futur variant à se
    /// décrire.
    pub fn description(&self) -> &'static str {
        match self {
            BrainKind::Wander { .. } => {
                "Dérive de cap aléatoire à chaque tick : ignore la perception, \
                 fourrage au hasard. Le groupe témoin naïf."
            }
            BrainKind::Hunter => {
                "Champ de pilotage : attiré vers la cible visible la plus proche, \
                 ET repoussé par toute menace (une espèce qui peut l'attaquer) — \
                 table de relations. Contourne murs et congénères sans les fuir ; \
                 sans mémoire, hors de portée il explore. Le groupe témoin compétent."
            }
            BrainKind::Mlp { .. } => {
                "Perceptron multicouche évolué : décide en lisant ses canaux de \
                 vision/cible, et APPREND par neuroévolution (mutation gaussienne des \
                 poids à la reproduction). Couches cachées éditables ; entrée et sortie \
                 fixées par le contrat."
            }
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

/// Chasseur **déterministe** (item 16, étendu par la *fuite active*) : pas d'état,
/// pas de RNG — une même perception donne toujours la même action.
///
/// **Deux modes** dans un même champ de pilotage par rayons, sélectionnés par
/// *subsomption* (§4 — un réflexe de survie court-circuite la couche fourrage) :
///
/// 1. **Fourrage** (item 16, inchangé), tant qu'aucune menace n'est *proche* : chaque
///    rayon pousse vers sa direction d'un poids `ATTRACTION · cible + (1 − obstacle)`.
///    Une cible attire (`ATTRACTION > 1`, plus que l'espace dégagé) ; un obstacle
///    neutre (mur, congénère) ne pousse pas vers lui (poids ≈ 0) → on le contourne ;
///    en terrain vide, l'éventail symétrique se résout vers l'avant (balayage droit).
/// 2. **Fuite**, dès qu'une menace dépasse [`FLEE_THRESHOLD`] en proximité : la survie
///    prime, le fourrage est *suspendu*. Chaque rayon pousse à l'**opposé** d'un poids
///    `RÉPULSION · menace + obstacle` → on s'éloigne des menaces (pondérées fort) ET
///    des obstacles (murs), sans plus se laisser attirer par une cible. Le seuil évite
///    deux écueils : une répulsion simplement *ajoutée* au fourrage ne renverse jamais
///    l'éventail-avant pour une menace lointaine (elle ne pèse qu'un rayon contre tout
///    le champ dégagé), et fuir *toute* menace visible ferait mourir de faim une proie
///    qu'un prédateur survole de loin. On ne fuit donc que ce qui est assez **proche**
///    pour être dangereux.
///
/// Ainsi le **même** cerveau partagé, selon la table de relations (§3) lue *par
/// l'espèce qui perçoit*, fait d'une proie un fourrageur **qui détale** quand son
/// prédateur approche (canal `target` vers les plantes, canal `threat` depuis le
/// prédateur) et d'un prédateur de sommet un pur chasseur (aucune menace → mode
/// fourrage seul → le comportement de l'item 16, inchangé) — le pendant exact, côté
/// fuite, de l'insight de l'item 17. C'est le **groupe témoin compétent** (§4) : même
/// dépense d'énergie qu'un errant, mais il *trouve* sa nourriture et *évite* ses
/// prédateurs. Il ne sait toutefois pas mémoriser : hors de portée, il ne fait
/// qu'explorer (un MLP, lui, pourra apprendre mieux — c'est l'enjeu).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HunterBrain;

impl HunterBrain {
    /// Sur-pondération d'une cible face à l'espace dégagé (`> 1` pour qu'un rayon
    /// pointant une cible l'emporte sur un rayon simplement libre).
    const ATTRACTION: f32 = 2.5;
    /// Sur-pondération d'une menace en **fuite** (répulsion). `> ATTRACTION` : une fois
    /// la fuite déclenchée, l'éloignement domine nettement l'évitement d'obstacle.
    const REPULSION: f32 = 3.0;
    /// Proximité de menace (dans `[0, 1]`) au-delà de laquelle la fuite court-circuite
    /// le fourrage (subsomption). En deçà — prédateur encore lointain — le fourrage
    /// continue ; au-delà — prédateur assez proche pour être dangereux — la proie
    /// détale. `0.35` ≈ « le prédateur est entré dans le tiers proche de ma vision ».
    const FLEE_THRESHOLD: f32 = 0.35;

    fn think(&self, perception: &Perception) -> Action {
        // Subsomption (§4) : une menace assez PROCHE bascule en fuite, qui *suspend* le
        // fourrage. Un prédateur lointain (sous le seuil) n'interrompt rien → le mode
        // fourrage de l'item 16 reste strictement intact pour les scénarios sans menace.
        let fleeing = perception.threat.iter().any(|&t| t > Self::FLEE_THRESHOLD);
        let mut steer = Vec2::ZERO;
        for i in 0..perception.ray_dirs.len() {
            let weight = if fleeing {
                // S'éloigner de tout : menaces (× RÉPULSION) ET obstacles (murs,
                // congénères) — poids négatif, sans attraction (on ne fourrage pas en
                // fuyant).
                -(Self::REPULSION * perception.threat[i] + perception.vision[i])
            } else {
                // Champ de fourrage (item 16) : attraction des cibles + espace dégagé.
                Self::ATTRACTION * perception.target[i] + (1.0 - perception.vision[i])
            };
            steer += perception.ray_dirs[i] * weight;
        }
        let dir = steer.normalize_or_zero();
        // Encerclé (tout occlus) ou aveugle (zéro rayon) : on garde le cap.
        let dir = if dir == Vec2::ZERO {
            perception.heading
        } else {
            dir
        };
        Action { dir, throttle: 1.0 }
    }
}

/// Une couche dense : `out × in` poids (row-major, `weights[o*inputs + i]`) + `out`
/// biais. Pré-activation du neurone de sortie `o` : `bias[o] + Σ_i w[o,i]·in[i]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Layer {
    /// Fan-in (taille du vecteur d'entrée attendu).
    inputs: usize,
    /// Poids aplatis, `outputs × inputs` en row-major.
    weights: Vec<f32>,
    /// Biais, un par neurone de sortie (sa longueur = nombre de sorties).
    biases: Vec<f32>,
}

impl Layer {
    fn outputs(&self) -> usize {
        self.biases.len()
    }

    /// Poids initiaux aléatoires, façon Xavier (écart `1/√fan_in`) pour éviter la
    /// saturation de `tanh` dès le départ ; biais nuls.
    fn random(rng: &mut Rng, inputs: usize, outputs: usize) -> Self {
        let scale = 1.0 / (inputs.max(1) as f32).sqrt();
        let weights = (0..inputs * outputs)
            .map(|_| rng.next_gaussian() * scale)
            .collect();
        Self {
            inputs,
            weights,
            biases: vec![0.0; outputs],
        }
    }

    /// Propagation `tanh(bias + W·in)` ; `input.len()` doit valoir `self.inputs`.
    fn forward(&self, input: &[f32]) -> Vec<f32> {
        (0..self.outputs())
            .map(|o| {
                let row = &self.weights[o * self.inputs..(o + 1) * self.inputs];
                let sum = self.biases[o] + row.iter().zip(input).map(|(w, x)| w * x).sum::<f32>();
                sum.tanh()
            })
            .collect()
    }

    /// Couche enfant : chaque poids et biais perturbé d'un bruit gaussien d'écart
    /// `std` (la neuroévolution, mutation-seule).
    fn mutated(&self, rng: &mut Rng, std: f32) -> Self {
        Self {
            inputs: self.inputs,
            weights: self
                .weights
                .iter()
                .map(|w| w + rng.next_gaussian() * std)
                .collect(),
            biases: self
                .biases
                .iter()
                .map(|b| b + rng.next_gaussian() * std)
                .collect(),
        }
    }
}

/// Perceptron multicouche fait maison (item 18b) — le cerveau **appris**. Même
/// contrat `Perception → Action` que tout cerveau (§2), mais sa décision n'est pas
/// écrite à la main : elle émerge des poids, que la sélection façonne par mutation à
/// la reproduction ([`MlpBrain::mutated`]). Fait maison à dessein : les libs ML
/// visent le gros réseau GPU, l'inverse du besoin (§5).
///
/// **Entrée** : les canaux normalisés `vision` puis `target` concaténés (il ignore la
/// géométrie `ray_dirs`, cf. `components.rs`). Le canal `threat` (fuite active) existe
/// dans [`Perception`] mais n'est **pas encore** câblé ici : seul le chasseur
/// déterministe le consomme pour l'instant — on valide la fuite sur le témoin avant de
/// la confier à l'appris, exactement comme `target` (introduit sur le chasseur à
/// l'item 16, puis consommé par le MLP à l'item 18b). Le brancher (entrée passant à
/// `3 × vision_rays`) est l'étape suivante. **Sortie** : 2 neurones lus comme un
/// vecteur de pilotage *en repère du corps*, tourné vers le monde par le cap →
/// orientation-équivariant (le réseau n'a pas à apprendre l'orientation absolue,
/// comme le chasseur lit « rayon i » relativement au cap).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MlpBrain {
    /// Couches denses, entrée→sortie. Topologie cachée figée à la construction
    /// (héritée telle quelle) ; la couche d'entrée peut, elle, être redimensionnée à
    /// la reproduction quand la précision visuelle de l'enfant change (cf.
    /// [`MlpBrain::reproduced`]) ; les poids muent. **Seul état** du cerveau : son égalité
    /// et sa sérialisation ne portent que la topologie + les poids (pas d'activations
    /// transitoires — celles-ci se recalculent à la demande, cf. [`MlpBrain::forward_activations`]).
    layers: Vec<Layer>,
}

impl MlpBrain {
    /// Nombre de neurones de sortie : le vecteur de pilotage égocentrique (x, y).
    pub const OUTPUTS: usize = 2;
    /// Échelle d'un pas de mutation de poids (multiplié par `mutation_rate`).
    const WEIGHT_STEP: f32 = 0.6;

    /// Taille de la couche d'entrée pour `vision_rays` rayons : les canaux `vision`
    /// ET `target`, d'où `2 × vision_rays`.
    pub fn input_size(vision_rays: usize) -> usize {
        2 * vision_rays
    }

    /// Réseau aux poids **aléatoires** : dims = `[n_inputs] ++ hidden ++ [OUTPUTS]`.
    /// Les fondateurs d'une espèce MLP partent ainsi de cerveaux aléatoires — c'est
    /// l'évolution qui doit *découvrir* le fourrage (tout l'enjeu de l'item 18b).
    pub fn random(seed: u64, n_inputs: usize, hidden: &[usize]) -> Self {
        let mut rng = Rng::new(seed);
        let mut dims = Vec::with_capacity(hidden.len() + 2);
        dims.push(n_inputs);
        dims.extend_from_slice(hidden);
        dims.push(Self::OUTPUTS);
        let layers = dims
            .windows(2)
            .map(|w| Layer::random(&mut rng, w[0], w[1]))
            .collect();
        Self { layers }
    }

    /// Enfant : **même topologie**, poids perturbés (neuroévolution). `rate` =
    /// `mutation_rate` du génotype (le gène par lignée, §2).
    pub fn mutated(&self, rng: &mut Rng, rate: f32) -> Self {
        let std = rate * Self::WEIGHT_STEP;
        Self {
            layers: self.layers.iter().map(|l| l.mutated(rng, std)).collect(),
        }
    }

    /// Enfant à la reproduction : comme [`MlpBrain::mutated`] (topologie cachée
    /// héritée, poids mutés), **mais** la couche d'entrée est d'abord redimensionnée
    /// à `n_inputs` si la précision visuelle de l'enfant diffère de celle du parent
    /// (gène `vision_rays`, item 3). C'est le premier pas vers une topologie variable.
    ///
    /// Quand `n_inputs` est inchangé, aucun tirage de redimensionnement n'a lieu et
    /// le résultat coïncide bit à bit avec `mutated` (flux RNG des scénarios à
    /// précision fixe préservé).
    pub fn reproduced(&self, rng: &mut Rng, rate: f32, n_inputs: usize) -> Self {
        let std = rate * Self::WEIGHT_STEP;
        let layers = self
            .layers
            .iter()
            .enumerate()
            .map(|(idx, layer)| {
                // Seule la première couche voit le vecteur de perception : c'est sa
                // seule dont le fan-in dépend du nombre de rayons.
                let adapted = if idx == 0 && layer.inputs != n_inputs {
                    Self::resize_input_fan(layer, rng, n_inputs)
                } else {
                    layer.clone()
                };
                adapted.mutated(rng, std)
            })
            .collect();
        Self { layers }
    }

    /// Redimensionne le fan-in de la couche d'entrée à `n_inputs`, **en respectant
    /// les deux blocs** du vecteur de perception (`vision` puis `target`, chacun de
    /// `rayons` canaux — cf. [`MlpBrain::input_vector`]). Chaque bloc est tronqué (si
    /// l'enfant voit moins fin) ou complété de poids neufs façon Xavier (s'il voit
    /// plus fin), de sorte que les poids conservés restent **alignés sur le bon
    /// canal**. Les biais (par neurone de sortie) sont inchangés.
    fn resize_input_fan(layer: &Layer, rng: &mut Rng, n_inputs: usize) -> Layer {
        let outputs = layer.outputs();
        let old_in = layer.inputs;
        let old_rays = old_in / 2; // entrée = 2 × rayons (vision ++ cible)
        let new_rays = n_inputs / 2;
        let scale = 1.0 / (n_inputs.max(1) as f32).sqrt();
        let mut weights = Vec::with_capacity(n_inputs * outputs);
        for o in 0..outputs {
            let row = &layer.weights[o * old_in..(o + 1) * old_in];
            // Bloc vision (offset 0) puis bloc cible (offset `old_rays`).
            for block_start in [0usize, old_rays] {
                for r in 0..new_rays {
                    weights.push(if r < old_rays {
                        row[block_start + r]
                    } else {
                        rng.next_gaussian() * scale
                    });
                }
            }
        }
        Layer {
            inputs: n_inputs,
            weights,
            biases: layer.biases.clone(),
        }
    }

    /// Vecteur d'entrée : `vision` puis `target` (les mêmes canaux que l'inspecteur
    /// affiche, dans le même ordre).
    fn input_vector(perception: &Perception) -> Vec<f32> {
        perception
            .vision
            .iter()
            .chain(perception.target.iter())
            .copied()
            .collect()
    }

    fn think(&self, perception: &Perception) -> Action {
        let mut signal = Self::input_vector(perception);
        for layer in &self.layers {
            // Robuste à une perception de mauvaise taille (forme changée entre runs) :
            // si le fan-in ne colle pas, on garde le cap (réseau muet ce tick).
            if signal.len() != layer.inputs {
                return Action {
                    dir: perception.heading,
                    throttle: 0.0,
                };
            }
            signal = layer.forward(&signal);
        }
        // 2 sorties = vecteur de pilotage en repère du corps, tourné vers le monde
        // par le cap (le +X du corps pointe vers `heading`).
        let body = Vec2::new(signal[0], signal[1]);
        let world = perception.heading.rotate(body);
        let dir = world.normalize_or_zero();
        let dir = if dir == Vec2::ZERO {
            perception.heading
        } else {
            dir
        };
        Action {
            dir,
            throttle: body.length().min(1.0),
        }
    }

    /// Rejoue la propagation pour exposer les activations couche par couche (entrée
    /// comprise) : `[input, h0, …, sortie]`. Pour la **visualisation** de l'inspecteur
    /// (item 18b-viz), calculée **à la demande** sur le seul agent inspecté — le
    /// `think` du cœur de sim ne porte donc plus ce coût (clone par couche × agent ×
    /// tick, inutile en headless/`record`). Déterministe : mêmes poids + même
    /// perception ⇒ mêmes activations que le dernier `think`. S'arrête (vecteur
    /// tronqué) si la perception n'a pas le bon fan-in — l'inspecteur colore alors les
    /// nœuds restants en neutre.
    pub fn forward_activations(&self, perception: &Perception) -> Vec<Vec<f32>> {
        let mut signal = Self::input_vector(perception);
        let mut acts = Vec::with_capacity(self.layers.len() + 1);
        acts.push(signal.clone());
        for layer in &self.layers {
            if signal.len() != layer.inputs {
                break;
            }
            signal = layer.forward(&signal);
            acts.push(signal.clone());
        }
        acts
    }

    /// Tailles des couches pour le dessin du graphe (item 18b-viz), **entrée
    /// incluse** : `[n_inputs, h0, …, OUTPUTS]`. Une colonne de nœuds par taille.
    pub fn layer_sizes(&self) -> Vec<usize> {
        let mut sizes = Vec::with_capacity(self.layers.len() + 1);
        if let Some(first) = self.layers.first() {
            sizes.push(first.inputs);
        }
        sizes.extend(self.layers.iter().map(Layer::outputs));
        sizes
    }

    /// Nombre de couches de poids (= nombre d'inter-colonnes d'arêtes à dessiner).
    pub fn weight_layers(&self) -> usize {
        self.layers.len()
    }

    /// Poids de la couche `l` (`outputs × inputs`, row-major) + ses dimensions, pour
    /// dessiner les arêtes pondérées (item 18b-viz). `l < weight_layers()`.
    pub fn layer_weights(&self, l: usize) -> (&[f32], usize, usize) {
        let layer = &self.layers[l];
        (&layer.weights, layer.inputs, layer.outputs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trois rayons : gauche (+Y), avant (+X), droite (-Y) — un éventail symétrique
    /// autour du cap +X, comme `perceive` les produirait. Les trois canaux (obstacle,
    /// cible, menace) sont fournis explicitement.
    fn perception(vision: [f32; 3], target: [f32; 3], threat: [f32; 3]) -> Perception {
        Perception {
            heading: Vec2::X,
            vision: vision.into(),
            target: target.into(),
            threat: threat.into(),
            ray_dirs: vec![Vec2::Y, Vec2::X, Vec2::NEG_Y].into_boxed_slice(),
        }
    }

    /// Deux cibles visibles → le pilotage penche vers la plus proche (-Y, proximité
    /// 0.9) plutôt que vers la lointaine (+Y, 0.3) : l'attraction est graduée par la
    /// proximité.
    #[test]
    fn hunter_favors_the_nearer_target() {
        let p = perception([0.3, 0.0, 0.9], [0.3, 0.0, 0.9], [0.0, 0.0, 0.0]);
        let action = HunterBrain.think(&p);
        assert!(
            action.dir.y < 0.0,
            "penche vers la cible la plus proche (-Y), dir={:?}",
            action.dir
        );
        assert_eq!(action.throttle, 1.0);
    }

    /// Une **cible** d'un côté (+Y) et un **obstacle non-cible** (mur) de l'autre
    /// (-Y), à proximité égale → le chasseur va vers la cible et s'écarte du mur.
    /// C'est le correctif : il ne fuit plus la nourriture comme un obstacle.
    #[test]
    fn hunter_approaches_target_not_plain_obstacle() {
        // +Y : nourriture (vision == target) ; -Y : mur (vision sans target).
        let action = HunterBrain.think(&perception([0.6, 0.0, 0.6], [0.6, 0.0, 0.0], [0.0; 3]));
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
        let action = HunterBrain.think(&perception([0.9, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0; 3]));
        assert!(
            action.dir.y < 0.0,
            "doit s'écarter de l'obstacle en +Y, dir={:?}",
            action.dir
        );
    }

    /// Terrain entièrement dégagé : poussées symétriques → cap maintenu vers l'avant.
    #[test]
    fn hunter_cruises_forward_in_the_open() {
        let action = HunterBrain.think(&perception([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0; 3]));
        assert!(
            action.dir.x > 0.9,
            "doit filer droit devant, dir={:?}",
            action.dir
        );
        assert!(action.dir.y.abs() < 1e-6);
    }

    /// La cible prime sur l'évitement : même cerné d'obstacles, un chasseur qui voit
    /// une cible va dessus (le réflexe de chasse court-circuite l'exploration).
    #[test]
    fn target_takes_priority_over_avoidance() {
        let action = HunterBrain.think(&perception([0.8, 0.8, 0.8], [0.0, 0.8, 0.0], [0.0; 3]));
        assert_eq!(action.dir, Vec2::X, "la cible au centre l'emporte");
    }

    /// Déterminisme : deux évaluations de la même perception donnent la même action
    /// (pas d'état, pas de RNG — c'est ce qui en fait un groupe témoin reproductible).
    #[test]
    fn hunter_is_deterministic() {
        let p = perception([0.1, 0.4, 0.2], [0.0, 0.4, 0.0], [0.0; 3]);
        assert_eq!(HunterBrain.think(&p).dir, HunterBrain.think(&p).dir);
    }

    /// FUITE ACTIVE — une **menace** droit devant (+X), côtés dégagés : le rayon
    /// avant prend un poids négatif (répulsion) → le pilotage **rebrousse chemin**
    /// (dir.x < 0). Le miroir exact de `hunter_cruises_forward_in_the_open`, mais où
    /// l'obstacle avant est une menace au lieu d'un vide.
    #[test]
    fn hunter_flees_a_threat_ahead() {
        // +X : une menace proche (donc aussi un hit, vision 0.6) ; côtés dégagés.
        let action = HunterBrain.think(&perception([0.0, 0.6, 0.0], [0.0; 3], [0.0, 0.6, 0.0]));
        assert!(
            action.dir.x < 0.0,
            "doit rebrousser chemin face à la menace, dir={:?}",
            action.dir
        );
    }

    /// FUITE par subsomption — une **menace proche** sur le flanc gauche (+Y, 0.8 >
    /// seuil) ET une **cible** droit devant (+X) : la fuite *suspend* le fourrage. La
    /// proie s'écarte nettement de la menace (dir.y < 0) et **n'avance plus** vers sa
    /// cible (dir.x < 0) — la survie court-circuite le fourrage tant que le danger dure.
    #[test]
    fn flight_suspends_foraging() {
        // +Y : menace proche (0.8 > FLEE_THRESHOLD) ; +X : cible comestible (0.6) ;
        // -Y : dégagé.
        let action = HunterBrain.think(&perception(
            [0.8, 0.6, 0.0],
            [0.0, 0.6, 0.0],
            [0.8, 0.0, 0.0],
        ));
        assert!(
            action.dir.y < 0.0,
            "doit s'écarter de la menace en +Y (filer vers -Y), dir={:?}",
            action.dir
        );
        assert!(
            action.dir.x < 0.0,
            "fuite : ne fourrage plus vers la cible en +X, dir={:?}",
            action.dir
        );
    }

    /// Une menace **lointaine** (proximité 0.2 < seuil) ne déclenche pas la fuite : le
    /// fourrage continue. Cible droit devant (+X, 0.6), menace faible sur le flanc
    /// (+Y, 0.2) → la proie poursuit sa cible (dir.x > 0). C'est ce seuil qui évite la
    /// famine d'une proie qu'un prédateur ne fait que survoler de loin.
    #[test]
    fn distant_threat_does_not_interrupt_foraging() {
        // +X : cible (0.6) ; +Y : menace LOINTAINE (0.2 < FLEE_THRESHOLD) ; -Y : dégagé.
        let action = HunterBrain.think(&perception(
            [0.2, 0.6, 0.0],
            [0.0, 0.6, 0.0],
            [0.2, 0.0, 0.0],
        ));
        assert!(
            action.dir.x > 0.0,
            "menace lointaine : doit continuer à fourrager vers +X, dir={:?}",
            action.dir
        );
    }

    /// Le paramètre propre au variant `Wander` (turn_rate) est bien transmis au
    /// cerveau compilé : c'est ce que le sélecteur d'éditeur (item 15) fait varier.
    /// (`n_inputs` n'importe que pour le MLP ; ici 0.)
    #[test]
    fn brainkind_wander_carries_its_turn_rate() {
        match (BrainKind::Wander { turn_rate: 0.4 }).build(1, 0.0, 0) {
            Brain::Wander(w) => assert_eq!(w.turn_rate, 0.4),
            other => panic!("attendu Wander, obtenu {other:?}"),
        }
        assert!(matches!(BrainKind::default(), BrainKind::Wander { .. }));
        assert!(matches!(
            (BrainKind::Hunter).build(1, 0.0, 0),
            Brain::Hunter(_)
        ));
    }

    /// L'héritage du cerveau (item 18a) : un enfant **reconduit le type** de son
    /// parent — c'est ce qui fait cohabiter un témoin déterministe et un cerveau
    /// appris (§4). Le Wander hérite le `turn_rate` du parent (paramètre d'archétype)
    /// mais reçoit un état RNG frais ; le Hunter, sans état, est cloné. Ni l'un ni
    /// l'autre ne tire dans `rng` (le flux des scénarios non-MLP reste intact).
    #[test]
    fn reproduce_keeps_the_parent_variant() {
        let mut rng = Rng::new(0);
        // Hunter → Hunter (déterministe, cloné). `n_inputs` ignoré par Hunter.
        let hunter = Brain::Hunter(HunterBrain);
        assert!(matches!(
            hunter.reproduce(7, 1.0, &mut rng, 0.1, 6),
            Brain::Hunter(_)
        ));

        // Wander → Wander, turn_rate hérité, état RNG distinct (graine ≠).
        let parent = Brain::Wander(WanderBrain::new(1, 0.0, 0.37));
        match parent.reproduce(2, 0.5, &mut rng, 0.1, 6) {
            Brain::Wander(child) => {
                assert_eq!(child.turn_rate, 0.37, "le turn_rate du parent est hérité");
                let Brain::Wander(p) = &parent else {
                    unreachable!()
                };
                assert_ne!(child.rng, p.rng, "l'enfant a un état RNG frais");
            }
            other => panic!("attendu Wander, obtenu {other:?}"),
        }
        // Wander/Hunter n'ont pas consommé `rng` : son état est celui du départ.
        assert_eq!(
            rng,
            Rng::new(0),
            "les cerveaux non-MLP ne touchent pas au flux RNG"
        );
    }

    /// Perception à 3 rayons pour les tests MLP : 6 entrées (vision ++ cible).
    fn mlp_perception(heading: Vec2, vision: [f32; 3], target: [f32; 3]) -> Perception {
        Perception {
            heading,
            vision: vision.into(),
            target: target.into(),
            // Le MLP ne lit pas (encore) le canal menace : il l'ignore quoi qu'il vaille.
            threat: [0.0; 3].into(),
            ray_dirs: vec![Vec2::Y, Vec2::X, Vec2::NEG_Y].into_boxed_slice(),
        }
    }

    /// Le MLP construit par `BrainKind` respecte le contrat d'E/S : entrée =
    /// `2 × vision_rays`, sortie = `OUTPUTS`, couches cachées telles que demandées.
    #[test]
    fn brainkind_mlp_builds_with_contract_io() {
        let n_inputs = MlpBrain::input_size(3); // 6
        let Brain::Mlp(m) = (BrainKind::Mlp { hidden: vec![5] }).build(42, 0.0, n_inputs) else {
            panic!("attendu un MLP");
        };
        assert_eq!(m.layers.len(), 2, "1 cachée + 1 sortie");
        assert_eq!(m.layers[0].inputs, 6, "entrée = 2 × rayons");
        assert_eq!(m.layers[0].outputs(), 5, "couche cachée demandée");
        assert_eq!(m.layers[1].inputs, 5);
        assert_eq!(m.layers[1].outputs(), MlpBrain::OUTPUTS);
        // L'API de visualisation (item 18b-viz) reflète la même topologie.
        assert_eq!(m.layer_sizes(), vec![6, 5, MlpBrain::OUTPUTS]);
        assert_eq!(m.weight_layers(), 2);
        let (w, fan_in, fan_out) = m.layer_weights(0);
        assert_eq!((fan_in, fan_out), (6, 5));
        assert_eq!(w.len(), 6 * 5);
    }

    /// Le MLP est déterministe (mêmes poids + même perception → même action) et
    /// **orientation-équivariant** : tournés du même cap, les mêmes canaux donnent une
    /// action tournée d'autant (la décision vit en repère du corps).
    #[test]
    fn mlp_is_deterministic_and_orientation_equivariant() {
        let brain = MlpBrain::random(7, MlpBrain::input_size(3), &[6]);
        let (vision, target) = ([0.2, 0.7, 0.1], [0.0, 0.7, 0.0]);

        let a1 = brain.think(&mlp_perception(Vec2::X, vision, target));
        let a2 = brain.think(&mlp_perception(Vec2::X, vision, target));
        assert_eq!(a1.dir, a2.dir, "déterministe");
        assert_eq!(a1.throttle, a2.throttle);

        // Même perception relative, cap tourné de +90° → l'action tourne de +90°.
        let a_y = brain.think(&mlp_perception(Vec2::Y, vision, target));
        let expected = Vec2::Y.rotate(a1.dir); // rotation de +90°
        assert!(
            (a_y.dir - expected).length() < 1e-5,
            "la sortie doit être en repère du corps : {:?} vs {:?}",
            a_y.dir,
            expected
        );
    }

    /// La neuroévolution : muter **perturbe les poids** mais **garde la topologie** ;
    /// un taux nul est l'identité (régime évolution éteinte).
    #[test]
    fn mlp_mutation_perturbs_weights_keeps_topology() {
        let mut rng = Rng::new(3);
        let parent = MlpBrain::random(11, MlpBrain::input_size(4), &[8, 4]);

        // Taux nul → clone fidèle. L'égalité ne porte que topologie + poids (le
        // cerveau n'a pas d'autre état : les activations se recalculent à la demande).
        assert_eq!(parent.mutated(&mut rng, 0.0), parent, "taux nul = identité");

        // Taux non nul → poids changés, mais même nombre de couches et mêmes dims.
        let child = parent.mutated(&mut rng, 0.2);
        assert_ne!(child, parent, "les poids ont bougé");
        assert_eq!(child.layers.len(), parent.layers.len());
        for (c, p) in child.layers.iter().zip(&parent.layers) {
            assert_eq!(c.inputs, p.inputs);
            assert_eq!(c.outputs(), p.outputs());
        }
    }

    /// La reproduction adapte la **couche d'entrée** au nombre de rayons de l'enfant
    /// (gène `vision_rays`, item 3) : la première couche prend `2 × rayons` entrées,
    /// la topologie cachée et la sortie ne bougent pas. À précision constante et taux
    /// nul, c'est exactement l'identité (même flux RNG que `mutated`).
    #[test]
    fn mlp_reproduce_resizes_input_layer_to_child_rays() {
        let mut rng = Rng::new(5);
        let parent = MlpBrain::random(11, MlpBrain::input_size(3), &[8, 4]); // 6 entrées

        // Précision inchangée + taux nul = clone fidèle.
        let same = parent.reproduced(&mut rng, 0.0, MlpBrain::input_size(3));
        assert_eq!(same, parent, "précision constante, taux nul → identité");

        // Enfant qui voit plus fin : 5 rayons → 10 entrées (couche d'entrée agrandie).
        let grown = parent.reproduced(&mut rng, 0.1, MlpBrain::input_size(5));
        assert_eq!(grown.layer_sizes(), vec![10, 8, 4, MlpBrain::OUTPUTS]);

        // Enfant qui voit plus grossier : 2 rayons → 4 entrées (couche d'entrée rétrécie).
        let shrunk = parent.reproduced(&mut rng, 0.1, MlpBrain::input_size(2));
        assert_eq!(shrunk.layer_sizes(), vec![4, 8, 4, MlpBrain::OUTPUTS]);
    }

    /// La visualisation des activations est calculée **à la demande**, hors du cœur
    /// de sim : `forward_activations` rejoue la propagation et expose une couche par
    /// colonne (`[input, h0, …, sortie]`, tailles = `layer_sizes`), et sa dernière
    /// couche coïncide avec la sortie brute du `think` (avant rotation/normalisation).
    /// C'est ce qui permet à `think` de ne plus rien mémoriser.
    #[test]
    fn forward_activations_match_topology_and_think() {
        let brain = MlpBrain::random(7, MlpBrain::input_size(3), &[6, 4]);
        let (vision, target) = ([0.2, 0.7, 0.1], [0.0, 0.7, 0.0]);
        let p = mlp_perception(Vec2::X, vision, target);

        let acts = brain.forward_activations(&p);
        // Une couche d'activations par colonne du graphe (entrée incluse).
        assert_eq!(acts.len(), brain.layer_sizes().len());
        for (layer, &size) in acts.iter().zip(&brain.layer_sizes()) {
            assert_eq!(layer.len(), size);
        }
        // L'entrée exposée = vision ++ cible (le vecteur d'entrée du réseau).
        assert_eq!(acts[0], vec![0.2, 0.7, 0.1, 0.0, 0.7, 0.0]);

        // La sortie brute (dernière couche) est cohérente avec l'action de `think` :
        // `throttle = min(|sortie|, 1)`, et la direction est cette sortie tournée par
        // le cap (+X ici, donc inchangée à la normalisation près).
        let out = acts.last().unwrap();
        assert_eq!(out.len(), MlpBrain::OUTPUTS);
        let action = brain.think(&p);
        let raw = Vec2::new(out[0], out[1]);
        assert!((action.throttle - raw.length().min(1.0)).abs() < 1e-6);
        assert!((action.dir - raw.normalize_or_zero()).length() < 1e-5);
    }

    /// Une perception de mauvaise taille (fan-in qui ne colle pas) tronque la
    /// propagation sans paniquer : on récupère au moins l'entrée, l'inspecteur
    /// colorant les nœuds manquants en neutre.
    #[test]
    fn forward_activations_is_robust_to_wrong_input_size() {
        let brain = MlpBrain::random(1, MlpBrain::input_size(3), &[5]); // attend 6 entrées
        // Perception à 2 rayons → 4 entrées (≠ 6) : le premier produit ne colle pas.
        let p = Perception {
            heading: Vec2::X,
            vision: [0.1, 0.2].into(),
            target: [0.0, 0.0].into(),
            threat: [0.0, 0.0].into(),
            ray_dirs: vec![Vec2::X, Vec2::Y].into_boxed_slice(),
        };
        let acts = brain.forward_activations(&p);
        assert_eq!(acts.len(), 1, "seule l'entrée est exposée, sans panique");
        assert_eq!(acts[0].len(), 4);
    }
}
