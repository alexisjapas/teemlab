//! The top-level **screen router** of the windowed build (redesign Phase B).
//!
//! The redesign turns the one cramped docked layout into **five application
//! states** — Observe · Library · Studio · Lab · Analyze — over the **single** egui
//! context and the **single** [`Camera2d`] (`docs/ui-redesign.md` §1, the one-camera
//! hard constraint). This module is that router: a [`Screen`] enum, the [`Router`]
//! resource holding the current one, and the **pure** presentation model the nav
//! rail reads (order, labels). No egui, no ECS system logic — [`crate::panels::dock`]
//! is the thin caller that renders the rail and dispatches per screen, so the routing
//! rules stay unit-tested here.
//!
//! **One-camera discipline.** Only [`Screen::Observe`] renders the live arena through
//! the camera ([`Screen::shows_arena`]); every other screen fully covers the viewport
//! with opaque panels, so the sim is not shown and the camera / pointer-picking
//! systems no-op (they early-return on the empty central rect `dock` records for those
//! screens). Switching away from Observe therefore releases the arena cleanly without
//! ever spawning a second camera.

use bevy::prelude::Resource;

/// One of the five application screens, in the **fixed order** the nav rail presents
/// them (`docs/ui-redesign.md` §9): Observe · Library · Studio · Lab · Analyze.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Screen {
    /// Run **one** scenario live and watch it: the arena, transport, follow, live
    /// stats, the agent inspector, curves, video export. The **only** screen with the
    /// live arena.
    #[default]
    Observe,
    /// The low-friction hub: browse Worlds and Species, compose a Scenario, manage the
    /// catalog. (Built out in a later stage; a placeholder at B1.)
    Library,
    /// Deep-edit a World or a Scenario: the world stage, the cast, the per-archetype
    /// editor, with live food-web validation and an explicit save model.
    Studio,
    /// Run **headless** cohorts with no live arena — breed / sweep — and read the
    /// result metrics.
    Lab,
    /// Post-hoc comparison of saved runs. Deferred; a placeholder in the router.
    Analyze,
}

impl Screen {
    /// The five screens in nav-rail order. The nav rail iterates this; a screen's
    /// position here is purely presentation (the router keys on the value, never the
    /// index).
    pub const ALL: [Screen; 5] = [
        Screen::Observe,
        Screen::Library,
        Screen::Studio,
        Screen::Lab,
        Screen::Analyze,
    ];

    /// The nav-rail label (also the tab title). One word each, stable across renders.
    pub fn label(self) -> &'static str {
        match self {
            Screen::Observe => "Observe",
            Screen::Library => "Library",
            Screen::Studio => "Studio",
            Screen::Lab => "Lab",
            Screen::Analyze => "Analyze",
        }
    }

    /// Whether this screen renders the live arena through the shared camera. **Only
    /// Observe does** — the one-camera constraint (`docs/ui-redesign.md` §1): every
    /// other screen is opaque panels, so `dock` leaves it no central rect and the
    /// camera / picking systems idle.
    pub fn shows_arena(self) -> bool {
        matches!(self, Screen::Observe)
    }
}

/// The current screen — the top-level navigation state, switched by the nav rail.
/// Defaults to [`Screen::Observe`] (the launch landing, and the only live-arena
/// screen). A plain resource: switching is a UI concern, never touched by the sim.
#[derive(Resource, Default)]
pub struct Router {
    /// The screen currently shown.
    pub current: Screen,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_holds_every_screen_in_fixed_order() {
        // The nav rail's fixed order (ui-redesign §9). A miscount here would drop or
        // duplicate a destination.
        assert_eq!(
            Screen::ALL,
            [
                Screen::Observe,
                Screen::Library,
                Screen::Studio,
                Screen::Lab,
                Screen::Analyze
            ]
        );
    }

    #[test]
    fn default_is_observe() {
        // The launch landing and the only live-arena screen.
        assert_eq!(Screen::default(), Screen::Observe);
        assert_eq!(Router::default().current, Screen::Observe);
    }

    #[test]
    fn only_observe_shows_the_arena() {
        // The one-camera discipline: exactly one screen binds the arena camera.
        let with_arena: Vec<_> = Screen::ALL.iter().filter(|s| s.shows_arena()).collect();
        assert_eq!(with_arena, vec![&Screen::Observe]);
    }

    #[test]
    fn labels_are_unique_and_non_empty() {
        // Tabs must be distinguishable; a blank label would render an unclickable gap.
        let mut labels: Vec<_> = Screen::ALL.iter().map(|s| s.label()).collect();
        assert!(labels.iter().all(|l| !l.is_empty()));
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), Screen::ALL.len(), "labels must be unique");
    }
}
