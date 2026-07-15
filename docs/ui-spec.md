# teemlab — windowed UI specification

**Purpose.** The reference for a front review of the windowed binary (`teemlab`,
launched via `play`). It states what the UI is *supposed* to do — layout, behavior,
states, visual system, inputs, invariants — so a reviewer can check the
implementation (and the user-facing docs) against one document. Each section names
the module(s) that implement it.

**Scope.** The interactive windowed front only: the egui docked panels, the central
sim rendering it frames, the overlays and floating windows. Out of scope: the sim
logic itself (`SimPlugin`), the headless binaries, and the native video visualizer
(`dataviz.rs`, which renders through Bevy for `record` and keeps its own palette) —
except where the UI *drives* them (the Export window).

**Sources.** `README.md`, `docs/editor.md` (user guide), the module doc-comments of
the `src/*.rs` UI modules, and the code itself. Where these disagree, the code is
taken as current behavior and the disagreement is listed in §16.

---

## 1. Entry point & startup

- `main.rs` wires `DefaultPlugins` + `EguiPlugin` + `SimPlugin` + the UI modules.
- No CLI argument → start on the **empty arena** (`SimConfig::empty()`, the
  editor's canvas). An explicit scenario path wins.
- The app starts **paused**, so the user can place, edit and inspect before running.
- Fonts (`assets/fonts/`) are loaded at runtime **relative to the CWD**; a missing
  font file is skipped with a warning and egui's defaults take over (the app must
  still run). See §12 and the `BEVY_ASSET_ROOT` caveat in §15.

## 2. Architectural contract (binding — DEV Rule 1 & 3)

These are the invariants a front change must not break:

1. **No simulation logic in the UI.** Everything here lives in `Update` /
   `EguiPrimaryContextPass`; the sim lives in `FixedUpdate`. The UI edits the
   `SimConfig` (data), sets the virtual clock, or requests actions applied in
   `PreUpdate` *before* the fixed loop (reset, scenario load, single-step) — it
   never mutates living sim state directly. The inspector is strictly read-only.
2. **The sim stays byte-identical** under UI-only changes (DEV Rule 3;
   `tests/mlp.rs` is the tripwire).
3. **One `Camera2d`.** More than one breaks the egui primary context (panels
   disappear). Any additional view must create its camera lazily on activation.
4. **One root `Ui`, `show_inside`.** All panels dock into a single background-layer
   `Ui` covering the viewport (`panels::dock`); no top-level `Panel::show` (egui
   0.34 deprecates it). The free central rect is stashed in `CentralRect` for the
   camera and the pointer gate.
5. **Conditional panels are created last.** A `show_inside` child panel's ids mix in
   the parent's auto-id counter, so a conditional panel inserted before an
   unconditional one shifts ids when it toggles → "changed id between passes"
   warnings. The archetype editor is therefore added after every unconditional
   panel.
6. **Font readiness gate.** egui binds fonts at the start of the *next* pass;
   `FontsReady` gates the first panel render so a Phosphor icon is never drawn
   before its family exists (would panic).
7. **Single sources of truth**: colors in `theme.rs`, key/mouse bindings in
   `keymap.rs` (input handlers, tooltips and cheatsheet all read one table),
   transient feedback in `status.rs` (one status line). Help is **hover-first**:
   explanations live in tooltips on the control or its section header — there is
   no inline hint layer (only *state/empty* explanations may stay visible, weak).
8. **System order within a frame**: `panels::dock` → sim-area interaction systems
   (which read the fresh `CentralRect` via `pointer_over_ui`) → `set_sim_camera`.
   Running interactions before `dock` would let a click on a panel fall through to
   the sim.

## 3. Layout (`panels.rs`, `layout.rs`)

Docked panels frame a central, always fully visible simulation:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ Scenario ▾  <file> *   ── ▶/⏸ ⏭ speed ⟲ (centered) ──  View▾ Help▾ [Breeding] ⏺Export… │
├────────────┬────────────────┬─────────────────────────────┬─────────────────┤
│ WORLD      │ ARCHETYPE      │                             │ ANALYSIS        │
│ ▾ World    │ EDITOR         │        the simulation       │ ▸ Observation   │
│ ▾ Archetypes│ (on selection)│   t = …s   [Paused chip]    │   ▸ Live stats  │
│            │ Body·Genes·    │                             │   ▸ Inspector   │
│            │ Brain        ✕ │                             │                 │
├────────────┴────────────────┴─────────────────────────────┴─────────────────┤
│ status line (info/success/error)                                            │
│ [Breeding dashboard | Evolution — curves]   (curves full width w/o breeding)│
└─────────────────────────────────────────────────────────────────────────────┘
```

- **Semantic split**: master/detail on the left (the *world* as a whole; the one
  archetype being edited as detail), *Analysis* (what you read) on the right, time
  series at the bottom, scenario IO + transport + view/help/export in the top strip.
- **No `CentralPanel`**: the center is transparent; Bevy renders the sim through it.
- **Resizable side panels** within `layout.rs` ranges: `SIDE_MIN = 280`,
  `SIDE_DEFAULT = 370`, `SIDE_MAX = 520` pt. The drag range always reserves
  `CENTRAL_MIN = 480` pt for the sim (`side_range`); on a too-narrow viewport the
  range collapses to `SIDE_MIN`, never inverts.
- **Two-/single-column left region**: the archetype editor opens as a second left
  column only when `right + world + editor + CENTRAL_MIN` fits; otherwise it
  **folds into** the left panel, replacing the master in place. The mode flips with
  a `HYSTERESIS = 24` pt dead-band so resizing the window doesn't flicker.
  `layout.rs` is pure and unit-tested; `panels.rs` is a thin caller.
- **Bottom panel** spans only the central width (reserved after the side columns).
  Height-resizable: range `260..=520` pt, or `260..=760` with the breeding
  dashboard docked (it is tall); default 300 / 360.
- **Every region folds to a thin rail** (`RAIL_W` = 26 pt): a frameless chevron
  overlaid in the open region's top-right corner (no layout cost, `ui.put`) folds
  it; the rail's chevron — same spot — reopens it; keys `1`/`2`/`3` toggle
  left/right/bottom. The bottom rail carries the **status line**, so feedback is
  never hidden. Launch layout: **composing** (empty canvas → all deployed) vs
  **observing** (CLI scenario → side columns folded, arena + curves lead). The
  archetype-editor detail only shows while the left region is open; a Capture
  reopens it. Each region is always exactly one panel (open or rail), so later
  egui ids stay stable.

## 4. Top strip

### Scenario menu (`runs.rs`)

Owns the **document model** — three coexisting states: file on disk, config in
memory (live-edited by the panels), running world (rebuilt on Reset).

- **New (empty)** · **Open ▸** (two groups: committed `scenarios/examples/`,
  gitignored `scenarios/saved/`; list refreshed on menu open; plus an *Open path*
  field) · **Revert** · **Save** · **Save As…**.
- The current file name shows an amber **`*` dirty marker** when the in-memory
  config differs from the last load/save (compared against a baseline snapshot).
  A text asterisk, deliberately not a glyph like `●`: the embedded font subset
  renders some symbols as tofu.
- **Guardrails**: New / Open / Revert confirm before discarding unsaved edits
  (modal). Save refuses to silently overwrite a file the user did not create this
  session (bundled examples keep their hand-written comments/compact RON): it
  offers **Save a copy**. Save As onto an existing name asks first.
- Loading only sets a *pending action*; `apply_scenario_load` (in `PreUpdate`)
  installs the config and reuses the reset to rebuild the world.

### Transport (centered — `controls.rs`)

- **▶ Play / ⏸ Pause** (`Space`) — drives `Time<Virtual>`; pause freezes sim + HUD
  sampling while rendering continues.
- **⏭ Step** (`→`) — exactly one fixed tick; enabled only while paused.
- **Speed** — logarithmic slider ×0.1…×10 with presets ×1 ×2 ×5 ×10 (active preset
  highlighted). Changes evolution rate only, never rendering.
- **⟲ Reset** (`R`) — rebuilds the world from the current config: re-spawns,
  re-seeds, rebuilds the nutrient field/sources, re-applies `tick_hz`, clears the
  metrics history and the agent selection. The button turns **accent** while the
  running world no longer matches the config on the **reset-bound** fields (arena,
  seed, counts, bodies, genomes, brains, components, sources) — edits waiting for
  a rebuild; live-applied fields (relations, field relations, gene bounds, colors)
  never trigger it (`controls::world_diverged`).
- Buttons write into `SimControls`; `drive_steps` / `apply_reset` act in
  `PreUpdate` before the fixed loop (the egui pass is too late for the same frame).

### Right cluster

Reading order left→right: **View · Help · [Breeding] · Export** (emitted
right-to-left in code, Export first).

- **View ▾** — render-layer toggles (`visuals::Layers`): agents (default on) and
  the nutrient heatmap(s) (**default on in the windowed build**, shared opacity
  budget; the recorder keeps its own default of off unless `--nutrients`). View
  concerns are never saved with the scenario.
- **Help** — a direct button: opens the **keyboard shortcuts** cheatsheet (also
  `?` / `F1`). All other help is hover-first (tooltips), so no menu remains.
- **Breeding** toggle (Phosphor *sparkle* + label, `selectable_label`) — shown
  **only** when the scenario carries a `batch` block; docks/undocks the breeding
  dashboard (§10). Defaults to on (the panel appears as soon as a batch scenario
  loads).
- **⏺ Export…** — opens the Export video floating window (§11).

## 5. Central simulation area

### Camera & framing (`main.rs`)

- `set_sim_camera` frames the **whole square arena** centered and fully visible in
  whatever `CentralRect` the panels leave free; the off-arena margin is greyed
  (`ClearColor`, editable in Appearance).
- **User pan/zoom layered on top** of the fit: scroll = zoom toward the cursor,
  middle/right drag = pan, `Home` = recenter (zoom 1, arena centered). Zoom math
  and framing share one world-units-per-point definition. The pan gesture belongs
  to where it **started** (press origin): begun on the sim it survives crossing a
  panel, begun on a panel it never pans; the wheel is gated at the pointer's
  current position (it has no origin).

### Overlay (`panels::central_overlay`)

- Top-center read-out `t = 12.3 s`, with `· ×N` appended **only** when speed ≠ ×1.
  Time comes from the metrics history's latest sample (resets with the world).
- **Paused chip** (accent amber, rounded, "Paused — Space to run") under the
  read-out while the clock is paused.
- **Empty-arena hint**, centered, faint ink: "Drag a species from Archetypes into
  the arena" when archetypes exist, otherwise "Scenario ▸ Open, or add an archetype
  to begin".

### Interactions (`inspector.rs`, `editor.rs`, keymap `MOUSE`)

- **Click** an agent → select + inspect (ring + fan of vision rays rendered by
  `SelectionRenderPlugin`); click the void → deselect. Picking has a **~6-px
  screen-space slack** so small bodies stay clickable at any zoom, and the cursor
  becomes a pointing hand over a body.
- **Drag** an archetype from the list into the arena → place one entity (each
  hand-placed brain gets a distinct RNG stream). A drop on the greyed off-game
  margin is clamped inside the walls (body radius + clearance).
- **`Del` / `Backspace`** → delete the entity under the cursor (same
  radius-contains-point criterion as picking).
- **Pointer gate**: a click/drop counts as sim input **iff** the pointer is inside
  the central rect (`pointer_over_ui`); egui's built-in hover flag is unreliable
  under `show_inside`, hence the explicit rect test.

### Sim rendering (`visuals.rs`, `selection.rs` — shared with `record`)

- Entities are circles (mesh) shaded by reserve, with a heading tick; arena +
  play-area/off-game backgrounds from the scenario's Appearance; toggleable layers:
  agents over nutrient heatmaps (background). The detailed vision-ray fan is drawn
  only for the selected agent.

## 6. World panel (left master — `editor.rs`)

The scenario as a whole. Collapsible cards, ordered by touch frequency:

- **Arena & generation** — arena half-size, RNG seed. Applied on next Reset.
  (`tick_hz` is a scenario-file parameter, deliberately not exposed.)
- **Relations** — the interaction table, one card per *actor → target*: `transfer`
  (predation vs plain destruction), rate/s, range (0 = contact). Read live.
- **Nutrients** — the substrate field (grid resolution, diffusion, decay) and its
  emission sources (position, rate, color). Applied on Reset.
- **Gene bounds** — global min/max per gene; bound both mutation and the editor
  sliders.
- **Appearance** — play-area and off-game background colors (saved with the
  scenario, live preview).
- **Batch** (when present) — the generational-regime config (generations, cohort
  size, match ticks, fitness, scored species…), i.e. the breeding dashboard's
  configuration lives *here*, not in the dashboard.

**Archetypes** list: drag to place; click to select (opens the detail editor);
**＋ Agent / ＋ Food** create; **Duplicate / Move up / Move down / Delete** act on
the selection; a **✦** marks an archetype carrying captured MLP weights. Delete
keeps a **one-level undo** (a `Restore ‹name›` button re-installs the exact
pre-delete config snapshot; the slot is cleared on scenario load). Delete/reorder
remap **every index-keyed table**: relations, field relations, the batch's scored
species and the transient founder pools. The
**Species library** exports the selection to `species/*.ron`, imports a copy, and
can resync an imported species from its source file. Edits write **directly** into
`SimConfig.archetypes` (no copy/sync pass); the archetype's index is its identity.

## 7. Archetype editor (left detail — `editor.rs`)

Opens on selection as a second left column (or in place of the master in
single-column mode); closes via **✕** or deselection. Three cards:

- **Body** — name, color, spawn count, body radius, max reserve.
- **Genes** — the founding genotype in collapsible sections (Locomotion, Vision,
  Metabolism, Reproduction, Flora, Nutrients). Section defaults follow the entity's
  kind: fauna opens the mobile axes, a sessile plant opens flora/nutrients instead.
  **Costs are listed last** within each section (uniform order). Each gene is a
  slider bounded by the world's gene bounds. The **Edit mutability** toggle reveals
  an aligned per-gene **mutable** checkbox column (checked = drifts at
  reproduction; unchecked = inherited frozen). Inert genes for an immobile entity
  are hidden; a section left empty disappears.
- **Brain** — Wander · Hunter · Grazer · Sessile · Network (MLP). For the MLP:
  hidden-layer architecture editing (I/O fixed by the Perception/Action contract),
  a structure graph preview (neutral node color — no activation data), and a
  *clear captured weights* action.

## 8. Analysis panel (right — `inspector.rs`, `metrics.rs`)

The **Observation** row (follow-mode combo + Reset view) sits flat and pinned at
the top — no wrapping collapsible; the tall sections below scroll:

- **Live stats** (collapsed by default) — a grid with **one column per species**
  (name in the archetype colour): population, mean reserve and per-gene means (an
  em dash for a dead species). Scrolls horizontally on wide scenarios. The video's
  aggregate (`metrics::live_stats`) is a separate medium and stays aggregate.
- **Agent inspector** — for the clicked agent, read-only: *Identity* (species,
  brain, generation, age), *Energy*, *Genotype*, *Action* (the brain's output,
  including the eat/attack intent), the **MLP activation graph** for learned brains
  (nodes lerp rest→warm/cold by activation sign, edges tinted by weight sign),
  *Perception* (per-ray obstacle/target/threat bars using the theme's channel
  colors). **Capture as archetype** freezes this agent's evolved genome + concrete
  weights into a new reusable archetype (original species untouched).

## 9. Bottom panel — status line + curves (`status.rs`, `hud.rs`, `plot.rs`)

- **Status line** (full width, above the curves): the single sink for every
  transient feedback (save/load/reload, species import/export, capture, recording,
  breeding actions). Kind drives color and lifetime: **info** (muted ink) and
  **success** (green) expire after **8 s**; **error** (soft red) persists until
  replaced. Stamped with real (unpausable) time.
- **Curves** (no wrapping card or title — the panel *is* the curves surface):
  **Population per species** then **Gene drift — mutable genes (normalized 0–1)**
  (frozen genes are excluded — they'd plot flat).
  Header: sample count + **↻ Clear** (resets the history). The two plots split the
  panel's available height, each clamped to 64–240 pt; the panel's 260 pt floor
  guarantees no clipping at minimum height.
- **Plot widget** (shared, homemade — no `egui_plot`): autoscaled Y
  (fixed-window or auto-include-zero with padding, never a degenerate span), round
  grid steps, label-width-aware margins, monospace 9 pt tick labels, dark `SURFACE`
  background, `GRID` hairlines. **Hover** → vertical cursor, a dot per curve, and a
  tooltip with time + every value. Supports an optional accent **marker** at an X
  value (used by the breeding navigator). Scaling math is pure and unit-tested.

## 10. Breeding dashboard (`dashboard.rs` — gated)

- **Visibility**: docked into the **left half of the bottom panel** (curves keep
  the right half) iff the scenario has a `batch` **and** the top-bar Breeding
  toggle is on. Never a floating popup — it reserves real layout space; the sim
  stays visible (Replay plays out in it).
- **Threading**: Run spawns a **background worker** that drives the generational
  `Orchestrator` over isolated headless worlds; the UI reads the shared
  (status + reports) state through brief per-frame locks (the worker only locks
  between generations). The **live sim world is paused** when a run starts. Stop is
  graceful: after the in-flight generation. The Running→Done/Stopped **edge posts a
  status-line bridge** (browse the fitness curve / Replay) — the live world stays
  paused and the central chip alone would not say what to do next. A scenario load
  **forgets the session** (history + any in-flight worker, detached): reports index
  species of the scenario that bred them.
- **Contents**, top to bottom:
  - config hint (the batch config is edited in the World panel), status +
    **progress** (generations completed / total), **Run / Stop**;
  - **generation navigator**: *follow the latest* by default (live), or **click
    the fitness graph** to pin any completed generation (full history retained);
    an accent marker shows the inspected generation; **Replay** re-seeds the live
    world's founders from that generation's cohort, resets and un-pauses. The
    founder pools are **transient** (`serde(skip)`): they persist on the live config
    (a manual Reset replays the same generation) until the next scenario load, but
    are never saved or exported, never dirty the document, and a later Run starts
    clean;
  - per-faction **readout** (summary line for the inspected generation);
  - **fitness-vs-generation curve** — two lines per bred faction: best (faction
    color) + cohort mean (dimmed), on the shared plot widget;
  - **per-match metrics table** — every match of the cohort scored under every
    metric, the selection-driving metric accented;
  - **leaderboard** — ranked elites of the inspected generation, with a **faction
    selector** under co-evolution; selecting a row shows the genome's **MLP
    activation graph**; **Save to library** (that genome → `species/saved/`
    variant, captured onto the right base archetype) and **Save best of run**
    (highest best-fitness generation, ties broken toward the later one).
- Side effects go through a `BreedingAction` applied by `apply_action` (save →
  status line feedback; replay → seed + reset + un-pause + status).

## 11. Menus, floating windows & modals

- **Record menu** (`recorder.rs`, top-bar Record button) — the recording's
  **components** to capture (Video now; Sound / Metrics shown off + disabled, for
  later) then a single **Run record** button, the *only* entry point to a recording.
  Run record builds `outputs/run-NN/` (the first free one), freezes the current config
  (editor edits included) into it as `scenario.ron`, then launches the headless
  `record` binary to render `video.mp4` beside it — the recording is a self-describing
  folder. An `Update` system watches process exit; outcome lands in the status line.
  While it runs, the button reads **Recording** (accent) and the menu offers a spinner
  + **Cancel** (kills the subprocess, discards the partial folder). Render settings
  (fps/size/duration/follow/HUD) are fixed sensible defaults for now. `record` is looked
  up next to the current executable (hence the `play` wrapper builds all binaries);
  `ffmpeg` is an external runtime dependency — a missing one must fail with a clear
  message.
- **Keyboard shortcuts** cheatsheet — toggled by `?` / `F1` or Help ▾; renders both
  halves of the keymap table (keys + mouse gestures).
- **Confirm modals** (`runs.rs`) — discard-unsaved-edits (New/Open/Revert),
  overwrite-on-Save-As, save-a-copy for bundled examples.

## 12. Visual system (`theme.rs`, `fonts.rs`)

- **Dark theme**, one global egui `Style` installed once at startup. **Quiet, flat
  chrome**: controls carry no idle outline (a hairline appears on hover),
  separators drop to the faint grid gray, an 8-pt spacing rhythm (roomier button
  padding, 24-pt interact height, 8-pt menu margins) — structure reads from
  spacing and surface tones, not lines. Cards (`editor::card`) are **borderless**:
  a slightly recessed `CARD` tint (gray 23) instead of a stroke. Every color
  resolves to a semantic token — no ad-hoc `Color32` literals in panels:
  - `ACCENT` amber `(240,180,80)` — attention/pending: paused chip, dirty marker,
    breeding in flight; also egui `warn_fg_color`.
  - `SUCCESS` green `(120,200,120)`; `ERROR` soft red `(255,140,120)` (also egui
    `error_fg_color`).
  - Ink ramp: `SURFACE` gray18 (plot/graph backgrounds, `extreme_bg_color`) ·
    `GRID` gray36 · `INK_FAINT` gray90 · `INK_MUTED` gray140 · `INK` gray165.
  - Perception channels: `TARGET` orange `(220,130,40)`, `THREAT` red `(210,60,60)`.
  - MLP encodings, one sign convention: warm/orange = positive, cold/blue =
    negative; `ACT_REST` gray60 resting node, `ACT_NEUTRAL` gray110 structural
    (editor preview), `EDGE_POS` / `EDGE_NEG` weight tints.
- **Typography**, three roles: **Inter** → Proportional (labels, menus, panels);
  **Departure Mono** → Monospace (values, technical read-outs); **Phosphor v2.1**
  → icons, in a **dedicated named family** (Inter maps some PUA codepoints and
  would shadow icons if Phosphor were a mere fallback). Icons are drawn via
  `icon` / `icon_label`, sized so icon+label buttons match plain-text buttons.
  Codepoints are version-specific to the bundled `.ttf`.
- egui's built-in fonts remain as fallbacks for glyph coverage; missing font files
  degrade gracefully (§1).

## 13. Input reference (`keymap.rs` — single source of truth)

| Input | Action | Condition |
| --- | --- | --- |
| `Space` | Play / pause | |
| `→` | Advance one tick | when paused |
| `R` | Rebuild the world from the config | |
| `Home` | Recenter the view | |
| `Del` / `Backspace` | Delete the entity under the cursor | |
| `?` (`/` + Shift) / `F1` | Toggle the shortcuts cheatsheet | |
| Scroll | Zoom toward the cursor | pointer on sim |
| Middle / right drag | Pan the view | pointer on sim |
| Click | Select an agent (void = deselect) | pointer on sim |
| Drag from Archetypes | Place an entity | drop on sim |

Requirements: every action appears exactly once in the table; no key conflicts;
tooltips append the binding's key text (all three enforced by unit tests).
Shortcuts are ignored while a text field has keyboard focus.

## 14. Feedback & state summary

| State | Surface |
| --- | --- |
| Unsaved edits | amber `*` by the file name (top strip) |
| Reset-bound edits not yet in the running world | accented ⟲ Reset in the transport |
| Paused | accent chip in the sim area; Play button shows ▶ |
| Speed ≠ ×1 | `· ×N` suffix on the time read-out; highlighted preset |
| Empty arena | centered faint hint (two variants, §5) |
| Transient outcome | one status line (Lab panel; §9 lifetimes) |
| Breeding run | progress + status in the panel; live world paused |
| First frame | panels withheld until `FontsReady` |
| Recording in flight | accent "Recording" on the top-bar Record button; spinner + Cancel in its menu |
| Help | hover a control or a section header for its explanation |

## 15. Non-functional requirements

- **Responsiveness**: the render loop never blocks — breeding runs on a worker
  thread; video export is a subprocess; scenario loads/resets are deferred to
  `PreUpdate` actions.
- **Determinism**: UI features must not perturb sim bytes (append-only genes,
  defaults preserving behavior; `tests/mlp.rs` guards).
- **Id stability**: toggling any conditional UI must not shift other widgets' egui
  ids across passes (§2.5).
- **Layout robustness**: the sim never drops below `CENTRAL_MIN`; panels never
  clip their densest content at `SIDE_MIN`; mode flips are hysteresis-damped; the
  bottom panel floor keeps both plots unclipped.
- **Asset-root dependency**: fonts/scenarios/species resolve relative to the CWD —
  run via `play`, `cargo run`, or with `BEVY_ASSET_ROOT` set; otherwise text can
  measure 0 and render invisible (known DejaVu/dataviz pitfall for `record`).
- **Doc/UI coherence**: tooltips and the cheatsheet derive from the keymap table;
  `docs/editor.md` must match the shipped layout (see §16).

## 16. Doc drift found & fixed, and review leads

Drift found while writing this spec, **fixed on 2026-07-08** (same change set as
this document):

1. `docs/editor.md` — layout section rewritten (side panels *are* resizable with a
   min-central guarantee, single-column fold, height-resizable bottom panel; the
   "equal fixed widths keep the sim centred" claim dropped — the sim centers in
   the *remaining* central rect); sketch updated (Help menu, Breeding toggle,
   status line, docked breeding dashboard); **Revert** added to the Scenario menu
   and its guardrail; Inline help moved from the View to the **Help** menu;
   zoom/pan/`Home` documented; shortcuts table completed (`Home`, `?`/`F1`, mouse
   gestures).
2. `README.md` — the world editor does **not** expose `tick_hz` (it stays a
   scenario-file parameter, re-applied on Reset); claim corrected.
3. `src/panels.rs` — the Breeding toggle's tooltip said the dashboard docks "in
   the right panel (replaces Analysis)"; it docks in the **bottom panel's left
   half** since the bottom-dock change. Tooltip corrected.
4. `src/runs.rs` — module doc said a "`●` marker" and "bottom-bar Reset"; the code
   renders an amber `*` (glyph markers render as tofu in the embedded font
   subset) and Reset lives in the top-strip transport. Doc-comment corrected.

Review-worthy hotspots by construction (still open leads):

- `pointer_over_ui` — hand-rolled hover test replacing egui's built-in (unreliable
  under `show_inside`); verify it against floating windows over the central rect.
- The conditional-panel ordering constraint (§2.5) — easy to violate when adding a
  panel.
- The `FontsReady` first-frame gate — any new early-render path must respect it.
- The breeding worker's shared-state locking — the worker must never hold the lock
  during a match, the UI's locks must stay brief (never held across egui), and Stop
  must stay graceful (after the in-flight generation).
