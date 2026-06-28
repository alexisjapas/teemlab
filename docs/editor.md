# The editor — windowed build guide

The windowed binary (`teemlab`, launched with `play`) is **both** the *authoring*
tool — you build a scenario by hand — and the *observation* tool — you run it and
watch evolution unfold. Nothing here is simulation logic: the editor only reads and
writes the scenario **data** (a [`SimConfig`](../src/config.rs)), outside the fixed
loop (DEV Rule 1). A scenario *is* its `SimConfig`; three states coexist:

- the **file on disk** (`scenarios/*.ron`);
- the **config in memory** — what the panels edit, live;
- the **running world** — rebuilt from the config on **Reset**.

The window starts **paused** on the empty canvas (or on the scenario passed on the
command line), so you can place, edit and inspect before launching.

## Layout

```
┌───────────────────────────────────────────────────────────────────────────┐
│  Scenario ▾  file *          ▶ Play ⏭ Step  ×1 ×2 …      View ▾   ⏺ Export… │  top strip
├───────────────┬───────────────────────────────────────┬───────────────────┤
│   EDIT        │                                       │     ANALYSIS      │
│               │                                       │                   │
│  ▾ World      │            the simulation             │  ▸ Live stats     │
│  ▾ Entities   │         (t = …s · ×…   PAUSED)         │  Agent inspector  │
│               │                                       │                   │
├───────────────┴───────────────────────────────────────┴───────────────────┤
│  Evolution — curves  (population per species · gene drift)                  │
└───────────────────────────────────────────────────────────────────────────┘
```

**Edit** (everything you author) is on the **left**, **Analysis** (the state you
read) on the **right**, the evolution **curves** full-width at the bottom, and the
**simulation** fills the centre. The two side panels have a **fixed width** (equal,
so the sim stays centred) and are **not resizable** — the width is chosen to fit
their content; the curves panel auto-sizes to its content height.

## Top strip

### Scenario menu (`Scenario ▾`)

- **New (empty)** — start over from a blank canvas.
- **Open ▸** — the `scenarios/*.ron` list (refreshed each time the menu opens), plus
  an *Open path* field for an arbitrary file.
- **Save** / **Save As…** — write the current config to RON.

Next to the menu, the current file name shows with an amber **`*`** when there are
unsaved edits. Two guardrails protect your work:

- **New / Open** ask before **discarding unsaved edits**.
- **Save** never silently clobbers a file you did not create this session (a bundled
  scenario): it offers **“Save a copy”** instead — RON serialization drops the file’s
  comments and compact form. **Save As** onto an existing name also asks first.

### Transport controls (centred)

- **▶ Play / ⏸ Pause** — `Space`.
- **⏭ Step** — advance exactly one tick; enabled only while paused — `→`.
- **Speed** — a logarithmic slider (×0.1 … ×10) with quick presets **×1 ×2 ×5 ×10**
  (the active one stays highlighted).
- **⟲ Reset** — rebuild the world from the current config (re-spawns, reseeds,
  re-applies the sim rate, clears the history) — `R`.

### View menu (`View ▾`)

Toggles the render **layers**: the agents, and the nutrient-field **heatmap(s)**
(shown by default). A view concern only — never saved with the scenario.

### Export (`⏺ Export…`)

Opens a floating window to render the current scenario to a **video**: it drives the
headless `record` binary as a subprocess (a clean fresh re-render, without this UI),
encoded via `ffmpeg`. Configure the output file, duration, fps, size, the followed
agent, and the 9:16 HUD overlay.

## Edit panel (left)

### World

Collapsible cards, ordered by how often you touch them:

- **Arena & generation** — the arena half-size and the RNG **seed**. (The simulation
  rate `tick_hz` is a scenario-file parameter, not exposed here.) Applied on the next
  **Reset**.
- **Relations** — the **interaction table**: who acts on whom. Each card is
  *actor → target*, **transfer** (predation: the actor gains the drained energy;
  otherwise plain destruction), **rate/s**, and **range** (0 = contact). Read live by
  the sim.
- **Nutrients** — the substrate **field** (grid resolution, diffusion) and the
  emission **sources** (position, rate, colour). Applied on Reset.
- **Gene bounds** — the min/max of every gene; they bound both the **mutation** and
  the editor sliders. Global (shared by all archetypes).
- **Appearance** — the play-area and off-game **background colours** (saved with the
  scenario, with a live preview).

### Entities

- **Archetypes** — the species list. **Drag** one into the arena to place it,
  **click** to edit it, **Delete** (cursor on an entity) to remove it. **＋ Agent /
  ＋ Food** create one; **Duplicate / Move up / Move down / Delete** act on the
  selection. A **✦** marks an archetype carrying *captured weights*. The **Species
  library** exports the selection to `species/*.ron`, imports a copy, or resyncs an
  imported species from its source.
- **Archetype editor** — the selected archetype, in three cards:
  - **Body** — name, colour, count at spawn, body radius, max reserve.
  - **Genes** — the founding genotype, grouped into collapsible sections
    (Locomotion, Vision, Metabolism, Reproduction, Flora, Nutrients); the advanced
    flora / nutrient axes start collapsed to keep the common case lean, and within each
    section the **costs are listed last** (a uniform order). Each gene is a slider; the
    **Edit mutability** toggle at the top of the panel reveals a per-species
    **“mutable”** checkbox **beside** each gene (an aligned column on the left):
    checked ⇒ the gene drifts at reproduction, unchecked ⇒ transmitted but frozen at
    the founder’s value. Inert genes (locomotion, vision) stay hidden for an immobile
    entity — a section left with none disappears.
  - **Brain** — the decider: **Wander**, **Hunter** (hunt + flee), **Sessile**
    (flora), or **Network (MLP)** (learned by neuroevolution). For the MLP you edit
    the hidden-layer architecture (input/output are fixed by the contract) and see a
    structure graph; captured weights can be cleared.

## Analysis panel (right)

- **Live stats** (collapsed by default) — population, food count, mean reserve, and
  the mean of each gene, as a grid.
- **Agent inspector** — **click an agent** in the arena to read its state: *Identity*
  (species / brain / generation / age), *Energy*, *Genotype*, *Action* (the brain’s
  output), the **MLP activation graph** for learned brains (nodes coloured by
  activation, edges by weight), and *Perception* (per-ray obstacle / target / threat).
  **💾 Capture as archetype** freezes this agent’s **evolved genome and concrete
  weights** into a new reusable archetype (the original species is untouched).

## Curves (bottom)

- **Population per species** and **Gene drift** — the latter shows only the **mutable**
  genes (a frozen gene stays flat and would just clutter the plot). Light grid, a time
  axis; **hover** for a vertical cursor, a dot on each curve and a tooltip with the
  time and every value. **↻ Clear** resets the sampled history.

## The simulation area

- **Drag** an archetype from the palette to place it; **click** an agent to inspect
  it; **Delete / Backspace** removes the entity under the cursor.
- An overlay shows the **run time** and **speed**, with a prominent **PAUSED** banner
  when frozen.
- The whole square arena is always **centred and fully visible** — the camera fits it
  to whatever central area the panels leave free; the off-arena margin is greyed.

## Keyboard shortcuts

| Key                | Action                              |
| ------------------ | ----------------------------------- |
| `Space`            | Play / pause                        |
| `→`                | Step one tick (when paused)         |
| `R`                | Reset the world                     |
| `Delete` / `Backspace` | Remove the entity under the cursor |

Shortcuts are ignored while a text field (a path, a name…) has keyboard focus.
