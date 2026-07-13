# Abiogenesis — spontaneous emergence of life from the inert substrate

**Status: exploratory design note — NOT adopted, NOT scheduled.** This records a
direction and the reasoning behind it so it is not re-derived; it is *not* a binding
implementation plan (contrast [`component-emission-plan.md`](./component-emission-plan.md)
and [`nutrients-t2-plan.md`](./nutrients-t2-plan.md), which are decided). Captured
2026-07-14; the author is deliberately **not ready to integrate**. Laws cited by number
from [`CONSTITUTION-SIM.md`](../CONSTITUTION-SIM.md).

## 0. The question

Today every organism descends from a population seeded at config-time
(`spawn_agents`) and reproduced from a parent (`ecology::reproduce`). Nothing new
enters the world: a lineage that dies out is gone for good, and the very first life
form is placed by the designer. But life, in reality, arose **from something inert**.
Should an organism — a plant, our bottom-of-chain autotroph — be able to *emerge
spontaneously* from the substrate, e.g. by condensing out of accumulated nutrient?

## 1. It is not a Law 9 violation — it is the missing symmetric verb

The reflex objection is Law 9 (*nothing is created from nothing*). But abiogenesis is
**not** creation from nothing: it is the assembly of a body from matter that
**already exists in the field**. If the new organism's energy and matter are **paid
out of the field**, conservation holds. The field *is* the parent.

Better: `emit_at_death` already exists — a dying body returns its matter to the field
(component-emission Phase 4). Abiogenesis is its **symmetric inverse**, the verb that
is currently missing:

| verb | direction | meaning |
|---|---|---|
| `emit_at_death` | body → field | death dissolves an organism back into substrate |
| **`condense`** (proposed) | field → body | substrate self-assembles into an organism |

Death and spontaneous birth are the two directions of one matter↔organism exchange.
The symmetry is *native* to the constitution, not an alien addition.

## 2. The form that respects Law 11 — a verb, not an `if is_flora`

A system called "spontaneously generate plants" is exactly the per-kind code path
Law 11 forbids. The clean form is a **declarative verb in the `FieldRelation` table** —
`condense` (or `spawn_from`) — a peer of `absorb` / `emit` / `sense` / `affect` /
`emit_at_death`. When a component's local concentration exceeds a threshold, it
condenses into an agent, the matter being withdrawn from the field. Data-driven
(Law 1), uniform across kinds (Law 11), conservative (Law 9). No privileged biology.

## 3. The real crux: where does the genotype come from? (Law 6)

An agent needs a genotype (Law 6: genotype compiled to phenotype at spawn). This is
where the design forks, and the fork is a matter of *taste about what the designer is
allowed to author*. There are **three** postures, not two:

1. **Author the creature** — a founder recipe (`species/*.ron`) re-instantiated from
   accumulated matter. Tractable and constitution-clean; it removes permanent
   extinction and gives an anti-collapse floor. **Rejected by the author:** the
   organism is then pre-written, which defeats the point of "life from the inert."
2. **Author nothing, draw at random** across the whole gene space → in a space this
   large a random genotype is almost always non-viable. The world stays dead. This is
   not abiogenesis, it is a void.
3. **Author the _conditions_** under which inert matter self-organizes — not the
   creature, but the landscape in which one can appear.

**Posture 3 is the only honest one.** You cannot author *nothing*; the designer's
choice does not disappear, it *moves* — from the creature to the chemistry. This is
what a physicist does: write the economy, let the organism be a fixed point. The
genome is then written by no one — it is **discovered by the world**. "Life from the
inert" does not mean *no rules*; it means rules that speak only of matter, never of a
creature.

## 4. The mechanism that is "not pre-written" yet still produces life

The condensed genome is not a recipe. It is the **minimal self-replicator**: the
smallest set of active genes that closes the loop *absorb → threshold → reproduce*,
drawn at random **within the bounds the scenario already defines** (Law 7 already
prices and bounds every trait). The designer never draws a plant; the designer sets
the metabolic and absorption **costs** — which every scenario must set anyway — and
**the creature is the fixed point of that economy**.

Crucially, condensation is not *one* draw but **many filtered attempts**:

- a saturated cell emits many random proto-organisms;
- almost all fail to close the loop and **dissolve, returning their matter to the
  field** (Law 9 — nothing created, nothing lost);
- the rare one whose numbers happen to close the loop persists — and from there,
  **ordinary evolution takes over**.

This is the real mechanism: massive parallelism + a viability filter + a lot of
failed matter recycled into the pool.

## 5. Why this is actually tractable — *because it is flora*

The "random genome is never viable" objection is fatal **only if the organism must
think** — a random MLP does nothing. But flora is *sessile*: its decision is
degenerate. The genome that matters for it is a handful of body-economy numbers
(absorption, reproduction threshold, offspring cost, metabolism). **Low-dimensional →
a reachable target.**

So abiogenesis does not solve the origin of *cognition*; it solves the origin of a
*self-replicating body*. Cognition — fauna, the MLP brains — **evolves afterward**,
out of the substrate the flora created. That is the real order of evolutionary
history (autotrophs first, brains much later), and here it is not imposed but
**forced by the difficulty gradient** of the two problems.

## 6. Variant — the genome as a readout of the field (most poetic)

Instead of drawing genes at random, let condensation **read the local field state**:
the concentrations of the several components map to gene magnitudes. Life becomes a
*crystallization of the environment's own structure* — different regions (different
gradients) condense different proto-organisms. Neither authored nor pure noise: the
**environment writes the first recipe through its own gradients**. This sits naturally
on the multi-component substrate the `FieldRelation` table already provides.

## 7. Relation to the project's goal, and the honest caveats

The north star is *rich, non-collapsing ecosystems* (and the downstream science of
collapse factors). Abiogenesis is a legitimate stability lever: a **floor under
extinction**. And it is **self-regulating** when priced high (Law 7):

- while life is present it **absorbs** the substrate → concentration stays below the
  condensation threshold → no spontaneous birth;
- a die-off → nothing absorbs → substrate **accumulates** → until it **re-ignites**
  into new life.

A genuine ecological feedback (life suppresses its own abiogenesis by eating the
substrate), not a crutch.

**Caveats, stated plainly:**

- If condensation is too cheap it *trivially* prevents collapse — and thereby destroys
  the very science of collapse that is the downstream goal. Cost must be high.
- Tuning it so life emerges *sometimes* without erupting constantly is a real
  experimental problem: the concentration threshold, the number of attempts, and the
  draw's spread are **cadrans to explore**, not constants to guess. This is science,
  not plumbing — and exactly the kind of manipulation that yields a *watchable*
  milestone ("a saturated cell just ignited into life").

## 8. If/when adopted — the smallest first step

Not now. When the time comes, the minimal viable prototype:

1. Add a `condense` field to `FieldRelation` (threshold + matter cost), default `0.0`
   → byte-identical for every existing scenario (Law 1, DEV Rule 3).
2. A new `FixedUpdate` system: for each cell above threshold, attempt N condensations;
   each draws a minimal genotype within the species' bounds, spawns via the existing
   `spawn_agent` seam paying matter out of the field; a proto-organism that cannot
   close the loop within a grace window dies through the ordinary `reap`, its matter
   returned by `emit_at_death` (conservation closes for free).
3. One playable scenario on a vent (`Emits` source) to *see* whether a soup ignites.

Start with posture 4 (minimal replicator drawn in bounds); keep §6 (field-readout) as
a later refinement.
