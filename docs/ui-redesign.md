# teemlab — UI redesign (needs specification)

**Status.** Design synthesis, resolved in discussion **2026-07-14**. Binding as a
*needs* document once adopted; it is the brief handed to a designer.

**Altitude — read this first.** This document states **what each screen needs to
do and hold**, not **how to build it**. It decomposes every screen into content
blocks, says what each block must show or let the user do, and gives a **preferred
UI format** (orientation, footprint, density) as guidance — *not* a widget layout.
A designer owns the actual composition, spacing, and controls. Where this document
says "a list", "a form", "a diagram", read it as a *need*, not a prescription.

**Companion document.** The simulation-side redesign that this UI assumes —
**emergent (implicit) trophic interactions**, which removes the explicit relation
table and thereby removes a whole editing burden from the Studio — lives in
[`emergent-trophics.md`](emergent-trophics.md). Cross-references below point to it.

**Visual reference.** A high-fidelity visual comp of these five screens (produced
with Claude Design) is versioned at [`ui-mockups/teemlab.dc.html`](ui-mockups/) —
open it in a browser. Treat it as a **visual reference to re-implement in egui**,
not as consumable assets: it fixes the look, structure, and per-screen composition,
while behaviour and the simulation data it depicts are built per this document and
the companion. Two deliberate reconciliations the comp has **not** yet absorbed: the
per-archetype gene panel still shows the *old free cost genes*
(`move_cost` / `agility_cost`) that the allometric law replaces with **derived,
read-only** costs (companion §5), and the theme switcher is designer exploration
(ship a single theme at MVP).

**Framing (why the shape is what it is).** The project's resolved direction
(ROADMAP §0, 2026-07-14) is **a research bench, not a game**: the deliverable is
*falsifiable knowledge* about why ecosystems hold or collapse (a hypothesis, a
criterion, a control group, a curve). Every screen is shaped by that: the UI must
make it cheap to **compose an experiment, run it, and read a result**, and it must
treat *comparison* and *the science of fragility* as first-class, not decoration.

---

## 0. Method & vocabulary

**Preferred-format vocabulary** used per content block below:

- **Orientation** — *vertical column* (stacks, scrolls down), *horizontal strip*
  (wide and short), *2D spatial canvas* (a diagram/arena that needs area in both
  axes), *grid/gallery* (browsable cards).
- **Footprint** — *compact* (small, fixed), *tall* (long vertical scroll), *wide*
  (spans the width), *expansive* (wants a lot of the screen), *full-bleed* (the
  screen's centre of gravity).
- **Density** — *sparse-browsable* (few large targets, for scanning/choosing),
  *dense-editing* (many controls close together), *data-dense* (numbers, curves,
  tables).
- **Dominance** — *leads the screen* / *frames the edges* / *on-demand* (opens on a
  trigger, foldable away).

---

## 1. Hard constraints any design must respect

These are not style choices; they bound the solution space (their rationale is in
`docs/ui-spec.md` §2 and the ROADMAP native-UI post-mortem):

1. **One `Camera2d`.** The screens are **application states** over a single egui
   context and one camera, not separate cameras/windows. Only **Observe** actually
   renders the live arena through the camera; the other screens are panels. A screen
   router is a top-level state.
2. **egui stays.** The native-Bevy-UI migration was attempted and shelved; the
   redesign is a re-organisation *within* egui.
3. **No simulation logic in the UI.** The UI edits `SimConfig` (data), sets the
   virtual clock, or requests deferred actions (reset, load); it never mutates live
   sim state. The inspector is read-only.
4. **The sim stays byte-identical** under UI-only changes (the `tests/mlp.rs`
   tripwire). New persistence (run records) must be opt-in and inert when off.

---

## 2. The objects (nouns the screens manipulate)

The screens are organised around a small set of artifacts. Two of them are **new**
and are the reason the current single-screen UI feels cramped: today one *scenario*
file fuses the stage, the cast, and the wiring, and nothing is analysable after the
fact.

| Object | What it is | Persistence | New? |
|---|---|---|---|
| **World** | The abiotic stage: arena, sources/rocks, components (fields), gene bounds, allometric cost coefficients, appearance, seed. The environment **without inhabitants**. | catalog entry | **new** (today implicit inside a scenario) |
| **Species** (archetype) | Body + brain + genes (+ evolved weights for a variant), incl. its **nutritional profile** (needs/holds — see companion doc). | `species/*.ron` (examples committed, saved local) | exists |
| **Scenario** | A World **populated** by a cast of Species with counts. What runs. | `scenarios/*.ron` | exists (`SimConfig`) |
| **Experiment** | The **saved parameters** of a Lab run (which scenario, which search config, seeds). The MVP unit of "a manip I did". | catalog entry | **new (MVP)** |
| **Run record** | An Experiment **plus the time series it produced** (populations, genes, component quantities, events). The analysable artifact. | on disk, **optional** | **new (deferred)** |
| **Catalog** | The browsable collections of Worlds and Species (and later Experiments/records), across multiple libraries (committed vs local). | filesystem | management is **new** |

**Key consequence for the flow.** Because trophic interactions become **emergent**
(companion doc), composing a *Scenario* from a *World* plus *Species* **no longer
requires wiring a relation table** — dropping a species into a world is enough, and
a validator flags whether the resulting food web is viable. This is what makes the
low-friction Library flow (§8) possible.

---

## 3. Screen: **Observe**

*First in the fixed screen order: Observe · Library · Studio · Lab · Analyze.*
The default landing screen and the only one with the live arena.

**Purpose (the need).** Run **one** scenario live and *watch* it: read the live
state, follow agents, inspect one agent, optionally record a video, optionally
persist the run for later analysis.

**Content blocks.**

- **Live arena** — the simulation, rendered. Must support: pan / zoom / recenter;
  click-to-select an agent; toggleable **layers** (agents, component heatmaps, and
  the **dynamic trophic-graph overlay**, see §7). *Preferred format:* **2D spatial
  canvas, full-bleed, leads the screen** — the arena is square and must stay fully
  visible; everything else frames its edges and must be **foldable** so the arena
  can reclaim the space (observation wants the arena to lead).
- **Transport** — play / pause / step / speed / reset. *Format:* **horizontal
  strip, compact**, centred, framing the top.
- **Follow mode** — the auto-follow selector (hold-until-death, cycle, vanguard,
  …). *Format:* **compact**, near the arena.
- **Live stats** — per-species population and per-gene means. *Format:* **vertical
  column or compact grid, data-dense, on-demand** (foldable).
- **Agent inspector** — for the selected agent, read-only: identity (species,
  brain, generation, age), energy, per-component stores, genotype, action (incl.
  eat intent), the MLP activation graph for learned brains, perception channels.
  *Format:* **tall vertical column, data-dense, frames one edge**, its own scroll.
- **Curves** — population over time and gene drift. *Format:* **wide horizontal
  strip, short, data-dense**, frames the bottom.
- **Video export** — a *feature* invoked here (configure + launch an offline
  re-render). *Format:* **on-demand** floating surface; never permanent chrome.
- **Run-record toggle** — persist this run's metrics for Analyze. **Default OFF
  here** (observation is usually throwaway). *Format:* **compact toggle**.

**Entry / exit.** Entered from Library ("Observe this composition"), from Lab
("watch this result"), or as the launch default. Can hand a composition to Studio
(edit) or persist a run for Analyze.

**MVP vs deferred.** MVP: arena + transport + follow + live stats + inspector +
curves + video + trophic-graph overlay. Deferred: run-record persistence wiring is
only meaningful once Analyze exists (the toggle may be present but inert at MVP).

---

## 4. Screen: **Library**

**Purpose (the need).** The **low-friction entry** and the **catalog manager**
(the user's stated priority). Browse Worlds and Species; compose a Scenario in a few
clicks; manage the catalog (create, duplicate, rename, tag, delete, provenance).

**Content blocks.**

- **Worlds catalog** — a browsable collection of Worlds: name, a preview/thumbnail
  of the stage, tags, and metadata (which scenarios derive from it). *Format:*
  **grid/gallery, sparse-browsable, expansive** — this is a *scanning/choosing*
  surface, not a dense one.
- **Species catalog** — a browsable collection of Species/archetypes: name,
  provenance (captured-from lineage), tags, evolved-variant markers, and
  **cross-scenario usage** (which scenarios import this species). *Format:*
  **grid/gallery, sparse-browsable**; may sit as a sibling to Worlds (two galleries,
  tabbed or side by side).
- **Compose tray** — the *in-progress* selection: one chosen World + a set of
  Species with counts. Must let the user set counts and then **launch**: "Open in
  Studio" (edit further), "Observe" (watch), or "Send to Lab" (search/breed).
  *Format:* **compact horizontal strip or a docked side column**, always visible
  while composing.
- **Catalog management** — create / duplicate / rename / delete / tag entries;
  switch between **libraries** (committed `examples/` vs local `saved/`); search and
  filter. *Format:* **actions attached to each card + a global search bar**;
  *sparse-browsable*.
- **(Later) Experiments/records shelf** — once Analyze exists, saved Experiments and
  Run records are browsable here too.

**Entry / exit.** The hub. Exits into Studio, Observe, or Lab via the compose tray.
Receives *captures* back from Observe/Lab (a saved species, world, seed, or
generation appears here).

**MVP vs deferred.** MVP: both galleries, compose tray, and **catalog management**
(primordial). Deferred: rich metadata (behaviour notes), the Experiments/records
shelf, thumbnails if costly to generate.

---

## 5. Screen: **Studio**

**Purpose (the need).** Compose and **deep-edit** a World or a Scenario. Master /
detail editing of the stage and the cast, with **live validation** of the food web
(the static trophic-graph analyzer, §7), and a **save model** that offers
overwrite-vs-new for both Worlds and Species.

**Content blocks.**

- **World editor** — the stage as a whole: arena size + seed; **sources / rocks**
  (with a need for *place-by-click-in-arena*); **components** (each field's name,
  diffusion, decay); **gene bounds**; **allometric cost coefficients** (the
  world-level cost law — companion doc §5); **appearance**. *Format:* **tall
  vertical column of collapsible sections, dense-editing.* **Note:** the old
  *interaction relations* card is **gone** (interactions are emergent — companion
  doc); this is a deliberate, large simplification of this editor.
- **Cast / archetypes list** — the species in the scenario: add / duplicate /
  reorder / delete; drag to place into the arena; a marker for captured (evolved)
  weights. *Format:* **vertical list, compact**, the *master* of a master/detail.
- **Archetype editor (detail)** — the one selected species: **body** (name, colour,
  count, **size**; costs are now *derived and shown read-only* — companion doc §5);
  **genes** in **neutral categories** (no "Flora" category — companion doc §7);
  **nutritional profile** (the components it *needs* and *holds*, and its absorb /
  emit / sense behaviour); **brain** selector (Wander / Hunter / Grazer / Sessile /
  MLP); a **metabolic recipe** placeholder (deferred — companion doc §9). *Format:*
  **tall vertical column, dense-editing, on-demand** (opens on selection; a second
  column on a wide window, folds in place on a narrow one).
- **Trophic-graph validation panel** — the **static** food web derived from the
  current edit, with **live flags** for broken chains (an archetype whose required
  component is unreachable — companion doc §6). Must update *as the user edits*.
  *Format:* **2D spatial canvas, expansive, on-demand**; may overlay the arena or
  occupy a wide panel. The flags themselves also surface **inline** next to the
  offending species.
- **Save model** — for both World and Species: **overwrite** the loaded entry, or
  **save as new** (appears in the catalog); guardrails protect committed examples
  (offer save-a-copy). *Format:* **compact**, in the top command strip; a clear
  dirty/`*` marker.

**Entry / exit.** Entered from Library (edit a composition) or from Observe (tweak
what you're watching). Exits to Observe (run it) or back to Library (save to
catalog).

**MVP vs deferred.** MVP: World editor (minus relations), cast list, archetype
editor with neutral gene categories + nutritional profile, static validation
(broken-chain flags), overwrite-vs-new save. Deferred: place-by-click sources
polish, the metabolic-recipe editor, rich validation beyond reachability.

---

## 6. Screen: **Lab**

**Purpose (the need).** Run **headless** cohorts with **no live arena** — to
**breed/train** species to a situation **and/or sweep** worlds/parameters for
richness and robustness (**the two combinable** — see below). Show progress and
result metrics, **capture** results to the catalog, and **save the Experiment** (its
parameters). To *watch* any result, hand it off to Observe.

**Content blocks.**

- **Experiment setup** — pick a Scenario (from the catalog or a compose tray), then
  configure the **search**:
  - **Breeding** config — generations, cohort size, match length, fitness, scored
    species, survivors (the generational `run → score → breed` loop).
  - **Sweep** config — a seed sweep or a **parameter** sweep (which parameter, its
    range), scoring each world by biodiversity **and** by **web robustness/fragility**
    (companion doc §6, the new axis).
  - **Combination** (the need the user raised): the two must be **nestable** — e.g.
    *sweep a world parameter and, for each value, run a breeding* to see which
    species evolve where — i.e. a 2D search (an outer sweep over an inner breed, or
    vice-versa). The setup must express this nesting.
  *Format:* **vertical column, dense-editing** (a form); the setup *leads* until a
  run starts.
- **Run control** — run / stop, a **progress** read-out, a parallelism indicator
  (cohorts run across cores). *Format:* **horizontal strip, compact**, top of the
  results area.
- **Results** — **fitness-vs-generation** curves (breeding, per bred faction);
  **per-config scores** for a sweep (biodiversity + robustness); a **leaderboard**
  of elite genomes (inspect a genome's MLP graph); the **web-fragility metric**
  (companion doc §6). *Format:* **wide, data-dense**; curves + tables + leaderboard;
  this *leads* once a run is underway. **No arena** (deliberately).
- **Capture** — save an elite **genome/variant** to the Species catalog; save a
  **scenario / seed / generation** to the catalog; **save the Experiment** (its
  params — the MVP persistence unit). *Format:* **compact actions** on results rows.
- **Run-record toggle** — persist full metrics for Analyze. **Default ON here**
  (a training/search is worth analysing). *Format:* **compact toggle**.
- **Hand-off to Observe** — "watch this result live" re-seeds Observe from a chosen
  generation/genome. *Format:* **compact action**.

**Entry / exit.** Entered from Library (send a composition to search). Exits by
capturing to the catalog, saving an Experiment, or handing a result to Observe.

**MVP vs deferred.** MVP: breeding **and** sweep setup, combinable/nestable; run +
progress + results (curves, scores, leaderboard, fragility); capture to catalog;
**save Experiment (params only)**. Deferred: full run-record persistence wiring
(inert until Analyze exists), richer nesting UIs.

---

## 7. Screen: **Analyze** (placeholder — deferred)

**Purpose (the need), documented now, built later.** **Post-hoc** study of one or
several **Run records / Experiments**, and **comparison** between them. The user's
requirement: *select one or more simulations from the Lab (or persisted Observe
records) and compare* — overlay populations, gene trajectories, and **component
quantities over time**; compare species; export the data (CSV / plots) for the
"falsifiable knowledge" deliverable.

**Content blocks (future).**

- **Record selector** — multi-select over saved Experiments/records. *Format:*
  **vertical sidebar, sparse-browsable**.
- **Comparison plots** — overlaid or small-multiple time series across the selected
  records, per metric (population, per-gene, per-component). *Format:* **expansive,
  data-dense**, leads the screen.
- **Export** — CSV of series / PNG of plots. *Format:* **compact actions**.

**MVP now.** Only the **Experiment** object exists (saved parameters from the Lab).
The full **Run record** (persisted time series) and the **comparison** UI are
deferred; this screen is a **placeholder** in the router. See
[`emergent-trophics.md`](emergent-trophics.md) §9 for the run-record data need.

---

## 8. The catalog & the low-friction flow

The pipeline the whole redesign serves:

```
Library ── pick a World ──▶ + Species (counts) ──▶ [Scenario]
   ▲                                   │
   │                                   ├──▶ Studio  (edit; Save: ⟳ overwrite | + new → catalog)
   │                                   ├──▶ Observe (watch; → video / → run record)
   │                                   └──▶ Lab     (breed / sweep / both → results)
   │                                                     │
   └──── capture (Species · World · seed · generation · Experiment) ◀──┘
```

**Needs this flow imposes.**

- **Composition is drop-only.** Adding a Species to a World must not require wiring
  interactions — the emergent trophic model (companion doc) makes this true, and the
  **static validator** (in Studio) tells the user whether the resulting web is
  viable **before** running.
- **Save model, explicit.** Editing a World or Species offers two clear choices —
  **overwrite** the loaded entry, or **save as new** (a new catalog entry) — with
  guardrails for committed examples. (Today's scenario menu already protects bundled
  files; generalise it to Worlds and make both choices explicit rather than
  defensive.)
- **Capture closes the loop.** Anything discovered (an evolved species, a rich
  world, a good seed/generation, an Experiment's params) returns to the catalog and
  reappears in Library.
- **Catalog management is first-class** (primordial): browse, search, tag,
  duplicate, rename, delete, and see provenance/usage — across the committed and
  local libraries.

---

## 9. Cross-cutting needs

- **Video export** is a **feature, not a screen**: available from Observe (re-render
  the current scenario) and offline (a Lab result / a record). It must never occupy
  permanent chrome.
- **The trophic graph is one object with three surfaces** (companion doc §6): the
  **static** reachability validator in **Studio**, the **dynamic** fragility overlay
  in **Observe** (node size = population, edge thickness = flow, edge colour =
  dependency), and a **fragility metric** in the **Lab**. Build the derived-graph
  computation once; skin it three ways.
- **Feedback** — one transient status surface (info / success / error with
  lifetimes), consistent across screens.
- **Help is hover-first** — explanations in tooltips on controls/section headers;
  one shortcuts surface. No permanent inline-help layer.
- **The screen router** — a persistent top-level navigation across the five screens
  in the fixed order **Observe · Library · Studio · Lab · Analyze**. Only Observe
  binds the arena camera; switching away from Observe must release it cleanly (the
  one-camera constraint).

---

## 10. MVP boundary (summary)

**In:** the router + four working screens (Observe, Library, Studio, Lab) and the
Analyze **placeholder**; the World as a first-class catalog artifact; catalog
management; drop-only composition backed by the static validator; overwrite-vs-new
save; the trophic graph as Studio-validator + Observe-overlay; video as a feature;
run-record toggle (default OFF Observe / ON Lab) present but only fully meaningful
once Analyze lands; the **Experiment** (params) as the MVP persistence unit.

**Out (deferred, documented):** the Analyze screen proper, full **Run records**
(persisted time series) and **comparison**; rich catalog metadata (behaviour notes,
thumbnails); place-by-click source polish; the metabolic-recipe editor (rides with
the metabolization work — companion doc §9).
