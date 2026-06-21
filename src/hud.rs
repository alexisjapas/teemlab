//! HUD egui du build fenêtré : **tracé des courbes** d'évolution.
//!
//! Module du *binaire* fenêtré uniquement. Depuis le déménagement de l'échantillonnage
//! dans la lib ([`teemlab::metrics`]), ce module ne fait plus que **tracer** (au
//! `Painter` d'egui) la donnée déjà calculée — les mêmes [`Curve`] que le visualiseur
//! natif Bevy ([`teemlab::dataviz`]), d'où des courbes identiques entre l'aperçu egui et
//! la vidéo.
//!
//! On respecte l'invariant cardinal : aucune logique de simulation ici — ce module ne
//! fait que **lire** l'historique pour l'afficher.
//!
//! Pas de dépendance de tracé externe (egui_plot) : on dessine les polylignes à la main
//! avec le `Painter` d'egui — dans l'esprit « fait maison » du projet.

use bevy_egui::egui;
use teemlab::SimConfig;
use teemlab::metrics::{Curve, History, population_curves, trait_curves};

/// Convertit une couleur sRGB `[r, g, b] ∈ [0, 1]` (agnostique au backend) en `Color32`.
fn rgb(c: [f32; 3]) -> egui::Color32 {
    let q = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgb(q(c[0]), q(c[1]), q(c[2]))
}

/// Les courbes d'évolution — population par espèce puis dérive des gènes normalisée.
/// Rendue dans le panneau du bas. Lecture seule de l'historique (et du `config` pour
/// nommer/colorer/filtrer les espèces).
pub(crate) fn hud_section(ui: &mut egui::Ui, history: &mut History, config: &SimConfig) {
    ui.horizontal(|ui| {
        ui.weak(format!("{} échantillons", history.sample_count()));
        if ui.button("↻ Effacer").clicked() {
            history.clear();
        }
    });
    ui.separator();
    let history: &History = history;
    ui.strong("Population par espèce");
    draw_population(ui, history, config);
    ui.add_space(10.0);
    ui.strong("Dérive des gènes (normalisée 0–1)");
    draw_traits(ui, history);
}

fn draw_population(ui: &mut egui::Ui, history: &History, config: &SimConfig) {
    if history.is_empty() {
        ui.weak("(en attente de données…)");
        return;
    }
    let (curves, y_max) = population_curves(history, config);
    if curves.is_empty() {
        ui.weak("(aucune espèce vivante)");
        return;
    }
    plot(ui, 110.0, &curves, 0.0, y_max * 1.1);
    legend(ui, &curves);
}

fn draw_traits(ui: &mut egui::Ui, history: &History) {
    if history.is_empty() {
        ui.weak("(en attente de données…)");
        return;
    }
    // Bornes fixes [0, 1] : la dérive se lit contre l'étendue possible du gène.
    let curves = trait_curves(history);
    plot(ui, 110.0, &curves, 0.0, 1.0);
    legend(ui, &curves);
}

/// Une légende : une pastille colorée + le nom de chaque courbe.
fn legend(ui: &mut egui::Ui, curves: &[Curve]) {
    ui.horizontal_wrapped(|ui| {
        for c in curves {
            ui.colored_label(rgb(c.color), format!("● {}", c.name));
        }
    });
}

/// Trace les courbes dans un cadre de hauteur `height`, l'axe Y borné à `[y_min, y_max]`
/// et l'axe X couvrant l'étendue temporelle des données. Tracé maison au `Painter` : un
/// fond, les polylignes, et les graduations Y.
fn plot(ui: &mut egui::Ui, height: f32, curves: &[Curve], y_min: f32, y_max: f32) {
    let width = ui.available_width().max(64.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(
        rect,
        egui::CornerRadius::same(2),
        egui::Color32::from_gray(18),
    );

    let (mut x_min, mut x_max) = (f32::MAX, f32::MIN);
    for c in curves {
        for q in &c.pts {
            x_min = x_min.min(q[0]);
            x_max = x_max.max(q[0]);
        }
    }
    // Pas (encore) assez de points pour une ligne : on affiche un repère.
    if !x_max.is_finite() || x_max <= x_min {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "…",
            egui::FontId::monospace(12.0),
            egui::Color32::from_gray(90),
        );
        return;
    }

    let inner = rect.shrink(4.0);
    let y_span = (y_max - y_min).max(1e-6);
    let map = |x: f32, y: f32| {
        egui::pos2(
            inner.left() + (x - x_min) / (x_max - x_min) * inner.width(),
            inner.bottom() - (y - y_min) / y_span * inner.height(),
        )
    };

    for c in curves {
        let stroke = egui::Stroke::new(1.5, rgb(c.color));
        for w in c.pts.windows(2) {
            painter.line_segment([map(w[0][0], w[0][1]), map(w[1][0], w[1][1])], stroke);
        }
    }

    // Graduations Y : haut et bas du cadre.
    let tick = egui::Color32::from_gray(130);
    let font = egui::FontId::monospace(9.0);
    painter.text(
        rect.right_top() + egui::vec2(-2.0, 1.0),
        egui::Align2::RIGHT_TOP,
        format!("{y_max:.0}"),
        font.clone(),
        tick,
    );
    painter.text(
        rect.right_bottom() + egui::vec2(-2.0, -1.0),
        egui::Align2::RIGHT_BOTTOM,
        format!("{y_min:.0}"),
        font,
        tick,
    );
}
