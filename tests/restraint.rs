//! Restraint as a stability lever — the PAYOFF of the cognitive substrate
//! (proprioception + deliberate, costed eating; `docs/persistent-ecosystems.md` §2).
//!
//! The `Grazer` control brain forages exactly like the `Hunter` but gates its eating on
//! its own hunger (proprioception, `self_state` energy): a PRUDENT grazer (low threshold)
//! leaves food uneaten when sated; a GREEDY one (threshold 1.0) eats whatever is in
//! range. Same body, same economy — only the appetite gate differs. `05_restraint.ron` is a
//! SPLIT arena (a prudent monoculture | a greedy monoculture, each on its own half), giving
//! two robust results:
//!
//! (A) RESTRAINT IS A STABILITY LEVER ([`split_prudent_persists_greedy_collapses`]): run AS
//!     SHIPPED (the split), by a late window the GREEDY half has boom-busted to extinction
//!     while the identical PRUDENT half still PERSISTS — collapse vs persistence from the
//!     appetite gate alone, side by side in one frame.
//! (B) TRAGEDY OF THE COMMONS ([`greed_outcompetes_restraint`]): remove the divider so the two
//!     share ONE oasis, and greed is individually superior (eats more → harvests more nutrient
//!     → more offspring), so it out-competes prudence — restraint stabilises but is not
//!     individually selected.
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
/// founder counts — `0` yields a MONOCULTURE of the other. When `mixed`, the divider is
/// removed (each grazer's `spawn_zone` cleared and the solid wall rocks dropped) so the two
/// share ONE oasis — the tragedy-of-commons setup; otherwise they run as shipped, each
/// confined to its own half. Returns the final (greedy, prudent, flora) living counts.
/// Manual single-stepping (one `update()` = one fixed tick), the same world as both binaries.
fn run(
    seed: u64,
    greedy: usize,
    prudent: usize,
    seconds: usize,
    mixed: bool,
) -> (usize, usize, usize) {
    let mut config = SimConfig::from_ron_str(SCENARIO).expect("valid restraint scenario");
    config.seed = seed;
    config.archetypes[0].count = greedy;
    config.archetypes[1].count = prudent;
    if mixed {
        for a in &mut config.archetypes {
            a.spawn_zone = None;
        }
        config.sources.retain(|s| !s.solid);
    }
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

/// (A) RESTRAINT IS A STABILITY LEVER — the split's point, shown in one frame. Run the
/// scenario AS SHIPPED (both grazers, each confined to its own half with its own equal flora
/// and vents). By a late window the GREEDY monoculture has stripped its patch and BOOM-BUSTED
/// to extinction, while the identical PRUDENT monoculture — differing ONLY in its appetite
/// gate — still PERSISTS with its flora. Same body, same economy: the appetite is the whole
/// difference, and behavioural restraint is the difference between collapse and persistence.
#[test]
fn split_prudent_persists_greedy_collapses() {
    const HORIZON: usize = 110;
    for seed in SEEDS {
        let (greedy, prudent, _flora) = run(seed, 16, 16, HORIZON, false);
        // Prudence still standing where greed has crashed: a decisive side-by-side contrast.
        assert!(
            prudent >= 8 && greedy < prudent,
            "seed {seed}: the prudent half must persist where the greedy half collapses \
             (prudent {prudent} vs greedy {greedy} at {HORIZON}s)"
        );
    }
}

/// (B) TRAGEDY OF THE COMMONS. Remove the divider (`mixed`) so the two grazers share ONE
/// oasis: greed is now individually superior — it eats more, harvests more nutrient, and
/// out-reproduces prudence — so restraint is NOT individually selected, the greedy lineage
/// overtakes the prudent one while the shared commons still stands. (The greed-dominated
/// system then boom-busts — the stability it forfeited, cf. result A.) Making restraint
/// *selectable* needs spatial viscosity (§2), which mobile foragers here swamp — the
/// deferred open hypothesis (ROADMAP §9).
#[test]
fn greed_outcompetes_restraint() {
    // Long enough that the competition has played out (greed has overtaken prudence) while
    // greed still holds a real population — genuine displacement, not mutual collapse.
    const PEAK: usize = 60;
    for seed in SEEDS {
        let (greedy, prudent, _flora) = run(seed, 16, 16, PEAK, true);
        assert!(
            greedy > prudent && greedy >= 4,
            "seed {seed}: greed should out-compete restraint in a well-mixed world \
             (greedy {greedy} vs prudent {prudent} at {PEAK}s)"
        );
    }
}
