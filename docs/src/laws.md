# The laws & rules

teemlab is governed by two short constitutions. They are deliberately stable: breaking
one changes what the project *is*. This page summarizes them; the binding text lives in
`CONSTITUTION-SIM.md` and `CONSTITUTION-DEV.md`.

## The laws of the simulated world

The inviolable contracts that define the engine. Eleven laws, in brief:

1. **One engine, many scenarios.** A scenario is *data*, never code; it states only what
   it changes, and adding a field never breaks an existing scenario.
2. **The invariant loop** — every agent's agency is `perceive → decide → act`, and
   nothing else, in the fixed-timestep schedule.
3. **Brain ↔ body is a contract** — normalized floats in (`Perception`), floats out
   (`Action`). The brain's internals are interchangeable.
4. **The body shapes the brain's I/O**, never the reverse — genes vary the magnitudes and
   the *number* of channels; a learned brain adapts to match.
5. **Brains are an `enum`**, never `Box<dyn>` — static dispatch and an exhaustive match.
6. **Genotype ≠ phenotype** — evolution mutates the inherited recipe, compiled into the
   living body only at spawn; it never touches a running body.
7. **Every characteristic is priced** — a beneficial trait must cost something, or it
   drifts trivially to its bound. The cost is set by the *scenario*.
8. **One interaction primitive** — eating and attacking are the same directed verb;
   the scenario sets the semantics (transfer → predation, destroy → combat).
9. **Conservation** — interactions transfer or destroy reserve, never create it;
   a contested resource is shared, never duplicated.
10. **We replay experiments, not bits** — a seed reproduces a *configuration* to compare
    parameters, not a bit-for-bit run (determinism is traded for parallelism).
11. **Every life form runs the same systems** — fauna or flora, predator or prey, all are
    agents driven by the same loop, primitive and economy. A "plant" is just an agent
    with a sessile brain; difference is *data*, never a special code path.

## The rules of development

How the code is kept honest. The headlines:

- **No simulation logic in `Update`** — the cardinal invariant; agency and physics live
  in the fixed schedule, so the headless and windowed builds never diverge.
- **`cargo fmt` is authoritative; the tree stays clippy-clean.**
- **The sim stays byte-identical** unless a change is meant to alter it — append new genes
  at the end, non-mutable, defaulted to `0.0`.
- **Every feature ships with a test** — one multi-seed driver per scenario, asserting a
  property across seeds.
- **Extend the data, not the drivers** — a new gene touches one table entry, not every
  consumer.
- **Conventional Commits; tag minors; the annotated tag's message is the changelog.**

## License

teemlab is dual-licensed under **MIT OR Apache-2.0**, at your option — the conventional
Rust-ecosystem dual license, matching its dependencies. The full texts are `LICENSE-MIT`
and `LICENSE-APACHE` in the repository.

The whole dependency tree is permissive (MIT / Apache-2.0 / BSD / ISC / Zlib / Unicode /
… — no copyleft obligation), and every release archive ships a generated
`THIRD-PARTY-LICENSES.html` reproducing those notices. The bundled fonts keep their own
permissive licenses (SIL OFL 1.1, MIT, Bitstream/public-domain), carried alongside the
font data in `assets/fonts/`.
