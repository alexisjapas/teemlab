# teemlab — Emergent trophic interactions & allometric costs (design)

**Status.** Design synthesis, resolved in discussion **2026-07-14**. This is a
**decision record + contract** for a change that touches the constitution; it is
binding once adopted, and it supersedes the "explicit relation table" wherever they
disagree.

**Altitude.** Unlike the companion UI document
([`ui-redesign.md`](ui-redesign.md), which states *needs* for a designer), this one
records a **decided engineering design** — the mechanism, its invariants, its MVP
boundary, and the debts it defers. It does not pin code-level details (system
ordering, data layouts); those belong to implementation.

**Constitutional note (must be applied at implementation time).** This design
**amends SIM Laws 8 and 11**. Per `CLAUDE.md`, changing a SIM law is only allowed
by explicit decision — that decision was taken on 2026-07-14. The amended wording
below is to be written into [`CONSTITUTION-SIM.md`](../CONSTITUTION-SIM.md) **as part
of the implementing change**, not before. This document is where the amendment is
reasoned and recorded.

**Lineage.** This pulls forward the ROADMAP §9 *"Phase 3 — full generic nutrient
web"* vision, which already names *emergent targeting* as "a real change to SIM Law
8, hence deferred". It is no longer deferred. Related: `docs/persistent-ecosystems.md`.

---

## 1. The need — why interactions must become implicit

Today the scenario carries an explicit `relations` table: a per-pair `(actor,
target)` list declaring who eats/attacks whom. It works, but it is:

- **A designer burden that does not scale.** N species ⇒ up to N² hand-authored
  edges; every new species means re-wiring.
- **Un-checkable.** A hand-authored table cannot be validated ("did you forget an
  edge?" is meaningless) — you cannot statically ask whether the food web is even
  viable.
- **Against the grain of Law 1 and Law 11.** Law 1 wants scenarios to be *data with
  no per-scenario wiring*; Law 11 wants life forms to differ *only by their data*,
  with no privileged differentiator. An authored trophic table is exactly the kind
  of bespoke wiring those laws push against.

**The decided alternative.** Trophic interaction becomes **emergent**: an actor eats
what it is **able to dominate** (a body test) **and able to use** (a nutritional
test), computed by the engine from the two entities' data — never from a table. This
*strengthens* Law 1/Law 11 (no wiring, no privileged differentiator) and, crucially,
makes the food web **statically analysable** (§6) — the strongest argument for the
change.

---

## 2. Constitutional amendments (to apply at implementation)

### Law 8 — from "authored target filter" to "emergent target filter"

*Current* (abridged): "…the engine exposes one verb; the scenario sets its semantics
— transfer → predation, destroy → combat — and the **target filter (trophic or
factional)**."

*Amended* (proposed): the primitive **stays one verb**; what changes is the filter.

> **Law 8 — One interaction primitive, an emergent target filter.** Eating is a
> single directed interaction: an actor reduces a target's reserve, within reach,
> and the reduced reserve **transfers** to the actor (predation). The engine exposes
> one verb. The **target filter is emergent** — computed by the engine from the two
> bodies' data: a **dominance** test (size) and a **nutritional** test (does the
> target hold the components the actor needs?). It is never an authored per-pair
> table. Perception is derived from the same rule: an actor perceives as **prey**
> what it can eat and as **threat** what can eat it.

Two clauses of the old law are **dropped for now** and become **debts** (§9): the
*destroy → combat* semantics and the *factional* filter — i.e. **non-nutritional
interaction** — which the emergent nutritional filter does not express.

### Law 11 — the differentiator is the nutritional profile, not the relation table

*Current* (abridged): "…what distinguishes one life form from another is only its
data — its genes, its brain and body, and **the relation table (relations)**. …any
difference in its behavior must be an emergent consequence of its genes and
**relations**."

*Amended* (proposed): replace the relation-table differentiator with the
**nutritional profile**.

> …its data — its **genes**, its **brain and body**, and its **nutritional profile**
> (the components it **needs** and **holds**, from which trophic interactions
> emerge). …any difference in its behaviour must be an emergent consequence of its
> genes and its nutritional profile.

The rest of Law 11 (no per-kind code path; "a plant is just an agent") is unchanged
— indeed §7 shows it already did most of the flora/fauna dissolution.

---

## 3. Emergent predation — the mechanism

Interaction eligibility is the conjunction of **two independent axes**, both derived
from the two entities' data. No pairwise configuration exists.

### 3.1 Dominance — "weaker" = size (binary, with margin) [MVP]

- **Measure:** body **size** (radius). Chosen for MVP: simple, visible, already a
  body parameter, and legible.
- **Rule (MVP):** actor `A` may prey on target `B` iff `size(A) ≥ size(B) × (1 +
  margin)`, where `margin` is a **scenario parameter** (a size advantage the
  predator must have). Binary with a margin — deliberately not graded — because it
  is clean **and statically analysable** (§6).
- **Pricing (mandatory, Law 7).** Size is now a *dominance advantage*, so it **must
  be priced** or it is a free beneficial trait and everything converges to the
  giant. The pricing is the **allometric cost law** (§5): a bigger body costs more
  to maintain and to move. Dominance and cost are two faces of one parameter — this
  is what bounds the size arms race into an **optimal band per niche**.

### 3.2 Nutrition — digestibility as a needs∩contents match

- **Need vector.** Each **archetype** declares which components it **needs** (and how
  much per unit time) — its metabolic requirement.
- **Content vector.** Each **entity** holds a per-component **store** (what it has
  absorbed/eaten). *Dependency:* this requires **per-component stores** (today a
  single `Nutrients` store exists; the roadmap deferred the general case). See §8 —
  this is an **MVP prerequisite**, not a later nicety.
- **Digestibility:** `digestibility(A→B) = fraction of A's needs that B's content
  can supply ∈ [0, 1]`. `0` ⇒ B is not food to A (even if smaller); `1` ⇒ B fully
  covers A's needs.
- **Edibility:** `A can eat B` iff `dominance(A,B)` **and** `digestibility(A→B) > 0`.

### 3.3 Perception — target/threat, recomputed (contract unchanged)

The brain contract (Laws 3, 4) is **untouched**; only the *computation* of the
existing channels changes:

- **target** channel = `{ B : A can eat B }`, with **intensity = digestibility**
  (graded appetite — a richer signal for the MLP than the old binary).
- **threat** channel = `{ B : B can eat A }` — the **inverse of the same rule**,
  automatic and symmetric (A is prey to B when B dominates A and A is digestible to
  B).
- **obstacle** = the raw vision/occlusion channel, unchanged.

### 3.4 The primitive's per-pair parameters move to the body

The old `Relation` carried `transfer`, `rate`, `range`. With the table gone:

- **transfer** is always **true** (emergent interaction is predation — nutrition).
- **range** → **reach**, derived from the body (radius + a bite-reach gene).
- **rate** → a **body/gene** property (bite strength), optionally scaled by the
  size ratio.

### 3.5 MVP economy caveat (kept as-is, evolves later)

**The current energy/nutrient economy is kept for this MVP:** energy (photosynthesis
route + eating) governs **survival**; nutrients gate **reproduction**. The
digestibility machinery drives **targeting and perception** now, but the *survival*
coupling of nutrients is **not** switched on yet. Consequence to state plainly:
**without metabolization (§9), the web is shallow** — if every species needs the
same base component, digestibility ≈ constant and targeting collapses toward
size-only. Distinct trophic niches at MVP therefore come from **absorption
specialisation** (producers that capture *different* components); real trophic
*levels* wait on metabolization.

---

## 4. What is removed and cleaned at MVP

- The interaction **`relations` table** and its Studio editor card (large
  simplification of the World editor — companion doc §5).
- **Factional / non-nutritional combat** (`transfer: false`) and the affected
  showcases — `11_factions`, `14_battle_breed`, `15_red_queen` — lose their
  factional mechanism. **This amputates part of the P5 battle regime and the Red
  Queen; accepted** as a deliberate tradeoff (debt §9 restores a non-nutritional
  aggression mechanism later). The `Fitness::Dominance` metric and co-evolution
  built on faction-vs-faction combat are affected accordingly.
- **`move_cost` and `agility_cost` as free genes** — folded into the allometric law
  (§5).
- The **"Flora" gene category** framing (§7).

---

## 5. The allometric cost law

### 5.1 The need

Two needs in one mechanism: (a) **remove arbitrary per-species cost constants**
(ROADMAP §0 methodology — "constant-as-data, not constant-as-assumption": fewer
hidden confounds, and species become simpler to parameterise), and (b) **price the
new size-dominance trait** (§3.1) so Law 7 holds.

### 5.2 The unifying form

Every cost takes one shape — a coupling to the capability it prices, not a
free-floating value:

```
cost = world_coefficient × capability_measure × (1 / species_efficiency)
```

- **world_coefficient** — scenario data (a few per-world "physics constants"). Keeps
  Law 7 satisfied (the cost value lives in the scenario, not the engine).
- **capability_measure** — differs per cost (see table); this is the load-bearing
  point: "all costs follow *one law*" means *one form*, **not** one measure.
- **species_efficiency** — a per-species factor, **fixed at 1 at MVP**; the hook for
  the future *muscular-efficiency* gene (§9).

| Cost | capability_measure | Notes |
|---|---|---|
| Maintenance (base metabolism) | `size²` | tissue ∝ area (2D mass) |
| Locomotion — cruising | `size² × v²` | **super-linear in speed** — see §5.4 |
| Locomotion — manoeuvring | `size² × \|Δv\|` | accelerating a mass ∝ area |
| Brain | `neuron_count` | **unchanged** (its own coupling) |
| Vision | `range × rays` | **unchanged** (its own coupling) |

### 5.3 Derived quantities & the exponent

- **`reserve_max` is derived from size** (`∝ size²`): a bigger body is a bigger
  energy tank. Removes another free parameter. (Decided: derive it.)
- **`move_cost`, `agility_cost` disappear as genes** — replaced by the locomotion
  rows above.
- **Exponent = 2 by default** (2D area = mass — the geometrically consistent choice
  for circular 2D bodies), but it is a **scenario parameter**: *how metabolic scaling
  affects ecosystem stability* is a research question (goal 2), so the exponent must
  be an experimental knob, not baked in.

### 5.4 Speed: emergent effective speed, retained kinematic ceiling

The speed cost is **super-linear in v** (`∝ v²`, the power/kinetic-energy analogue).
This matters and is precise:

- **What it reliably induces:** a **finite optimal cruising speed per body** — going
  flat-out costs more than it returns, so **effective speed emerges below the
  ceiling from the cost**, without an authored max-speed law. This is the mechanism.
- **What it does *not* guarantee:** **"big = slower than small."** Sustainable speed
  comes from `income ≥ maintenance + locomotion = c₁·size² + c₂·size²·v²`. If income
  also scales as `size²`, the `size²` **cancels** and sustainable speed is
  size-independent. Income is emergent (predation), so *big-and-sluggish* is a
  **hypothesis the sim can test**, not an engine built-in — which is the correct
  posture for a research bench (don't bake in the conclusion). **Necessary
  condition:** the cost must be super-linear in `v`; a merely linear/per-distance
  cost penalises *distance*, not *speed*, and induces no self-limiting speed.
- **Decision (MVP):** **keep a kinematic ceiling** — the `speed` gene survives as a
  *cap* (a body capability, priced by the power law), while the **effective** speed
  is governed by cost. This preserves a design lever ("a fast small predator vs a
  slow small prey", Law 4) and keeps solver velocities bounded.
- **Future change (documented):** drop the ceiling entirely in favour of a **pure
  power budget** — max force/power ∝ `size² × efficiency`, from which max speed
  itself emerges (drag vs force). Deferred; revisit with the muscular-efficiency
  gene (§9).

### 5.5 Law 7 compliance

The **form** of each law (its exponent structure, the super-linear speed term) is
engine structure; the **coefficients** stay scenario data; the **evolvable** part is
`species_efficiency` (later). No unpriced beneficial trait: size is priced by
maintenance+locomotion, speed by the `v²` term, dominance by both.

---

## 6. The trophic graph — one object, three surfaces

Because edibility is **computed**, the whole food web can be **derived and reasoned
about**. Build the derived graph once; use it three ways.

- **Nodes** = `{ components } ∪ { archetypes }`.
- **Edges** = derived **edibility** (archetype→archetype, from §3) and **absorption
  / emission** (archetype↔component).

### 6.1 Static — reachability validator (Studio) [MVP]

A component is **available** to the world if a **source** emits it, a **metabolism**
produces it (future), or it is held by a **reachable edible** entity (transitively).
An archetype is **viable** if **every component it needs** is reachable by a route it
can actually use (direct absorption, or eating something that contains it). **Flag**
any archetype whose need is **unreachable** — a **broken trophic chain**. This runs
**live as the user edits** (companion doc §5). **MVP scope: this flag.** (A
hand-authored table could never be checked this way — this is the payoff of implicit
relations.)

### 6.2 Dynamic — fragility overlay (Observe)

The same graph, annotated with live data:

- **node size = population**;
- **edge thickness = flow** — the rate of *needed* nutrients moving along the edge;
- **edge colour = dependency** (below).

### 6.3 The dependency metric (precise)

For a predator `P` and a prey `Q`:

- `intake(P←Q) = digestibility(P,Q) × predation_rate(P on Q) × abundance(Q)` — a
  flow of *P's satisfied need per unit time* (not raw biomass; digestibility already
  weights by "fraction of P's needs in Q").
- `intake_total(P) = Σ_Q' intake(P←Q')`.
- **`dependency(P→Q) = intake(P←Q) / intake_total(P) ∈ [0, 1]`** — the **share** of
  P's nourishment coming from Q.

Reading: `dependency = 1` ⇒ P is a **pure specialist** on Q (if Q collapses, P has no
fallback → fragile); `dependency = 0.1` ⇒ Q is one of many sources (robust). This is
the **inverted** view the user asked for: not top-down predation pressure on Q, but
**bottom-up dependency** of P on Q. Normalising by `intake_total(P)` is deliberate —
thickness already carries magnitude; **colour carries the concentration of reliance**
("eggs in one basket"), independent of absolute flow. A **hot edge into a small node**
(P depends on a *rare* prey) is the pre-collapse signature.

### 6.4 Fragility as a Lab metric (documented idea)

Aggregate the dependency structure into fragility scores:

- **node fragility** `= Σ_Q dependency(P→Q)²` (Herfindahl/Simpson): `1` = specialist,
  `→0` = generalist.
- **prey criticality** `= Σ_P dependency(P→Q)` weighted by P's population — detects
  **keystone** prey whose loss cascades.
- **web robustness** (scalar) = an aggregate (e.g. the worst dependency onto a
  sub-critical prey, or population-weighted mean node fragility).

The `sweep` bin scores worlds by **Shannon biodiversity** today; **robustness is a
complementary axis** — a world can be *biodiverse yet fragile* (many species, all
narrowly dependent). Adding it as a Lab/sweep metric serves the near-term goal (rich
**and non-collapsing** ecosystems) and the *science of collapse factors* (fragility
*predicts* collapse). Computable **statically** (structural fragility, in Studio) or
**dynamically** (as populations drift, aggregated time-robustly over a run, like the
existing fitnesses).

---

## 7. Flora / fauna dissolution

**Law 11 already did the engine-level dissolution** ("no `if is_flora`", "a plant is
just an agent with a sessile brain and photosynthesis"). What remains is
**cosmetic/data**:

- the **"Flora" gene category** grouping in the editor;
- the **flora framing** of the `photosynthesis` / `seed_dispersal` genes;
- `Brain::Sessile`.

**MVP cleanup:** drop the "Flora" **gene-category label** (regroup those genes under
a neutral heading, e.g. "Metabolism / Environment"); **keep `Sessile` as a brain
option** (immobility is a body/brain choice, not a taxonomic kind); make **"Algae" a
library preset** (Sessile + absorbs an environmental "light" component + small size),
**not** an engine type. The **deep** dissolution — `photosynthesis` becoming a
**metabolic recipe** rather than a special-cased flora gene — rides with
metabolization (§9).

---

## 8. MVP prerequisites & boundary

**Prerequisite (not deferrable) — per-component stores.** Meaningful digestibility
(§3.2) needs each entity to hold a **content vector** across components. Today there
is a single `Nutrients` store (nutrient = component 0). **Decision needed at
implementation:** implement **minimal per-component stores now** (recommended), or
ship a single-component placeholder that makes digestibility degenerate. The clean
MVP is the former.

**In (MVP):** emergent predation (size binary-with-margin + digestibility);
target/threat recomputed (brain contract untouched); the allometric cost law
(maintenance/locomotion derived from `size²`, `reserve_max` derived, `move_cost` /
`agility_cost` removed, exponent default 2 & scenario-set, speed super-linear cost +
retained ceiling); removal of the relations table and factional combat; the static
reachability validator; the dynamic fragility overlay; the "Flora" cosmetic cleanup
+ Algae preset; minimal per-component stores.

**Out (deferred — see §9).**

---

## 9. Deferred debts (documented, with the need)

Each is a *need* recorded so it is not re-derived:

- **Metabolization** — the elegant target. Each species carries a **metabolic
  recipe**: a small stoichiometry `components in → components out` (stored / emitted),
  priced (Law 7), run uniformly (Law 11). One mechanism subsumes **photosynthesis**
  (a "light" component → energy), **digestion** (eaten biomass → own biomass +
  waste), **decomposition** (detritus → minerals), **toxin production**. **It is what
  *creates* trophic levels** (manufacturing level-specific components) — without it
  the web is shallow (§3.5) — and it is **required to *create energy*** (the current
  economy has no general energy-production mechanism beyond photosynthesis). Aligns
  with ROADMAP §9 "inter-layer metabolisation".
- **Nutrient-as-survival economy** — switch nutrients from *reproduction-gating* to a
  **survival requirement** (an entity must consume its needed components or die).
  History to respect: the T1 `minerals.ron` prototype died in a **death spiral**,
  which is *why* the economy decoupled survival (energy) from reproduction
  (nutrient). It is viable **now** in a way it was not then, because **recycling**
  (death → field) and **trophic transfer** (eating → up the chain) close the loop —
  but it remains the single riskiest change and is entangled with metabolization
  (do them together).
- **Muscular-efficiency gene** — make `species_efficiency` (§5.2) an **evolvable**
  per-species factor on locomotion. **Law 7 gotcha to design:** a beneficial
  efficiency, uncontested, drifts to infinity — it needs a counter-pressure (an
  efficient muscle is weaker/slower at peak, or costs to build).
- **Non-nutritional interaction** — restore what the emergent nutritional filter
  cannot express: **intraspecific aggression**, **territory protection**,
  **anti-predator defence**, faction combat. Likely a **second intent/verb** distinct
  from hunger, not a return to an authored trophic table.
- **Graded dominance outcome** — beyond the binary-with-margin gate: let size scale
  the *outcome* of a contested interaction (reusing the primitive's
  conservation-under-contention), not just eligibility.
- **Signed digestibility** — let **toxic** content (the existing `affect < 0`
  semantics) *lower* digestibility, folding poison-avoidance into the same targeting
  signal. MVP is positive-needs only.
- **Speed ceiling removal** — replace the retained kinematic ceiling (§5.4) with a
  pure **power budget** from which max speed emerges.

---

## 10. Cross-references

- [`../CONSTITUTION-SIM.md`](../CONSTITUTION-SIM.md) — Laws 7 (pricing), 8 & 11 (to
  amend at implementation), 1 (scenarios as data).
- [`../ROADMAP.md`](../ROADMAP.md) — §9 "Phase 3 — full generic nutrient web"
  (emergent targeting), §0 methodology (constant-as-data), the `sweep` bin
  (biodiversity scoring).
- [`persistent-ecosystems.md`](persistent-ecosystems.md) — the rich/non-collapsing
  orientation and the collapse-factor science this graph/fragility work feeds.
- [`ui-redesign.md`](ui-redesign.md) — the screens that surface this: Studio
  (validator), Observe (fragility overlay), Lab (fragility metric), and the
  drop-only composition this change enables.
