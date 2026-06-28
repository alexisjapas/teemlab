//! **Dismissable inline help.** The panels carry explanatory hints (what a control
//! does, how to interact). They help a newcomer but clutter once the tool is known,
//! so a single flag — toggled from the top-bar **View** menu ([`toggle`]) — gates them
//! all through [`hint`]. The flag lives in egui's own memory (a temp value keyed by
//! [`id`]), so no boolean has to be threaded through every panel function. Default
//! **on** (discoverable); off hides every hint at once.

use bevy_egui::egui;

/// egui memory key for the "show inline help" flag.
fn id() -> egui::Id {
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

/// The **Inline help** toggle, for the View menu. Reads/writes the flag in egui memory.
pub fn toggle(ui: &mut egui::Ui) {
    let mut show = enabled(ui);
    if ui
        .checkbox(&mut show, "Inline help")
        .on_hover_text("Show the explanatory hints in the panels.")
        .changed()
    {
        ui.data_mut(|d| d.insert_temp(id(), show));
    }
}
