# Genes & the genotype

A creature's body is described by its **genotype** — a flat list of genes, inherited and
mutated. At spawn the genotype is *compiled* into a living **phenotype** (the physics
components and the brain); evolution only ever rewrites the recipe, never the running
dish.

Every gene is a **triplet plus a flag**:

- a **value** (the number itself),
- **bounds** `[min, max]` that clamp both mutation and the editor sliders,
- a **cost coupling** — because *a beneficial trait must cost something*, or selection
  has nothing to push against,
- and a per-species **mutable?** flag: may this gene drift, or stay nailed to the
  founder's value? (A non-mutable gene is still *inherited* — "mutable", not
  "heritable".)

> **Why costs matter.** If a trait were free, it would simply drift to its maximum and
> nothing would emerge. Speed is paid for by `move_cost`, sharp turning by
> `agility_cost`, keen eyes by a vision cost proportional to range × ray count, a big
> brain by `brain_cost` per neuron, and merely staying alive by `base_metabolism`. The
> *cost* genes are themselves non-mutable by default — left to evolve, a lineage would
> just whittle its own costs to zero and the selection pressure would vanish.

## The genes

The values and bounds below are the **engine defaults**; any scenario can override the
bounds (they are global, in the scenario file) and the founding values (per archetype).
"Mutates?" is the default `Mutability`; a scenario sets it per species.

### Locomotion

| Gene             | Default | Default bounds | Cost? | Mutates? | What it does |
| ---------------- | ------: | -------------- | ----- | -------- | ------------ |
| `max_speed`      |   140   | 40 … 260       |  —    | ✅       | Top speed. **Setting it to `0` makes the agent sessile** (a plant). |
| `agility`        |  0.12   | 0.02 … 0.5     |  —    | ✅       | Steering responsiveness — how fast it can change heading. |
| `move_cost`      |    2    | 0 … 20         | 💲    | ❌       | Energy/s surcharge at full speed (prices `max_speed`). |
| `agility_cost`   |  0.02   | 0 … 2          | 💲    | ❌       | Energy per unit of maneuvering effort `|Δv|` (prices turning/accelerating). |

### Vision

| Gene             | Default | Default bounds | Cost? | Mutates? | What it does |
| ---------------- | ------: | -------------- | ----- | -------- | ------------ |
| `vision_range`   |   160   | 40 … 300       |  —    | ✅       | How far the rays reach. |
| `vision_fov_deg` |   120   | 40 … 280       |  —    | ✅       | Total field of view, in **degrees**. |
| `vision_rays`    |    7    | 0 … 21         |  —    | ✅       | **Visual precision**: number of rays (→ number of perception channels). `0` = blind. |
| `brain_cost`     |   0.1   | 0 … 2          | 💲    | ❌       | Energy/s **per decision neuron** of an MLP (zero neurons for a hand-written brain). |

Vision is priced indirectly: more rays and longer range cost more energy per tick, which
bounds how keen eyes can profitably get.

### Metabolism

| Gene              | Default | Default bounds | Cost? | Mutates? | What it does |
| ----------------- | ------: | -------------- | ----- | -------- | ------------ |
| `base_metabolism` |    4    | 0 … 20         | 💲    | ❌       | Energy/s drained at rest — the baseline cost of survival, the core selection pressure. |

### Reproduction

| Gene                     | Default | Default bounds | Cost? | Mutates? | What it does |
| ------------------------ | ------: | -------------- | ----- | -------- | ------------ |
| `reproduction_threshold` |   80    | 0 … 200        |  —    | ✅       | Energy a parent must reach to breed. `0` ⇒ this agent does not reproduce. |
| `offspring_energy`       |   40    | 10 … 120       |  —    | ✅       | Energy handed to each child, **deducted from the parent** (conservation). |
| `mutation_rate`          |  0.05   | 0 … 0.5        |  —    | ❌       | Std-dev of mutation, as a fraction of each gene's span. The gene that drives its own lineage's evolution speed. |

The reproduction strategy is itself selectable: a lineage can evolve toward breeding
early and cheap, or late and lavish.

### Flora

These are inert (`0`) for fauna; a plant scenario turns them on.

| Gene             | Default | Default bounds | Cost? | Mutates? | What it does |
| ---------------- | ------: | -------------- | ----- | -------- | ------------ |
| `photosynthesis` |    0    | 0 … 30         |  —    | ❌       | Energy/s **gained** passively — a sessile creature's food, the counterpart of eating. |
| `seed_dispersal` |    0    | 0 … 200        |  —    | ❌       | Distance a seed is dropped from the parent. `0` ⇒ a close default (clustered). |

### Nutrients

The second resource axis (see [The economy](./economy.md)). Inert by default.

| Gene                  | Default | Default bounds | Cost? | Mutates? | What it does |
| --------------------- | ------: | -------------- | ----- | -------- | ------------ |
| `nutrient_absorption` |    0    | 0 … 20         |  —    | ❌       | Rate at which the entity pulls nutrient from the field into its store. |
| `nutrient_capacity`   |    0    | 0 … 200        |  —    | ❌       | Size of the per-entity nutrient store. |
| `offspring_nutrient`  |    0    | 0 … 120        |  —    | ❌       | Nutrient **spent and consumed** per child — the child is born with an *empty* store. Makes the nutrient a true limiting resource. |

## Mutation

At reproduction, each **mutable** gene receives a Gaussian nudge of standard deviation
`mutation_rate × span`, then is clamped back into its bounds. A non-mutable gene is
copied unchanged. (A subtle but important rule: a gene that *doesn't* drift is copied
*exactly* — even if its founder value sits outside the bounds, like a plant's
`max_speed = 0` — so an immobile plant never accidentally becomes mobile at its first
child.)

## One table, no special cases

All of the above lives in a single `TRAITS` table in the engine. The mutation loop, the
editor sliders, the HUD curves, the inspector and the metrics all iterate that one
table. **Adding a new gene is one table entry plus a struct field** — no driver,
editor, or UI code to touch. That is the test of the abstraction: a new characteristic
must touch exactly one place.
