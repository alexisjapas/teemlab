//! Entity **selection** for observation — highlight + fan of vision rays —
//! shared by the windowed preview and the video recorder.
//!
//! This is strictly rendering/observation (everything in `Update`, never
//! `FixedUpdate`). The selection's *target* is driven differently per binary:
//!
//! - **windowed**: by mouse picking (cf. `inspector` on the binary side);
//! - **recorder**: by **automatic selection** ([`AutoSelectPlugin`]), so a video
//!   continuously shows an agent's rays without intervention — with a *roll mode*
//!   ([`SelectionRoll`]) that chooses how the highlighted agent changes over time.
//!
//! The **rendering** (the ring + the rays) lives in [`SelectionRenderPlugin`],
//! common to both: it only reads the [`Selection`] resource, wherever it comes
//! from.

use crate::components::{Age, Agent, Generation, Locomotion, Perception, Radius, Species, Vision};
use crate::rng::Rng;
use bevy::prelude::*;

/// The entity currently highlighted for observation (highlight + rays), if any.
/// Written by picking (windowed) or automatic selection (recorder), read by the
/// rendering of [`SelectionRenderPlugin`].
#[derive(Resource, Default)]
pub struct Selection(pub Option<Entity>);

/// **Roll mode** of the automatic selection: how the highlighted agent changes
/// over time during a recording. All "timer" modes (cf. [`rolls`](Self::rolls))
/// **hold** their target a whole interval — never a per-frame change — to stay
/// pleasant to watch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SelectionRoll {
    /// No automatic selection: the recorder highlights nothing.
    Off,
    /// **Sticky**: a single agent, kept while it lives (re-chosen at its death).
    Sticky,
    /// **Cycle**: moves to the next agent at a regular interval (round-robin).
    Cycle,
    /// **Active**: the agent whose rays detect the most (vision+target+threat) —
    /// re-evaluated at each interval. The best to *show* raycasts in situ.
    Active,
    /// **Species tour**: at each interval, the next species (one of its agents,
    /// the most "active") — each species thus gets its screen time.
    SpeciesTour,
    /// **Eldest** (default): the oldest living one. Changes only at its death (age
    /// grows for all at the same pace → the eldest stays the eldest): no timer, so
    /// calm — a steady follow of the survivor, pleasant by default.
    #[default]
    Eldest,
    /// **Vanguard**: follows the evolutionary frontier. Picks, **at random**, one of
    /// a species' **newest-generation** agents (the highest [`Generation`] — hence one
    /// of the youngest by lineage depth) and **holds it until it dies**; at its death
    /// it rotates to **another species** and applies the same rule. No timer (like
    /// `Eldest`/`Sticky`, it changes only at the target's death), but each death also
    /// steps the species — every lineage's cutting edge gets its turn.
    Vanguard,
}

impl SelectionRoll {
    /// All modes, to populate a UI selector.
    pub const ALL: [SelectionRoll; 7] = [
        Self::Off,
        Self::Sticky,
        Self::Cycle,
        Self::Active,
        Self::SpeciesTour,
        Self::Eldest,
        Self::Vanguard,
    ];

    /// Stable CLI token (passed to the `record` binary by the recording menu).
    pub fn cli(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Sticky => "sticky",
            Self::Cycle => "cycle",
            Self::Active => "active",
            Self::SpeciesTour => "species",
            Self::Eldest => "eldest",
            Self::Vanguard => "vanguard",
        }
    }

    /// Parse a CLI token; `None` if unknown.
    pub fn from_cli(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.cli() == s)
    }

    /// Label for the UI.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Off => "None",
            Self::Sticky => "Sticky",
            Self::Cycle => "Cycle",
            Self::Active => "Active",
            Self::SpeciesTour => "Species tour",
            Self::Eldest => "Eldest",
            Self::Vanguard => "Vanguard",
        }
    }

    /// `true` if this mode re-evaluates **at a regular interval** (and therefore
    /// shows/uses the interval). `Off`, `Sticky`, `Eldest` and `Vanguard` have no
    /// timer: they change only at the target's death.
    pub fn rolls(&self) -> bool {
        matches!(self, Self::Cycle | Self::Active | Self::SpeciesTour)
    }
}

/// Selection rendering: a ring around the highlighted agent + its fan of vision
/// rays. Common to the windowed build and the recorder — it only reads
/// [`Selection`]. Does **not** include the selection driver (picking or auto).
pub struct SelectionRenderPlugin;

impl Plugin for SelectionRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selection>()
            .add_systems(Update, (highlight_selection, draw_selected_vision));
    }
}

/// **Automatic selection**: always keeps a **mobile** agent highlighted, *rolling*
/// it according to [`SelectionRoll`]. Targets mobile agents because they alone cast
/// visible rays (immobile flora has none, cf. `movement`). To be added in addition
/// to [`SelectionRenderPlugin`].
///
/// Used by the **recorder** (fixed `roll` from the CLI) **and** the windowed build
/// (mounted with `Off`; the UI then flips [`AutoSelect::roll`] live — the same
/// follow modes as the video). Manual picking still works: a click overrides the
/// auto target, which the driver then *holds* until that agent dies (cf.
/// [`drive_selection`]).
pub struct AutoSelectPlugin {
    /// Chosen roll mode.
    pub roll: SelectionRoll,
    /// Interval between two changes, in seconds ("timer" modes, cf.
    /// [`SelectionRoll::rolls`]).
    pub interval: f32,
}

impl Plugin for AutoSelectPlugin {
    fn build(&self, app: &mut App) {
        // Seed the (observation-only) RNG from the wall clock so the random modes
        // (`Vanguard`) vary across sessions. This never touches the sim RNG, so
        // determinism of the simulation is unaffected (§7: selection is rendering).
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x5EED_C0DE, |d| d.as_nanos() as u64);
        app.init_resource::<Selection>()
            .insert_resource(AutoSelect {
                roll: self.roll,
                interval: self.interval.max(0.1),
                elapsed: 0.0,
                cursor: 0,
                rng: Rng::new(seed),
                applied: self.roll,
            })
            .add_systems(Update, drive_selection);
    }
}

/// State of the automatic selection driver. Public so the windowed UI can flip the
/// mode/interval live ([`roll`](Self::roll), [`interval`](Self::interval)); the
/// internal cursor/timer/RNG stay private.
#[derive(Resource)]
pub struct AutoSelect {
    /// Active roll mode (the windowed "Follow" selector writes this).
    pub roll: SelectionRoll,
    /// Interval between changes for the "timer" modes, in seconds.
    pub interval: f32,
    /// Time elapsed since the last change, in seconds.
    elapsed: f32,
    /// Round-robin cursor: agent index (`Cycle`), or species index (`SpeciesTour`/`Vanguard`).
    cursor: usize,
    /// Observation-only RNG for the random modes (`Vanguard`). Not the sim RNG.
    rng: Rng,
    /// The roll the current selection was made under. When [`roll`](Self::roll) is
    /// switched live (windowed selector), this differs → we re-pick **immediately**
    /// instead of holding a target chosen under the old rule.
    applied: SelectionRoll,
}

/// Metrics of an agent that is a candidate for highlighting, read at choice time.
struct Cand {
    entity: Entity,
    species: u16,
    age: f32,
    /// Lineage depth (0 at a founder). Drives the `Vanguard` mode.
    generation: u32,
    /// Sum of the perception channels (vision + target + threat): "how much it sees".
    stim: f32,
}

/// `Update`: keeps a mobile agent selected according to the mode.
///
/// We **re-choose** only when the target has disappeared (death), when the mode was
/// just **switched** (windowed selector → apply the new rule at once), or, for the
/// timer modes ([`SelectionRoll::rolls`]), at the interval's deadline — never per
/// frame. The target therefore holds a whole interval: no flicker, even when the
/// metric (energy, speed, "active"…) fluctuates fast.
fn drive_selection(
    time: Res<Time>,
    mut auto: ResMut<AutoSelect>,
    mut selection: ResMut<Selection>,
    agents: Query<
        (
            Entity,
            &Locomotion,
            &Species,
            &Age,
            &Generation,
            &Perception,
        ),
        With<Agent>,
    >,
) {
    if auto.roll == SelectionRoll::Off {
        return;
    }
    auto.elapsed += time.delta_secs();
    let due = auto.elapsed >= auto.interval;
    // The mode was switched live since the current selection was made.
    let mode_changed = auto.roll != auto.applied;
    // Is the current target still a living mobile agent?
    let valid = selection
        .0
        .is_some_and(|e| agents.get(e).is_ok_and(|(_, loco, ..)| !loco.is_immobile()));
    // Hold the target: we re-choose only at death, at a mode switch, or — for timer
    // modes — at the deadline.
    if valid && !mode_changed && !(auto.roll.rolls() && due) {
        return;
    }

    // Living MOBILE agents (the only ones showing rays) + their choice metrics.
    let mut cands: Vec<Cand> = agents
        .iter()
        .filter(|(_, loco, ..)| !loco.is_immobile())
        .map(|(entity, _, species, age, generation, perception)| Cand {
            entity,
            species: species.0,
            age: age.0,
            generation: generation.0,
            stim: perception
                .vision
                .iter()
                .chain(perception.target.iter())
                .chain(perception.threat.iter())
                .copied()
                .sum(),
        })
        .collect();
    if cands.is_empty() {
        selection.0 = None;
        return;
    }
    // Stable order (by entity bits) for a reproducible rotation.
    cands.sort_unstable_by_key(|c| c.entity.to_bits());

    auto.elapsed = 0.0;
    auto.applied = auto.roll; // this selection now reflects the active rule.
    let roll = auto.roll;
    selection.0 = Some(choose(roll, &cands, &mut auto));
}

/// Chooses the highlighted agent among `cands` (non-empty, sorted) per the mode.
fn choose(roll: SelectionRoll, cands: &[Cand], auto: &mut AutoSelect) -> Entity {
    // The agent maximizing a metric (stable tie-break: `cands` is sorted).
    let best = |key: &dyn Fn(&Cand) -> f32| -> Entity {
        cands
            .iter()
            .max_by(|a, b| key(a).total_cmp(&key(b)))
            .map_or(cands[0].entity, |c| c.entity)
    };
    match roll {
        // `Off` never reaches here (filtered); `Sticky` keeps the stable first one.
        SelectionRoll::Off | SelectionRoll::Sticky => cands[0].entity,
        SelectionRoll::Cycle => {
            auto.cursor = (auto.cursor + 1) % cands.len();
            cands[auto.cursor].entity
        }
        SelectionRoll::Active => best(&|c| c.stim),
        SelectionRoll::Eldest => best(&|c| c.age),
        // Species tour: next species (round-robin), then its most "active" agent.
        SelectionRoll::SpeciesTour => {
            let mut species: Vec<u16> = cands.iter().map(|c| c.species).collect();
            species.sort_unstable();
            species.dedup();
            auto.cursor = (auto.cursor + 1) % species.len();
            let target = species[auto.cursor];
            cands
                .iter()
                .filter(|c| c.species == target)
                .max_by(|a, b| a.stim.total_cmp(&b.stim))
                .map_or(cands[0].entity, |c| c.entity)
        }
        // Vanguard: this fires only at a (re)choice — i.e. a death — so each call
        // rotates the species and picks a fresh newest-generation member at random.
        SelectionRoll::Vanguard => {
            let pop: Vec<(u16, u32)> = cands.iter().map(|c| (c.species, c.generation)).collect();
            let idx = vanguard_pick(&pop, &mut auto.cursor, &mut auto.rng);
            cands[idx].entity
        }
    }
}

/// Core of the `Vanguard` mode, factored out (and pure) for testability: advance
/// `cursor` to the **next species** present in `pop` (sorted-unique, round-robin),
/// then return the index — into `pop` — of one of *that* species' **newest**
/// (max-[`Generation`]) members, chosen **uniformly** via `rng`. `pop` is non-empty
/// (its species necessarily has a member), so the result always indexes a candidate.
fn vanguard_pick(pop: &[(u16, u32)], cursor: &mut usize, rng: &mut Rng) -> usize {
    let mut species: Vec<u16> = pop.iter().map(|(s, _)| *s).collect();
    species.sort_unstable();
    species.dedup();
    *cursor = (*cursor + 1) % species.len();
    let target = species[*cursor];
    let max_gen = pop
        .iter()
        .filter(|(s, _)| *s == target)
        .map(|(_, g)| *g)
        .max()
        .unwrap_or(0);
    // Indices of the species' newest-generation members; pick one uniformly.
    let front: Vec<usize> = pop
        .iter()
        .enumerate()
        .filter(|(_, (s, g))| *s == target && *g == max_gen)
        .map(|(i, _)| i)
        .collect();
    let k = (rng.next_f32() * front.len() as f32) as usize;
    front[k.min(front.len() - 1)]
}

/// Rendering only: encircle the selected agent with a ring, to spot it in the area.
pub fn highlight_selection(
    mut gizmos: Gizmos,
    selection: Res<Selection>,
    agents: Query<(&Transform, &Radius), With<Agent>>,
) {
    if let Some(entity) = selection.0
        && let Ok((transform, radius)) = agents.get(entity)
    {
        gizmos.circle_2d(
            transform.translation.truncate(),
            radius.0 + 5.0,
            Color::srgb(1.0, 1.0, 1.0),
        );
    }
}

/// Rendering only: the fan of vision rays of the **selected** agent only — to
/// *see* occlusion at work without saturating the screen. We re-read the sensory
/// state already computed by the sim ([`Perception`]) — no raycast recomputed
/// here. A light ray = nothing seen; it reddens and shortens as an obstacle gets
/// closer.
///
/// An **immobile** entity (flora) has no usable vision: `perceive` casts no ray
/// for it (empty perception), so we draw nothing for it — selecting a bush does
/// not draw a misleading fan.
pub fn draw_selected_vision(
    mut gizmos: Gizmos,
    selection: Res<Selection>,
    agents: Query<(&Transform, &Vision, &Perception, &Locomotion), With<Agent>>,
) {
    let Some(entity) = selection.0 else {
        return;
    };
    let Ok((transform, vision, perception, loco)) = agents.get(entity) else {
        return;
    };
    if loco.is_immobile() {
        return; // flora: no ray to show.
    }
    let origin = transform.translation.truncate();
    let facing = perception.heading;
    for (i, &proximity) in perception.vision.iter().enumerate() {
        let dir = vision.ray_dir(i, facing);
        let length = vision.range * (1.0 - proximity);
        let color = Color::srgb(0.25 + 0.75 * proximity, 0.55 * (1.0 - proximity), 0.15);
        gizmos.line_2d(origin, origin + dir * length, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CLI tokens round-trip, labels/tokens are unique and non-empty — a guardrail
    /// against an omission (or a duplicate) when adding a mode.
    #[test]
    fn roll_cli_roundtrips() {
        let mut clis = std::collections::HashSet::new();
        for m in SelectionRoll::ALL {
            assert_eq!(SelectionRoll::from_cli(m.cli()), Some(m));
            assert!(!m.label().is_empty());
            assert!(clis.insert(m.cli()), "duplicate CLI token: {}", m.cli());
        }
        assert_eq!(SelectionRoll::from_cli("unknown"), None);
    }

    /// The **timer** modes (re-evaluated at an interval) "roll"; `Off`/`Sticky`/`Eldest`
    /// change only at the target's death.
    #[test]
    fn timer_modes_roll_others_dont() {
        for m in [
            SelectionRoll::Off,
            SelectionRoll::Sticky,
            SelectionRoll::Eldest,
            SelectionRoll::Vanguard,
        ] {
            assert!(!m.rolls(), "{m:?} should not roll on a timer");
        }
        for m in [
            SelectionRoll::Cycle,
            SelectionRoll::Active,
            SelectionRoll::SpeciesTour,
        ] {
            assert!(m.rolls(), "{m:?} should roll on a timer");
        }
    }

    /// `Vanguard` always lands on a **newest-generation** member of *some* species,
    /// and successive (death-triggered) picks **rotate through every species**.
    #[test]
    fn vanguard_picks_newest_generation_and_rotates_species() {
        // species 0: gens {1,3,3} · species 1: gen {5} · species 2: gens {2,2}
        let pop = [(0u16, 1u32), (0, 3), (0, 3), (1, 5), (2, 2), (2, 2)];
        let mut rng = Rng::new(42);
        let mut cursor = 0usize;
        let mut seen = std::collections::HashSet::new();
        for _ in 0..9 {
            let i = vanguard_pick(&pop, &mut cursor, &mut rng);
            let (s, g) = pop[i];
            let max_g = pop
                .iter()
                .filter(|(ps, _)| *ps == s)
                .map(|(_, pg)| *pg)
                .max()
                .unwrap();
            assert_eq!(g, max_g, "must pick a newest-generation member of {s}");
            seen.insert(s);
        }
        assert_eq!(seen.len(), 3, "rotation should visit every species");
    }
}
