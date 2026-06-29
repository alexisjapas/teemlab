# Interactions: predation, combat, competition

teemlab has **one** interaction verb, not three. Eating, attacking and competing are the
*same* directed action — an actor reduces a target's reserve, within range — and a
single table decides who may do it to whom and what it means. One primitive plus a
relation table covers an entire ecology with no special-cased code.

## The relation table

A scenario's `relations` list is the wiring of the whole web. Each entry is one directed
arrow:

```ron
(
    actor: 0,        // archetype index of the one acting
    target: 1,       // archetype index of the one acted upon
    transfer: true,  // true = predation (actor gains), false = combat (pure destruction)
    rate: 45.0,      // reserve drained per second of contact
    range: 16.0,     // surface-to-surface clearance; 0 = must touch
)
```

`actor` and `target` are **indices into the scenario's archetype list** (a species'
identity is its position in that list). `range` is the *gap* between the two bodies, so
`0` means contact and a larger value lets the action land from a distance.

## Three behaviours from one verb

The single `transfer` flag, plus where you point the arrow, gives you the whole
repertoire:

| You want…       | Set it up as…                                                       | Example |
| --------------- | ------------------------------------------------------------------- | ------- |
| **Predation**   | `transfer: true` from predator to prey — drained reserve (and a share of nutrient) flows to the actor. | [`hunt`](../scenarios.md#05--hunt), [`predator_prey`](../scenarios.md#10--predator-prey) |
| **Combat**      | `transfer: false` between enemies — the reserve is destroyed, the attacker gains nothing. War is a pure cost. | [`factions`](../scenarios.md#11--factions) |
| **Competition** | a relation from a species **onto itself**, `transfer: false` — crowded individuals drain each other and the densest die back. | [`flora`](../scenarios.md#03--flora) |

That last one is worth dwelling on: intraspecific competition for light and space is not
a new mechanism — it is the same destructive interaction pointed at one's own kind. It
turns unbounded growth into a logistic plateau, with no engine change.

## Perception is the table's mirror

The relation table does double duty: it also *defines perception*. If actor A may act on
target B, then **B lights up A's `target` channel** (A's prey) and **A lights up B's
`threat` channel** (B's predator). A [`Hunter`](./brains.md#hunter--the-competent-control)
brain reads exactly these channels, so the *same* brain becomes predator or prey
depending only on how the arrows point. Add a mutual `transfer: false` pair between two
factions and each perceives the other as both a target (to attack) and a threat (to
beware) — and you have a war.

## Conservation under contention

When several actors crowd a single target, the target's reserve is **shared**, never
duplicated — three wolves on one sheep split the sheep, they do not each get a whole
one. Conservation (nothing created from nothing) holds even under a scrum, which is what
keeps a contested-resource scenario honest.
