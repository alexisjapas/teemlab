# teemlab — agent & contributor brief

Evolutionary simulation engine: **one engine interprets data**, each simulation is
a *scenario* (RON). Single agent loop: **perceive → decide → act**. Top-down 2D,
entities are circles. Rust, Bevy 0.19 + Avian 0.7.

## Read these first — they are binding

- [`CONSTITUTION-SIM.md`](CONSTITUTION-SIM.md) — the **inviolable laws of the
  simulated world**. Changing one changes what the project *is*; don't, without an
  explicit decision to do so.
- [`CONSTITUTION-DEV.md`](CONSTITUTION-DEV.md) — the **rules of development**
  (architecture, formatting, testing, method).
- [`ROADMAP.md`](ROADMAP.md) — the evolving design synthesis: status (§0 first),
  principles, and implementation order (the `§N` / "item N" references in the code
  point here).
- [`README.md`](README.md) — setup (Nix, `play`) and the module-by-module map.

## Reflexes that are easy to violate by accident

- **No simulation logic in `Update`** (sim + physics live in `FixedUpdate` /
  `FixedPostUpdate`). — DEV Rule 1.
- **`cargo fmt` before committing; keep `cargo clippy --all-targets` clean.** —
  DEV Rule 2.
- **Keep the sim byte-identical** unless the change is meant to alter it: append
  new genes at the end, non-mutable by default, defaulted to `0.0`; `tests/mlp.rs`
  is the chaos-sensitive tripwire. — DEV Rule 3.
- **Every characteristic is priced** (value, bounds, cost) — no free beneficial
  trait. — SIM Law 7.
- **English for everything written for the project** — not just code and docs but
  also commit messages, tags, branch names, PRs and issues; French only in live
  discussion. Cite laws by number.
