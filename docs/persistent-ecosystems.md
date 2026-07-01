# Rich, persistent ecosystems — orientation & design synthesis

**Status:** orientation, not an implementation plan. This records the conclusions of a
design discussion so they are not re-derived: it fixes the **near-term product goal** and
the **priority order** of the next work, and points into [`ROADMAP.md`](../ROADMAP.md)
§8/§9 where the individual threads live. Nothing here changes a built mechanic (no code,
no RON, no RNG draw — DEV Rule 3 is not in play); it changes *what we aim at next* and
*why*. Laws are cited by number ([`CONSTITUTION-SIM.md`](../CONSTITUTION-SIM.md),
[`CONSTITUTION-DEV.md`](../CONSTITUTION-DEV.md)).

---

## 0. Framing — what the product aims at

teemlab is an **experimentation platform** (one engine, scenarios as data — SIM Law 1). It
must serve **several needs**, not one; the science of ecological collapse is *one* of them,
downstream.

- **Near-term goal:** generate simulations that are **rich** (behavioural and ecological
  diversity — multi-level food webs, emergent behaviour) **and that do not collapse**
  (populations persist over a long horizon).
- **A downstream need:** once persistent ecosystems are reachable, use the platform to
  **determine which factors drive a system to collapse** — a science of tipping points.

### What "does not collapse" means

Persistent does **not** mean *constant*. Oscillation — predator–prey Lotka–Volterra cycles
— is real ecology, not a defect (cf. roadmap §7, the L–V wall). "Does not collapse" means it
does not **totally** collapse. The distinction that matters, for both the near-term goal and
the downstream science, is between:

- an **irreversible collapse** (extinction; a loss of diversity with no return — an attractor
  the system cannot leave), and
- a **deep but reversible trough** (the population dips and recovers).

Telling them apart requires a **collapse metric** that captures irreversibility, and running a
scenario **past its first trough** — a single dip is not a collapse.

## 1. Endogenous vs exogenous factors — the organising frame

The factors that govern persistence are not all **exogenous** knobs set by the scenario (arena
size, food rate, costs). Some are **endogenous** — the system selects them or inflicts them on
itself. Two symmetric poles:

- an **endogenous stabiliser**: behavioural **restraint** — agents that do not consume
  everything (selected under the right conditions, §2);
- an **endogenous destabiliser**: **toxin accumulation** — a component the agents emit that
  poisons their own environment (emergent, suffered — §3).

This is the interesting part of the collapse science: some causes of collapse are properties the
agents *produce*, not parameters we turn.

## 2. Cognition is a stability lever — the prioritised substrate

It is tempting to split the levers of persistence into two families: **ecological/mechanical**
ones (turnover/recycling, spatial refuges, the energy economy) and **cognitive** capabilities of
the agents (choosing actions, sensing themselves, memory, communication), filing the latter under
"intelligence for its own sake". **That split is wrong:** some cognitive capabilities are directly
levers of persistence.

The central case is **restraint** — not eating everything within reach:

- It is a **selectable stabilising mechanism**: a species that does not systematically consume
  all it touches leaves its environment less prone to collapse.
- But restraint is **individually costly** (the greedy individual reproduces more). In a
  well-mixed medium the greedy invades the prudent — the **tragedy of the commons** — and
  collapse follows. Restraint becomes selectable only under **spatial viscosity**: limited
  dispersal, so an agent's offspring inherit the environment it degraded or preserved. This is
  exactly where the seeding-distance gene (`seed_dispersal`), **spatial refuges**, and locality
  become load-bearing — and it echoes the settled finding that the decisive predator–prey
  stabiliser was *spatial* (an enlarged arena as refuge), not fine tuning (item 17, roadmap §8).
- **Hypothesis** (stated as such — not a claim, and not asserted to be systematic): across many
  replays, the persistent runs may turn out to be precisely those where the species
  **self-managed** their environment. Plausible **conditionally** on a spatial structure that
  makes degradation local and inherited. The dispersal-strong → dispersal-weak transition is then
  itself a **collapse factor to measure**.

**Consequence — proprioception is instrumentally necessary**, not a cognitive luxury. To
*modulate* its consumption (eat by need — energy, satiety — instead of automatically on contact),
an agent needs an **internal state** as input. Without proprioception, "choosing whether to eat"
has nothing to act on. Hence:

> **Deliberate eating (a brain-driven, costed action) + proprioception (self-state perception
> channels) = the minimal substrate for restraint to be *expressible* at all** — and therefore
> evaluable. It also enriches the behavioural repertoire, so it serves *richness* as much as
> *stability*.

**This substrate is the prioritised near-term work.** It touches SIM Law 8 (the interaction
primitive stays one verb; only its *triggering* moves from automatic-in-range to brain-driven — an
`Action` output, with a cost, SIM Law 7) and extends the perception contract (SIM Law 3) with
self-referential channels. Both threads live in roadmap §9 ("Eating/attacking as a deliberate,
costed action"; "Proprioception").

## 3. Emission of components into the environment

Two current gaps point at a single missing mechanism: agents cannot **die without disappearing**
(so there are no corpses), and they cannot **emit components during life** (organic waste,
excretions). Both are the same absent capability: an **agent → environment** emission — writing a
component into the environment (a field, or an entity), during life **or** at death.

This is the **symmetric of absorption**, which already exists (T2: environment → agent). The
inbound direction is built; the outbound *voluntary/continuous* one is not — today's recycling
(dead body → field, roadmap §9 "recycling / closed loop") is only a special case wired into the
death system. Emission = *writing* to the environment; absorption = *reading* it. That symmetry is
the sign it is the right abstraction (SIM Law 8 applied to the environment side; SIM Law 11 — no
per-kind code path).

**Current architectural approach: one layer per component/nutrient type** (already the case for
nutrients — the substrate field and its heatmap layer, roadmap §9). Emission means agents *write*
into these per-component layers; death deposits biomass into a layer (corpses / the missing
**turnover**). **Inter-layer interactions** (metabolisation — transforming one component into
another) are a **later, optional** addition, not required first.

With one mechanism — *emitted component + a field-perception channel + relation-defined semantics*
— and no per-kind code (SIM Law 11), this unifies four separate wishlist items:

- **corpses** = biomass emitted at death (the turnover a flat carrying capacity needs);
- **organic waste** = continuous emission during life;
- **toxicity** = an emitted component whose relation says *toxic to X / edible to Y / inert to Z*;
- **communication (pheromones)** = an emitted component meant to be *perceived*, not consumed.

A chemical pheromone and a toxic waste differ **only** by the relation attached to them.

**Open question (not settled): the audio/wave modality does not fit the diffusion-layer model
cleanly.** Audio communication (several frequencies, wave propagation) is not diffusion. One path
is a layer per frequency with different diffusion configs — but that would still require modelling
**wave collision and reflection**, which the diffusion model does not do. More broadly, **the
per-layer approach is challengeable** if it proves non-optimal.

**Double-edged for persistence.** Emission both stabilises and destabilises: recycling closes the
matter loop (SIM Law 9 in spirit — matter moved or transformed, never created), while **toxin
accumulation is a new, emergent collapse mode** (self-poisoning; an eutrophication-like dynamic) —
exactly the kind of endogenous collapse factor (§1) the downstream science wants to isolate.

Roadmap points this subsumes/extends: the recycling loop (done), the "grazed plants cannot die"
decision (mortality/turnover), and the conservation-at-reproduction invariant (nutrients T3).

## 4. The science of collapse factors (downstream)

Once persistent ecosystems are reachable, the platform must let us **find the factors that tip a
system into collapse**. This reframes the experimental protocol:

- the object of interest is the **frontier** (the tipping threshold), not the persistent run — so
  collapses must be **induced**, not only avoided;
- it favours **single-factor gradients** (hold everything else fixed, read the critical threshold)
  over pure random seed sweeps;
- it needs a **collapse metric** that distinguishes irreversible collapse from a reversible trough
  (§0), and runs that continue **past the first trough**.

The headless `sweep` bin (roadmap §0/§6 — scores final worlds by biodiversity over a seed or
parameter sweep) is the first brick of this instrument; what it still lacks is the irreversibility
metric and the single-factor gradient framing.

## 5. Far horizon (noted, not prioritised)

- **Species as an emergent cluster.** Today a species is a fixed **archetype index** — a
  designer-assigned label. Genuine speciation would make a species a **cluster in phenotype
  space**, derived rather than declared. It also **subsumes the de-hardcoding of identity**:
  recognising others by an *evolved signal* rather than a hard-coded role or colour is the same
  problem as "what makes two individuals the same species". A research programme, not a feature;
  kept out of the near-term path.

## 6. Performance

For a sweep-driven use, the bottleneck that matters is **experiments per hour**, which is
embarrassingly parallel and already parallelised across matches (~5×, item 20). **Scale
horizontally (cores/machines) before any GPU port**, and **profile before optimising** (confirm
the raycast vision cost, roadmap §7, is the real bottleneck; treat rays/range as a cost — SIM
Law 7 — if so). This restates the roadmap's existing performance positions (§5–§7, §9
"per-`think` allocations"); it does not change them.

## 7. Priority summary

1. **Prioritised now — the cognitive substrate:** deliberate (brain-driven, costed) eating +
   proprioception (§2). Makes restraint *expressible*, and enriches behaviour.
2. **High-leverage for persistence:** turnover (via emission / corpses, §3) and spatial refuges —
   the latter also the condition that makes restraint selectable.
3. **Emission of components** via per-component layers (§3), unlocking turnover, toxicity and
   communication at once; the audio/wave case left open.
4. **Downstream:** the collapse-factor science instrument (§4).
5. **Far horizon:** species as an emergent cluster; de-hardcoded identity (§5).

**In one line:** the near-term goal is rich, persistent ecosystems (the science of collapse
factors comes after); the prioritised work is the deliberate-eating + proprioception substrate,
which makes *restraint* — an endogenous stabiliser — expressible, while component emission opens
both stabilising turnover and destabilising toxicity, with audio communication left as an open
question.
