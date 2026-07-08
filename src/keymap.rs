//! Windowed-UI **key bindings** — the single source of truth for the keyboard and
//! mouse controls. Both the input systems ([`pressed`]) and what the user is *told*
//! (button tooltips via [`tooltip`], the shortcuts cheatsheet over [`BINDINGS`] /
//! [`MOUSE`]) read this one table, so a tooltip and the cheatsheet can never drift
//! from what the code actually does.
//!
//! A module of the windowed *binary* only; it sets no sim state (the actions it names
//! only drive `Time<Virtual>`, the view, or a UI toggle — cf. their call sites).

use bevy::prelude::*;

/// A keyboard-driven UI action. The mouse gestures (which have no `KeyCode`) live in
/// [`MOUSE`] instead.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UiAction {
    /// Toggle the run/pause state of the sim clock.
    PlayPause,
    /// Advance exactly one fixed tick (only while paused).
    StepOnce,
    /// Rebuild the world from the current config.
    ResetWorld,
    /// Recenter the view on the whole arena (pan/zoom).
    ResetView,
    /// Delete the entity under the cursor.
    DeleteUnderCursor,
    /// Show/hide the keyboard-shortcuts cheatsheet.
    ToggleShortcuts,
    /// Fold/unfold the left (World) column to its rail.
    ToggleLeftPanel,
    /// Fold/unfold the right (Analysis) column to its rail.
    ToggleRightPanel,
    /// Fold/unfold the bottom strip (status + curves + breeding) to its rail.
    ToggleBottomPanel,
}

/// One row of the binding table: the action, the physical key(s) that trigger it, the
/// text shown to the user for those keys, a human label, and an optional condition.
pub struct Binding {
    pub action: UiAction,
    /// Any of these keys fires the action ([`pressed`] uses `just_pressed`).
    pub keys: &'static [KeyCode],
    /// How the key(s) read in a tooltip / the cheatsheet (e.g. `"Del / Backspace"`).
    pub keys_text: &'static str,
    /// Human-readable description of the action.
    pub label: &'static str,
    /// When the binding applies, empty if always (e.g. `"when paused"`).
    pub when: &'static str,
}

/// Every keyboard binding — the one table tooltips and the cheatsheet both read.
pub const BINDINGS: &[Binding] = &[
    Binding {
        action: UiAction::PlayPause,
        keys: &[KeyCode::Space],
        keys_text: "Space",
        label: "Play / pause",
        when: "",
    },
    Binding {
        action: UiAction::StepOnce,
        keys: &[KeyCode::ArrowRight],
        keys_text: "→",
        label: "Advance one tick",
        when: "when paused",
    },
    Binding {
        action: UiAction::ResetWorld,
        keys: &[KeyCode::KeyR],
        keys_text: "R",
        label: "Rebuild the world from the config",
        when: "",
    },
    Binding {
        action: UiAction::ResetView,
        keys: &[KeyCode::Home],
        keys_text: "Home",
        label: "Recenter the view",
        when: "",
    },
    Binding {
        action: UiAction::DeleteUnderCursor,
        keys: &[KeyCode::Delete, KeyCode::Backspace],
        keys_text: "Del / Backspace",
        label: "Delete the entity under the cursor",
        when: "",
    },
    Binding {
        // Slash is `?` with Shift on most layouts; F1 is the layout-agnostic fallback.
        action: UiAction::ToggleShortcuts,
        keys: &[KeyCode::Slash, KeyCode::F1],
        keys_text: "?",
        label: "Keyboard shortcuts",
        when: "",
    },
    Binding {
        action: UiAction::ToggleLeftPanel,
        keys: &[KeyCode::Digit1],
        keys_text: "1",
        label: "Fold / unfold the World panel",
        when: "",
    },
    Binding {
        action: UiAction::ToggleRightPanel,
        keys: &[KeyCode::Digit2],
        keys_text: "2",
        label: "Fold / unfold the Analysis panel",
        when: "",
    },
    Binding {
        action: UiAction::ToggleBottomPanel,
        keys: &[KeyCode::Digit3],
        keys_text: "3",
        label: "Fold / unfold the bottom strip",
        when: "",
    },
];

/// The mouse gestures — no `KeyCode`, so a separate table (for the cheatsheet). Kept
/// beside [`BINDINGS`] so the two halves of the cheatsheet share one source.
pub const MOUSE: &[(&str, &str)] = &[
    ("Scroll", "Zoom toward the cursor"),
    ("Middle / right drag", "Pan the view"),
    ("Click", "Select an agent (void = deselect)"),
    ("Drag from Archetypes", "Place an entity"),
];

/// The binding for `action`, or `None` if the table has none (a bug — see the tests).
fn binding(action: UiAction) -> Option<&'static Binding> {
    BINDINGS.iter().find(|b| b.action == action)
}

/// Whether `action`'s key(s) were **just pressed** this frame. The single input
/// chokepoint: every keyboard handler routes through it, so the physical keys are
/// defined once, here.
pub fn pressed(keys: &ButtonInput<KeyCode>, action: UiAction) -> bool {
    binding(action).is_some_and(|b| b.keys.iter().any(|k| keys.just_pressed(*k)))
}

/// `base` with the action's keys appended (`"Play / pause  ·  Space"`) — the way every
/// key-bearing tooltip is built, so tooltips can't drift from [`BINDINGS`].
pub fn tooltip(base: &str, action: UiAction) -> String {
    match binding(action) {
        Some(b) => format!("{base}  ·  {}", b.keys_text),
        None => base.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[UiAction] = &[
        UiAction::PlayPause,
        UiAction::StepOnce,
        UiAction::ResetWorld,
        UiAction::ResetView,
        UiAction::DeleteUnderCursor,
        UiAction::ToggleShortcuts,
        UiAction::ToggleLeftPanel,
        UiAction::ToggleRightPanel,
        UiAction::ToggleBottomPanel,
    ];

    #[test]
    fn bindings_cover_every_action_once() {
        for &action in ALL {
            let n = BINDINGS.iter().filter(|b| b.action == action).count();
            assert_eq!(n, 1, "{action:?} should appear exactly once, found {n}");
        }
        assert_eq!(BINDINGS.len(), ALL.len(), "an action is missing from ALL");
    }

    #[test]
    fn no_conflicting_keys() {
        for (i, a) in BINDINGS.iter().enumerate() {
            for b in &BINDINGS[i + 1..] {
                for k in a.keys {
                    assert!(
                        !b.keys.contains(k),
                        "{:?} shares {k:?} with {:?}",
                        a.action,
                        b.action
                    );
                }
            }
        }
    }

    #[test]
    fn tooltip_appends_keys_text() {
        assert!(tooltip("Play / pause", UiAction::PlayPause).ends_with("Space"));
        assert_eq!(
            tooltip("Delete", UiAction::DeleteUnderCursor),
            "Delete  ·  Del / Backspace"
        );
    }
}
