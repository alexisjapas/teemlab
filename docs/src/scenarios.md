# The scenario crescendo

The bundled examples (`scenarios/examples/`) are numbered by **discovery order** —
simplest mechanism first, full ecosystems last. Read top to bottom and each scenario
introduces one new idea on top of the last; that is the crescendo. Open any of them from
the editor's **Scenario ▸ Open ▸ Examples** menu, or on the command line:

```sh
play scenarios/examples/04_evolution.ron
```

Every example is annotated in its own file (the header comment explains what to watch);
this page is the map.

---

## `00` · Empty

A blank canvas — one species declared but spawned zero times, so the world starts empty.
This is the editor's starting point: place species by hand, tune them, and press play.
Kept identical to the engine's `empty()` default.

## `01` · Default

The out-of-the-box creature: a single default species ready to evolve, mirroring the
engine's `default()`. The plainest "it's alive" starting template.

## `02` · Nutrients

**The substrate axis.** Four vents emit the nutrient at *graded* rates (6 → 12 → 18 →
24 per second), growing four oases of visibly different size. The decisive lesson is the
split between **survival and reproduction**: the scattered founder plants *all* survive
on sunlight, everywhere — but they only *breed* where they can absorb nutrient, so dense
thickets build around the vents while the barren ground keeps only its lone founders.
Toggle the heatmap (View ▸ Layers) to see the gradient. *No death spiral: a plant short
of nutrient pauses, it does not die.*

→ concepts: [the nutrient substrate](./model/nutrients.md), [the two axes](./model/economy.md)

## `03` · Flora

**Self-limiting growth.** A handful of sessile plants seed a green **wave** that spreads
across the arena and then *stops* — not for lack of room but because crowded plants drain
each other to death through an intraspecific-competition relation (the interaction
primitive pointed at one's own kind). Growth halts at an equilibrium **density**: a
logistic plateau, negative feedback, no runaway. Watch the reproduction and dispersal
genes drift under the pressure.

→ concepts: [competition](./model/interactions.md), [brains: Sessile](./model/brains.md#sessile--the-plant)

## `04` · Evolution

**Natural selection, made visible.** A naive [`Wander`](./model/brains.md#wander--the-naive-control)
grazer forages by chance, breeds when fed, and mutates. The founders start *far-sighted*
(vision range 200, 9 rays) — but a wanderer cannot act on what it sees, and vision is
priced. So selection **melts the eyes down** toward their floor over the generations:
watch the vision curves slide in the HUD's gene-drift panel. A trait is kept only where
it pays — the exact contrast with `hunt` below.

→ concepts: [genes & mutation](./model/genes.md), [the economy](./model/economy.md)

## `05` · Hunt

**The competent brain.** Where the `evolution` grazer stumbles onto food, a
[`Hunter`](./model/brains.md#hunter--the-competent-control) *sees* it and steers straight
at the nearest plant via the perception **target** channel. Click a hunter to watch its
rays lock on. A self-regulating two-level ecosystem: the hunters' reproduction is gated
by the nutrient they harvest, so they track the food instead of crashing it.

→ concepts: [perception](./model/the-loop.md#1-perceive), [interactions](./model/interactions.md)

## `06` · Cohabitation

**Competitive exclusion — the cleanest A/B test of a brain.** A hunter and a wanderer
share the *same body, economy and food*; only the brain differs. On patchy oasis food the
hunter navigates to a patch and stays fed while the wanderer only blunders across one by
chance — so the hunter out-forages it and, on a limited resource, excludes it. Watch the
two population curves diverge. This is the bar a *learned* brain must reach next.

→ concepts: [brains](./model/brains.md)

## `07`-`09` · The MLP learning story

Three scenarios telling one story about the [learned `Mlp`](./model/brains.md#mlp--the-learned-brain)
brain. Scenarios `07` and `09` are **generated** by `cargo run --bin train`.

- **`07` · MLP brain (naive)** — a from-random MLP vs a wander control. With random
  weights it forages *worse* than the wanderer (it has learned nothing yet). The baseline.
- **`08` · MLP training ground** — a large population of MLPs (80 founders, high mutation)
  evolve *alone* on the oasis food; selection rewards the lineages that navigate to a
  patch. The `train` binary captures the best-evolved individual.
- **`09` · MLP evolved (trained)** — the captured brain, reused vs the same control. It
  now reaches parity-to-dominance — a world away from the naive MLP. *Training worked.*

Inspect any MLP agent to see its live activation graph.

→ concepts: [neuroevolution](./model/brains.md#mlp--the-learned-brain)

## `10` · Predator-prey

**A three-level trophic chain.** A count **pyramid** — flora ≫ prey ≫ predators — with
the *same* `Hunter` brain making one species a grazer (its target is the plant) and
another a carnivore (its target is the prey), resolved entirely by the relation table.
The prey actively **flees** its predator (the threat channel). The nutrient cascades up
the chain so every level stays bounded; predator and prey coexist in an oscillating band
across seeds, neither going extinct nor exploding.

→ concepts: [interactions](./model/interactions.md), [the food web](./model/nutrients.md#the-food-web-nutrient-travels-by-eating)

## `11` · Factions

**Combat — the destructive half of the interaction primitive.** Two mobile factions wage
war (`transfer: false`: each destroys the other's reserve, gaining nothing) while both
forage the same flora to live. Fighting is therefore a *pure cost*, paid on top of making
a living — a war of attrition that grinds both armies down. The `Hunter` brain makes each
faction seek the enemy (a target) and warily skirmish it (a threat).

→ concepts: [combat](./model/interactions.md#three-behaviours-from-one-verb)

## `12` · Nutrient web

**The closed loop — the finale.** The full cycle in one scenario, watchable end to end:
source → field → **plant absorbs** → **herbivore eats** (nutrient travels up the chain) →
herbivore dies → **recycling returns** its nutrient to the field → … Click an entity to
read its nutrient store; toggle the heatmap to see a bright spot bloom where a body fell.
A closed loop oscillates (Lotka–Volterra overshoot, a known wall) — this scenario is
built to make the *mechanisms* observable, and it stays lively for a good while.

→ concepts: [recycling & the closed loop](./model/nutrients.md#recycling-closing-the-loop)

---

## Make your own

These are starting points, not limits. Open one in the [editor](./editor.md), change a
gene, rewire a relation, add a species, and press play — or write a new
[`.ron` file](./scenario-format.md) from scratch. The same single engine runs all of it.
