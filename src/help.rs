//! **Dismissable inline help.** The panels carry explanatory hints (what a control
//! does, how to interact). They help a newcomer but clutter once the tool is known,
//! so a single flag — toggled from the top-bar **Help** menu ([`toggle`]) — gates them
//! all through [`hint`]. The flag's source of truth is `panels::UiPrefs::inline_help`,
//! which `panels::dock` **mirrors** into egui memory (a temp value keyed by [`id`])
//! each frame, so no boolean has to be threaded through every panel function. Default
//! **on** (discoverable); off hides every hint at once.

use bevy_egui::egui;

/// egui memory key for the "show inline help" flag (written by `panels::dock` from
/// `UiPrefs`, read by [`enabled`]).
pub(crate) fn id() -> egui::Id {
    egui::Id::new("teemlab_show_help")
}

/// Whether inline help is currently shown (default `true`).
pub fn enabled(ui: &egui::Ui) -> bool {
    ui.data(|d| d.get_temp::<bool>(id())).unwrap_or(true)
}

/// Renders `text` as a small hint **only when** inline help is enabled — the single
/// chokepoint every panel routes its explanatory text through (also unifying the hint
/// typography on `ui.small`).
pub fn hint(ui: &mut egui::Ui, text: impl Into<String>) {
    if enabled(ui) {
        ui.small(text.into());
    }
}

/// The **Inline help** toggle, for the Help menu. Bound directly to the preference
/// (`panels::UiPrefs::inline_help`); `panels::dock` mirrors it into egui memory next
/// frame, where [`enabled`] / [`hint`] read it.
pub fn toggle(ui: &mut egui::Ui, inline_help: &mut bool) {
    ui.checkbox(inline_help, "Inline help")
        .on_hover_text("Show the explanatory hints in the panels.");
}
