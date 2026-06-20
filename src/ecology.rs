//! L'**économie d'énergie** du scénario de sélection naturelle (item 8) :
//! *manger, dépenser, mourir*.
//!
//! C'est ici que se joue, selon §7, tout l'équilibre de la sélection naturelle —
//! du **réglage**, pas de l'algo. Trois systèmes :
//!
//! - [`metabolize`] fait le bilan d'énergie : **dépenses** (base + locomotion +
//!   **coût de la vision**, le couplage quantifié à l'item 6 trouvant enfin son
//!   consommateur) moins le **gain** de photosynthèse (gène de flore) ;
//! - [`reap`] retire les agents à court d'énergie ;
//! - [`reproduce`] ferme la boucle évolutive.
//!
//! Depuis la Phase 3b, il n'y a plus de système `replenish_food` ni de type `Food` :
//! une *source de nourriture* est un agent **sessile** (Phase 3a) qui regagne son
//! énergie sur place par photosynthèse — l'offre d'énergie de l'écosystème émerge donc
//! de [`metabolize`], sans robinet séparé. Manger, lui, n'est pas ici non plus : c'est
//! la primitive d'interaction (item 7) qui transfère l'énergie d'une cible vers
//! l'acteur. Le moteur n'a qu'un verbe.

use crate::brain::{Brain, MlpBrain};
use crate::components::{Age, Agent, Generation, Reserve, Species, Vision};
use crate::config::SimConfig;
use crate::genotype::Genotype;
use crate::rng::Rng;
use crate::spawn::spawn_agent_with_brain;
use avian2d::prelude::*;
use bevy::prelude::*;

/// Flux aléatoire de la simulation pour les événements stochastiques (ici, les
/// décalages de semis et les mutations à la reproduction). Vit dans le monde de sim,
/// seedé depuis la config — on rejoue une *expérience*, pas le bit-à-bit (§5).
#[derive(Resource)]
pub struct SimRng(pub Rng);

impl SimRng {
    /// Flux de sim seedé depuis la config, décalé du peuplement (`^ 0xF00D`) pour
    /// ne pas corréler les deux flux. Source unique : utilisée à l'insertion de la
    /// ressource (au build) **et** à la réinitialisation à chaud (item 11).
    pub fn from_config(config: &SimConfig) -> Self {
        Self(Rng::new(config.seed ^ 0xF00D))
    }
}

/// MÉTABOLISME : le bilan d'énergie par seconde de chaque agent. **Dépenses** — base +
/// surcoût de vitesse + coût du capteur de vision ; **gain** — la photosynthèse (gène de
/// flore, gain passif). Borné à `[0, max]` ; la mort à zéro est laissée à [`reap`].
pub fn metabolize(
    time: Res<Time>,
    config: Res<SimConfig>,
    mut agents: Query<(&mut Reserve, &Genotype, &Species, &Vision, &LinearVelocity), With<Agent>>,
) {
    let dt = time.delta_secs();
    for (mut reserve, genotype, species, vision, velocity) in &mut agents {
        // Métabolisme, coût de locomotion et photosynthèse sont des gènes (per-espèce).
        // Un agent sans aucun poste d'énergie (les trois à zéro) est dans un monde
        // inerte (scénarios pré-item-8) : ni drain ni gain, pas même le coût de vision.
        if genotype.base_metabolism == 0.0
            && genotype.move_cost == 0.0
            && genotype.photosynthesis == 0.0
        {
            continue;
        }
        // Vitesse de *référence* : la vitesse max **fondatrice de l'archétype** (pas
        // celle, peut-être mutée, de l'agent) — sinon un mutant deux fois plus rapide
        // paierait pareil et le gène de vitesse n'aurait aucun coût. Ainsi « vitesse →
        // énergie » (§2) tient, et le coût reste rapporté à une référence par espèce.
        let reference_speed = config.genotype_of(species.0).max_speed.max(1e-3);
        let speed_ratio = velocity.0.length() / reference_speed;
        let drain =
            genotype.base_metabolism + genotype.move_cost * speed_ratio + vision.metabolic_cost();
        // Bilan net = gain passif − dépenses. Pour la faune (photosynthèse 0) c'est
        // l'ancien drain pur, et le plafond à `max` est alors un no-op (manger plafonne
        // déjà à `max`, cf. `interaction`) → comportement inchangé.
        let net = genotype.photosynthesis - drain;
        reserve.current = (reserve.current + net * dt).clamp(0.0, reserve.max);
    }
}

/// MOURIR : retirer du monde les agents dont l'énergie est épuisée.
pub fn reap(mut commands: Commands, agents: Query<(Entity, &Reserve), With<Agent>>) {
    for (entity, reserve) in &agents {
        if reserve.current <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// VIEILLIR : chaque agent vivant gagne `dt` secondes d'âge à chaque tick. Système
/// trivial mais à part — l'âge est une propriété d'entité **observable** (généalogie,
/// et un jour des stratégies dépendantes de l'âge), pas un sous-produit d'un autre
/// système. Tourne dans `FixedUpdate`, donc headless et fenêtré vieillissent pareil.
pub fn age_agents(time: Res<Time>, mut agents: Query<&mut Age, With<Agent>>) {
    let dt = time.delta_secs();
    for mut age in &mut agents {
        age.0 += dt;
    }
}

/// REPRODUCTION (régime continu-implicite, §4) : la fitness est endogène — *tu
/// t'es reproduit*. Un agent dont l'énergie atteint son seuil paie son
/// `offspring_energy` (conservation : rien n'est créé) pour engendrer un enfant
/// au génotype muté, posé près de lui. C'est ce qui ferme la **boucle évolutive
/// continue** : la sélection agit sur les gènes via le simple fait de survivre
/// assez pour se reproduire.
///
/// Seuil, coût et taux de mutation sont des **gènes de l'entité** (§1, *le
/// corps*) : la stratégie de reproduction évolue elle-même et peut différer d'une
/// espèce à l'autre.
///
/// Le cerveau de l'enfant **hérite de celui du parent**
/// ([`Brain::reproduce`], item 18a) plutôt que d'être reconstruit depuis le
/// `config` : c'est ce qui fait cohabiter durablement un témoin déterministe et un
/// cerveau appris (§4), et la couture que 18b étendra pour muter les poids.
pub fn reproduce(
    mut commands: Commands,
    config: Res<SimConfig>,
    mut rng: ResMut<SimRng>,
    mut parents: Query<
        (
            &Transform,
            &mut Reserve,
            &Genotype,
            &Species,
            &Brain,
            &Generation,
        ),
        With<Agent>,
    >,
) {
    for (transform, mut reserve, genotype, species, brain, generation) in &mut parents {
        // Seuil et coût sont des **gènes** (per-entité, évolvables) : un seuil nul
        // = cet agent ne se reproduit pas.
        //
        // On exige aussi `current >= offspring_energy` : seuil et coût étant deux
        // gènes qui dérivent indépendamment, rien ne garantit `seuil >= coût`. Sans
        // ce garde, un parent dont le coût dépasse la réserve passerait en négatif
        // (puis mourrait), MAIS l'enfant emporterait quand même la pleine
        // `offspring_energy` → de l'énergie créée ex nihilo, et une lignée
        // « seuil bas / enfant cher » serait *avantagée* (runaway). Le garde rend la
        // conservation **inconditionnelle** : on ne paie jamais plus qu'on n'a.
        if genotype.reproduction_threshold <= 0.0
            || reserve.current < genotype.reproduction_threshold
            || reserve.current < genotype.offspring_energy
        {
            continue;
        }
        reserve.current -= genotype.offspring_energy;
        let child = genotype.mutate(&mut rng.0, &config.mutable_of(species.0), &config);
        // L'enfant naît décalé. La distance est le gène de **dissémination** (flore)
        // s'il est non nul, sinon le décalage rapproché par défaut (rayon × 2.5) — le
        // comportement de la faune, inchangé. Mêmes 2 tirages (la direction) dans les
        // deux cas → flux RNG préservé pour les scénarios sans dissémination.
        let spread = if genotype.seed_dispersal > 0.0 {
            genotype.seed_dispersal
        } else {
            config.agent_radius_of(species.0) * 2.5
        };
        let offset =
            Vec2::new(rng.0.next_signed(), rng.0.next_signed()).normalize_or_zero() * spread;
        let pos = transform.translation.truncate() + offset;
        // Mêmes tirages (heading puis seed) qu'avant l'héritage : le cerveau enfant
        // les consomme via `reproduce` au lieu de `config.brain.build` → flux RNG
        // inchangé pour les scénarios non-MLP. Le MLP, lui, tire en plus dans `rng.0`
        // pour muter ses poids (neuroévolution), pilotée par `mutation_rate` (item 18b).
        let heading = rng.0.next_f32() * std::f32::consts::TAU;
        let brain_seed = rng.0.next_u64();
        // Taille d'entrée du MLP enfant = sa précision visuelle (gène `vision_rays`,
        // item 3) ; si elle diffère de celle du parent, `reproduce` adapte la couche
        // d'entrée. Sans MLP, c'est ignoré → flux RNG des scénarios non-MLP intact.
        let n_inputs = MlpBrain::input_size(child.ray_count());
        let child_brain = brain.reproduce(
            brain_seed,
            heading,
            &mut rng.0,
            genotype.mutation_rate,
            n_inputs,
        );
        spawn_agent_with_brain(
            &mut commands,
            &config,
            child,
            *species,
            pos,
            child_brain,
            genotype.offspring_energy,
            generation.0 + 1,
            0.0, // un nouveau-né naît à l'âge 0.
        );
    }
}
