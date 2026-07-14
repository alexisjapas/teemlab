# Constitution of Development — the rules of work

How we work on **teemlab**. These rules are stable and binding; the
[ROADMAP](ROADMAP.md) holds the evolving plan and status, the [README](README.md)
holds setup and commands. The laws of the *simulated world* live in
[`CONSTITUTION-SIM.md`](CONSTITUTION-SIM.md) — this document is about the **code
and the process** around it.

Working language of **everything written for the project** is **English** — without
exception. Not only code, comments, docs and scenarios, but **also** commit messages,
tag and release notes, branch names, PR titles and descriptions, and issues: every
artifact that lands in the repo or its infrastructure. French is fine only in *live
discussion* (a chat like this one), never in anything persisted. Cite an article by
number ("cf. SIM Law 7").

---

## Rule 1 — No simulation logic in `Update` (the cardinal invariant)

All simulation logic **and** the physics step live in the fixed-timestep schedule
(`FixedUpdate` / `FixedPostUpdate`), identical with or without a window. `Update`
is reserved for rendering, input and UI. A simulation system placed in `Update`
makes the headless build diverge from the windowed one.

**Why.** Headless ⇄ windowed parity is what lets us trust a multi-seed driver as a
proxy for the real run; it dies the instant agency leaks into `Update`.

**Anchored in.** `lib.rs` (the `FixedUpdate` chain), `main.rs` / `panels.rs` /
`visuals.rs` (`Update` = render/UI only), `bin/headless.rs`.

---

## Rule 2 — `cargo fmt` is authoritative; the tree stays clippy-clean

There is **no `rustfmt.toml`** — the default formatter decides, and layout is not a
review battleground. Run `cargo fmt` *before* committing; every commit leaves
`cargo fmt --check` clean and `cargo clippy --all-targets` warning-free.

**Why.** Formatting and lint debates are pure friction; delegating them to the
tools keeps reviews about substance.

**Anchored in.** README ("Format convention"); `Cargo.toml` (edition 2024).

---

## Rule 3 — The simulation stays byte-identical unless a change is *meant* to alter it

Display, UI and tooling features must not perturb the sim's numeric path. When
adding a gene or a mechanic, **preserve the RNG draw stream**: append new genes at
the **end** of `Genotype` and `TRAITS`, keep them **non-mutable by default** (a
non-mutable gene draws nothing in `mutate`) and **defaulted to `0.0`** (inert until
a scenario opts in). The multi-seed drivers — `tests/mlp.rs` above all — are
chaos-sensitive tripwires: if one breaks unexpectedly, you changed the economy.

**Why.** The whole evolutionary economy is a calibration; an accidental shift in
the draw stream silently invalidates every tuned scenario.

**Anchored in.** `genotype.rs` (append-at-end convention, `mutate`), `config.rs`
(`Mutability` defaults), `ecology.rs` (`metabolize`), `tests/mlp.rs`. This is the
operational corollary of [SIM Law 10](CONSTITUTION-SIM.md).

---

## Rule 4 — Every feature ships with a test

Unit tests per module for pure logic; **one multi-seed integration driver per
scenario**, asserting a property that holds *across seeds* (a single seed is
anecdotal). Integration tests run the *real* sim world, in manual single-stepping.

**Why.** Emergent behavior is the product; only a multi-seed assertion can tell
"it learned" from "that seed got lucky".

**Anchored in.** `tests/*.rs` (`cohabitation`, `predator_prey`, `mlp`, `flora`,
`flight`, …), `tests/common/mod.rs` (`stepping_app`).

---

## Rule 5 — Extend the data, not the drivers

A characteristic is added generically: a new gene = one `TRAITS` entry + a
`Genotype` field + its bounds. The editor, HUD, inspector and metrics loop over
`TRAITS` and need **no** edit. Do not hard-code a per-trait branch where a table
entry would do.

**Why.** Item 15's falsification: modularity is real only if a new trait touches
one place. A per-trait `if` in a driver is the smell that the abstraction leaked.

**Anchored in.** `genotype.rs` (`TRAITS`), `editor.rs` / `inspector.rs` /
`metrics.rs` (generic loops).

---

## Rule 6 — Generality ≠ modularity: falsify against plurality

A general mechanism can be deeply coupled; modularity is proven only against
**plurality** — at least two real instances per axis. Do not introduce an
abstraction (or a config knob, or an `enum`) until a *second* concrete case demands
it. In particular, do not reify `enum Regime`: keep the reproduction-timing and
fitness-source seams separable.

**Why.** Premature generality freezes the wrong coupling into a type and is harder
to undo than to add later.

**Anchored in.** `ROADMAP.md` §4 (architectural guard), §8 (method).

---

## Rule 7 — Stub the behavior, never the schema

A no-op *behavior* shell is legitimate scaffolding (e.g. a brain that decides
nothing). A no-op *data-contract* shell is not: the schema's shape **is** the
abstraction, and freezing the wrong shape is the expensive mistake.

**Why.** Behavior is cheap to fill in later; a wrong schema propagates into every
consumer and is costly to migrate.

**Anchored in.** `brain.rs` (`Sessile`, a legitimate no-op brain); `ROADMAP.md`
§8.

---

## Rule 8 — Validate on the deterministic control before the learned brain

Wire and falsify a new mechanism (a perception channel, a cost, a reflex) with the
hand-written brains — `Hunter`, `Wander` — before the MLP consumes it. The
deterministic control is also the bar a learned brain must beat: one that does not
out-perform it has learned nothing.

**Why.** Debugging a mechanic and debugging neuroevolution at once is two unknowns;
the control isolates the first.

**Anchored in.** `scenarios/hunt.ron` & `cohabitation.ron` (controls) preceding
`mlp_brain.ron`; `tests/mlp.rs` (the ≥2× domination gate).

---

## Rule 9 — Document the *why*, cite the law

Doc-comments justify non-obvious decisions and cross-reference the relevant law,
`§`, or item — they explain *why*, not *what* the code plainly says. New invariants
are added to a constitution and cited, not buried in a comment.

**Why.** The reasoning is the part that rots silently; recording it (and linking it
to the binding law) is what keeps the next change honest.

**Anchored in.** Pervasive across `src/` (the doc-comment style is the norm).

---

## Rule 10 — Commit hygiene

Conventional Commits (`feat:`, `fix:`, `chore:`, `refactor:`, …). Never commit a
tree that is not `fmt`/`clippy`-clean (Rule 2). Commit or push only on request; if
work would land on `main`, branch first. Scenarios stay **self-contained** —
species imported by copy with a provenance link, never a live cross-reference.

**Why.** A readable history and self-contained scenarios are what make an
experiment reproducible and a regression bisectable.

**Anchored in.** `git log` (conventional style); `config.rs` (`Archetype.source`,
import-by-copy).

---

## Rule 11 — Strict SemVer, auto-bumped; a major bump needs confirmation

`Cargo.toml`'s `version` is the single source of truth, and it is **strict
[SemVer](https://semver.org) of the shipped artifact** (the released binaries) — not
a commit counter. **Every change that touches the shipped artifact bumps the version
in the same change**, classified by SemVer:

- **patch** (`x.y.Z`) — a backwards-compatible bug fix (`fix:`).
- **minor** (`x.Y.0`) — new backwards-compatible functionality, including any purely
  **additive** public API (`feat:` — a new item, never a removal or a signature change).
- **major** (`X.0.0`) — a **breaking** change: a removed / renamed / retyped public
  item, a changed public signature, a new variant on an exhaustive public `enum`, or a
  behavioral contract a consumer relies on being broken.

A change with **no effect on the shipped artifact** — `docs:` / `test:` / `ci:`, or a
`chore:` / `refactor:` confined to dev-only tooling (benches, CI, the dev shell) —
has no SemVer category and therefore **does not bump**. Version the artifact's
*behavior*, never the repo's activity.

**Patch and minor bumps are applied automatically** — classify the change and roll
`Cargo.toml` as part of it, no need to ask. **A major bump requires explicit
confirmation before it is applied**: a breaking release is consequential, so state
*that* the change is breaking and *why*, and get the go-ahead before incrementing the
major. Never break the public contract silently to dodge the bump.

**Pre-1.0 caveat (we are at `0.y.z`).** Cargo treats the left-most non-zero component
as the "major", so while under `0.y` the roles shift down by one: a **breaking**
change bumps the minor (`0.Y.0`) — *this* is the major-equivalent that needs
confirmation — and any **backwards-compatible** change, feature or fix alike, bumps
the patch (`0.y.Z`), applied automatically. The `1.0.0` release is itself a major
bump and so needs confirmation.

Every release **tag** is `v<that exact version>` (e.g. `Cargo.toml = 0.6.1` → tag
`v0.6.1`); the release CI **fails** the run if they disagree. Tags are cut **on
explicit request** (pushing a `vX.Y.Z` tag is the *only* release trigger — build →
archive for Linux / Windows / macOS, `dist` profile → GitHub Release); before rolling
onto a new minor/major line, first tag the *outgoing* version if untagged, so the last
state of the closing line is captured. To pin an arbitrary build to its commit, use the
**git SHA** (optionally as SemVer build metadata, `0.6.1+a1b2c3d`, ignored for
precedence). The tag is **annotated** (`git tag -a`) and its message **is the
changelog** — a hand-written description of the evolutions since the previous tag,
which the release CI lifts into the release notes; a lightweight tag, or an annotated
tag with an empty message, is a defect.

**Why.** Auto-bumping keyed to each change keeps `Cargo.toml` a truthful, always-current
statement of the artifact's compatibility, so a reader (and Cargo's resolver) can map a
version to what actually changed. Gating only the *major* bump on a human puts the
friction exactly where the stakes are — a broken contract — while letting the routine
patch/minor bumps flow. Versioning behavior, not activity, is what keeps a docs-only or
CI-only change from inflating the number.

**Anchored in.** `Cargo.toml` (`version`); `.github/workflows/release.yml`
(`version-check` guard, the `dist` build matrix); `Cargo.toml` `[profile.dist]`.

---

## Rule 12 — The scope filter: this is a research bench, not a game

teemlab exists to **study emergence** — non-collapsing biomes, genetic pathways, the
factors of functional ecosystems (ROADMAP §0). Before adding any feature, apply the
one filter: **does it help understand why an ecosystem holds or collapses?** If it
only buys mob controllability, faster convergence, or visual spectacle, it waits —
those serve the demoted game-mob goal, which is a downstream beneficiary, never a
design driver. The deliverable of a feature is **falsifiable knowledge** — a
hypothesis with a criterion that could disprove it, a control, a curve (Rules 4 and
8) — not an impressive-looking run.

This is also why "fewest arbitrary values" is methodology, not taste: a constant
baked into the *engine* is a hidden confound that pollutes a conclusion, whereas one
that lives in the *scenario* (RON) is an experimental parameter. Prefer
**constant-as-data over constant-as-assumption** — the form/value split of SIM Law 7
(the "no free trait" *form* is inviolable; the cost *value* is scenario data), and
the operational echo of Rule 5 (extend the data, not the drivers).

**Why.** The two-goal tension (research vs. game mobs) was the source of felt
scope-creep; naming the primary goal turns "should I build this?" from taste into a
test. A bench that impresses without falsifying anything is decoration, not science.

**Anchored in.** `ROADMAP.md` §0 (the resolved direction, 2026-07-14); [SIM Law
7](CONSTITUTION-SIM.md) (priced traits); Rules 4, 5, 8 above.
