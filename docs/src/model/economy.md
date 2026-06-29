# The economy: energy & nutrient

teemlab has **two resource axes**, and keeping them separate is what makes its
populations stable instead of crashing.

| Axis         | Governs        | Source                                            | Runs out? |
| ------------ | -------------- | ------------------------------------------------- | --------- |
| **Energy**   | **survival**   | the sun (`photosynthesis`) or eating              | reach 0 → death |
| **Nutrient** | **reproduction** | absorbed from the field, or eaten up the chain  | short ⇒ no breeding, but **no death** |

## Energy — the survival axis

Every agent holds a **reserve** of energy. It drains continuously:

```
ΔE/tick  =  + photosynthesis            (plants: passive solar gain)
            + food eaten                (fauna: predation transfer)
            − base_metabolism           (the cost of being alive)
            − move_cost  × speed²/…      (locomotion)
            − agility_cost × |Δv|        (maneuvering)
            − vision cost                (rays × range)
            − brain_cost × neurons       (an MLP's tissue)
```

When the reserve hits zero, the agent **dies**. Energy is therefore the relentless
pressure that natural selection works against: a trait that does not pay for the energy
it costs is selected away.

Energy is **conserved**. Eating *transfers* reserve from prey to predator; combat
*destroys* it; reproduction *moves* `offspring_energy` from parent to child and never
pays out more than the parent holds. Nothing is created from nothing — a leak would let
a "cheap child" lineage win for free, with nothing to do with fitness.

## Nutrient — the reproduction axis

A second, decoupled resource — the *substrate*, a mineral in the ground — gates
**reproduction only**. The motivation is a failure mode of single-axis worlds: if the
same resource governs both survival and breeding, a food shortage triggers a *death
spiral*. Splitting them fixes it:

- A plant **lives on the sun** (energy) regardless of the nutrient, so it never starves
  for lack of mineral.
- But it can only **breed** where it can absorb nutrient and pay `offspring_nutrient`
  per child. No nutrient ⇒ it simply stops reproducing, and waits.

The result is a self-limiting population: growth is throttled by a finite resource, but
a shortage causes a *pause*, not a collapse. See [The nutrient substrate](./nutrients.md)
for the field, sources, diffusion, the food web and recycling.

> **Fauna and the nutrient.** A grazer usually has `nutrient_absorption: 0` — it cannot
> pull mineral from the ground. Instead it acquires nutrient by **eating**: a bite
> carries a share of the prey's nutrient store up the food chain. So a predator's
> reproduction is coupled to the nutrient flowing up from the plants, and the whole
> chain stays bounded with no overshoot.

## Law 7: everything is priced

The two axes are the macro shape of one rule — **every characteristic is priced** — and
the price is set by the *scenario*, not hard-coded by the engine. Energy is the price of
living and acting; the nutrient is the price of multiplying. Tuning a scenario is
largely a matter of balancing these prices so the world neither dies out nor explodes —
which is exactly the craft the [example scenarios](../scenarios.md) demonstrate.
