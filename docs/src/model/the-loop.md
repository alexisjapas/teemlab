# The agent loop

Every living thing in teemlab — a wolf, a sheep, a blade of grass — is an **agent**,
and every agent runs the *same* three-stage loop, every fixed tick:

```
perceive  →  decide  →  act
```

Nothing else. A plant is not a special case with its own rules; it is an agent whose
brain happens to decide "do nothing" and whose body happens not to move. This
uniformity is a deliberate law (every life form runs the same systems), and it is what
lets a brand-new kind of creature exist purely as *data*.

The loop lives entirely in Bevy's **fixed-timestep schedule** (`FixedUpdate`), together
with the physics step. The windowed and headless builds therefore run the *identical*
simulation — rendering and UI never touch it. This is teemlab's cardinal invariant, and
it is what makes a headless multi-seed test a trustworthy proxy for what you see on
screen.

## 1. Perceive

Each agent casts a fan of **vision rays** (their number and spread are genes —
`vision_rays`, `vision_fov_deg`, `vision_range`). The result is a `Perception`: a set
of per-ray channels, each a value in `[0, 1]`.

| Channel    | Meaning                                                                  |
| ---------- | ------------------------------------------------------------------------ |
| `vision`   | **obstacle** proximity — the nearest hit on that ray, whatever it is (wall, agent, food). `0` = nothing in range, `1` = touching. |
| `target`   | **prey** proximity — non-zero only if the ray's nearest hit is a species this agent may *act on* (per the relation table). The channel that *attracts* a hunter. |
| `threat`   | **predator** proximity — the exact inverse: non-zero only if the nearest hit is a species that may act *on us*. The channel a prey *flees*. |

`target` and `threat` are symmetric opposites, both derived from the same
[relation table](./interactions.md): if A may eat B, then B appears in A's `target`
channel and A appears in B's `threat` channel. An apex predator simply reads an
all-zero `threat` channel; a plant reads nothing meaningful at all.

Perception also carries the agent's `heading` and the world direction of each ray, so a
hand-written brain can reason about geometry without knowing anything about the body.

## 2. Decide

The agent's **brain** reads the `Perception` and writes an `Action`. That is the entire
contract — normalized floats in, normalized floats out:

```
Action {
    dir: Vec2,      // desired movement direction
    throttle: f32,  // desired fraction of max speed, in [0, 1]
}
```

The brain's internals are a black box behind this contract, which is exactly why they
are interchangeable: a `Wander` reflex, a `Hunter` state machine, a `Sessile` no-op, or
a learned `Mlp` all satisfy it. See [Brains](./model/brains.md).

> **The body shapes the brain, never the reverse.** The number of vision rays (a gene)
> sets the size of the perception vector; a learned brain's input layer resizes to match
> at reproduction. Evolving the body therefore never breaks the brain's contract — the
> two axes stay independent.

## 3. Act

The `act` system turns the desired direction and throttle into a physical velocity,
bounded by the agent's `max_speed` and `agility` (how fast it can change heading). Avian
then integrates the motion and resolves collisions in `FixedPostUpdate`.

Acting **costs energy**, and that is where the loop meets the [economy](./model/economy.md):
moving burns `move_cost`, maneuvering burns `agility_cost`, merely existing burns
`base_metabolism`, and seeing burns a cost proportional to the number and reach of the
rays. A trait that helps must be paid for — otherwise selection has nothing to push
against.

## Why three distinct stages

Keeping perceive, decide and act as separate stages is what keeps the **brain ↔ body
seam clean**. The brain never queries the physics engine and never writes a velocity
directly; it only reads a `Perception` and returns an `Action`. Swap the brain and the
body is untouched; evolve the body and the brain adapts through the contract. One
invariant loop, shared by every scenario and every species — that is the whole engine.
