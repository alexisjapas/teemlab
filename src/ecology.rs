//! The **energy economy** of the natural-selection scenario (item 8):
//! *eat, spend, die*.
//!
//! This is where, per §7, the whole balance of natural selection plays out — a
//! matter of **tuning**, not of the algorithm. Three systems:
//!
//! - [`metabolize`] computes the energy balance: **expenses** (base, locomotion
//!   cruising, **agility** maneuvering, **vision cost** — the coupling quantified
//!   in item 6 finally finding its consumer — and **brain cost**, per decision
//!   neuron) minus the **gain** from photosynthesis (a flora gene);
//! - [`reap`] removes agents that have run out of energy;
//! - [`reproduce`] closes the evolutionary loop.
//!
//! Since Phase 3b, there is no more `replenish_food` system nor `Food` type: a
//! *food source* is a **sessile** agent (Phase 3a) that regains its energy in
//! place through photosynthesis — the ecosystem's energy supply therefore
//! emerges from [`metabolize`], without a separate faucet. Eating, too, is not
//! here: it is the interaction primitive (item 7) that transfers energy from a
//! target to the actor. The engine has only one verb.

use crate::brain::{Brain, MlpBrain};
use crate::components::{Age, Agent, Generation, Maneuver, Reserve, Species, Vision};
use crate::config::SimConfig;
use crate::genotype::Genotype;
use crate::nutrients::Nutrients;
use crate::rng::Rng;
use crate::spawn::spawn_agent_with_brain;
use avian2d::prelude::*;
use bevy::prelude::*;

/// The simulation's random stream for stochastic events (here, the seeding
/// offsets and the mutations at reproduction). Lives in the sim world, seeded
/// from the config — we replay an *experiment*, not bit-for-bit (§5).
#[derive(Resource)]
pub struct SimRng(pub Rng);

impl SimRng {
    /// The sim stream seeded from the config, offset from population (`^ 0xF00D`)
    /// so the two streams are not correlated. Single source: used at resource
    /// insertion (at build) **and** at hot reset (item 11).
    pub fn from_config(config: &SimConfig) -> Self {
        Self(Rng::new(config.seed ^ 0xF00D))
    }
}

/// METABOLISM: each agent's per-second energy balance. **Expenses** — base +
/// speed surcharge (cruising) + agility surcharge (maneuvering) + vision sensor
/// cost + brain cost (per decision neuron); **gain** — photosynthesis (a flora
/// gene, passive gain). Bounded to `[0, max]`; death at zero is left to [`reap`].
pub fn metabolize(
    time: Res<Time>,
    config: Res<SimConfig>,
    mut agents: Query<
        (
            &mut Reserve,
            &Genotype,
            &Species,
            &Vision,
            &LinearVelocity,
            &Brain,
            &Maneuver,
        ),
        With<Agent>,
    >,
) {
    let dt = time.delta_secs();
    for (mut reserve, genotype, species, vision, velocity, brain, maneuver) in &mut agents {
        // Metabolism, locomotion, agility, photosynthesis and brain cost are genes
        // (per-species). An agent with no energy item at all (all five zero) is in
        // an inert world (pre-item-8 scenarios): neither drain nor gain, not even
        // the vision or brain cost.
        if genotype.base_metabolism == 0.0
            && genotype.move_cost == 0.0
            && genotype.agility_cost == 0.0
            && genotype.photosynthesis == 0.0
            && genotype.brain_cost == 0.0
        {
            continue;
        }
        // *Reference* speed: the archetype's **founding** max speed (not the
        // agent's, possibly mutated one) — otherwise a mutant twice as fast would
        // pay the same and the speed gene would have no cost. This keeps "speed →
        // energy" (§2) true, and the cost stays measured against a per-species
        // reference.
        let reference_speed = config.founder_max_speed_of(species.0).max(1e-3);
        let speed_ratio = velocity.0.length() / reference_speed;
        // Agility cost: the energy of *maneuvering*. `maneuver.0` is the magnitude
        // of the velocity change `act` applied this tick (cf. `Maneuver`), i.e. the
        // work done against inertia to turn/accelerate — the transient counterpart
        // of `move_cost`, which prices steady-state cruising. Cruising in a straight
        // line (already at the desired velocity) costs nothing here.
        //
        // Brain cost: energy/s per decision neuron (hidden + output). A hand-written
        // brain counts zero neurons (cf. `Brain::neuron_count`) → no cost, so
        // non-MLP scenarios are unaffected. The counterpart, for the *decision
        // system*, of the vision sensor's cost.
        let drain = genotype.base_metabolism
            + genotype.move_cost * speed_ratio
            + genotype.agility_cost * maneuver.0
            + vision.metabolic_cost()
            + genotype.brain_cost * brain.neuron_count() as f32;
        // Net balance = passive gain − expenses. For fauna (photosynthesis 0)
        // this is the old pure drain, and the cap at `max` is then a no-op (eating
        // already caps at `max`, cf. `interaction`) → unchanged behavior.
        let net = genotype.photosynthesis - drain;
        reserve.current = (reserve.current + net * dt).clamp(0.0, reserve.max);
    }
}

/// DIE: remove from the world the agents whose energy is depleted. Runs **before**
/// [`metabolize`] (cf. the `lib.rs` schedule): an entity grazed to zero by `interact`
/// dies before photosynthesis could refill it, so a flora grazed empty dies like a
/// fauna starved empty (SIM Law 11 — one uniform death rule, no kind exempted).
pub fn reap(mut commands: Commands, agents: Query<(Entity, &Reserve), With<Agent>>) {
    for (entity, reserve) in &agents {
        if reserve.current <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// AGE: each living agent gains `dt` seconds of age every tick. A trivial but
/// separate system — age is an **observable** entity property (genealogy, and one
/// day age-dependent strategies), not a by-product of another system. Runs in
/// `FixedUpdate`, so headless and windowed age the same.
pub fn age_agents(time: Res<Time>, mut agents: Query<&mut Age, With<Agent>>) {
    let dt = time.delta_secs();
    for mut age in &mut agents {
        age.0 += dt;
    }
}

/// REPRODUCTION (continuous-implicit regime, §4): fitness is endogenous — *you
/// reproduced*. An agent whose energy reaches its threshold pays its
/// `offspring_energy` (conservation: nothing is created) to spawn a child with a
/// mutated genotype, placed near it. This is what closes the **continuous
/// evolutionary loop**: selection acts on the genes through the mere fact of
/// surviving long enough to reproduce.
///
/// Threshold, cost and mutation rate are **entity genes** (§1, *the body*): the
/// reproduction strategy evolves itself and may differ from one species to the
/// next.
///
/// The child's brain **inherits the parent's** ([`Brain::reproduce`], item 18a)
/// rather than being rebuilt from the `config`: this is what lets a deterministic
/// control and a learned brain coexist durably (§4), and the seam that 18b will
/// extend to mutate the weights.
///
/// **Nutrient axis (T2):** reproduction is *also* gated on the nutrient store
/// ([`Nutrients`]) — a parent needs `offspring_nutrient` in store to reproduce, and
/// **spends** it (the child is born with an **empty** store). With the nutrient
/// genes at 0 (every pre-T2 scenario) the gate passes spending nothing → unchanged.
/// This is the second axis of ROADMAP §9's two-axis design: a missing nutrient stops
/// reproduction but, unlike energy, never causes death (no spiral).
///
/// **Why the child is born empty (and not endowed like `offspring_energy`):** a seed
/// endowed with `offspring_nutrient` would meet the gate *immediately*, so the
/// nutrient would merely **circulate** down a lineage and never limit anything
/// (energy, the abundant solar axis, would set the pace → unbounded growth). Making
/// the nutrient a **consumable** removed from the pool makes it a genuine limiting
/// resource (Liebig): each new plant must absorb its **own** fresh nutrient from the
/// field to reproduce, so the population's growth is throttled by the field's supply.
/// This deviates from the originally-planned "born with `offspring_nutrient`
/// (conservation)" — the conserving, closed loop returns with **recycling**
/// (deferred: a dead body returns its nutrient to the field), cf.
/// `docs/nutrients-t2-plan.md`.
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
            &mut Nutrients,
        ),
        With<Agent>,
    >,
) {
    for (transform, mut reserve, genotype, species, brain, generation, mut nutrients) in
        &mut parents
    {
        // Threshold and cost are **genes** (per-entity, evolvable): a zero
        // threshold = this agent does not reproduce.
        //
        // We also require `current >= offspring_energy`: threshold and cost being
        // two genes that drift independently, nothing guarantees `threshold >=
        // cost`. Without this guard, a parent whose cost exceeds its reserve would
        // go negative (then die), BUT the child would still carry the full
        // `offspring_energy` → energy created out of nothing, and a "low threshold
        // / expensive child" lineage would be *favored* (runaway). The guard makes
        // conservation **unconditional**: we never pay more than we have.
        if genotype.reproduction_threshold <= 0.0
            || reserve.current < genotype.reproduction_threshold
            || reserve.current < genotype.offspring_energy
            || nutrients.current < genotype.offspring_nutrient
        {
            continue;
        }
        reserve.current -= genotype.offspring_energy;
        // Spend the nutrient cost from the parent's store — it is **consumed**, not
        // handed to the child (which is born empty, see below): this is what makes
        // the nutrient a true limiting resource. `0` for a pre-T2 scenario → no-op.
        nutrients.current -= genotype.offspring_nutrient;
        let child = genotype.mutate(&mut rng.0, &config.mutable_of(species.0), &config);
        // The child is born offset. The distance is the **seed-dispersal** gene
        // (flora) if non-zero, otherwise the default close offset (radius × 2.5) —
        // fauna behavior, unchanged. Same 2 draws (the direction) in both cases →
        // RNG stream preserved for scenarios without dispersal.
        let spread = if genotype.seed_dispersal > 0.0 {
            genotype.seed_dispersal
        } else {
            config.agent_radius_of(species.0) * 2.5
        };
        let offset =
            Vec2::new(rng.0.next_signed(), rng.0.next_signed()).normalize_or_zero() * spread;
        // Born **inside** the arena. A parent that has drifted a little past the wall
        // (the solver pushes it back, but not within one tick) would otherwise place
        // its child even further out — and that child, born outside, reproduces again
        // before returning: an outward escape via reproduction. Clamping the spawn to
        // `[-h+r, h-r]` keeps every birth in the world. (The two RNG draws above are
        // untouched, so scenarios with no edge births stay bit-identical.)
        let h = config.arena_half_extent;
        let r = config.agent_radius_of(species.0);
        let pos = (transform.translation.truncate() + offset)
            .clamp(Vec2::splat(-h + r), Vec2::splat(h - r));
        // Same draws (heading then seed) as before inheritance: the child brain
        // consumes them via `reproduce` instead of `config.brain.build` → RNG
        // stream unchanged for non-MLP scenarios. The MLP additionally draws from
        // `rng.0` to mutate its weights (neuroevolution), driven by
        // `mutation_rate` (item 18b).
        let heading = rng.0.next_f32() * std::f32::consts::TAU;
        let brain_seed = rng.0.next_u64();
        // The child MLP's input size = its visual precision (gene `vision_rays`,
        // item 3); if it differs from the parent's, `reproduce` adapts the input
        // layer. Without an MLP, this is ignored → non-MLP scenarios' RNG stream
        // intact.
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
            0.0, // born with an **empty** nutrient store (T2): must absorb its own.
            generation.0 + 1,
            0.0, // a newborn is born at age 0.
        );
    }
}
