# Constitution of the Simulation — the inviolable laws

These are the inviolable laws of the simulated world: the contracts that define
what **teemlab** *is*. They are deliberately stable. The [ROADMAP](ROADMAP.md)
records what we build and in what order; **this document records what may never
change without changing the project itself**. Breaking a law forfeits the
modularity that is the entire point of the engine.

The design rests on **one axis with three authors** — who writes behavior and
structure:

| Author | Moment | Decision via… | Body via… |
|---|---|---|---|
| **Engine** | compile-time | systems that interpret data | components and their effects |
| **Designer** | config-time | deterministic brain (rules) | archetype-editor values |
| **Evolution** | run-time | neural-network weights | genes that mutate |

The axis applies twice — to the **decision** and to the **body**. The laws below
keep it clean. (See also [`ROADMAP.md`](ROADMAP.md) §1–§4, of which these laws are
the distilled, binding form.)

---

## Law 1 — One engine, many scenarios

A single engine interprets data. A **scenario is data** (a RON file), never code;
what varies from one simulation to the next is configuration, not the engine. A
scenario states only what it changes — everything else falls back to engine
defaults — and adding a field must never break an existing scenario.

**Why.** The whole project is "one engine, many scenarios"; the day a behavior
needs a code branch per scenario, the abstraction has failed.

**Anchored in.** `config.rs` (`SimConfig` from RON, `#[serde(default)]`),
`scenarios/*.ron`, `species/*.ron`.

---

## Law 2 — The invariant loop: perceive → decide → act

Every agent's agency is this three-stage loop and nothing else, run in the
fixed-timestep schedule. The three stages stay distinct so the brain/body seam
remains clean.

**Why.** A single, invariant loop is what lets every scenario and every brain type
share one engine.

**Anchored in.** `movement.rs` (`perceive` / `decide` / `act`), chained in
`FixedUpdate` by `lib.rs`.

---

## Law 3 — Brain ↔ body is a contract: normalized floats in → floats out

A brain only reads [`Perception`] and only writes [`Action`] (normalized floats).
Its internals — neural network, decision tree, reflex — are interchangeable and
know nothing of the physics engine.

**Why.** The contract is what makes a learned brain and a hand-written one
substitutable, and what makes "the inside is interchangeable" true.

**Anchored in.** `components.rs` (`Perception`, `Action`), `brain.rs`
(`Brain::think`).

---

## Law 4 — The body imposes the shape of the brain's I/O

Sensors and actuators define the size of the I/O vectors; the brain adapts to the
body, never the reverse. Genes vary the *magnitudes* and — since `vision_rays` —
the *number of channels*; the MLP's input layer resizes to match at reproduction.

**Why.** If the brain dictated the body's shape, evolving the body would mean
rewriting the brain's contract — coupling the two axes that must stay independent.

**Anchored in.** `components.rs` (`Vision` → channels), `genotype.rs`
(`ray_count`), `brain.rs` (`MlpBrain` input sizing, `reproduced`).

---

## Law 5 — Brains are an `enum`, never `Box<dyn>`

Brain implementations are stored as an enum: static dispatch, clean `serde`, and
an **exhaustive `match`** so adding a variant is a compile error to resolve
everywhere it matters. Crossover is intra-type (one does not cross a neural net
with a state machine).

**Why.** The compiler, not vigilance, must enforce that every brain handles every
contract point.

**Anchored in.** `brain.rs` (`enum Brain`; exhaustive matches in `think`,
`reproduce`, `neuron_count`).

---

## Law 6 — Genotype ≠ phenotype

Evolution mutates the **genotype** — an inherited description — which is compiled
into the living **phenotype** (physics components + brain) only at spawn.
Evolution never touches an agent's current physical state: it rewrites the recipe,
not the dish.

**Why.** Keeps evolution and the running physics decoupled; a mutation can never
corrupt an in-flight body.

**Anchored in.** `genotype.rs` (`Genotype`, `mutate`, phenotype compilation),
`spawn.rs` (`spawn_agent`).

---

## Law 7 — Every characteristic is priced

A characteristic is a triplet — **(value, bounds, cost coupling)** — plus a
*mutable?* facet. **A beneficial trait must cost something**: without a cost
coupling it drifts to its bound and nothing emerges. The cost is defined by the
**scenario**, not hard-coded by the engine. The *mutable?* flag governs mutation
only — a gene is transmitted to offspring in both cases (hence *mutable*, not
*heritable*).

**Why.** Selection pressure *is* the cost structure; an unpriced advantage makes
the trait converge trivially and removes it from the evolutionary game.

**Anchored in.** `genotype.rs` (`Genotype`, `TRAITS`, `mutate`), `config.rs`
(`Bounds`, `Mutability`), `ecology.rs` (`metabolize` — the cost consumer; e.g.
`move_cost`, `vision`, `brain_cost`, `agility_cost`).

---

## Law 8 — One interaction primitive

Eating and attacking are the **same directed interaction**: an actor reduces a
target's reserve, within range. The engine exposes one verb; the scenario sets its
semantics — *transfer* → predation, *destroy* → combat — and the target filter
(trophic or factional). Perception is symmetric: spatial queries are engine
machinery, the scenario chooses which channels become brain inputs.

**Why.** Two verbs would be two code paths to keep in sync; one primitive + a
relation table covers predation, combat and competition with no new mechanism.

**Anchored in.** `interaction.rs`, `config.rs` (`Relation` table).

---

## Law 9 — Conservation: nothing is created from nothing

Interactions transfer or destroy reserve, never create it. Reproduction pays the
child's energy out of the parent and never pays more than the parent holds. A
resource contested by several actors is **shared**, never duplicated.

**Why.** A leak in conservation makes a "cheap child / low threshold" lineage
win for free — a runaway that has nothing to do with fitness.

**Anchored in.** `interaction.rs` (conservation under contention), `ecology.rs`
(`reproduce` guard).

---

## Law 10 — We replay experiments, not bits

Determinism is traded for parallelism and speed (no `enhanced-determinism`). A
seed reproduces an **experiment configuration** — to compare parameters — not a
bit-for-bit run. The fixed timestep exists for solver stability, not for
reproducibility.

**Why.** Bit-for-bit replay is incompatible with the parallelism the simulation
needs to scale; replaying the *configuration* is what the science actually
requires.

**Anchored in.** `rng.rs` (seeded `SplitMix64`), `lib.rs` (fixed-timestep
schedule). See the operational corollary in
[`CONSTITUTION-DEV.md`](CONSTITUTION-DEV.md) Rule 3.

---

## Law 11 — Every life form runs the same systems

Every living entity — fauna or flora, predator or prey, mover or source — is an
`Agent` driven by the **same** base systems: the *perceive → decide → act* loop, the
interaction primitive, and the life economy (`metabolize`, `reap` for death,
`reproduce` for birth). What distinguishes one life form from another is **only its
data** — its **genes** (configuration), its **brain and body** (capabilities), and
the **relation table** (relations). There is **no per-kind code path**: no
`if is_flora`, no privileged death, reproduction, or feeding rule, and no schedule
ordering tuned to exempt one kind. A "plant" is just an agent with a sessile brain and
photosynthesis; any difference in its behavior must be an *emergent consequence of its
genes and relations*, never of a special-cased system.

**Why.** The day one life form earns its own rule in the engine, the "one engine,
scenarios as data" abstraction (Law 1) has leaked into the biology: a privileged kind,
and two code paths to keep in sync. Uniform base behavior is what lets a new life form
exist purely as data — and what keeps comparisons between life forms honest, since they
then differ only by their genes.

**Anchored in.** `spawn` (one agent bundle), `ecology` (`metabolize` / `reap` /
`reproduce`, no per-species branch), `movement` (one loop), `interaction` (one
primitive). Phase 3b dissolved the `Food` type into exactly this principle.
