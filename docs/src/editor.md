# The editor

The windowed build (`teemlab`, launched with `play`) is **both** the authoring tool —
you build a scenario by hand — and the observation tool — you run it and watch evolution
unfold. None of it is simulation logic: the editor only reads and writes the scenario
*data*, outside the fixed loop. A scenario *is* its config, and three states coexist:

- the **file on disk** (`scenarios/*.ron`),
- the **config in memory** — what the panels edit, live,
- the **running world** — rebuilt from the config on **Reset**.

The window starts **paused** (on the empty canvas, or the scenario you passed on the
command line), so you can place, edit and inspect before launching.

## Layout

```
┌───────────────────────────────────────────────────────────────────────────┐
│  Scenario ▾  file *        ▶ Play ⏭ Step  ×1 ×2 …       View ▾   ⏺ Export… │  top
├───────────────┬───────────────────┬───────────────────┬───────────────────┤
│   WORLD       │  ARCHETYPE EDITOR │                   │     ANALYSIS      │
│  ▾ World      │  (opens on click) │   the simulation  │  ▸ Live stats     │
│  ▾ Archetypes │  Body·Genes·Brain │  (t = …s  PAUSED) │  Agent inspector  │
├───────────────┴───────────────────┴───────────────────┴───────────────────┤
│  Evolution — curves  (population per species · gene drift)                  │
└───────────────────────────────────────────────────────────────────────────┘
```

A **master / detail** split: the **World** panel (left) holds the scenario as a whole;
clicking an archetype opens its **editor** in a second column; **Analysis** (what you
read) is on the right; the **curves** run full-width along the bottom; the **simulation**
fills the centre, always framed and fully visible.

## Top strip

**Scenario ▾** — New (empty), Open ▸ (the `scenarios/*.ron` list, plus an arbitrary
path), Save / Save As. An amber `*` marks unsaved edits. Two guardrails: New/Open ask
before discarding edits, and Save never silently clobbers a bundled scenario (it offers
"Save a copy", since RON serialization drops comments).

**Transport** (centre) — ▶ Play/Pause (`Space`), ⏭ Step one tick while paused (`→`), a
logarithmic **speed** slider with ×1 ×2 ×5 ×10 presets, and ⟲ Reset (`R`) which rebuilds
the world from the current config.

**View ▾** — toggles the render **layers** (agents, nutrient heatmap) and inline help.
View-only; never saved with the scenario.

**⏺ Export…** — renders the current scenario to a [video](./recording.md) by driving the
headless `record` binary as a subprocess.

## World panel

Collapsible cards, ordered by how often you touch them:

- **Arena & generation** — arena half-size and the RNG seed (applied on Reset).
- **Relations** — the [interaction table](./model/interactions.md): each card is
  *actor → target*, transfer, rate/s, range. Read live by the sim.
- **Nutrients** — the [substrate field](./model/nutrients.md) (resolution, diffusion) and
  the emission sources (position, rate, colour).
- **Gene bounds** — the min/max of every gene; they bound both mutation and the sliders.
- **Appearance** — the background colours, with a live preview.

Below the cards, the **Archetypes** list: **drag** one into the arena to place it,
**click** to select (which opens its editor), **Delete** to remove the entity under the
cursor. ＋ Agent / ＋ Food create one; Duplicate / Move up / Move down / Delete act on the
selection. A **✦** marks an archetype carrying captured weights. The **Species library**
exports the selection to `species/*.ron`, imports a copy, or re-syncs an import.

## Archetype editor (opens on selection)

The selected species, in three cards:

- **Body** — name, colour, count, radius, max reserve.
- **Genes** — the founding genotype, in collapsible sections (Locomotion, Vision,
  Metabolism, Reproduction, Flora, Nutrients). Which sections open follows the entity's
  *kind*: for fauna the mobile axes open, for a plant the flora/nutrient axes do. Costs
  sort to the bottom of each section. The **Edit mutability** toggle reveals a per-gene
  "mutable?" checkbox beside each slider. Inert genes (e.g. vision on a plant) hide.
- **Brain** — [Wander, Hunter, Sessile, or MLP](./model/brains.md). For an MLP you edit
  the hidden-layer topology and see a structure graph; captured weights can be cleared.

## Analysis panel

- **Live stats** — population, food count, mean reserve and the mean of each gene.
- **Agent inspector** — **click an agent** to read it: identity (species / brain /
  generation / age), energy, genotype, the brain's action output, the **MLP activation
  graph** for learned brains, and per-ray perception (obstacle / target / threat).
  **💾 Capture as archetype** freezes this agent's *evolved genome and concrete weights*
  into a new reusable archetype — the seam that lets you reuse a trained brain.

## Curves

**Population per species** and **Gene drift** (the latter shows only *mutable* genes — a
frozen one would just be a flat line). Hover for a cross-hair tooltip with the time and
every value; ↻ Clear resets the sampled history.

## Keyboard shortcuts

| Key                    | Action                              |
| ---------------------- | ----------------------------------- |
| `Space`                | Play / pause                        |
| `→`                    | Step one tick (when paused)         |
| `R`                    | Reset the world                     |
| `Delete` / `Backspace` | Remove the entity under the cursor  |

Shortcuts are ignored while a text field has keyboard focus.
