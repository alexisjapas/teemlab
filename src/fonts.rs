//! Windowed-UI **typography**: registers the project fonts into egui.
//!
//! Crucial distinction: bevy_egui renders the editor through egui's **own** font
//! system ([`egui::FontDefinitions`]), *not* Bevy's `AssetServer` / `cosmic-text`
//! (that path is the video `dataviz`, which keeps its own `DejaVuSans`). So the
//! editor's fonts are configured here, on the egui context, once at startup.
//!
//! The chosen typographic system, three roles:
//! - **Inter** — text (labels, menus, panels): fronts egui's *Proportional* family.
//! - **Departure Mono** — values (numbers, the technical look): fronts *Monospace*.
//! - **Phosphor** — icons (play/pause/…): a **dedicated named family** ([`phosphor`]),
//!   drawn by its Private-Use-Area codepoint (cf. [`icons`]) via [`icon`] / [`icon_label`].
//!   A separate family (rather than a fallback on Proportional) is required because
//!   Inter itself maps *some* PUA codepoints, which would shadow our icons.
//!
//! Each is read from `assets/fonts/` **at runtime** (relative to the CWD, like
//! `scenarios/` and `species/`). egui's built-in fonts stay behind ours as fallbacks
//! (broad glyph coverage). A **missing file is skipped** with a warning, so the
//! windowed build still runs on egui's defaults until the assets are dropped in.
//!
//! **Timing:** egui binds new fonts only at the **start of the next pass**, so the
//! `phosphor` family is unbound during the first pass — drawing an icon then would
//! panic. [`FontsReady`] flips true once the fonts are live; the panel system gates
//! its first render on it, so no icon is ever drawn before its family exists.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use std::sync::Arc;

/// Expected font files under `assets/fonts/`. Place these to activate the theme.
const INTER: (&str, &str) = ("Inter", "assets/fonts/Inter-Regular.ttf");
const DEPARTURE: (&str, &str) = ("DepartureMono", "assets/fonts/DepartureMono-Regular.otf");
const PHOSPHOR: (&str, &str) = ("Phosphor", "assets/fonts/Phosphor.ttf");

/// `true` once [`setup_ui_fonts`]'s fonts are **live** (the pass after `set_fonts`).
/// The panel system skips its first render until then, so an icon (which needs the
/// [`phosphor`] family) is never drawn before egui has bound it.
#[derive(Resource, Default)]
pub struct FontsReady(pub bool);

/// **Phosphor icon codepoints**, font **v2.1** — verified against
/// `assets/fonts/Phosphor.ttf` by rendering each in a Phosphor-only family (the `.ttf`
/// carries no glyph names; codepoints come from the v2 web mapping). Codepoints are
/// **version-specific** (Phosphor reassigns them across major versions), so re-derive
/// these if the bundled font is upgraded. Draw with [`icon`] / [`icon_label`].
pub mod icons {
    pub const PLAY: char = '\u{E3D0}';
    pub const PAUSE: char = '\u{E39E}';
    pub const STEP: char = '\u{E5A6}'; // skip-forward
    pub const RESET: char = '\u{E036}'; // arrow-clockwise
    pub const RECORD: char = '\u{E3EE}'; // export / record dot
    pub const PLUS: char = '\u{E3D4}';
    pub const TRASH: char = '\u{E4A6}';
    pub const ARROW_UP: char = '\u{E08E}'; // move up
    pub const ARROW_DOWN: char = '\u{E03E}'; // move down
    pub const COPY: char = '\u{E1CA}'; // duplicate
    pub const X: char = '\u{E4F6}'; // close / remove
    pub const CIRCLE: char = '\u{E18A}'; // archetype bullet (unselected)
    pub const CARET_RIGHT: char = '\u{E13A}'; // selected mark
    pub const CARET_DOWN: char = '\u{E136}'; // menu caret
    pub const SPARKLE: char = '\u{E6A2}'; // captured weights
    pub const ARROW_RIGHT: char = '\u{E06C}'; // relation actor → target
    pub const DOWNLOAD: char = '\u{E20C}'; // import (download-simple)
    pub const UPLOAD: char = '\u{E4C0}'; // export species (upload-simple)
    pub const FLOPPY: char = '\u{E248}'; // capture as archetype
}

/// The egui font family carrying the **Phosphor** icons.
pub fn phosphor() -> egui::FontFamily {
    egui::FontFamily::Name("phosphor".into())
}

/// A [`egui::RichText`] for a Phosphor `glyph` (cf. [`icons`]) — for an icon-only
/// button or label: `ui.button(fonts::icon(icons::TRASH))`. For an icon **+** a text
/// label (which need different families), use [`icon_label`].
pub fn icon(glyph: char) -> egui::RichText {
    egui::RichText::new(glyph).family(phosphor())
}

/// A [`egui::WidgetText`] mixing a Phosphor `glyph` (icon family) and a `label` (Inter):
/// `ui.button(fonts::icon_label(icons::PLUS, "Agent"))`. A single `RichText` carries one
/// family, so we build a two-section `LayoutJob`. Both use [`egui::Color32::PLACEHOLDER`]
/// (egui's sentinel) so the widget recolours them per state (hover / disabled).
pub fn icon_label(glyph: char, label: &str) -> egui::WidgetText {
    let size = 14.0; // body size; egui buttons use the body text style.
    let fmt = |family: egui::FontFamily| egui::TextFormat {
        font_id: egui::FontId::new(size, family),
        color: egui::Color32::PLACEHOLDER,
        valign: egui::Align::Center,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    job.append(&glyph.to_string(), 0.0, fmt(phosphor()));
    job.append(
        &format!("  {label}"),
        0.0,
        fmt(egui::FontFamily::Proportional),
    );
    job.into()
}

/// Renders `add_contents` with the **value** font — Departure Mono (the Monospace
/// family) at the body size — via a scoped `override_font_id`. Put a value-bearing
/// widget inside (a `Slider`/`DragValue` *without* its label, a numeric read-out) and
/// keep the label **outside**, so the digits are monospace while labels stay Inter.
/// Returns the closure's value (e.g. the widget's `Response`).
pub fn value<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope(|ui| {
        let size = egui::TextStyle::Body.resolve(ui.style()).size;
        ui.style_mut().override_font_id =
            Some(egui::FontId::new(size, egui::FontFamily::Monospace));
        add_contents(ui)
    })
    .inner
}

/// Installs the UI fonts on the egui context. Inter fronts the *Proportional* family,
/// Departure Mono the *Monospace* family (egui's built-ins behind them as fallback);
/// Phosphor is a separate [`phosphor`] family for the icon codepoints. Files absent ⇒
/// that role keeps egui's default (a warning is logged), so the build is safe before
/// the assets exist. Runs for the first two passes (a `Local` step): pass 0 calls
/// `set_fonts` (live next pass), pass 1 flips [`FontsReady`] now that they are bound.
pub fn setup_ui_fonts(
    mut contexts: EguiContexts,
    mut ready: ResMut<FontsReady>,
    mut step: Local<u8>,
) -> Result {
    match *step {
        // Pass 1: the fonts set on pass 0 are now live → unblock the panels.
        1 => {
            ready.0 = true;
            *step = 2;
            return Ok(());
        }
        2.. => return Ok(()),
        _ => {}
    }
    *step = 1;

    let ctx = contexts.ctx_mut()?;
    let mut fonts = egui::FontDefinitions::default();

    // Register `key`'s bytes if the .ttf loaded; else keep egui's default.
    let load = |fonts: &mut egui::FontDefinitions, (key, path): (&str, &str)| -> bool {
        match std::fs::read(path) {
            Ok(bytes) => {
                fonts
                    .font_data
                    .insert(key.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
                true
            }
            Err(e) => {
                warn!("UI font '{path}' not loaded ({e}); keeping the egui default.");
                false
            }
        }
    };

    if load(&mut fonts, INTER) {
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, INTER.0.to_owned());
    }
    if load(&mut fonts, DEPARTURE) {
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, DEPARTURE.0.to_owned());
    }
    if load(&mut fonts, PHOSPHOR) {
        // A dedicated family (opt-in), NOT a Proportional fallback: Inter maps some PUA
        // codepoints and would shadow our icons there. Icons are drawn only via this
        // family ([`icon`] / [`icon_label`]), gated on [`FontsReady`] so they are never
        // requested before egui binds the family (next pass).
        //
        // Phosphor sits **first** (its icon codepoints win), with the Proportional fonts
        // appended **behind** it purely as a glyph fallback. Without that fallback, an
        // icon-only font has no replacement character ('◻'/'?') and epaint logs
        // "Failed to find replacement characters …" when it builds this family. The
        // fallback never shadows an icon (Phosphor is queried first) and we never draw
        // text through this family, so it is invisible in practice — it only silences
        // the warning.
        let mut family = vec![PHOSPHOR.0.to_owned()];
        if let Some(proportional) = fonts.families.get(&egui::FontFamily::Proportional) {
            family.extend(proportional.iter().cloned());
        }
        fonts.families.insert(phosphor(), family);
    }

    ctx.set_fonts(fonts);
    Ok(())
}
