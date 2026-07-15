//! The brain.
//!
//! Contract: `&Perception` (normalized floats) → `Action` (floats). The inside is
//! interchangeable. We store it as an **`enum`**, not a `Box<dyn>`: static
//! dispatch, clean `serde`, and the compiler lists the `match`es to complete when
//! a brain type is added. Crossover is intra-type anyway (one does not cross a NN
//! with an FSM).

use crate::components::{Action, Perception};
use crate::rng::Rng;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;

thread_local! {
    /// Reused forward-pass scratch for [`MlpBrain::think`]: two `Vec<f32>` ping-ponged
    /// so the per-tick decision allocates nothing after warm-up. Thread-local because
    /// `decide` may run on any Bevy worker thread — each thread gets its own pair, so
    /// there is no sharing and no locking. Purely an allocation optimisation; it never
    /// changes a computed value (byte-identical to the allocating path).
    static THINK_SCRATCH: RefCell<(Vec<f32>, Vec<f32>)> =
        const { RefCell::new((Vec::new(), Vec::new())) };
}

/// An agent's brain. One variant per implementation.
///
/// Clean `serde` (§2): a brain is serializable, hence capturable in a run
/// snapshot (item 13) — and will be for a future MLP without changing the
/// contract.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Brain {
    /// A trivial deterministic scaffold: random walk (wandering). De-risks the
    /// whole perceive→decide→act chain before any learned brain exists, and serves
    /// as a "naive" control group.
    Wander(WanderBrain),
    /// Deterministic reflex (item 16): charges toward the nearest perceived
    /// target. The **competent control group** — a learned brain that does not
    /// beat it has learned nothing (§4) — and the 2nd variant that makes the brain
    /// selector falsifiable.
    Hunter(HunterBrain),
    /// **Grazer** — the deterministic control for **restraint**. Forages exactly
    /// like [`Brain::Hunter`] (shared steering field) but eats *deliberately*: it
    /// gates its eat/attack intent on its own **hunger** (proprioception — the
    /// `self_state` energy fraction) against a `hunger_threshold`, so a *prudent*
    /// grazer leaves food uneaten once sated. It is to restraint what the hunter is
    /// to foraging (§4.2 "validate on the control before the learned brain"): it
    /// makes the selective value of restraint under spatial viscosity legible before
    /// a learned brain is asked to discover it (`docs/persistent-ecosystems.md` §2).
    Grazer(GrazerBrain),
    /// **Sessile** (Phase 3): does not decide, does not move — the trivial brain of
    /// *flora*. "Stub the behavior, never the schema" (§8): a legitimate behavior
    /// shell, for an entity that lives on photosynthesis and reproduces by seeding,
    /// without moving.
    Sessile(SessileBrain),
    /// Homemade multilayer perceptron (item 18b), the **learned brain**: its
    /// weights mutate at reproduction (neuroevolution). It is what the hunter and
    /// the wanderer served to gauge (§4) — the only variant whose decision is not
    /// hand-written but discovered by selection.
    Mlp(MlpBrain),
}

impl Brain {
    /// The contract. An exhaustive `match` → adding a variant = a compile error
    /// here, exactly what we want.
    pub fn think(&mut self, perception: &Perception) -> Action {
        match self {
            Brain::Wander(b) => b.think(perception),
            Brain::Hunter(b) => b.think(perception),
            Brain::Grazer(b) => b.think(perception),
            Brain::Sessile(b) => b.think(perception),
            Brain::Mlp(b) => b.think(perception),
        }
    }

    /// Short label of the brain type, for the inspector (item 12).
    pub fn name(&self) -> &'static str {
        match self {
            Brain::Wander(_) => "Wander",
            Brain::Hunter(_) => "Hunter",
            Brain::Grazer(_) => "Grazer",
            Brain::Sessile(_) => "Sessile",
            Brain::Mlp(_) => "MLP",
        }
    }

    /// Stable index of the brain **family** (parameter-free) in [`Brain::FAMILIES`],
    /// for observation filters (the Follow "brains" picker) and any UI that groups
    /// agents by controller. Same order as [`Brain::FAMILIES`].
    pub fn family_index(&self) -> usize {
        match self {
            Brain::Wander(_) => 0,
            Brain::Hunter(_) => 1,
            Brain::Grazer(_) => 2,
            Brain::Sessile(_) => 3,
            Brain::Mlp(_) => 4,
        }
    }

    /// Number of distinct brain families (the length of [`Brain::FAMILIES`]).
    pub const FAMILY_COUNT: usize = 5;

    /// The distinct brain families in stable order, indexed by [`Brain::family_index`]
    /// — the canonical list a UI iterates to offer a per-family filter.
    pub const FAMILIES: [&'static str; Self::FAMILY_COUNT] =
        ["Wander", "Hunter", "Grazer", "Sessile", "MLP"];

    /// Size of the **decision system** for the metabolic cost (the energy economy,
    /// cf. [`crate::ecology::metabolize`]): the number of decision neurons of an
    /// MLP, `0` for the hand-written brains (Wander/Hunter/Sessile), which carry no
    /// evolvable network and therefore pay nothing. Exhaustive `match` so a new
    /// variant forces a decision here, like [`Brain::think`].
    pub fn neuron_count(&self) -> usize {
        match self {
            Brain::Wander(_) | Brain::Hunter(_) | Brain::Grazer(_) | Brain::Sessile(_) => 0,
            Brain::Mlp(m) => m.neuron_count(),
        }
    }

    /// A child's brain from **the parent's** (and not from a global [`BrainKind`]):
    /// the seam through which a *learned* behavior is transmitted (item 18a). This
    /// is where **neuroevolution** lives (item 18b):
    ///
    /// - `Wander` inherits the parent's `turn_rate` (an archetype parameter, not
    ///   mutated), with a **fresh** RNG state (`seed`/`heading`) to decorrelate the
    ///   lineage;
    /// - `Hunter`, deterministic and stateless, is simply cloned;
    /// - `Mlp` inherits the parent's **hidden topology**, **adapts its input layer**
    ///   to `n_inputs` (the child's visual precision may differ from the parent's,
    ///   gene `vision_rays`) and **mutates its weights** by Gaussian perturbation of
    ///   std-dev `rate · WEIGHT_STEP` (cf. [`MlpBrain::reproduced`]).
    ///
    /// `n_inputs` (= the child's `CHANNELS × rays`) only serves the MLP; Wander and
    /// Hunter ignore it. `seed`/`heading` only feed the stateful brains
    /// (wandering); `rng`/`rate` the MLP's mutation/adaptation. Wander and Hunter
    /// **do not draw** from `rng` → the RNG stream stays **identical** to non-MLP
    /// scenarios, `rate` coming from the genotype (`mutation_rate`, the per-lineage
    /// gene, §2).
    pub fn reproduce(
        &self,
        seed: u64,
        heading: f32,
        rng: &mut Rng,
        rate: f32,
        n_inputs: usize,
        n_sensed: usize,
    ) -> Brain {
        match self {
            Brain::Wander(w) => Brain::Wander(WanderBrain::new(seed, heading, w.turn_rate)),
            Brain::Hunter(_) => Brain::Hunter(HunterBrain),
            // Deterministic, stateless save its threshold param: the child grazes with
            // the same hunger rule (a designer strategy, not a mutated gene — the
            // evolvable `hunger_threshold` gene is deferred, §9). Draws no RNG, like the
            // other hand-written brains → non-MLP RNG stream intact.
            Brain::Grazer(g) => Brain::Grazer(g.clone()),
            Brain::Sessile(_) => Brain::Sessile(SessileBrain),
            Brain::Mlp(m) => Brain::Mlp(m.reproduced(rng, rate, n_inputs, n_sensed)),
        }
    }
}

/// The **type** of brain — the choice of the decision's author (§1), scenario
/// data. Separates *which* brain (and its **archetype parameters**, specific to
/// each variant — `turn_rate` for wandering, none for the hunter) from its *living
/// state*: a `BrainKind` (RON: `Wander(turn_rate: …)` / `Hunter`) compiles into a
/// fresh [`Brain`] at spawn, like a genotype into a phenotype (§2). Edited by the
/// editor's brain selector (item 15); each variant exposes its own parameters
/// there. Substitution by scenario (§4) **and** by species (item 18a) done.
///
/// No longer `Copy` since item 18b: the MLP carries its topology in a `Vec`. We
/// therefore `clone()` it explicitly (infrequent: spawn, `brain_of` fallback).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BrainKind {
    /// [`Brain::Wander`] — wandering. `turn_rate`: amplitude of the heading drift
    /// per tick (the parameter specific to this variant). Backward-compatible
    /// default (item 16).
    Wander { turn_rate: f32 },
    /// [`Brain::Hunter`] — deterministic reflex, no parameter.
    Hunter,
    /// [`Brain::Grazer`] — deterministic hunger-gated forager (the **restraint**
    /// control). `hunger_threshold`: eat while the energy reserve fraction is below
    /// it — `1.0` = greedy (always eats, exactly the hunter's reflex), a lower value
    /// = prudent (leaves food uneaten once sated). A designer strategy fixed at the
    /// founder (**not** mutated), like `Wander`'s `turn_rate`.
    Grazer { hunger_threshold: f32 },
    /// [`Brain::Sessile`] — immobile flora (Phase 3), no parameter.
    Sessile,
    /// [`Brain::Mlp`] — evolved multilayer perceptron (item 18b). `hidden`: the
    /// width of each **hidden layer** (designer data, editable). The input
    /// (perception channels) and the output (`OUTPUTS`: the steering vector + the
    /// eat/attack intent) are fixed by the contract → only the hidden topology is
    /// free (variable/NEAT topology remains deferred, §2).
    Mlp { hidden: Vec<usize> },
}

impl Default for BrainKind {
    /// Wandering at the archetype rate: the pre-item-16 default (scenarios that do
    /// not mention `brain` stay wandering worlds).
    fn default() -> Self {
        BrainKind::Wander {
            turn_rate: WanderBrain::DEFAULT_TURN_RATE,
        }
    }
}

impl BrainKind {
    /// Compiles the choice into a fresh brain. `seed` seeds the stateful brains
    /// (wandering; the MLP's *random* initial weights); `heading` the wandering;
    /// `n_inputs` (= number of perception channels, `3 × vision_rays`) sizes the
    /// MLP's input layer. The hunter ignores everything, wandering ignores `n_inputs`.
    pub fn build(&self, seed: u64, heading: f32, n_inputs: usize) -> Brain {
        match self {
            BrainKind::Wander { turn_rate } => {
                Brain::Wander(WanderBrain::new(seed, heading, *turn_rate))
            }
            BrainKind::Hunter => Brain::Hunter(HunterBrain),
            BrainKind::Grazer { hunger_threshold } => {
                Brain::Grazer(GrazerBrain::new(*hunger_threshold))
            }
            BrainKind::Sessile => Brain::Sessile(SessileBrain),
            BrainKind::Mlp { hidden } => Brain::Mlp(MlpBrain::random(seed, n_inputs, hidden)),
        }
    }

    /// Short label of the type, for the editor selector (item 15).
    pub fn name(&self) -> &'static str {
        match self {
            BrainKind::Wander { .. } => "Wander",
            BrainKind::Hunter => "Hunter",
            BrainKind::Grazer { .. } => "Grazer",
            BrainKind::Sessile => "Sessile",
            BrainKind::Mlp { .. } => "Network (MLP)",
        }
    }

    /// *Functional* description of the brain — how it decides, not just its name —
    /// shown by the editor selector. The **heterogeneous** counterpart of
    /// [`name`](Self::name): the exhaustive `match` forces every future variant to
    /// describe itself.
    pub fn description(&self) -> &'static str {
        match self {
            BrainKind::Wander { .. } => {
                "Random heading drift every tick: ignores perception, forages at \
                 random. The naive control group."
            }
            BrainKind::Hunter => {
                "Steering field: attracted toward the nearest visible target, AND \
                 repelled by any threat (a species that can attack it) — relation \
                 table. Skirts walls and conspecifics without fleeing them; with no \
                 memory, out of range it explores. The competent control group."
            }
            BrainKind::Grazer { .. } => {
                "Forages exactly like the hunter, but eats DELIBERATELY: it holds its \
                 eat/attack intent only while hungry (energy below its threshold), so a \
                 prudent grazer leaves food uneaten once sated. The deterministic \
                 control for behavioural restraint — a lever of ecosystem persistence \
                 under spatial viscosity."
            }
            BrainKind::Sessile => {
                "Does not decide, does not move: the brain of flora. Lives on \
                 photosynthesis (a gene) and reproduces by local seeding — the rest \
                 of the ecology (growth, dispersal, competition) emerges from the \
                 genes and the relation table."
            }
            BrainKind::Mlp { .. } => {
                "Evolved multilayer perceptron: decides by reading its \
                 vision/target/threat channels, and LEARNS through neuroevolution \
                 (Gaussian mutation of the weights at reproduction) — including to \
                 flee. Editable hidden layers; input and output fixed by the contract."
            }
        }
    }
}

/// Wandering by steering: the heading drifts by a small random increment every
/// tick, producing plausible curved trajectories.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WanderBrain {
    rng: Rng,
    /// Current heading, in radians.
    heading: f32,
    /// Maximum amplitude of the heading drift per tick, in radians.
    turn_rate: f32,
}

impl WanderBrain {
    /// Default amplitude of the heading drift (rad/tick) — the `Wander` variant's
    /// archetype value when the scenario does not specify one.
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
            // A reflex forager: it eats on contact as before (byte-identical). Only
            // the MLP *decides* whether to act (deliberate eating).
            act: 1.0,
        }
    }
}

/// **Deterministic** hunter (item 16, extended by *active flight*): no state, no
/// RNG — the same perception always gives the same action.
///
/// **Two modes** within a single ray-based steering field, selected by
/// *subsumption* (§4 — a survival reflex short-circuits the foraging layer):
///
/// 1. **Foraging** (item 16, unchanged), as long as no threat is *close*: each ray
///    pushes toward its direction with a weight `ATTRACTION · target + (1 −
///    obstacle)`. A target attracts (`ATTRACTION > 1`, more than open space); a
///    neutral obstacle (wall, conspecific) does not push toward it (weight ≈ 0) →
///    we skirt it; in empty terrain, the symmetric fan resolves forward (straight
///    sweep).
/// 2. **Flight**, as soon as a threat exceeds [`FLEE_THRESHOLD`] in proximity:
///    survival prevails, foraging is *suspended*. Each ray pushes the **opposite**
///    way with a weight `REPULSION · threat + obstacle` → we move away from threats
///    (heavily weighted) AND obstacles (walls), no longer letting a target attract
///    us. The threshold avoids two pitfalls: a repulsion simply *added* to foraging
///    never overturns the forward-fan for a distant threat (it weighs only one ray
///    against the whole open field), and fleeing *every* visible threat would
///    starve a prey that a predator merely overflies from afar. We therefore flee
///    only what is close **enough** to be dangerous.
///
/// Thus the **same** shared brain, depending on the relation table (§3) read *by
/// the perceiving species*, makes a prey a forager **that bolts** when its predator
/// approaches (`target` channel toward the plants, `threat` channel from the
/// predator) and an apex predator a pure hunter (no threat → foraging mode alone →
/// the item 16 behavior, unchanged) — the exact counterpart, on the flight side, of
/// the item 17 insight. It is the **competent control group** (§4): the same energy
/// expenditure as a wanderer, but it *finds* its food and *avoids* its predators.
/// It cannot, however, memorize: out of range, it merely explores (an MLP, for its
/// part, will be able to learn better — that is the stake).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HunterBrain;

impl HunterBrain {
    /// Over-weighting of a target against open space (`> 1` so a ray pointing at a
    /// target wins over a ray that is merely free).
    const ATTRACTION: f32 = 2.5;
    /// Over-weighting of a threat in **flight** (repulsion). `> ATTRACTION`: once
    /// flight is triggered, moving away clearly dominates obstacle avoidance.
    const REPULSION: f32 = 3.0;
    /// Threat proximity (in `[0, 1]`) beyond which flight short-circuits foraging
    /// (subsumption). Below it — predator still distant — foraging continues; above
    /// it — predator close enough to be dangerous — the prey bolts. `0.35` ≈ "the
    /// predator has entered the near third of my vision".
    const FLEE_THRESHOLD: f32 = 0.35;

    /// The **shared steering field** (foraging + flight-by-subsumption), returning
    /// the desired world direction. Extracted so the prudent [`GrazerBrain`] reuses
    /// the **exact** hunter locomotion and differs only in *when it eats*: the hunter
    /// and the grazer forage identically; only the `act` gate differs. Moving the
    /// computation verbatim keeps the hunter **byte-identical** (its unit tests guard
    /// this).
    fn steer(perception: &Perception) -> Vec2 {
        // Subsumption (§4): a CLOSE enough threat switches to flight, which
        // *suspends* foraging. A distant predator (below the threshold) interrupts
        // nothing → the item 16 foraging mode stays strictly intact for scenarios
        // without threats.
        let fleeing = perception.threat.iter().any(|&t| t > Self::FLEE_THRESHOLD);
        let mut steer = Vec2::ZERO;
        for i in 0..perception.ray_dirs.len() {
            let weight = if fleeing {
                // Move away from everything: threats (× REPULSION) AND obstacles
                // (walls, conspecifics) — a negative weight, with no attraction (we
                // do not forage while fleeing).
                -(Self::REPULSION * perception.threat[i] + perception.vision[i])
            } else {
                // Foraging field (item 16): attraction of targets + open space.
                Self::ATTRACTION * perception.target[i] + (1.0 - perception.vision[i])
            };
            steer += perception.ray_dirs[i] * weight;
        }
        let dir = steer.normalize_or_zero();
        // Surrounded (all occluded) or blind (zero rays): we keep the heading.
        if dir == Vec2::ZERO {
            perception.heading
        } else {
            dir
        }
    }

    fn think(&self, perception: &Perception) -> Action {
        // Reflex: the hunter always holds the intent (`act: 1.0`) — it eats/attacks on
        // contact as before (byte-identical). The grazer reuses `steer` but gates the
        // intent on hunger (deliberate eating).
        Action {
            dir: Self::steer(perception),
            throttle: 1.0,
            act: 1.0,
        }
    }
}

/// **Grazer** brain — the deterministic control for behavioural **restraint**
/// (`docs/persistent-ecosystems.md` §2). It forages with the **exact** hunter steering
/// field ([`HunterBrain::steer`]) — same locomotion, same flight reflex — and differs
/// in a single respect: *when it eats*. Where the hunter reflexively holds `act = 1.0`
/// (eat whatever is in range), a grazer holds its eat/attack intent only while
/// **hungry** — its energy reserve fraction (proprioception,
/// [`Perception::self_state`]`[0]`) at or below `hunger_threshold` — and abstains once
/// sated, leaving the resource it does not need.
///
/// The threshold *is* the strategy: `1.0` is **greedy** (the energy fraction is always
/// ≤ 1, so it eats exactly like the hunter), a lower value is **prudent** (stops eating
/// earlier, preserving the local food). Prudence is *individually costly* (less energy,
/// slower reproduction) yet *collectively* stabilising — and it becomes selectable only
/// under **spatial viscosity** (limited `seed_dispersal`, so a lineage inherits the
/// patch it preserved or exhausted; §2, item 17's spatial lesson). No state, no RNG —
/// like the other hand-written brains, so it draws nothing at reproduction (the non-MLP
/// stream stays intact).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GrazerBrain {
    /// Eat while the energy reserve fraction is at or below this, in `[0, 1]`. `1.0` =
    /// greedy (the hunter's reflex), lower = prudent. A designer strategy fixed at the
    /// founder — **not** a mutated gene (the evolvable `hunger_threshold` gene is
    /// deferred, §9).
    hunger_threshold: f32,
}

impl GrazerBrain {
    /// Default appetite for a freshly-created grazer (editor): moderately **prudent**
    /// — eats until roughly two-thirds full, then coasts. A visible contrast with the
    /// greedy reflex, without starving the lineage.
    pub const DEFAULT_HUNGER: f32 = 0.6;

    pub fn new(hunger_threshold: f32) -> Self {
        Self { hunger_threshold }
    }

    fn think(&self, perception: &Perception) -> Action {
        // Deliberate eating (SIM Law 8), gated on proprioception: hold the intent only
        // while hungry (energy fraction ≤ threshold). `self_state[0]` is the energy
        // reserve fraction (`perceive`; `SELF_LABELS[0] == "nrg"`). A sated grazer sets
        // `act ≤ 0`, so `interact` makes it abstain from ALL its interactions this tick
        // — it leaves the food it does not need. The steering is the hunter's,
        // unchanged: it still moves toward food and flees threats; it simply does not
        // *draw* when comfortable.
        let energy = perception.self_state[0];
        let act = if energy <= self.hunger_threshold {
            1.0
        } else {
            -1.0
        };
        Action {
            dir: HunterBrain::steer(perception),
            throttle: 1.0,
            act,
        }
    }
}

/// **Sessile** brain (Phase 3): the no-op of flora. No state, no RNG, does not read
/// perception — it always returns a **zero** throttle (the entity does not move).
/// It is a behavior shell ("stub the behavior, never the schema", §8): the plant
/// exists as a full-fledged entity (genotype, energy, reproduction), only its
/// *decision* is trivial.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessileBrain;

impl SessileBrain {
    fn think(&self, perception: &Perception) -> Action {
        // Immobile: we keep the heading (with no effect, the throttle being zero)
        // rather than an arbitrary vector.
        Action {
            dir: perception.heading,
            throttle: 0.0,
            // A plant "acts" reflexively (`1.0`, not `0.0`): a flora's Plant→Plant
            // self-competition is a *sessile actor* relation (§3), and gating it off
            // would break flora self-limitation (`flora.ron`). Deliberate eating is an
            // MLP capability, not a sessile one → byte-identical.
            act: 1.0,
        }
    }
}

/// A dense layer: `out × in` weights (row-major, `weights[o*inputs + i]`) + `out`
/// biases. Pre-activation of output neuron `o`: `bias[o] + Σ_i w[o,i]·in[i]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Layer {
    /// Fan-in (size of the expected input vector).
    inputs: usize,
    /// Flattened weights, `outputs × inputs` in row-major.
    weights: Vec<f32>,
    /// Biases, one per output neuron (its length = number of outputs).
    biases: Vec<f32>,
}

impl Layer {
    fn outputs(&self) -> usize {
        self.biases.len()
    }

    /// Random initial weights, Xavier-style (std `1/√fan_in`) to avoid `tanh`
    /// saturation from the start; zero biases.
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

    /// Propagation `tanh(bias + W·in)` **into a reused buffer** (`out` is cleared and
    /// refilled); `input.len()` must equal `self.inputs`. Writing into a caller-owned
    /// buffer is what lets the hot path ([`MlpBrain::think`]) avoid a per-layer
    /// allocation. Same values, same order as the allocating [`forward`](Self::forward).
    fn forward_into(&self, input: &[f32], out: &mut Vec<f32>) {
        out.clear();
        out.reserve(self.outputs());
        for o in 0..self.outputs() {
            let row = &self.weights[o * self.inputs..(o + 1) * self.inputs];
            let sum = self.biases[o] + row.iter().zip(input).map(|(w, x)| w * x).sum::<f32>();
            out.push(sum.tanh());
        }
    }

    /// Allocating propagation — a thin wrapper over [`forward_into`](Self::forward_into)
    /// for the on-demand paths (the inspector's [`MlpBrain::forward_activations`]) that
    /// keep one vector per layer; the per-tick `think` uses the buffer form instead.
    fn forward(&self, input: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        self.forward_into(input, &mut out);
        out
    }

    /// Child layer: each weight and bias perturbed by Gaussian noise of std-dev
    /// `std` (neuroevolution, mutation-only).
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

/// Homemade multilayer perceptron (item 18b) — the **learned** brain. The same
/// `Perception → Action` contract as any brain (§2), but its decision is not
/// hand-written: it emerges from the weights, which selection shapes by mutation
/// at reproduction ([`MlpBrain::mutated`]). Homemade by design: ML libs aim at the
/// big GPU network, the opposite of the need (§5).
///
/// **Input**: the per-ray exteroceptive channels `vision`, `target` then `threat`
/// concatenated (`CHANNELS × vision_rays`), followed by the scalar **proprioceptive**
/// channels (`self_state` — energy, nutrient, speed; [`Perception::SELF_CHANNELS`]),
/// hence `CHANNELS × vision_rays + SELF_CHANNELS` (it ignores the `ray_dirs` geometry,
/// cf. `components.rs`). The `threat` channel (active flight, item 18e) was first
/// validated **on the deterministic hunter** before being entrusted to the learned
/// one — exactly like `target` (introduced on the hunter at item 16, then consumed
/// by the MLP at item 18b): the learned brain now therefore receives what it needs
/// to *learn* to flee, where the hunter applies a hard-wired reflex. The
/// proprioceptive channels let it modulate on its own state (eat when hungry — the
/// substrate for restraint, `docs/persistent-ecosystems.md` §2). **Output**: 3
/// neurons — the first 2 a steering vector *in body frame*, rotated to the world by
/// the heading → orientation-equivariant (the network need not learn the absolute
/// orientation, as the hunter reads "ray i" relative to the heading); the 3rd the
/// **eat/attack intent** ([`crate::components::Action::act`], gated in `interact`),
/// the *deliberate* half of the interaction primitive (SIM Law 8) that pairs with
/// the proprioceptive input to make **restraint** expressible.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MlpBrain {
    /// Dense layers, input→output. Hidden topology frozen at construction
    /// (inherited as-is); the input layer, for its part, can be resized at
    /// reproduction when the child's visual precision changes (cf.
    /// [`MlpBrain::reproduced`]); the weights mutate. The **only state** of the
    /// brain: its equality and its serialization carry only the topology + the
    /// weights (no transient activations — these are recomputed on demand, cf.
    /// [`MlpBrain::forward_activations`]).
    layers: Vec<Layer>,
}

impl MlpBrain {
    /// Number of output neurons: the egocentric steering vector (x, y) **plus** the
    /// eat/attack intent ([`crate::components::Action::act`], item "deliberate
    /// eating"). Growing this widens the network's last layer (more Xavier draws) →
    /// MLP scenarios shift and their captures must be regenerated, exactly as
    /// widening the *input* did for threat/proprioception.
    pub const OUTPUTS: usize = 3;
    /// Scale of a weight-mutation step (multiplied by `mutation_rate`).
    const WEIGHT_STEP: f32 = 0.6;
    /// Number of **per-ray** perception channels wired into the input: `vision`
    /// (obstacle), `target` (attracting target) and `threat` (fleeing threat). The
    /// per-ray part of the input is therefore `CHANNELS × vision_rays` — and its
    /// resizing at reproduction respects these `CHANNELS` blocks (cf.
    /// [`MlpBrain::resize_input_fan`]). The **proprioceptive** channels
    /// ([`Perception::SELF_CHANNELS`]) are appended after them, scalar and
    /// ray-count-independent.
    const CHANNELS: usize = 3;

    /// Abbreviated names of the input perception channels, in input-vector order — one
    /// block of `vision_rays` each (`input = CHANNELS × vision_rays`, cf.
    /// [`MlpBrain::input_vector`]). The **single source of truth** the two network-graph
    /// renderers (the inspector's egui graph and the video visualizer's gizmo graph)
    /// label the input column from, so a contract change (a renamed/added channel)
    /// updates every view at once. The array length is pinned to `CHANNELS`, so changing
    /// the channel count without the labels is a compile error.
    pub const CHANNEL_LABELS: [&'static str; Self::CHANNELS] = ["vis", "tgt", "thr"];
    /// Abbreviated names of the **proprioceptive** input channels, appended after the
    /// per-ray blocks in input-vector order (cf. [`Perception::self_state`] and
    /// `perceive`): energy reserve, nutrient store, speed. Same single-source role as
    /// [`CHANNEL_LABELS`](Self::CHANNEL_LABELS) for the graph renderers; length pinned
    /// to [`Perception::SELF_CHANNELS`], so changing the self-channel count without the
    /// labels is a compile error.
    pub const SELF_LABELS: [&'static str; Perception::SELF_CHANNELS] = ["nrg", "nut", "spd"];
    /// Names of the output neurons, in order: the body-frame steering vector (forward,
    /// side) then the eat/attack intent, cf. [`MlpBrain::think`]. The output-column
    /// counterpart of [`CHANNEL_LABELS`](Self::CHANNEL_LABELS) — same single-source role,
    /// length pinned to `OUTPUTS`.
    pub const OUTPUT_LABELS: [&'static str; Self::OUTPUTS] = ["fwd", "side", "act"];

    /// Size of the input layer: the per-ray `vision`, `target` AND `threat` channels
    /// (`CHANNELS × vision_rays`), then the scalar **proprioceptive** channels
    /// ([`Perception::SELF_CHANNELS`]), then the `n_sensed` **field-sense** channels
    /// (pheromones, [`Perception::field_state`]) — hence `CHANNELS × vision_rays +
    /// SELF_CHANNELS + n_sensed`. The two scalar tails are ray-count-independent (carried
    /// over unchanged when visual precision drifts, cf. [`resize_input_fan`]).
    pub fn input_size(vision_rays: usize, n_sensed: usize) -> usize {
        Self::CHANNELS * vision_rays + Perception::SELF_CHANNELS + n_sensed
    }

    /// Network with **random** weights: dims = `[n_inputs] ++ hidden ++ [OUTPUTS]`.
    /// The founders of an MLP species thus start from random brains — it is
    /// evolution that must *discover* foraging (the whole stake of item 18b).
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

    /// Child: **same topology**, perturbed weights (neuroevolution). `rate` =
    /// the genotype's `mutation_rate` (the per-lineage gene, §2).
    pub fn mutated(&self, rng: &mut Rng, rate: f32) -> Self {
        let std = rate * Self::WEIGHT_STEP;
        Self {
            layers: self.layers.iter().map(|l| l.mutated(rng, std)).collect(),
        }
    }

    /// Child at reproduction: like [`MlpBrain::mutated`] (hidden topology
    /// inherited, weights mutated), **but** the input layer is first resized to
    /// `n_inputs` if the child's visual precision differs from the parent's (gene
    /// `vision_rays`, item 3). This is the first step toward a variable topology.
    ///
    /// When `n_inputs` is unchanged, no resizing draw occurs and the result
    /// coincides bit-for-bit with `mutated` (the RNG stream of fixed-precision
    /// scenarios preserved).
    pub fn reproduced(&self, rng: &mut Rng, rate: f32, n_inputs: usize, n_sensed: usize) -> Self {
        let std = rate * Self::WEIGHT_STEP;
        let layers = self
            .layers
            .iter()
            .enumerate()
            .map(|(idx, layer)| {
                // Only the first layer sees the perception vector: it is the only
                // one whose fan-in depends on the number of rays.
                let adapted = if idx == 0 && layer.inputs != n_inputs {
                    Self::resize_input_fan(layer, rng, n_inputs, n_sensed)
                } else {
                    layer.clone()
                };
                adapted.mutated(rng, std)
            })
            .collect();
        Self { layers }
    }

    /// Resizes the input layer's fan-in to `n_inputs`, **respecting the input
    /// layout**: `CHANNELS` per-ray blocks (`vision`, `target`, `threat`, each of
    /// `rays` channels) followed by the [`SELF_CHANNELS`](Perception::SELF_CHANNELS)
    /// scalar proprioceptive channels (cf. [`MlpBrain::input_vector`]). Only the ray
    /// count changes at reproduction (gene `vision_rays`): each per-ray block is
    /// truncated (child sees less finely) or padded with fresh Xavier-style weights
    /// (child sees more finely), so the kept weights stay **aligned on the right
    /// channel**; the self block is **ray-count-independent** and its weights are
    /// carried over **unchanged**. The biases (per output neuron) are unchanged.
    fn resize_input_fan(layer: &Layer, rng: &mut Rng, n_inputs: usize, n_sensed: usize) -> Layer {
        let outputs = layer.outputs();
        let old_in = layer.inputs;
        // The scalar tail (proprioception + field-sense) is ray-count-independent; strip
        // it to recover the per-ray counts on both sides. `n_sensed` is per-species
        // constant, so the tail size is the same on parent and child.
        let tail = Perception::SELF_CHANNELS + n_sensed;
        let old_rays = (old_in - tail) / Self::CHANNELS;
        let new_rays = (n_inputs - tail) / Self::CHANNELS;
        let old_tail_start = Self::CHANNELS * old_rays; // where the scalar tail begins
        let scale = 1.0 / (n_inputs.max(1) as f32).sqrt();
        let mut weights = Vec::with_capacity(n_inputs * outputs);
        for o in 0..outputs {
            let row = &layer.weights[o * old_in..(o + 1) * old_in];
            // One block per per-ray channel (vision, target, threat), each `old_rays` wide.
            for block in 0..Self::CHANNELS {
                let block_start = block * old_rays;
                for r in 0..new_rays {
                    weights.push(if r < old_rays {
                        row[block_start + r]
                    } else {
                        rng.next_gaussian() * scale
                    });
                }
            }
            // The scalar tail (proprioception + field-sense): fixed size, carried over
            // unchanged (its channels do not depend on the ray count).
            for s in 0..tail {
                weights.push(row[old_tail_start + s]);
            }
        }
        Layer {
            inputs: n_inputs,
            weights,
            biases: layer.biases.clone(),
        }
    }

    /// Input vector: the per-ray `vision`, `target` then `threat` blocks (`CHANNELS`
    /// blocks of `rays`, the same channels the inspector displays, in the same order),
    /// then the scalar **proprioceptive** channels (`self_state`) appended at the tail
    /// — matching [`input_size`](Self::input_size) and the [`resize_input_fan`] layout.
    fn input_vector(perception: &Perception) -> Vec<f32> {
        let mut v = Vec::new();
        Self::fill_input(perception, &mut v);
        v
    }

    /// Fills `out` (cleared first) with the input vector — same channels, same order as
    /// [`input_vector`](Self::input_vector). The buffer form the per-tick `think` uses
    /// to avoid allocating the input vector every tick.
    fn fill_input(perception: &Perception, out: &mut Vec<f32>) {
        out.clear();
        out.extend(perception.vision.iter().copied());
        out.extend(perception.target.iter().copied());
        out.extend(perception.threat.iter().copied());
        out.extend(perception.self_state.iter().copied());
        out.extend(perception.field_state.iter().copied());
    }

    fn think(&self, perception: &Perception) -> Action {
        // Reuse two thread-local buffers (`cur`/`next`, ping-ponged by `swap`) so the
        // per-tick forward pass allocates **nothing** — the input vector and each
        // layer's output are written into reused storage. Thread-local: `decide` may
        // run on any worker thread and each gets its own buffers (no sharing, no lock).
        // Purely an allocation optimisation — the computed values are byte-identical to
        // the allocating path (`forward` == `forward_into` into a fresh Vec).
        THINK_SCRATCH.with_borrow_mut(|(cur, next)| {
            Self::fill_input(perception, cur);
            for layer in &self.layers {
                // Robust to a wrongly-sized perception (shape changed between runs): if
                // the fan-in does not match, keep the heading (network mute this tick).
                if cur.len() != layer.inputs {
                    return Self::mute(perception);
                }
                layer.forward_into(cur, next);
                std::mem::swap(cur, next);
            }
            // A stale capture serialized before the output grew to `OUTPUTS` would have
            // a narrower last layer → reading `cur[2]` below would panic. Go **inert**
            // (mute) rather than crash; the committed captures are regenerated under the
            // new contract, so this only shields a hand-loaded old brain.
            if cur.len() < Self::OUTPUTS {
                return Self::mute(perception);
            }
            // Outputs 0-1 = steering vector in body frame, rotated to the world by the
            // heading (the body's +X points toward `heading`); output 2 = the eat/attack
            // intent (`Action::act`), the *deliberate* half of the primitive (SIM Law 8).
            let body = Vec2::new(cur[0], cur[1]);
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
                act: cur[2],
            }
        })
    }

    /// A mute action (keep the heading, no throttle, **no** eat intent) for when the
    /// network cannot run this tick — a perception of the wrong fan-in or a stale,
    /// too-narrow capture (cf. [`MlpBrain::think`]).
    fn mute(perception: &Perception) -> Action {
        Action {
            dir: perception.heading,
            throttle: 0.0,
            act: 0.0,
        }
    }

    /// Replays the propagation to expose the activations layer by layer (input
    /// included): `[input, h0, …, output]`. For the inspector's **visualization**
    /// (item 18b-viz), computed **on demand** for the single inspected agent — the
    /// sim core's `think` therefore no longer bears this cost (a clone per layer ×
    /// agent × tick, useless in headless/`record`). Deterministic: same weights +
    /// same perception ⇒ same activations as the last `think`. Stops (truncated
    /// vector) if the perception does not have the right fan-in — the inspector then
    /// colors the remaining nodes neutral.
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

    /// Layer sizes for drawing the graph (item 18b-viz), **input included**:
    /// `[n_inputs, h0, …, OUTPUTS]`. One column of nodes per size.
    pub fn layer_sizes(&self) -> Vec<usize> {
        let mut sizes = Vec::with_capacity(self.layers.len() + 1);
        if let Some(first) = self.layers.first() {
            sizes.push(first.inputs);
        }
        sizes.extend(self.layers.iter().map(Layer::outputs));
        sizes
    }

    /// Number of **decision neurons**: hidden + output units. The input layer is
    /// excluded — it is the sensory afferent vector (`CHANNELS × rays`), already
    /// taxed by the vision cost ([`crate::components::Vision::metabolic_cost`]);
    /// counting it here would double-charge it. Each [`Layer`]'s `outputs()` is one
    /// such neuron, and the input is the output of no layer, so the sum is exactly
    /// hidden + output. Drives the brain's metabolic cost (gene `brain_cost`).
    pub fn neuron_count(&self) -> usize {
        self.layers.iter().map(Layer::outputs).sum()
    }

    /// Number of weight layers (= number of edge inter-columns to draw).
    pub fn weight_layers(&self) -> usize {
        self.layers.len()
    }

    /// Weights of layer `l` (`outputs × inputs`, row-major) + its dimensions, to
    /// draw the weighted edges (item 18b-viz). `l < weight_layers()`.
    pub fn layer_weights(&self, l: usize) -> (&[f32], usize, usize) {
        let layer = &self.layers[l];
        (&layer.weights, layer.inputs, layer.outputs())
    }

    /// Biases of layer `l` (one per output neuron), to **size the graph's nodes**
    /// by their bias (item 18b-viz). `l < weight_layers()`. Layer `l` feeds the
    /// **column `l+1`** of the graph; the input column (0) has no bias.
    pub fn layer_biases(&self, l: usize) -> &[f32] {
        &self.layers[l].biases
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The graph renderers' channel/output labels stay pinned to the MLP I/O contract:
    /// one input label per perception channel (`input_size(1, 0) == CHANNELS`) and one
    /// output label per output neuron. (The array lengths are already `CHANNELS` /
    /// `OUTPUTS` at compile time; this guards the relationship through the public API.)
    #[test]
    fn contract_label_arrays_match_io_arity() {
        // One per-ray label per per-ray channel + one self label per self channel =
        // the full single-ray input width.
        assert_eq!(
            MlpBrain::CHANNEL_LABELS.len() + MlpBrain::SELF_LABELS.len(),
            MlpBrain::input_size(1, 0)
        );
        assert_eq!(MlpBrain::SELF_LABELS.len(), Perception::SELF_CHANNELS);
        assert_eq!(MlpBrain::OUTPUT_LABELS.len(), MlpBrain::OUTPUTS);
    }

    /// Three rays: left (+Y), forward (+X), right (-Y) — a symmetric fan around the
    /// +X heading, as `perceive` would produce them. The three channels (obstacle,
    /// target, threat) are provided explicitly.
    fn perception(vision: [f32; 3], target: [f32; 3], threat: [f32; 3]) -> Perception {
        Perception {
            heading: Vec2::X,
            vision: vision.into(),
            target: target.into(),
            threat: threat.into(),
            self_state: [0.0; Perception::SELF_CHANNELS],
            field_state: Box::default(),
            ray_dirs: vec![Vec2::Y, Vec2::X, Vec2::NEG_Y].into_boxed_slice(),
        }
    }

    /// Two visible targets → steering leans toward the nearer one (-Y, proximity
    /// 0.9) rather than the far one (+Y, 0.3): attraction is graded by proximity.
    #[test]
    fn hunter_favors_the_nearer_target() {
        let p = perception([0.3, 0.0, 0.9], [0.3, 0.0, 0.9], [0.0, 0.0, 0.0]);
        let action = HunterBrain.think(&p);
        assert!(
            action.dir.y < 0.0,
            "leans toward the nearest target (-Y), dir={:?}",
            action.dir
        );
        assert_eq!(action.throttle, 1.0);
    }

    /// A **target** on one side (+Y) and a **non-target obstacle** (wall) on the
    /// other (-Y), at equal proximity → the hunter goes toward the target and moves
    /// away from the wall. This is the fix: it no longer flees food as an obstacle.
    #[test]
    fn hunter_approaches_target_not_plain_obstacle() {
        // +Y: food (vision == target); -Y: wall (vision without target).
        let action = HunterBrain.think(&perception([0.6, 0.0, 0.6], [0.6, 0.0, 0.0], [0.0; 3]));
        assert!(
            action.dir.y > 0.0,
            "must go toward the target (+Y), not the wall (-Y), dir={:?}",
            action.dir
        );
    }

    /// No target but an obstacle on the left (+Y) → the resultant leans the other
    /// way (toward -Y): the hunter moves away from the wall.
    #[test]
    fn hunter_steers_toward_open_space_when_no_target() {
        let action = HunterBrain.think(&perception([0.9, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0; 3]));
        assert!(
            action.dir.y < 0.0,
            "must move away from the obstacle at +Y, dir={:?}",
            action.dir
        );
    }

    /// Fully open terrain: symmetric pushes → heading kept forward.
    #[test]
    fn hunter_cruises_forward_in_the_open() {
        let action = HunterBrain.think(&perception([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0; 3]));
        assert!(
            action.dir.x > 0.9,
            "must go straight ahead, dir={:?}",
            action.dir
        );
        assert!(action.dir.y.abs() < 1e-6);
    }

    /// The target prevails over avoidance: even hemmed in by obstacles, a hunter
    /// that sees a target goes for it (the hunting reflex short-circuits exploration).
    #[test]
    fn target_takes_priority_over_avoidance() {
        let action = HunterBrain.think(&perception([0.8, 0.8, 0.8], [0.0, 0.8, 0.0], [0.0; 3]));
        assert_eq!(action.dir, Vec2::X, "the centered target wins");
    }

    /// Determinism: two evaluations of the same perception give the same action (no
    /// state, no RNG — which is what makes it a reproducible control group).
    #[test]
    fn hunter_is_deterministic() {
        let p = perception([0.1, 0.4, 0.2], [0.0, 0.4, 0.0], [0.0; 3]);
        assert_eq!(HunterBrain.think(&p).dir, HunterBrain.think(&p).dir);
    }

    /// ACTIVE FLIGHT — a **threat** straight ahead (+X), clear sides: the forward
    /// ray takes a negative weight (repulsion) → steering **turns back** (dir.x <
    /// 0). The exact mirror of `hunter_cruises_forward_in_the_open`, but where the
    /// forward obstacle is a threat instead of a void.
    #[test]
    fn hunter_flees_a_threat_ahead() {
        // +X: a close threat (and therefore also a hit, vision 0.6); clear sides.
        let action = HunterBrain.think(&perception([0.0, 0.6, 0.0], [0.0; 3], [0.0, 0.6, 0.0]));
        assert!(
            action.dir.x < 0.0,
            "must turn back facing the threat, dir={:?}",
            action.dir
        );
    }

    /// FLIGHT by subsumption — a **close threat** on the left flank (+Y, 0.8 >
    /// threshold) AND a **target** straight ahead (+X): flight *suspends* foraging.
    /// The prey clearly moves away from the threat (dir.y < 0) and **no longer
    /// advances** toward its target (dir.x < 0) — survival short-circuits foraging as
    /// long as the danger lasts.
    #[test]
    fn flight_suspends_foraging() {
        // +Y: close threat (0.8 > FLEE_THRESHOLD); +X: edible target (0.6); -Y: clear.
        let action = HunterBrain.think(&perception(
            [0.8, 0.6, 0.0],
            [0.0, 0.6, 0.0],
            [0.8, 0.0, 0.0],
        ));
        assert!(
            action.dir.y < 0.0,
            "must move away from the threat at +Y (go toward -Y), dir={:?}",
            action.dir
        );
        assert!(
            action.dir.x < 0.0,
            "flight: no longer forages toward the target at +X, dir={:?}",
            action.dir
        );
    }

    /// A **distant** threat (proximity 0.2 < threshold) does not trigger flight:
    /// foraging continues. Target straight ahead (+X, 0.6), weak threat on the flank
    /// (+Y, 0.2) → the prey pursues its target (dir.x > 0). It is this threshold that
    /// avoids starving a prey that a predator merely overflies from afar.
    #[test]
    fn distant_threat_does_not_interrupt_foraging() {
        // +X: target (0.6); +Y: DISTANT threat (0.2 < FLEE_THRESHOLD); -Y: clear.
        let action = HunterBrain.think(&perception(
            [0.2, 0.6, 0.0],
            [0.0, 0.6, 0.0],
            [0.2, 0.0, 0.0],
        ));
        assert!(
            action.dir.x > 0.0,
            "distant threat: must keep foraging toward +X, dir={:?}",
            action.dir
        );
    }

    /// The **sessile** brain (flora) produces no movement — zero throttle, whatever
    /// the perception — and inheritance carries the type over without touching the
    /// RNG stream (like the other deterministic brains).
    #[test]
    fn sessile_brain_never_moves_and_is_inherited() {
        let action = SessileBrain.think(&perception(
            [0.9, 0.2, 0.5],
            [0.0, 0.2, 0.0],
            [0.5, 0.0, 0.0],
        ));
        assert_eq!(action.throttle, 0.0, "a plant does not move");

        let mut rng = Rng::new(0);
        assert!(matches!(
            Brain::Sessile(SessileBrain).reproduce(1, 0.0, &mut rng, 0.1, 6, 0),
            Brain::Sessile(_)
        ));
        assert_eq!(rng, Rng::new(0), "the sessile does not draw from the RNG");
    }

    /// The `Wander` variant's specific parameter (turn_rate) is indeed passed to the
    /// compiled brain: it is what the editor selector (item 15) varies. (`n_inputs`
    /// only matters for the MLP; here 0.)
    #[test]
    fn brainkind_wander_carries_its_turn_rate() {
        match (BrainKind::Wander { turn_rate: 0.4 }).build(1, 0.0, 0) {
            Brain::Wander(w) => assert_eq!(w.turn_rate, 0.4),
            other => panic!("expected Wander, got {other:?}"),
        }
        assert!(matches!(BrainKind::default(), BrainKind::Wander { .. }));
        assert!(matches!(
            (BrainKind::Hunter).build(1, 0.0, 0),
            Brain::Hunter(_)
        ));
    }

    /// Brain inheritance (item 18a): a child **carries over the type** of its parent
    /// — this is what lets a deterministic control and a learned brain coexist (§4).
    /// The Wander inherits the parent's `turn_rate` (an archetype parameter) but
    /// receives a fresh RNG state; the Hunter, stateless, is cloned. Neither draws
    /// from `rng` (the non-MLP scenarios' stream stays intact).
    #[test]
    fn reproduce_keeps_the_parent_variant() {
        let mut rng = Rng::new(0);
        // Hunter → Hunter (deterministic, cloned). `n_inputs` ignored by Hunter.
        let hunter = Brain::Hunter(HunterBrain);
        assert!(matches!(
            hunter.reproduce(7, 1.0, &mut rng, 0.1, 6, 0),
            Brain::Hunter(_)
        ));

        // Wander → Wander, turn_rate inherited, distinct RNG state (seed ≠).
        let parent = Brain::Wander(WanderBrain::new(1, 0.0, 0.37));
        match parent.reproduce(2, 0.5, &mut rng, 0.1, 6, 0) {
            Brain::Wander(child) => {
                assert_eq!(child.turn_rate, 0.37, "the parent's turn_rate is inherited");
                let Brain::Wander(p) = &parent else {
                    unreachable!()
                };
                assert_ne!(child.rng, p.rng, "the child has a fresh RNG state");
            }
            other => panic!("expected Wander, got {other:?}"),
        }
        // Wander/Hunter did not consume `rng`: its state is the starting one.
        assert_eq!(
            rng,
            Rng::new(0),
            "non-MLP brains do not touch the RNG stream"
        );
    }

    /// The grazer **forages exactly like the hunter** (it reuses `HunterBrain::steer`):
    /// the same perception yields the same steering direction. Only *when it eats*
    /// differs — the whole point of the restraint control.
    #[test]
    fn grazer_forages_like_the_hunter() {
        // A near target on -Y (proximity 0.9), open elsewhere.
        let p = perception([0.3, 0.0, 0.9], [0.3, 0.0, 0.9], [0.0; 3]);
        let grazer = GrazerBrain::new(0.6);
        assert_eq!(
            grazer.think(&p).dir,
            HunterBrain.think(&p).dir,
            "the grazer's locomotion is the hunter's"
        );
    }

    /// **Deliberate eating gated on hunger** (the restraint substrate): a prudent
    /// grazer (threshold 0.6) holds its eat intent when hungry (energy 0.3 ≤ 0.6) and
    /// **abstains** when sated (energy 0.9 > 0.6) — reading its own proprioceptive
    /// `self_state[0]`. This is what lets it leave food uneaten, the behaviour whose
    /// selective value under spatial viscosity the scenario demonstrates.
    #[test]
    fn grazer_gates_eating_on_hunger() {
        let grazer = GrazerBrain::new(0.6);
        let mut hungry = perception([0.0, 0.0, 0.5], [0.0, 0.0, 0.5], [0.0; 3]);
        hungry.self_state[0] = 0.3;
        let mut sated = perception([0.0, 0.0, 0.5], [0.0, 0.0, 0.5], [0.0; 3]);
        sated.self_state[0] = 0.9;
        assert!(
            grazer.think(&hungry).act > 0.0,
            "a hungry grazer eats (holds the intent)"
        );
        assert!(
            grazer.think(&sated).act <= 0.0,
            "a sated grazer abstains (releases the intent)"
        );
    }

    /// A **greedy** grazer (threshold 1.0) is behaviourally the hunter: the energy
    /// fraction is always ≤ 1, so it always holds the intent — its `Action` matches the
    /// hunter's exactly, even at a full reserve. Greed is thus one end of the appetite
    /// axis, prudence the other.
    #[test]
    fn greedy_grazer_matches_the_hunter_reflex() {
        let mut full = perception([0.2, 0.0, 0.7], [0.2, 0.0, 0.7], [0.0; 3]);
        full.self_state[0] = 1.0; // full reserve
        let greedy = GrazerBrain::new(1.0);
        let h = HunterBrain.think(&full);
        let g = greedy.think(&full);
        assert_eq!(g.act, h.act, "the greedy grazer eats like the hunter");
        assert_eq!(g.dir, h.dir);
        assert_eq!(g.throttle, h.throttle);
    }

    /// Grazer inheritance: the child keeps the parent's appetite (a designer strategy,
    /// not mutated) and, like the other hand-written brains, **draws no RNG** at
    /// reproduction (the non-MLP stream stays intact). `BrainKind::Grazer` builds a
    /// grazer carrying its threshold, and a grazer has zero decision neurons (no cost).
    #[test]
    fn grazer_is_inherited_without_touching_the_rng() {
        let mut rng = Rng::new(0);
        match Brain::Grazer(GrazerBrain::new(0.6)).reproduce(1, 0.0, &mut rng, 0.1, 6, 0) {
            Brain::Grazer(child) => assert_eq!(child.hunger_threshold, 0.6),
            other => panic!("expected Grazer, got {other:?}"),
        }
        assert_eq!(rng, Rng::new(0), "the grazer does not draw from the RNG");

        match (BrainKind::Grazer {
            hunger_threshold: 0.4,
        })
        .build(1, 0.0, 0)
        {
            Brain::Grazer(g) => assert_eq!(g.hunger_threshold, 0.4),
            other => panic!("expected Grazer, got {other:?}"),
        }
        assert_eq!(Brain::Grazer(GrazerBrain::new(0.5)).neuron_count(), 0);
    }

    /// A 3-ray perception for the MLP tests: 12 inputs (vision ++ target ++ threat ++
    /// self_state), threat and self_state left at zero here (cf. `mlp_reads_threat_channel`
    /// for a non-zero threat case).
    fn mlp_perception(heading: Vec2, vision: [f32; 3], target: [f32; 3]) -> Perception {
        Perception {
            heading,
            vision: vision.into(),
            target: target.into(),
            threat: [0.0; 3].into(),
            self_state: [0.0; Perception::SELF_CHANNELS],
            field_state: Box::default(),
            ray_dirs: vec![Vec2::Y, Vec2::X, Vec2::NEG_Y].into_boxed_slice(),
        }
    }

    /// The **threat** channel is now wired into the MLP's input (item 18e →
    /// learned): two perceptions identical except for the `threat` channel produce
    /// **different** actions. It is the learned-side analogue of what the hunter does
    /// with `target` at item 16: the falsifiable proof that the channel is not
    /// ignored. (We do not prescribe *how* the random network responds to it — only
    /// that it does; learning to flee *well* is up to selection, as for foraging.)
    #[test]
    fn mlp_reads_threat_channel() {
        let brain = MlpBrain::random(7, MlpBrain::input_size(3, 0), &[6]);
        let (vision, target) = ([0.2, 0.7, 0.1], [0.0, 0.7, 0.0]);

        let calm = brain.think(&mlp_perception(Vec2::X, vision, target));
        let mut threatened = mlp_perception(Vec2::X, vision, target);
        threatened.threat = [0.6, 0.0, 0.0].into(); // a predator on the left flank
        let scared = brain.think(&threatened);

        assert_ne!(
            calm.dir, scared.dir,
            "the threat channel must influence the MLP's decision"
        );
    }

    /// The **proprioceptive** channels are now wired into the MLP's input (this
    /// increment): two perceptions identical except for `self_state` produce
    /// **different** actions. The self-referential analogue of
    /// `mlp_reads_threat_channel` — the falsifiable proof that the brain can sense
    /// its own internal state (the substrate for behavioural restraint,
    /// `docs/persistent-ecosystems.md` §2). We do not prescribe *how* the random
    /// network responds — only that it does; using the signal *well* (e.g. eat when
    /// hungry) is up to selection, exactly as for the exteroceptive channels.
    #[test]
    fn mlp_reads_self_state_channel() {
        let brain = MlpBrain::random(7, MlpBrain::input_size(3, 0), &[6]);
        let (vision, target) = ([0.2, 0.7, 0.1], [0.0, 0.7, 0.0]);

        let mut full = mlp_perception(Vec2::X, vision, target);
        full.self_state = [1.0, 0.0, 0.0]; // full energy reserve
        let mut empty = mlp_perception(Vec2::X, vision, target);
        empty.self_state = [0.0, 0.0, 0.0]; // depleted energy reserve

        assert_ne!(
            brain.think(&full).dir,
            brain.think(&empty).dir,
            "the proprioceptive channels must influence the MLP's decision"
        );
    }

    /// The **field-sense** channels are wired into the MLP's input (Phase 3, pheromones):
    /// two perceptions identical except for `field_state` produce **different** actions —
    /// the chemoreception analogue of `mlp_reads_self_state_channel`. Falsifiable proof
    /// the sensed local concentration reaches the decision (using it *well* — following
    /// or fleeing a trail — is up to selection). Emit + sense on a shared component is
    /// then a communication substrate whose meaning is evolved.
    #[test]
    fn mlp_reads_field_state_channel() {
        // One field-sense channel (n_sensed = 1): input = 3×rays + 3 self + 1 field.
        let brain = MlpBrain::random(7, MlpBrain::input_size(3, 1), &[6]);
        let (vision, target) = ([0.2, 0.7, 0.1], [0.0, 0.7, 0.0]);

        let mut quiet = mlp_perception(Vec2::X, vision, target);
        quiet.field_state = vec![0.0].into_boxed_slice(); // no pheromone here
        let mut scented = mlp_perception(Vec2::X, vision, target);
        scented.field_state = vec![0.8].into_boxed_slice(); // a strong local trail

        assert_ne!(
            brain.think(&quiet).dir,
            brain.think(&scented).dir,
            "the field-sense channel must influence the MLP's decision"
        );
    }

    /// The MLP now emits a **3rd output** — the eat/attack intent
    /// ([`crate::components::Action::act`], the *deliberate* half of the interaction
    /// primitive, SIM Law 8, gated in `interact`). Falsifiable proof it is a **live
    /// output** of the network, not a constant: two different perceptions yield
    /// different `act` values, and the scalar stays in `tanh`'s range (the gate reads
    /// its sign). The output-side analogue of the input-channel proofs above — we do
    /// not prescribe *when* the random network chooses to act, only that the intent is
    /// wired; learning to eat *well* is up to selection, as for steering.
    #[test]
    fn mlp_emits_act_output() {
        let brain = MlpBrain::random(7, MlpBrain::input_size(3, 0), &[6]);
        let a = brain.think(&mlp_perception(Vec2::X, [0.2, 0.7, 0.1], [0.0, 0.7, 0.0]));
        let b = brain.think(&mlp_perception(Vec2::X, [0.9, 0.1, 0.5], [0.5, 0.0, 0.3]));
        assert_ne!(
            a.act, b.act,
            "the act output must be a live function of perception"
        );
        assert!(
            a.act.is_finite() && a.act.abs() <= 1.0,
            "the act intent is a bounded tanh scalar (got {})",
            a.act
        );
    }

    /// The MLP built by `BrainKind` respects the I/O contract: input =
    /// `3 × vision_rays + SELF_CHANNELS` (vision ++ target ++ threat ++ self_state),
    /// output = `OUTPUTS`, hidden layers as requested.
    #[test]
    fn brainkind_mlp_builds_with_contract_io() {
        let n_inputs = MlpBrain::input_size(3, 0); // 12 = 3 channels × 3 rays + 3 self
        let Brain::Mlp(m) = (BrainKind::Mlp { hidden: vec![5] }).build(42, 0.0, n_inputs) else {
            panic!("expected an MLP");
        };
        assert_eq!(m.layers.len(), 2, "1 hidden + 1 output");
        assert_eq!(m.layers[0].inputs, 12, "input = 3 × rays + 3 self");
        assert_eq!(m.layers[0].outputs(), 5, "requested hidden layer");
        assert_eq!(m.layers[1].inputs, 5);
        assert_eq!(m.layers[1].outputs(), MlpBrain::OUTPUTS);
        // The visualization API (item 18b-viz) reflects the same topology.
        assert_eq!(m.layer_sizes(), vec![12, 5, MlpBrain::OUTPUTS]);
        assert_eq!(m.weight_layers(), 2);
        let (w, fan_in, fan_out) = m.layer_weights(0);
        assert_eq!((fan_in, fan_out), (12, 5));
        assert_eq!(w.len(), 12 * 5);
        // The biases (which size the graph's nodes): one per output neuron of each
        // layer, zero at construction (Xavier init).
        assert_eq!(m.layer_biases(0).len(), 5, "one bias per hidden neuron");
        assert_eq!(m.layer_biases(1).len(), MlpBrain::OUTPUTS);
        assert!(m.layer_biases(0).iter().all(|&b| b == 0.0));
    }

    /// The decision-neuron count (which drives the brain's metabolic cost) is
    /// hidden + output, input **excluded** (afferents already taxed by vision); and
    /// the hand-written brains, having no network, count zero.
    #[test]
    fn neuron_count_is_hidden_plus_output() {
        let n_inputs = MlpBrain::input_size(7, 0); // 24 inputs (7 rays × 3 + 3 self) — must NOT be counted
        let Brain::Mlp(m) = (BrainKind::Mlp { hidden: vec![8] }).build(42, 0.0, n_inputs) else {
            panic!("expected an MLP");
        };
        assert_eq!(
            m.neuron_count(),
            8 + MlpBrain::OUTPUTS,
            "hidden + output only"
        );

        // A bare perceptron (no hidden layer): just the output neurons.
        let Brain::Mlp(p) = (BrainKind::Mlp { hidden: vec![] }).build(1, 0.0, n_inputs) else {
            panic!("expected an MLP");
        };
        assert_eq!(p.neuron_count(), MlpBrain::OUTPUTS);

        // The wrapper exposes the same count, and the hand-written brains pay nothing.
        assert_eq!(Brain::Mlp(m).neuron_count(), 8 + MlpBrain::OUTPUTS);
        assert_eq!(Brain::Hunter(HunterBrain).neuron_count(), 0);
        assert_eq!(Brain::Sessile(SessileBrain).neuron_count(), 0);
        assert_eq!(
            Brain::Wander(WanderBrain::new(0, 0.0, 0.25)).neuron_count(),
            0
        );
    }

    /// The MLP is deterministic (same weights + same perception → same action) and
    /// **orientation-equivariant**: rotated by the same heading, the same channels
    /// give an action rotated by the same amount (the decision lives in body frame).
    #[test]
    fn mlp_is_deterministic_and_orientation_equivariant() {
        let brain = MlpBrain::random(7, MlpBrain::input_size(3, 0), &[6]);
        let (vision, target) = ([0.2, 0.7, 0.1], [0.0, 0.7, 0.0]);

        let a1 = brain.think(&mlp_perception(Vec2::X, vision, target));
        let a2 = brain.think(&mlp_perception(Vec2::X, vision, target));
        assert_eq!(a1.dir, a2.dir, "deterministic");
        assert_eq!(a1.throttle, a2.throttle);

        // Same relative perception, heading rotated by +90° → the action rotates by +90°.
        let a_y = brain.think(&mlp_perception(Vec2::Y, vision, target));
        let expected = Vec2::Y.rotate(a1.dir); // +90° rotation
        assert!(
            (a_y.dir - expected).length() < 1e-5,
            "the output must be in body frame: {:?} vs {:?}",
            a_y.dir,
            expected
        );
    }

    /// Neuroevolution: mutating **perturbs the weights** but **keeps the topology**;
    /// a zero rate is the identity (evolution-off regime).
    #[test]
    fn mlp_mutation_perturbs_weights_keeps_topology() {
        let mut rng = Rng::new(3);
        let parent = MlpBrain::random(11, MlpBrain::input_size(4, 0), &[8, 4]);

        // Zero rate → faithful clone. Equality carries only topology + weights (the
        // brain has no other state: the activations are recomputed on demand).
        assert_eq!(
            parent.mutated(&mut rng, 0.0),
            parent,
            "zero rate = identity"
        );

        // Non-zero rate → weights changed, but same number of layers and same dims.
        let child = parent.mutated(&mut rng, 0.2);
        assert_ne!(child, parent, "the weights moved");
        assert_eq!(child.layers.len(), parent.layers.len());
        for (c, p) in child.layers.iter().zip(&parent.layers) {
            assert_eq!(c.inputs, p.inputs);
            assert_eq!(c.outputs(), p.outputs());
        }
    }

    /// Reproduction adapts the **input layer** to the child's number of rays (gene
    /// `vision_rays`, item 3): the first layer takes `2 × rays` inputs, the hidden
    /// topology and the output do not move. At constant precision and a zero rate, it
    /// is exactly the identity (same RNG stream as `mutated`).
    #[test]
    fn mlp_reproduce_resizes_input_layer_to_child_rays() {
        let mut rng = Rng::new(5);
        let parent = MlpBrain::random(11, MlpBrain::input_size(3, 0), &[8, 4]); // 12 inputs (3×3 + 3 self)

        // Unchanged precision + zero rate = faithful clone.
        let same = parent.reproduced(&mut rng, 0.0, MlpBrain::input_size(3, 0), 0);
        assert_eq!(same, parent, "constant precision, zero rate → identity");

        // Child that sees more finely: 5 rays → 18 inputs (5×3 + 3 self; input layer enlarged).
        let grown = parent.reproduced(&mut rng, 0.1, MlpBrain::input_size(5, 0), 0);
        assert_eq!(grown.layer_sizes(), vec![18, 8, 4, MlpBrain::OUTPUTS]);

        // Child that sees more coarsely: 2 rays → 9 inputs (2×3 + 3 self; input layer shrunk).
        let shrunk = parent.reproduced(&mut rng, 0.1, MlpBrain::input_size(2, 0), 0);
        assert_eq!(shrunk.layer_sizes(), vec![9, 8, 4, MlpBrain::OUTPUTS]);
    }

    /// The activations visualization is computed **on demand**, outside the sim
    /// core: `forward_activations` replays the propagation and exposes one layer per
    /// column (`[input, h0, …, output]`, sizes = `layer_sizes`), and its last layer
    /// coincides with `think`'s raw output (before rotation/normalization). This is
    /// what lets `think` memorize nothing anymore.
    #[test]
    fn forward_activations_match_topology_and_think() {
        let brain = MlpBrain::random(7, MlpBrain::input_size(3, 0), &[6, 4]);
        let (vision, target) = ([0.2, 0.7, 0.1], [0.0, 0.7, 0.0]);
        let p = mlp_perception(Vec2::X, vision, target);

        let acts = brain.forward_activations(&p);
        // One activations layer per graph column (input included).
        assert_eq!(acts.len(), brain.layer_sizes().len());
        for (layer, &size) in acts.iter().zip(&brain.layer_sizes()) {
            assert_eq!(layer.len(), size);
        }
        // The exposed input = vision ++ target ++ threat ++ self_state (the network's
        // input vector; `mlp_perception` sets threat and self_state to zero).
        assert_eq!(
            acts[0],
            vec![0.2, 0.7, 0.1, 0.0, 0.7, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        );

        // The raw output (last layer) is consistent with `think`'s action:
        // `throttle = min(|output|, 1)`, and the direction is that output rotated by
        // the heading (+X here, so unchanged up to normalization).
        let out = acts.last().unwrap();
        assert_eq!(out.len(), MlpBrain::OUTPUTS);
        let action = brain.think(&p);
        let raw = Vec2::new(out[0], out[1]);
        assert!((action.throttle - raw.length().min(1.0)).abs() < 1e-6);
        assert!((action.dir - raw.normalize_or_zero()).length() < 1e-5);
    }

    /// A wrongly-sized perception (fan-in that does not match) truncates the
    /// propagation without panicking: we recover at least the input, the inspector
    /// coloring the missing nodes neutral.
    #[test]
    fn forward_activations_is_robust_to_wrong_input_size() {
        let brain = MlpBrain::random(1, MlpBrain::input_size(3, 0), &[5]); // expects 12 inputs
        // 2-ray perception → 9 inputs (2×3 + 3 self ≠ 12): the first product does not match.
        let p = Perception {
            heading: Vec2::X,
            vision: [0.1, 0.2].into(),
            target: [0.0, 0.0].into(),
            threat: [0.0, 0.0].into(),
            self_state: [0.0; Perception::SELF_CHANNELS],
            field_state: Box::default(),
            ray_dirs: vec![Vec2::X, Vec2::Y].into_boxed_slice(),
        };
        let acts = brain.forward_activations(&p);
        assert_eq!(acts.len(), 1, "only the input is exposed, without panic");
        assert_eq!(acts[0].len(), 9);
    }
}
