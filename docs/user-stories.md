# teemlab — user stories (the real need behind the UI)

**Status.** Working draft (2026-07-22). The needs-first counterpart to
[`ui-redesign.md`](ui-redesign.md): that document decomposes the app into five
screens; this one enumerates the **jobs** the tool must serve, then reads a screen
structure back from them. The central question — should **Observe** and **Studio**
merge — is **resolved**: yes, into a single **Studio** tab, giving a **four-tab** app
(Studio · Lab · Analyze · Library) with Library also embedded in-context. See the
closing section.

**Method.** Each story is a *job*: `As <hat>, I want <capability>, so that <research
benefit>`, the benefit tied to the project's one deliverable — **falsifiable
knowledge about why an ecosystem holds or collapses** (ROADMAP §0, "a research bench,
not a game"). Stories carry **stable IDs** (`WLD-1`, `OBS-3`, …) for cross-reference,
and a status tag:

- **[done]** — built on `redesign/emergent-trophics`.
- **[mvp]** — in the Phase-B MVP boundary (planned; may be partial).
- **[deferred]** — documented, scheduled later.
- **[new]** — not built; a need surfaced or re-specified by this exercise.

---

## Personas — one researcher, several hats

teemlab is a single-user research bench, but the user wears distinct hats depending
on the phase of work. Keeping them separate is what lets us ask "which hat does this
screen serve, and does it serve it whole?"

- **Builder** — authors the raw material: a *World* (abiotic stage) and a *Species*
  (body + brain + genes + nutrition). Works close to the metal (SIM Law 7 pricing,
  gene bounds, cost law).
- **Composer** — assembles a runnable *Scenario* by dropping Species into a World
  with counts. Low-friction, catalog-driven.
- **Observer** — runs one scenario live, watches it, follows and inspects agents, and
  **tunes it while it runs** (the live-edit loop).
- **Experimenter** — runs *headless* cohorts (breed / train / sweep) with no live
  arena, to search a space and read result metrics.
- **Analyst** — studies *finished* runs post-hoc and **compares** them: the moment
  the "falsifiable knowledge" is actually extracted.
- **Curator** — manages the catalog of Worlds, Species, and Experiments across the
  committed and local libraries.

The **three authors** of SIM (Engine / Designer / Evolution) cut across these hats:
the Builder and Composer speak for the **Designer** (config-time), the Experimenter
hands the wheel to **Evolution** (run-time weights), and every hat relies on the
**Engine** staying fixed and deterministic (SIM Law 1, byte-identical).

---

## Epic WLD — Build a World (the abiotic stage)

*The environment without inhabitants: arena, sources/rocks, component fields, gene
bounds, the allometric cost law, appearance, seed.*

- **WLD-1** [done] As a Builder, I want to set the **arena size and seed**, so that I
  fix the stage's scale and its reproducible starting draw.
- **WLD-2** [new] As a Builder, I want to **place emission sources and rocks by
  drag-and-drop directly in the arena**, so that I sculpt spatial structure — vents,
  gradients, refugia, barriers, winding zones — *while seeing where each element
  lands*. Sources and rocks are the two **spatial world elements** and share the exact
  same placement need. Today the World editor can add/remove a source or rock and set
  its position **only numerically**, and the editor has **no arena in view**, so
  placement is effectively *blind*. Drag-n-drop needs the arena and the editor on the
  **same screen** — a driver of the Observe/Studio merge, covering **all** spatial
  world editing (see also SCN-6).
- **WLD-3** [done] As a Builder, I want to define **component fields** (name,
  diffusion, decay), so that I control the diffusing resources/signals agents live off
  and emit into.
- **WLD-4** [done] As a Builder, I want to set **gene bounds** (the min/max each
  priced trait may reach), so that I bound the evolutionary search space per world.
  Because bounds are **world-level**, they must be validated against the cast — see
  VAL-5.
- **WLD-5** [done] As a Builder, I want to tune the **allometric cost coefficients**
  (the world-level cost law, from which per-body costs derive), so that I set the
  *price of being alive* for the whole stage (SIM Law 7) in one place.
- **WLD-6** [new] As a Builder, I want a **warm-up / initial-nutrient seeding** option,
  so that a world starts from a plausible standing stock rather than empty. The
  *mechanism* exists as scenario data (`random_initial_nutrients`,
  `random_initial_energy`, `ComponentConfig.initial`, honoured by `spawn.rs`), but
  there is **no Builder UI** for it — it is set by hand in RON only.
- **WLD-7** [mvp] As a Builder, I want the World to be a **first-class savable
  artifact** (its own catalog entry, independent of any cast), so that one stage can
  back many scenarios and be reused/compared.
- **WLD-8** [folded into WLD-2] The **split-arena barrier** (the diagonal wall of rocks
  that halves the arena into a control/treatment pair) is **not** its own device: it is
  a *use-case* of rock placement. It is a **structured line**, so it is served by WLD-2
  (drag-n-drop) or explicit placement — not by the WLD-9 random scatter. A bespoke
  barrier device would violate the "fewest arbitrary values / constant-as-data"
  discipline (ROADMAP §0); the primitives WLD-2 / WLD-9 / WLD-10 are what it needs.
- **WLD-9** [new] As a Builder, I want to **define a rock field by parameters** — e.g.
  "between A and B rocks, each of radius between C and D", over a region, from a seed —
  so that I populate a stage with structure without placing every rock by hand,
  reproducibly. It is a **seeded generator** (same seed + params → same scatter, so it
  stays *constant-as-data*, ROADMAP §0); an empty spec draws **no RNG** (existing
  scenarios byte-identical); it needs a **placement distribution** (uniform in the
  arena, or within a region / spawn-zone) and **overlap handling**. Data-authored, but
  best **previewed with the arena**. *Open (symmetric) question:* whether the scatter
  should also serve **sources** (a field of vents), not only rocks — the same generator
  would.
- **WLD-10** [new] As a Builder, I want **rocks to be a first-class type, dissociated
  from sources** (today a rock is a `Source { solid: true }`), so that a pure physical
  obstacle does not carry meaningless emitter fields (`component`, `rate`) and the two
  concepts — *a field emitter* vs *a body-blocking structure* — stop being conflated.
  This is the enabler for WLD-2 / WLD-9 (a `rocks` list to place into / generate).
  *Byte-identity:* clean — **every existing rock has `rate: 0.0`** (verified in `02`,
  `05`, `reef`), so a `Rock` with no emission is numerically identical; the disciplined
  path is **additive** (a new `#[serde(default)] rocks: Vec<Rock>`, empty default → old
  scenarios untouched), then migrate the solid-source scenarios and **verify
  byte-identical on those specific scenarios** (the `tests/mlp` tripwire has no rocks,
  so it does not cover this — spawn-order / collider parity must be checked on
  `02`/`05`).

## Epic SPC — Build a Species (the archetype)

*Body + brain + genes + nutritional profile.*

- **SPC-1** [done] As a Builder, I want to set the **body**: name, colour, size (with
  costs shown **derived and read-only**, per the allometric law), so that I see the
  price of a body without being able to cheat it (SIM Law 7).
- **SPC-2** [done] As a Builder, I want to edit **genes in neutral categories** (no
  "Flora" vs "Fauna" split), so that the cast is not pre-sorted into trophic roles the
  engine is supposed to *derive* (SIM Laws 8 & 11).
- **SPC-3** [done] As a Builder, I want to set the **nutritional profile** — which
  components the species *needs*, *holds*, and its *absorb / emit / sense / affect*
  behaviour (the `field_relations`) — so that I define what it eats and secretes
  without wiring an explicit predator/prey table.
- **SPC-4** [done] As a Builder, I want to choose the **brain** (Wander / Hunter /
  Grazer / Sessile / MLP), so that I pick who authors the decision — Designer rules or
  evolved weights (SIM three authors).
- **SPC-5** [mvp] As a Builder, I want to mark a species as carrying **evolved weights**
  (a captured variant), so that a trained genome is reusable as a first-class archetype.
- **SPC-6** [deferred] As a Builder, I want a **metabolic-recipe** editor (component →
  component transformation), so that distinct trophic tiers can partition resources (the
  blocker for the 3-level web and the toxin scene — companion §9).
- **SPC-7** [new] As a Builder, I want a species' **nutritional profile to travel with
  it through the catalog** (not be re-set per scenario), so that dropping a captured
  species into a new world keeps its diet (recorded gap, ui-redesign §11 (c)).

## Epic SCN — Compose a Scenario (World + cast)

- **SCN-1** [mvp] As a Composer, I want to pick **one World** and **drop Species into it
  with counts** — no interaction wiring — so that assembling a runnable scenario is a
  few clicks (emergent trophics makes this possible).
- **SCN-2** [done] As a Composer, I want **per-species founding spawn zones** (confine
  to / exclude from a region), so that I place founders deliberately — e.g. one herd per
  half of a split arena.
- **SCN-3** [mvp] As a Composer, I want a **live viability chip** on the compose tray,
  so that I know *before* launching whether the food web can even close.
- **SCN-4** [mvp] As a Composer, I want to **observe, edit, and easily run searches
  starting from my scene**, so that a scene I have in hand flows into any next step
  without friction. The need is that a scene is *one object* I can watch, tweak, or
  search from; the screens that deliver that are an implementation choice.
- **SCN-5** [new] As a Composer, I want to **duplicate an existing scenario as a
  starting point**, so that a variant (one parameter changed) is cheap to author — the
  atomic move of an experiment.
- **SCN-6** [new] As a Composer, I want to **define spawn zones from the editor** (method
  TBD, non-urgent), so that founder placement is authored visually rather than
  hand-written in RON. Spatial editing again — like WLD-2 it wants the arena in view.

## Epic VAL — Validate viability (the food web)

*The derived trophic graph, one object with three surfaces.*

- **VAL-1** [done] As a Builder/Composer, I want a **static reachability validator** —
  the derived food web with **broken-chain flags** (a species whose required component
  is unreachable) updating **as I edit** — so that I catch a dead web before spending a
  run on it.
- **VAL-2** [done] As an Observer, I want the **dynamic trophic graph** (node size ∝
  population, edge thickness ∝ flow, edge colour ∝ dependency) **in a panel** (like the
  MLP brain inspector), **not** as an arena overlay, so that I read the web breathing
  without cluttering the arena. Form: panel, not overlay — the derived-graph computation
  is unchanged.
- **VAL-3** [done] As an Experimenter/Analyst, I want a **web-fragility metric**
  (diet-concentration / Herfindahl), so that fragility is a *number* I can sweep, score,
  and compare — not just a picture.
- **VAL-4** [new] As a Builder, I want the validator to explain **why** a chain is broken
  (which component, reachable from where), so that a flag is actionable, not just red.
- **VAL-5** [new] As a Builder/Composer, I want the scenario validator to **flag any
  archetype whose gene value falls outside the world's gene bounds** (a config-level
  check, per gene, per species), so that an out-of-bounds cast is caught *statically*
  before a run — bounds are **world-level** (WLD-4), so a species imported from another
  world can silently violate them. *Done when:* for every archetype and every priced
  gene, the validator asserts `bounds.min ≤ gene ≤ bounds.max` and names the offending
  species + gene inline, next to the broken-chain flags (VAL-1).

## Epic OBS — Observe a run live

- **OBS-1** [done] As an Observer, I want **transport** (play / pause / step / speed /
  reset), so that I control the run's time — including stepping one tick to watch a
  transition.
- **OBS-2** [new] As an Observer, I want the arena and its panels to **coexist in a
  stable, fixed multi-panel layout** — no foldable panels, no full-screen arena — so
  that the workbench is predictable. This makes a permanent editor panel beside the
  arena natural.
- **OBS-3** [done] As an Observer, I want **pan / zoom / recenter** and a stable
  **fit-to-arena**, so that I can move between the whole stage and a local detail.
- **OBS-4** [done] As an Observer, I want **follow modes** (hold-until-death, cycle,
  vanguard, …), so that the camera tracks a life or a lineage without manual chasing.
- **OBS-5** [done] As an Observer, I want **toggleable arena layers** (agents, component
  heatmaps), so that I read one signal at a time over the arena. (The trophic graph is a
  panel, not an arena layer — VAL-2.)
- **OBS-6** [done] As an Observer, I want **live population and per-gene stats**, so that
  I read the aggregate state at a glance.
- **OBS-7** [done] As an Observer, I want **live curves** — population over time and gene
  drift, **filterable by gene and by species** — so that I watch trajectories, not just
  instantaneous values, and isolate one lineage.
- **OBS-8** [new] As an Observer, I want a run to **auto-stop on a condition** (e.g. a
  brain-filtered subset going extinct), so that a dead run is not left padding an empty
  arena. This exists only in the `record` bin today (offline video, `--stop-when BRAINS
  [--stop-after S]`); bringing it to the **live** run is the need here.

## Epic INS — Inspect one agent

- **INS-1** [done] As an Observer, I want to **click to select** an agent and see its
  **identity** (species, brain, generation, age), so that I can study an individual.
- **INS-2** [done] As an Observer, I want the inspector to show **energy, per-component
  stores, genotype, current action (incl. eat intent), and perception channels**,
  read-only, so that I understand *why* this agent is doing what it does.
- **INS-3** [done] As an Observer, I want the **MLP activation graph** for a learned
  brain, so that an evolved decision is legible, not a black box.
- **INS-4** [mvp] As an Observer, I want to **capture** the selected agent's species /
  genome back to the catalog, so that an interesting individual becomes reusable
  material.
- **INS-5** [new] As an Observer, I want to see the **parent and children** of the
  selected agent, so that I can follow a lineage one hop at a time (backed by the
  generation / inert `Lineage(u16)` tag already tracked). **Later (→ Analyze):** the
  **full lineage tree**, with **dead branches** vs lineages that **outperformed their
  environment** — see ANL-5.

## Epic TUN — Tune while watching (the live-edit loop)

- **TUN-1** [new] As an Observer, I want to **edit a live-applied parameter** (a diet
  `field_relation`, a gene bound, a cost coefficient, predation, colours) **and see the
  running population respond immediately**, so that I can feel the effect of a rule
  without a restart. (The data model already applies these live — `world_diverged` in
  `controls.rs` lists them as reset-exempt.)
- **TUN-2** [new] As an Observer, I want to **edit an initial-condition parameter**
  (arena size, sources, counts, seed, initial nutrients) **and re-seed with one Reset**
  without leaving the arena, so that the edit→run→watch loop stays tight. (These cannot
  apply retroactively to a running population; they define how the world is built.) I
  also want to **save the resulting world** — as *itself* or as a *new* world — with a
  **guard-rail popup when the world is shared by other scenarios** (overwriting it could
  silently break them), so that a live tweak can become a kept artifact without
  collateral damage. (Needs the cross-usage knowledge of CAT-5.)
- **TUN-3** [done] As an Observer, I want a **visible signal that an edit is waiting for
  a Reset** (the accented Reset when the live config diverges from the world's baseline),
  so that I always know whether what I see reflects what I edited. (Built: `WorldBaseline`
  + `world_diverged` + the accented Reset button.)
- **TUN-4** [new] As an Observer, I want the **editor available beside the arena** (not
  on another screen), so that I do not lose the running state and my place when I tune.
  Resolved: the editor and the arena share the **Studio** tab (see the closing section).
- **TUN-5** [new] As an Observer, I want to tune **without accidentally corrupting a
  committed example** (edits stay in a working copy; saving is an explicit, guard-railed
  choice), so that live experimentation is safe.

## Epic VID — Record video

- **VID-1** [done] As an Observer, I want to **configure and launch an offline re-render**
  of the current scenario to a 9:16 video, so that I can share a run.
- **VID-2** [done] As an Observer, I want the recording to honour **display choices**
  (which genes/species the curves show) and a **stop-when-extinct** bound, so that the
  film shows what I meant and does not run past the story.
- **VID-3** [mvp] As an Observer, I want video to be a **feature, never permanent chrome**
  (an on-demand surface), so that it does not steal space from the arena.

## Epic BRD — Breed / train (headless search over behaviour)

- **BRD-1** [mvp] As an Experimenter, I want to configure a **breeding run** (generations,
  cohort size, match length, fitness function, scored species, survivors), so that I
  evolve competent behaviour under a stated selection pressure.
- **BRD-2** [mvp] As an Experimenter, I want **no live arena** during a headless run
  (progress + parallelism read-out instead), so that cohorts run fast across cores.
- **BRD-3** [mvp] As an Experimenter, I want **fitness-vs-generation curves** and an
  **elite leaderboard** (inspect a genome's MLP), so that I read whether and how the
  search is improving.
- **BRD-4** [mvp] As an Experimenter, I want to choose the **fitness function**
  (Population, lineage-relative Advantage `w/w̄`, …), so that I can escape the
  carrying-capacity flatline and select on the axis I care about.
- **BRD-5** [deferred] As an Experimenter, I want to **watch a chosen generation/genome
  live** (hand off to the arena), so that I can see what a number-on-a-curve actually
  does.

## Epic SWP — Sweep (headless search over worlds/parameters)

- **SWP-1** [mvp] As an Experimenter, I want to configure a **seed or parameter sweep**
  (which parameter, its range), so that I map how an outcome depends on a world constant.
- **SWP-2** [mvp] As an Experimenter, I want to **choose (and combine) the scoring
  function** for a sweep — biodiversity, web-fragility, or others — **the same way
  breeding lets me choose a fitness function** (BRD-4), rather than a fixed
  biodiversity+fragility pair, so that I rank worlds on the axis I care about. *Open:*
  unify sweep-scoring and breeding-fitness into one choosable/combinable scoring concept.
- **SWP-3** [mvp] As an Experimenter, I want breeding and sweeping to be **nestable**
  (sweep a world parameter, breed inside each value), so that I can ask "which species
  evolve *where*" — a 2D search.
- **SWP-4** [deferred] As an Experimenter, I want the nested sweep to **run in-app** (today
  it is authored/saved in-app but executed via the `sweep` bin), so that the whole search
  lives in one place.
- **SWP-5** [new] As an Experimenter, I want a sweep to **log what it dropped** (top-N
  truncation, no-retry), so that a bounded search does not read as exhaustive coverage.

## Epic CAP — Capture discoveries back to the catalog

- **CAP-1** [mvp] As any hat, I want to **capture a Species / World / seed / generation /
  genome** into the catalog, so that anything discovered mid-work becomes reusable
  material (the loop that closes the pipeline).
- **CAP-2** [mvp] As an Experimenter, I want to **save an Experiment** (its parameters:
  scenario, search config, seeds), so that a manipulation I did is reproducible — the MVP
  unit of persistence.

## Epic REC — Persist runs (the analysable artifact)

- **REC-1** [deferred] As any hat, I want to **persist a run's full time series**
  (populations, genes, component quantities, events) as a Run record, so that a run is
  analysable *after* it ends.
- **REC-2** [mvp] As an Observer, I want run-record persistence **OFF by default**
  (observation is usually throwaway) with an explicit toggle, so that I opt in only when
  a run is worth keeping.
- **REC-3** [mvp] As an Experimenter, I want run-record persistence **ON by default** for
  a search, so that a training/sweep is analysable without my remembering to arm it.

## Epic ANL — Analyze & compare (extract the knowledge)

- **ANL-1** [deferred] As an Analyst, I want to **select one or several finished
  runs/Experiments**, so that I can study and compare manipulations post-hoc.
- **ANL-2** [deferred] As an Analyst, I want **overlaid / small-multiple time series**
  across selected records (population, per-gene, per-component), so that I read a
  *difference between conditions*, not a single run in isolation.
- **ANL-3** [deferred] As an Analyst, I want to **compare against a control** (the
  split-arena or a frozen-gene half), so that a claim has the control group the
  deliverable requires.
- **ANL-4** [deferred] As an Analyst, I want to **export** series (CSV) and plots (PNG),
  so that the falsifiable-knowledge deliverable leaves the tool.
- **ANL-5** [deferred] As an Analyst, I want to **reconstruct and study full lineage
  trees** from a run — visualise the branching, mark **extinct branches**, and surface
  the lineages that **outperformed their environment** — so that *which* genealogies won
  or died, and why, becomes analysable (the post-hoc home for INS-5's one-hop view).

## Epic CAT — Manage the catalog

- **CAT-1** [mvp] As a Curator, I want to **browse Worlds and Species galleries** (name,
  preview, tags, metadata), so that I can scan and choose, not hunt files.
- **CAT-2** [mvp] As a Curator, I want library membership (committed `examples/` vs local
  `saved/`) to be a **multi-select filter** — the same control as the gene/species curve
  filters — **not a hard switch**, with **examples visually distinguished** (e.g. a
  special colour), so that I can view both at once and always tell bundled from personal
  material at a glance.
- **CAT-3** [mvp] As a Curator, I want to **delete saved entries** and be **guard-railed
  against clobbering committed examples** (offer save-a-copy), so that the reference set
  stays intact.
- **CAT-4** [deferred] As a Curator, I want to **duplicate / rename / tag** entries, so
  that the catalog stays organised as it grows.
- **CAT-5** [deferred] As a Curator, I want to see **provenance and cross-usage** (this
  species captured from lineage X; used by scenarios Y, Z), so that I understand where
  material came from and what depends on it.
- **CAT-6** [deferred] Beyond a Library **tab** that holds *all* material (worlds,
  scenarios, species, experiments, brains…), I want a **reusable library picker**
  available **at every place material is consumed**, in the **same form as the Library
  tab** — invoked as an **in-context sub-page** that *replaces* the panel it was called
  from (an "invisible" panel) with **back navigation**. Example: in the scenario editor,
  open the picker to select and compose material, then return. This is really a **SYS**
  pattern — one library component, many entry points (see SYS-5).

## Epic DET — Determinism & scientific method (cross-cutting)

*These are not a screen; they are constraints every story inherits.*

- **DET-1** [done] As any hat, I want the sim to stay **byte-identical** under UI-only and
  additive changes (the `tests/mlp.rs` tripwire), so that a result is a property of the
  *scenario*, never of the tool version.
- **DET-2** [done] As any hat, I want **every constant that shapes an outcome to live in
  the scenario (data), not the engine**, so that it is an experimental parameter, not a
  hidden confound (ROADMAP §0 methodology).
- **DET-3** [done] As any hat, I want **priced traits with no free beneficial gene** (SIM
  Law 7), so that selection has a real gradient to act on.
- **DET-4** [new] As a researcher, I want a scenario to **carry its own hypothesis and
  control** (what it demonstrates, what the control half is), so that an *example to
  watch* states its falsifiable claim, not just its mechanics.
- **DET-5** [new] As a researcher, I want to **reproduce a past result from its
  Experiment** (same scenario + seeds → same curves), so that a finding is checkable.

## Epic SYS — System, help, feedback (cross-cutting)

- **SYS-1** [done] As any hat, I want **one persistent screen router** (one camera bound
  only where the arena shows), so that navigation is predictable and the one-camera
  constraint holds.
- **SYS-2** [done] As any hat, I want **one transient feedback surface** (info / success /
  error with lifetimes), consistent across screens, so that outcomes are reported in one
  place.
- **SYS-3** [done] As any hat, I want **hover-first help** (tooltips on controls and
  section headers) and **one shortcuts surface**, so that help never becomes permanent
  clutter.
- **SYS-4** [mvp] As any hat, I want the app to **start paused**, so that I can place,
  edit, and prepare before anything runs.
- **SYS-5** [new] As any hat, I want **one reusable "material picker" surface** (the
  Library's browse/compose UI) that can be **opened in-context anywhere material is
  used**, as a drill-in sub-page with back navigation, so that selecting material is the
  same gesture everywhere and never forces a detour to the Library screen (see CAT-6).

---

## Reading the stories back against a screen structure

Most epics map cleanly to a single mode: **Library** is browse/compose, **Lab** is
headless search, **Analyze** is post-hoc study, and *watching* the arena is its own
thing. Two needs, however, are torn by any **Observe/Studio split**, and both require
**the editor and the arena on one screen**:

1. **TUN — tune while watching.** Editing a rule and feeling the running population
   respond needs the editor beside the live arena (the *parameter* case).
2. **WLD-2 / SCN-6 — spatial placement.** Placing sources, rocks, or spawn zones is
   *blind* without the arena in view (the *spatial* case).

Two facts make merging cheap rather than costly:

- The data model **already** splits edits into *live-applied* (diets, gene bounds, cost
  law, predation, colours, display filters) and *reset-bound* (arena, sources, counts,
  seed, initial nutrients), and **already** signals a pending reset (`world_diverged` +
  the accented Reset). TUN-1/2/3 are largely wired; TUN-4 (editor next to the arena) is
  what is missing.
- A **fixed multi-panel workbench** (OBS-2) makes the editor a permanent panel beside
  the arena, and the trophic graph its own panels — static validator (VAL-1/5) + dynamic
  graph (VAL-2), no overlay.

## Resolved — the front-end structure (2026-07-22)

The app is **four tabs** — Studio · Lab · Analyze · Library:

- **Studio** (*observe + edit*) — the **workbench** and the **only arena-bound screen**
  (the one-camera constraint binds *here*, not a separate Observe). It holds the live
  arena + transport + follow (OBS), the agent inspector + lineage hop (INS), live stats
  + curves (OBS-6/7), the **World / cast / species editor** with **drag-n-drop spatial
  placement** (WLD-2, SCN-6) and **live parameter tuning** (TUN), and the trophic graph
  as **panels** — static validator (VAL-1/5) + dynamic graph (VAL-2). **Observe is
  absorbed; there is no separate Observe tab.**
- **Lab** (*breed + sweep*) — headless search, no arena (BRD, SWP, CAP).
- **Analyze** — post-hoc records, comparison, and lineage trees (REC, ANL incl. ANL-5).
- **Library** — the catalog of *all* material (worlds, scenarios, species, experiments,
  brains). Each Library sub-view is **also reachable as an in-context panel inside the
  other tabs, only where relevant** (the SYS-5 / CAT-6 picker): e.g. the Studio editor
  opens a Library panel to pick / compose material, then returns. So "Library" is both a
  **tab** *and* an **ambient component** embedded at every point of consumption.

**Consequences for the spec.** This drops the five fixed screens (Observe · Library ·
Studio · Lab · Analyze) to **four** (Studio · Lab · Analyze · Library), folds Observe
into Studio, and makes Library ambient. It **supersedes `ui-redesign.md`** on: the
screen list (§9), the one-camera binding (now Studio, not Observe — §1/§3), the dynamic
trophic surface (overlay → **panel**, VAL-2 vs §7/§9), and the foldable arena-leads
layout (fixed multi-panel workbench, OBS-2 vs §3). Reconcile that document when this
lands.

**Still open** (not blockers for the structure): the *method* for spatial placement
(the drag-n-drop interaction; SCN-6 spawn-zone editing); whether the parametric scatter
also serves sources (WLD-9); unifying sweep-scoring with breeding-fitness (SWP-2); and
build sequencing (WLD-10 → WLD-9, then WLD-2 with the merge).
