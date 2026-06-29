# The nutrient substrate

The nutrient is teemlab's *second* resource — a mineral in the ground that gates
[reproduction](./economy.md#nutrient--the-reproduction-axis). Unlike a creature, it is
pure environment: a concentration field underneath the world, outside the agents and
outside the "every life form is an agent" law. This page is the mechanics of that layer.

## The field

The substrate is a **grid of concentrations** laid over the arena — one value per cell,
configured by:

```ron
nutrient: (
    resolution: 256,   // cells per side of the square field
    diffusion: 0.3,    // fraction rebalanced toward neighbours each tick, in [0, 1]
)
```

`diffusion` is the *local-vs-global* knob. At `0` the field never spreads — nutrient
stays exactly where it is emitted. Higher values let it bleed outward into smooth
gradients. It is what turns point emissions into **oases**.

You can watch the field directly: the renderer draws it as a **heatmap layer** (toggle
it in **View ▸ Layers**, or pass `--nutrients` to the recorder).

## Sources

Nutrient enters the world from fixed **sources** — think volcanic vents — each emitting
at a steady rate:

```ron
sources: [
    (pos: (-150.0, 150.0), nutrient: 0, rate: 12.0, color: (1.0, 0.55, 0.2), radius: 12.0),
    …
]
```

Emission plus diffusion produces a gradient: a bright core at the vent fading outward.
Vary the `rate` between sources and you get oases of different sizes — exactly the
demonstration in the [`nutrients`](../scenarios.md#02--nutrients) scenario, where four
graded vents grow four blooms you can read at a glance.

## Plants: absorb and spend

A plant interacts with the field through three genes
([Nutrients category](./genes.md#nutrients)):

- **`nutrient_absorption`** — it pulls mineral from the cell under it into its store,
- **`nutrient_capacity`** — the size of that store,
- **`offspring_nutrient`** — what it must hold (and spends) to seed a child.

Because the child is born with an *empty* store, the nutrient is a genuine *limiting*
resource, not a self-perpetuating endowment: a plant must keep absorbing to keep
breeding. Where the field is rich, plants thicken into a bloom; where it is barren, the
founders survive on sunlight but never multiply.

## The food web: nutrient travels by eating

Fauna usually cannot absorb from the ground. Instead, the **single interaction
primitive** carries the nutrient up the chain: when a predator eats prey
(`transfer: true`), it receives the share of the prey's nutrient store proportional to
the biomass it consumed. So a herbivore's nutrient comes from the plants it eats, a
carnivore's from the herbivores — and every level's reproduction is coupled to the
nutrient flowing up from the soil. This is what keeps a multi-level chain bounded
without overshoot.

## Recycling: closing the loop

When an agent **dies**, its accumulated nutrient store is returned to the field at the
cell where it fell (a brighter spot on the heatmap). Without this, eating would slowly
*destroy* the world's nutrient; with it, matter is *moved*, never created or destroyed —
conservation in spirit. Source → field → plant → herbivore → death → field again: the
closed loop you can watch end-to-end in the
[`nutrient_web`](../scenarios.md#12--nutrient-web) finale.

> **A known wall.** A closed, well-mixed nutrient loop tends to *oscillate*
> (Lotka–Volterra overshoot) rather than settle into a tidy steady state. The
> `nutrient_web` scenario is built to make the mechanisms *observable*, not to be a
> calibrated equilibrium — it stays lively for a good while, then the overshoot plays
> out, exactly as the dynamics predict.
