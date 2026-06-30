# Evolutionary regimes

Everything so far — [genes](./genes.md), [brains](./brains.md), the
[economy](./economy.md) — describes a *single agent's* life. This page is about the
population: **how evolution actually runs**. teemlab has two regimes for that, and they
are not a setting you flip but two *compositions* of the same parts.

## Two axes, not one switch

A regime is a point on a grid of two independent questions:

- **When does reproduction happen?** *Continuously*, inside the running sim (a parent
  breeds the instant it can afford to), or in *batches*, at a generation boundary outside
  the sim.
- **Where does fitness come from?** *Implicitly*, from the world itself (you survived and
  bred — that *was* the fitness), or *explicitly*, from a score someone computes and
  selects on.

|                      | Implicit fitness      | Explicit fitness |
| -------------------- | --------------------- | ---------------- |
| **Continuous repro** | **Natural selection** | steady-state GA  |
| **Batched repro**    | "seasonal" regime     | **Battle**       |

The two corners on the diagonal are the ones teemlab ships; the off-diagonal cells are
valid recompositions too. Crucially there is **no `enum Regime`** in the code — a new
regime must *fall out* of these pieces, never be special-cased (a development law).

## Continuous — natural selection

This is what every scenario from [`02`](../scenarios.md#02--nutrients) to
[`12`](../scenarios.md#12--nutrient-web) runs. Reproduction is a sim system: when an
agent's reserve crosses its `reproduction_threshold` it spends `offspring_energy` on a
child that inherits the genotype and brain, [mutated](./genes.md#mutation). Nobody computes
a fitness — the *world* is the judge. A trait that does not pay for [its cost](./economy.md)
loses ground; a brain that cannot feed itself leaves no descendants. You watch the gene
drift in the HUD and read the story off the curves.

It is open-ended and honest, but slow and noisy: a *directed* search — evolve a **good**
forager, breed a **winning** fighter — needs many stable generations, which a living-food
ecosystem rarely grants.

## Generational — breeding

So the second regime takes the search *out* of the sim. An out-of-sim **orchestrator** runs
the loop **run → score → breed**:

1. **Run** a *cohort* of independent headless matches (parallelized across cores) — each
   match is the byte-identical engine, just without a window.
2. **Score** each finished match by an explicit **fitness**, a small menu of engine
   primitives: `BestEvolved` (the deepest-evolved lineage), `Population` (standing biomass
   — a good forager sustains more), or `Dominance` (own survivors minus living rivals — the
   combat measure).
3. **Breed**: keep the genomes from the best-*scoring* matches and **re-seed** them as the
   next cohort's founders — so the fitness genuinely drives where the search goes.

Repeat for a fixed number of generations. The inner match still runs the *continuous* loop
(agents reproduce and mutate in-match), so the orchestrator sits **on top of** natural
selection rather than replacing it — a recomposition, exactly as the grid demands.

Run it headless with the **`breed` binary** or watch it in the windowed **breeding
dashboard** (a fitness-vs-generation curve, a leaderboard of the cohort's best genomes, and
Save-as-variant into the [catalog](./editor.md)). The carrier scenarios are
[`13`–`15`](../scenarios.md#13-15--the-generational-regime).

## Battle & the Red Queen

Point a mutual [`transfer: false`](./interactions.md#three-behaviours-from-one-verb) war
between two factions and score by `Dominance`, and breeding becomes a **battle**: breed one
faction to beat a fixed rival ([`14`](../scenarios.md#13-15--the-generational-regime)).

Breed **both** factions at once and you get **co-evolution** — the *Red Queen*
([`15`](../scenarios.md#13-15--the-generational-regime)). Neither side can pull permanently
ahead: every gain by one is pressure on the other to match it, so their fitness curves
*track* each other instead of diverging. "It takes all the running you can do, to keep in
the same place."
