# Scenario file format

A scenario is a single [RON](https://github.com/ron-rs/ron) file describing one world.
RON is chosen for being hand-readable and annotatable — comments, named fields, hex
literals. The engine deserializes it into a `SimConfig`; both the windowed and headless
builds load it identically.

> **Partial by design.** A scenario only states what it *changes*; every omitted field
> falls back to the engine default. An empty `()` is a valid scenario (= all defaults).
> Adding a new field to the engine therefore never breaks an existing file. Unknown
> fields, by contrast, are *rejected* — a typo is a loud error, not a silent default.

## Top level

```ron
(
    tick_hz: 64.0,              // fixed-timestep rate (solver stability, not rendering)
    arena_half_extent: 480.0,   // half-side of the square arena, in world units
    seed: 12648430,             // RNG seed — replays a config, not bit-for-bit (see Law 10)

    archetypes: [ … ],          // the species — the central data (see below)
    relations:  [ … ],          // who may act on whom (see Interactions)

    nutrient: ( … ),            // the substrate field
    sources:  [ … ],            // nutrient emitters

    // Per-gene bounds (global): each is `(min: …, max: …)`. They clamp both mutation
    // and the editor sliders.
    speed_bounds: (min: 40.0, max: 280.0),
    agility_bounds: (min: 0.02, max: 0.5),
    // … one *_bounds entry per gene (see the gene reference) …

    // Presentation (windowed only), saved with the scenario:
    play_area_color: (0.07, 0.07, 0.09),
    off_game_color:  (0.17, 0.17, 0.19),
)
```

| Field               | Type        | Meaning |
| ------------------- | ----------- | ------- |
| `tick_hz`           | float       | Fixed simulation rate. Higher = finer physics, more CPU. |
| `arena_half_extent` | float       | Half the side of the square arena. |
| `seed`              | int (hex ok)| Seeds the deterministic RNG. Same seed + config ⇒ same *experiment*. |
| `archetypes`        | list        | The species. **Order matters**: an index is a species' identity. |
| `relations`         | list        | The interaction table. |
| `nutrient`          | record      | The substrate field parameters. |
| `sources`           | list        | Nutrient emitters. |
| `*_bounds`          | record      | `(min, max)` for each gene. |
| `play_area_color` / `off_game_color` | rgb | Background tints (rendering only). |

## Archetype (a species)

The heart of a scenario. Its **index** in the `archetypes` list is its identity — what
the relation table targets.

```ron
(
    name: "Hunter",
    color: (0.3, 0.7, 1.0),     // rgb in [0, 1]
    count: 12,                  // how many to spawn
    radius: 8.0,                // body & collider radius
    reserve_max: 120.0,         // energy/HP capacity
    genotype: ( … ),            // the founding genes (see the gene reference)
    brain: Hunter,              // the decider (see Brains)
    mutable: ( … ),             // per-gene "may it mutate?" flags
    // optional:
    // source: "species/examples/hunter.ron",   // provenance of an imported species
    // captured_brain: Some(Mlp(( … ))),         // frozen learned weights
)
```

| Field            | Meaning |
| ---------------- | ------- |
| `name`, `color`  | Label and tint for the palette / inspector. |
| `count`          | Number spawned at start. `0` = declared but not placed (author by hand). |
| `radius`         | Body size (also the physics collider). |
| `reserve_max`    | Maximum energy the body can hold. |
| `genotype`       | The founding [genes](./model/genes.md). Partial — omit a gene to take its default. |
| `brain`          | The [brain](./model/brains.md): `Wander(turn_rate: …)`, `Hunter`, `Sessile`, or `Mlp(hidden: […])`. |
| `mutable`        | A record of booleans, one per gene — may that gene drift in this species? |
| `source`         | *(optional)* the library file this was imported from (kept for re-sync). |
| `captured_brain` | *(optional)* concrete frozen weights, so founders are born already trained. |

### Genotype

The genes, each optional (omit to take the engine default). The full list, with default
values, bounds, costs and mutability, is the [gene reference](./model/genes.md). A
sessile plant, for instance, sets `max_speed: 0` and turns on `photosynthesis`.

### Brain

```ron
brain: Wander(turn_rate: 0.25)   // random walk
brain: Hunter                    // chase target, flee threat
brain: Sessile                   // a plant — decides and moves nothing
brain: Mlp(hidden: [10])         // a learned network; [10] = one hidden layer of 10
```

### Mutability

```ron
mutable: (
    max_speed: true,
    vision_range: true,
    base_metabolism: false,   // costs are usually frozen
    photosynthesis: false,
    // … one flag per gene …
)
```

A frozen gene is still *inherited* — it just does not drift. Costs and the mutation rate
are frozen by default so they cannot evolve away the very pressure that drives selection.

## Relation (the interaction table)

```ron
relations: [
    (actor: 0, target: 1, transfer: true,  rate: 45.0, range: 16.0),  // predation
    (actor: 1, target: 1, transfer: false, rate: 8.0,  range: 32.0),  // self-competition
]
```

`actor` / `target` are archetype **indices**. `transfer: true` = predation (the actor
gains), `false` = combat/competition (destruction). `rate` is reserve/second of contact;
`range` is the surface-to-surface gap (`0` = touch). See
[Interactions](./model/interactions.md).

## Nutrient field & sources

```ron
nutrient: (
    resolution: 256,   // grid cells per side
    diffusion: 0.3,    // spread per tick, in [0, 1] (0 = no spreading)
),
sources: [
    (pos: (-220.0, 220.0), nutrient: 0, rate: 12.0, color: (1.0, 0.55, 0.2), radius: 12.0),
],
```

`pos` is world coordinates; `rate` is emission per second; `nutrient` is the index
(always `0` for now); `color`/`radius` are the vent's visual only. See
[The nutrient substrate](./model/nutrients.md).

## Tips

- **Keep scenarios self-contained.** When you import a species from the library it is
  copied in (with a `source` link for re-syncing), never referenced live — so a scenario
  always carries everything it needs to run.
- **The seed replays a *configuration*, not bits.** Determinism is traded for
  parallelism; the same seed reproduces an experiment to compare parameters, not a
  bit-identical movie (see [the laws](./laws.md)).
- The easiest way to learn the format is to **open an example in the editor**, change
  something, and **Save a copy** — then diff the RON.
