//! Restraint as a stability lever — the PAYOFF of the cognitive substrate
//! (proprioception + deliberate, costed eating; `docs/persistent-ecosystems.md` §2).
//!
//! The `Grazer` control brain forages exactly like the `Hunter` but gates its eating on
//! its own hunger (proprioception, `self_state` energy): a PRUDENT grazer (low threshold)
//! leaves food uneaten when sated; a GREEDY one (threshold 1.0) eats whatever is in
//! range. Same body, same economy — only the appetite gate differs. On
//! `scenarios/examples/17_restraint.ron`, two robust results:
//!
//! (A) RESTRAINT IS A STABILITY LEVER ([`restraint_prevents_collapse`]): a PRUDENT
//!     monoculture persists and its flora thrives, where a GREEDY monoculture overshoots
//!     and collapses to extinction — the appetite gate alone separates a living ecosystem
//!     from a dead one (§2's firm claim).
//! (B) TRAGEDY OF THE COMMONS ([`greed_outcompetes_restraint`]): in a MIXED world greed is
//!     individually superior (eats more → harvests more nutrient → more offspring), so it
//!     out-competes prudence — restraint stabilises but is not individually selected.
//!
//! What would make restraint SELECTABLE (evolvable) is spatial viscosity strong enough
//! that a lineage inherits the patch it preserved or exhausted (§2). Mobile foragers here
//! swamp it (adult roaming homogenises impact), so that stays §2's open hypothesis,
//! deferred (ROADMAP §9). The single-stepping app is the same world as both binaries.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use teemlab::components::{Agent, Species};
use teemlab::{SimConfig, SimPlugin};

const SCENARIO: &str = include_str!("../scenarios/examples/17_restraint.ron");

/// Several seeds guard against a fluke — the solver reproduces the order of magnitude,
/// not the exact run (Law 10), as in `predator_prey`/`cohabitation`.
const SEEDS: [u64; 3] = [1, 2, 3];

/// Living (greedy, prudent, flora) counts — species 0, 1, 2 of the scenario.
fn counts(app: &mut App) -> (usize, usize, usize) {
    let world = app.world_mut();
    let mut q = world.query_filtered::<&Species, With<Agent>>();
    let (mut g, mut p, mut f) = (0usize, 0usize, 0usize);
    for s in q.iter(world) {
        match s.0 {
            0 => g += 1,
            1 => p += 1,
            2 => f += 1,
            _ => {}
        }
    }
    (g, p, f)
}

/// Run the scenario for `seconds` at `seed`, overriding the greedy (0) and prudent (1)
/// founder counts — `0` yields a MONOCULTURE of the other. Returns the final
/// (greedy, prudent, flora) living counts. Manual single-stepping (one `update()` = one
/// fixed tick), the same world as both binaries.
fn run(seed: u64, greedy: usize, prudent: usize, seconds: usize) -> (usize, usize, usize) {
    let mut config = SimConfig::from_ron_str(SCENARIO).expect("valid restraint scenario");
    config.seed = seed;
    config.archetypes[0].count = greedy;
    config.archetypes[1].count = prudent;
    let hz = config.tick_hz as usize;

    let mut app = App::new();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / config.tick_hz,
    )));
    app.add_plugins(MinimalPlugins);
    app.add_plugins(SimPlugin::new(config));
    // Avian inserts some resources in these hooks; we pump the loop by hand.
    app.finish();
    app.cleanup();
    for _ in 0..seconds * hz {
        app.update();
    }
    counts(&mut app)
}

/// (A) Restraint is a STABILITY LEVER. Same body, same economy — only the appetite gate
/// differs: a GREEDY monoculture overshoots its flora and collapses to extinction, while
/// a PRUDENT monoculture eats sustainably, so its flora THRIVES (far beyond its 190
/// founders) and its own population persists. The falsifiable core of §2: behavioural
/// restraint is the difference between a living ecosystem and a dead one.
#[test]
#[ignore = "behavioural: awaits scenario re-tuning after the emergent-trophics refactor"]
fn restraint_prevents_collapse() {
    const HORIZON: usize = 120;
    for seed in SEEDS {
        let (greedy_only, _, greedy_flora) = run(seed, 16, 0, HORIZON);
        let (_, prudent_only, prudent_flora) = run(seed, 0, 16, HORIZON);

        assert!(
            greedy_only <= 4,
            "seed {seed}: greed should overshoot and collapse — {greedy_only} greedy \
             foragers still alive at {HORIZON}s"
        );
        assert!(
            prudent_only >= 20,
            "seed {seed}: prudence should persist — only {prudent_only} prudent foragers \
             at {HORIZON}s"
        );
        // The mechanism: prudence leaves the flora to grow far beyond the 190 founders,
        // where greed strips it (near-bald, only slowly recovering once its grazers die).
        assert!(
            prudent_flora >= 400,
            "seed {seed}: a prudently-grazed flora should thrive — only {prudent_flora} at {HORIZON}s"
        );
        assert!(
            prudent_flora > greedy_flora,
            "seed {seed}: prudence must preserve more flora than greed \
             ({prudent_flora} vs {greedy_flora})"
        );
    }
}

/// (B) TRAGEDY OF THE COMMONS. In a MIXED world, greed is individually superior — it eats
/// more, harvests more nutrient, and out-reproduces prudence — so restraint is NOT
/// individually selected: the greedy lineage overtakes the prudent one while the shared
/// commons still stands. (The greed-dominated system then boom-busts — the stability it
/// forfeited, cf. [`restraint_prevents_collapse`].) Making restraint *selectable* needs
/// spatial viscosity (§2), which mobile foragers here swamp — the deferred open
/// hypothesis (ROADMAP §9).
#[test]
#[ignore = "behavioural: awaits scenario re-tuning after the emergent-trophics refactor"]
fn greed_outcompetes_restraint() {
    // Long enough that the competition has played out, while the shared flora still
    // stands (so it is genuine competition, not a post-collapse artefact).
    const PEAK: usize = 40;
    for seed in SEEDS {
        let (greedy, prudent, flora) = run(seed, 16, 16, PEAK);
        assert!(
            flora > 0,
            "seed {seed}: the commons should still stand at {PEAK}s (flora {flora})"
        );
        assert!(
            greedy > prudent,
            "seed {seed}: greed should out-compete restraint in a well-mixed world \
             (greedy {greedy} vs prudent {prudent})"
        );
    }
}
