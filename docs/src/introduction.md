# teemlab

**An evolutionary simulation engine where one engine interprets data, and every
simulation is a *scenario*.** Top-down 2D, entities are circles, and every creature
runs the same three-stage loop: **perceive → decide → act**. Built in Rust on
Bevy 0.19 and Avian 0.7.

You do not write code to make a new world. You write a **scenario** — a plain-text
[RON](https://github.com/ron-rs/ron) file describing the arena, the species, their
genes, their brains, and how they may act on one another — and the single engine runs
it. Natural selection, a predator–prey chase, a war between factions, a neural network
learning to forage: all are the *same engine*, fed *different data*.

> This site documents **what teemlab is and how to drive it**: the model (genes,
> brains, the energy/nutrient economy, interactions), the catalog of bundled example
> scenarios, the scenario file format, and the in-app editor. For the *design
> rationale* and the implementation history, see the repository's `ROADMAP.md`.

## The central idea: one engine, many scenarios

Most "evolution sims" hard-code one world. teemlab inverts that: the engine is fixed
and minimal, and the **interesting variety lives entirely in data**. A scenario states
only what it changes; everything else falls back to engine defaults. Adding a field
never breaks an existing scenario.

That single constraint pays off everywhere:

- A **food source** is not a special type — it is just an agent with a *sessile* brain
  that lives on photosynthesis and does not move. A "plant" and a "wolf" run the
  identical death, birth and feeding systems; they differ **only** in their genes,
  brain and relations.
- **Eating and attacking are one verb** — a directed interaction that drains a
  target's reserve. *Transfer* it to the attacker and you have predation; destroy it
  and you have combat; point it at one's own species and you have competition.
- A creature's **decider is swappable**: a hand-written reflex, a finite rule, or a
  small neural network *learned by neuroevolution* — all read the same perception and
  write the same action.

## Three authors of behaviour

Everything in the world is written by one of three authors, at one of three moments:

| Author        | Moment       | Writes the **decision** via… | Writes the **body** via… |
| ------------- | ------------ | ---------------------------- | ------------------------ |
| **Engine**    | compile-time | systems that interpret data  | components and physics   |
| **Designer**  | config-time  | the brain you pick (rules)   | the archetype's values   |
| **Evolution** | run-time     | neural-network weights       | genes that mutate        |

You are the **Designer**: you author scenarios. The **Engine** is fixed. **Evolution**
takes over at run-time, mutating genes and (for learned brains) weights under the
selection pressure your scenario defines.

## What you can do with it

- **Watch natural selection happen.** Set up a grazer with a costly trait it cannot
  use, and watch selection melt that trait away over generations (the
  [`evolution`](./scenarios.md#04--evolution) scenario).
- **Compare brains head-to-head.** Put a competent hunter and a naive wanderer in the
  same body on the same food and watch competitive exclusion
  ([`cohabitation`](./scenarios.md#06--cohabitation)).
- **Train a neural forager.** Let a population of small MLPs evolve to navigate to
  food, then reuse the trained brain ([the MLP story](./scenarios.md#07-09--the-mlp-learning-story)).
- **Build food webs.** Stack a count pyramid — flora ≫ prey ≫ predators — and watch a
  three-level chain coexist with prey that flee ([`predator_prey`](./scenarios.md#10--predator-prey)).
- **Author by hand.** The windowed build is a full [editor](./editor.md): drag species
  into the arena, tune every gene with a slider, wire the relation table, and run it —
  all without touching code.
- **Record it.** Render any scenario to a video, headless, with an optional nutrient
  heatmap overlay ([recording](./recording.md)).

## Where to go next

- New here? Start with [Getting started](./getting-started.md) to build and launch.
- Want to understand the world? Read [The agent loop](./model/the-loop.md), then
  [Genes](./model/genes.md) and [The economy](./model/economy.md).
- Want to *use* it? Browse [The scenario crescendo](./scenarios.md) and the
  [scenario file format](./scenario-format.md).
