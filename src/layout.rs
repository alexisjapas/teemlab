//! Windowed-UI **layout math**: the resizable side-panel width ranges. A pure
//! function (no egui, no ECS) so [`panels::dock`] stays a thin caller and the sizing
//! rule is unit-tested.
//!
//! [`panels::dock`]: crate::panels::dock
//!
//! The rule exists to solve one problem: a side panel dragged too wide would squeeze
//! the sim to a sliver on a laptop. The side panels are **resizable** within a range
//! that always reserves [`CENTRAL_MIN`] for the sim, so neither can be dragged into
//! the arena's space. (The old two-column archetype-editor fold is gone: the Phase B
//! Studio gives the editor its own screen and the arena no longer competes with it —
//! `docs/ui-redesign.md`.)

use std::ops::RangeInclusive;

/// Narrowest a side panel may be dragged (egui points): still fits the densest
/// content (the gene grid) without clipping.
pub const SIDE_MIN: f32 = 280.0;
/// The side panel's width on a fresh launch — the historical fixed width.
pub const SIDE_DEFAULT: f32 = 370.0;
/// Widest a side panel may be dragged: past this it is just wasted space.
pub const SIDE_MAX: f32 = 520.0;
/// The sim area is never allowed below this (egui points): a drag on either
/// separator stops here.
pub const CENTRAL_MIN: f32 = 480.0;

/// The allowed width range for a side panel, given the viewport width and the
/// **other** side panel's current width. The maximum is whatever leaves
/// [`CENTRAL_MIN`] for the sim (clamped into `[SIDE_MIN, SIDE_MAX]`); on a viewport
/// too narrow to honor that, it collapses to the single point `SIDE_MIN` (never an
/// inverted range). This *is* the min-central guarantee: neither panel can be
/// dragged into the sim's space.
pub fn side_range(viewport_w: f32, other_side_w: f32) -> RangeInclusive<f32> {
    let max = (viewport_w - other_side_w - CENTRAL_MIN).clamp(SIDE_MIN, SIDE_MAX);
    SIDE_MIN..=max
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_range_preserves_central_min() {
        // A comfortable 1920-wide window with a 370 opposite panel: the max leaves at
        // least CENTRAL_MIN for the sim.
        let r = side_range(1920.0, 370.0);
        assert!(*r.end() + 370.0 + CENTRAL_MIN <= 1920.0 + 0.01);
        assert!(*r.start() <= *r.end());

        // Degenerate: a tiny window can't honor the floor → collapse to a point, not
        // an inverted range.
        let tight = side_range(600.0, 370.0);
        assert_eq!(*tight.start(), SIDE_MIN);
        assert_eq!(*tight.end(), SIDE_MIN);
    }

    #[test]
    fn side_range_never_inverted() {
        for &vw in &[320.0_f32, 800.0, 1280.0, 1920.0, 3840.0] {
            for &other in &[SIDE_MIN, SIDE_DEFAULT, SIDE_MAX] {
                let r = side_range(vw, other);
                assert!(*r.start() <= *r.end(), "inverted at vw={vw}, other={other}");
                assert!(*r.end() <= SIDE_MAX);
                assert!(*r.start() >= SIDE_MIN);
            }
        }
    }
}
