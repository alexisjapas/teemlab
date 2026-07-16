//! Restraint as a stability lever — the PAYOFF of the cognitive substrate
//! (proprioception + deliberate, costed eating; `docs/persistent-ecosystems.md` §2).
//!
//! The `Grazer` control brain forages exactly like the `Hunter` but gates its eating on
//! its own hunger (proprioception, `self_state` energy): a PRUDENT grazer (low threshold)
//! leaves food uneaten when sated; a GREEDY one (threshold 1.0) eats whatever is in
//! range. Same body, same economy — only the appetite gate differs. On
//! `scenarios/examples/05_restraint.ron`, two robust results:
//!
//! (A) RESTRAINT GRAZES GENTLER ([`restraint_grazes_gentler_than_greed`]): over the same
//!     window a PRUDENT monoculture leaves MORE flora standing than a GREEDY one (its lower
//!     appetite is a lighter footprint on the commons), while the GREEDY one over-consumes
//!     and over-breeds. Both eventually wind down on this living-food economy (the
//!     Lotka-Volterra wall, ROADMAP §7) — so we assert the CONTRAST at a mid-window, not a
//!     persistence the post-refactor economy no longer sustains.
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

const SCENARIO: &str = include_str!("../scenarios/examples/05_restraint.ron");

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

/// (A) Restraint grazes GENTLER on the commons. Same body, same economy — only the appetite
/// gate differs. Over the same mid-window, a PRUDENT monoculture leaves MORE flora standing
/// than a GREEDY one (the lower ceiling is a lighter footprint), while the GREEDY one
/// over-consumes and over-breeds (a boom that later dooms it). Both wind down on this
/// living-food economy (§7), so we pin the CONTRAST, not a persistence: behavioural restraint
/// is a measurably lighter hand on the shared resource.
#[test]
fn restraint_grazes_gentler_than_greed() {
    const HORIZON: usize = 40;
    for seed in SEEDS {
        let (greedy_only, _, greedy_flora) = run(seed, 16, 0, HORIZON);
        let (_, prudent_only, prudent_flora) = run(seed, 0, 16, HORIZON);

        // Prudence leaves more of the commons standing than greed.
        assert!(
            prudent_flora > greedy_flora,
            "seed {seed}: prudence must preserve more flora than greed \
             (prudent {prudent_flora} vs greedy {greedy_flora} at {HORIZON}s)"
        );
        // Greed over-consumes → over-breeds: the boom (that later busts) is bigger.
        assert!(
            greedy_only > prudent_only,
            "seed {seed}: greed should over-breed on what it strips \
             (greedy {greedy_only} vs prudent {prudent_only} at {HORIZON}s)"
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
fn greed_outcompetes_restraint() {
    // Long enough that the competition has played out (greed has overtaken prudence) while
    // greed still holds a real population — genuine displacement, not mutual collapse.
    const PEAK: usize = 60;
    for seed in SEEDS {
        let (greedy, prudent, _flora) = run(seed, 16, 16, PEAK);
        assert!(
            greedy > prudent && greedy >= 4,
            "seed {seed}: greed should out-compete restraint in a well-mixed world \
             (greedy {greedy} vs prudent {prudent} at {PEAK}s)"
        );
    }
}
