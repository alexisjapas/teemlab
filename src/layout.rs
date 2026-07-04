//! Windowed-UI **layout math**: the panel width ranges and the master/detail mode
//! of the left region. Pure functions (no egui, no ECS) so [`panels::dock`] stays a
//! thin caller and the sizing rules are unit-tested.
//!
//! [`panels::dock`]: crate::panels::dock
//!
//! The rules exist to solve one problem: the side panels used to be a fixed 370 pt
//! each and, with the archetype editor open, the left region doubled — 1110 pt of
//! chrome that squeezed the sim to a sliver on a laptop. Now the side panels are
//! **resizable** within a range that always reserves [`CENTRAL_MIN`] for the sim,
//! and on a narrow window the archetype editor **folds into** the left panel
//! (single column) instead of opening a second one.

use std::ops::RangeInclusive;

/// Narrowest a side panel may be dragged (egui points): still fits the densest
/// content (the gene grid) without clipping.
pub const SIDE_MIN: f32 = 280.0;
/// The side panel's width on a fresh launch — the historical fixed width.
pub const SIDE_DEFAULT: f32 = 370.0;
/// Widest a side panel may be dragged: past this it is just wasted space.
pub const SIDE_MAX: f32 = 520.0;
/// The sim area is never allowed below this (egui points): a drag on either
/// separator stops here, and the two-column left mode is refused when it would break
/// this floor.
pub const CENTRAL_MIN: f32 = 480.0;
/// Dead-band (egui points) around the two-/single-column threshold: the mode only
/// flips once the window crosses the boundary by this much, so dragging the window
/// edge across it doesn't make the layout flicker.
pub const HYSTERESIS: f32 = 24.0;

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

/// Whether the left region shows the world and the archetype editor as **two
/// columns** or folds the editor into a **single column**.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LeftMode {
    /// World panel + a separate archetype-editor panel (wide windows).
    #[default]
    TwoColumn,
    /// One left panel whose content is the world **or** the archetype editor
    /// (narrow windows) — the detail view replaces the master in place.
    SingleColumn,
}

/// Picks the left-region mode with hysteresis. Two columns are used only when the
/// viewport can hold `right_w + world_w + editor_w + CENTRAL_MIN`; the `prev` mode is
/// kept while the viewport sits within [`HYSTERESIS`] of that threshold, so crossing
/// the boundary while resizing the window doesn't flip-flop the layout.
pub fn left_mode(
    prev: LeftMode,
    viewport_w: f32,
    right_w: f32,
    world_w: f32,
    editor_w: f32,
) -> LeftMode {
    let threshold = right_w + world_w + editor_w + CENTRAL_MIN;
    // Widen the "stay" band around the threshold by ±HYSTERESIS, keyed on `prev`:
    // to switch *up* to two columns we must clear `threshold + HYSTERESIS`; to fall
    // *back* to one we must drop below `threshold - HYSTERESIS`. In between, hold.
    match prev {
        LeftMode::TwoColumn if viewport_w >= threshold - HYSTERESIS => LeftMode::TwoColumn,
        LeftMode::TwoColumn => LeftMode::SingleColumn,
        LeftMode::SingleColumn if viewport_w >= threshold + HYSTERESIS => LeftMode::TwoColumn,
        LeftMode::SingleColumn => LeftMode::SingleColumn,
    }
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

    #[test]
    fn left_mode_threshold_and_hysteresis() {
        let (right, world, editor) = (370.0, 370.0, 370.0);
        let threshold = right + world + editor + CENTRAL_MIN; // 1590

        // Comfortably wide → two columns; comfortably narrow → single.
        assert_eq!(
            left_mode(
                LeftMode::SingleColumn,
                threshold + 100.0,
                right,
                world,
                editor
            ),
            LeftMode::TwoColumn
        );
        assert_eq!(
            left_mode(LeftMode::TwoColumn, threshold - 100.0, right, world, editor),
            LeftMode::SingleColumn
        );

        // Inside the band the previous mode holds (no flip-flop while dragging).
        assert_eq!(
            left_mode(LeftMode::TwoColumn, threshold, right, world, editor),
            LeftMode::TwoColumn
        );
        assert_eq!(
            left_mode(LeftMode::SingleColumn, threshold, right, world, editor),
            LeftMode::SingleColumn
        );

        // Flipping up requires clearing threshold + HYSTERESIS; down requires
        // dropping below threshold - HYSTERESIS.
        assert_eq!(
            left_mode(
                LeftMode::SingleColumn,
                threshold + HYSTERESIS - 1.0,
                right,
                world,
                editor
            ),
            LeftMode::SingleColumn
        );
        assert_eq!(
            left_mode(
                LeftMode::TwoColumn,
                threshold - HYSTERESIS + 1.0,
                right,
                world,
                editor
            ),
            LeftMode::TwoColumn
        );
    }
}
