//! HUD du build fenêtré : **courbes d'évolution en temps réel** (item 10).
//!
//! Module du *binaire* fenêtré uniquement (jamais compilé dans le headless) :
//! comme [`crate::editor`], tout ce qui touche egui vit ici, à l'écart du cœur
//! render-agnostic.
//!
//! On respecte l'invariant cardinal : aucune logique de simulation ici. Le HUD
//! ne fait que **lire** l'état du monde pour l'afficher — il n'écrit jamais dans
//! la sim. L'échantillonnage tourne dans `Update` et se cale sur `Time<Virtual>`
//! (donc il se fige avec la pause et suit l'accéléré, comme la sim — cf. §6).
//!
//! Pas de dépendance de tracé externe (egui_plot) : on dessine les polylignes à
//! la main avec le `Painter` d'egui — dans l'esprit « fait maison » du projet et
//! sans version à accorder avec l'egui qu'embarque bevy_egui.

use std::collections::VecDeque;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use teemlab::SimConfig;
use teemlab::components::{Agent, Food, Species};
use teemlab::config::Bounds;
use teemlab::genotype::Genotype;

use crate::editor::species_color32;

/// Un instantané de métriques, daté en temps simulé.
struct Sample {
    /// Temps simulé (`Time<Virtual>`) de l'échantillon, en secondes.
    t: f32,
    /// Population vivante par espèce (indexée comme `Species`).
    population: Vec<u32>,
    /// Sources de nourriture présentes.
    food: u32,
    // Gènes moyens, chacun **normalisé dans ses bornes** (`[0, 1]`) pour que des
    // traits d'échelles différentes (vitesse vs angle) se comparent sur un seul
    // graphe.
    speed: f32,
    vision: f32,
    agility: f32,
    fov: f32,
}

/// Historique glissant des métriques — l'état du HUD. Vit dans le binaire
/// fenêtré seul (jamais dans la sim).
#[derive(Resource)]
pub struct History {
    /// Intervalle entre deux échantillons, en secondes simulées.
    interval: f32,
    /// Nombre maximal d'échantillons conservés (fenêtre glissante).
    max_samples: usize,
    /// Prochain instant d'échantillonnage (temps simulé).
    next_at: f32,
    /// Les échantillons, du plus ancien au plus récent.
    samples: VecDeque<Sample>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            interval: 0.5,
            max_samples: 1200, // 0,5 s × 1200 = 10 min de temps simulé
            next_at: 0.0,
            samples: VecDeque::new(),
        }
    }
}

impl History {
    /// Repart de zéro : vide les échantillons et réarme l'horloge. Appelé par le
    /// bouton « Effacer » du HUD et par la réinitialisation à chaud (item 11).
    pub fn clear(&mut self) {
        self.samples.clear();
        self.next_at = 0.0;
    }
}

/// Normalise une valeur de gène dans ses bornes, vers `[0, 1]`.
fn norm(v: f32, b: Bounds) -> f32 {
    if b.span() > 0.0 {
        ((v - b.min) / b.span()).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Échantillonne les métriques du monde à cadence fixe en temps simulé. Lecture
/// seule : c'est de l'observation pour affichage, pas de la logique de sim — d'où
/// sa place légitime dans `Update`.
pub fn sample_history(
    time: Res<Time<Virtual>>,
    config: Res<SimConfig>,
    mut history: ResMut<History>,
    agents: Query<(&Species, &Genotype), With<Agent>>,
    food: Query<(), With<Food>>,
) {
    let now = time.elapsed_secs();
    if now < history.next_at {
        return;
    }
    history.next_at = now + history.interval;

    let species_count = config.species_count.max(1) as usize;
    let mut population = vec![0u32; species_count];
    let (mut sum_speed, mut sum_vision, mut sum_agility, mut sum_fov) = (0.0, 0.0, 0.0, 0.0);
    let mut n = 0u32;
    for (species, g) in &agents {
        let idx = (species.0 as usize).min(species_count - 1);
        population[idx] += 1;
        sum_speed += norm(g.max_speed, config.speed_bounds);
        sum_vision += norm(g.vision_range, config.vision_range_bounds);
        sum_agility += norm(g.agility, config.agility_bounds);
        sum_fov += norm(g.vision_fov.to_degrees(), config.vision_fov_bounds);
        n += 1;
    }

    // Population zéro → on garde les derniers gènes moyens connus (un graphe qui
    // s'effondre à zéro laisserait croire que les gènes ont fondu, pas que la
    // population s'est éteinte).
    let (speed, vision, agility, fov) = if n > 0 {
        let inv = 1.0 / n as f32;
        (sum_speed * inv, sum_vision * inv, sum_agility * inv, sum_fov * inv)
    } else if let Some(last) = history.samples.back() {
        (last.speed, last.vision, last.agility, last.fov)
    } else {
        (0.0, 0.0, 0.0, 0.0)
    };

    history.samples.push_back(Sample {
        t: now,
        population,
        food: food.iter().count() as u32,
        speed,
        vision,
        agility,
        fov,
    });
    while history.samples.len() > history.max_samples {
        history.samples.pop_front();
    }
}

/// La fenêtre du HUD : population par espèce + dérive des gènes. Tourne dans
/// `EguiPrimaryContextPass`, aux côtés de l'éditeur.
pub fn hud_ui(mut contexts: EguiContexts, mut history: ResMut<History>) -> Result {
    let ctx = contexts.ctx_mut()?;
    egui::Window::new("Évolution — courbes")
        .default_pos([270.0, 12.0])
        .default_width(340.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.weak(format!("{} échantillons", history.samples.len()));
                if ui.button("↻ Effacer").clicked() {
                    history.clear();
                }
            });
            ui.separator();
            ui.strong("Population par espèce");
            draw_population(ui, &history);
            ui.add_space(10.0);
            ui.strong("Dérive des gènes (normalisée 0–1)");
            draw_traits(ui, &history);
        });
    Ok(())
}

/// Une courbe à tracer : un nom, une couleur, et ses points `[temps, valeur]`.
struct Line {
    name: String,
    color: egui::Color32,
    pts: Vec<[f32; 2]>,
}

fn draw_population(ui: &mut egui::Ui, history: &History) {
    let Some(last) = history.samples.back() else {
        ui.weak("(en attente de données…)");
        return;
    };
    let mut lines = Vec::new();
    let mut y_max = 1.0_f32;
    for sp in 0..last.population.len() {
        let pts: Vec<[f32; 2]> = history
            .samples
            .iter()
            .map(|s| [s.t, *s.population.get(sp).unwrap_or(&0) as f32])
            .collect();
        for q in &pts {
            y_max = y_max.max(q[1]);
        }
        lines.push(Line {
            name: format!("espèce {sp}"),
            color: species_color32(sp as u16),
            pts,
        });
    }
    let food: Vec<[f32; 2]> = history.samples.iter().map(|s| [s.t, s.food as f32]).collect();
    for q in &food {
        y_max = y_max.max(q[1]);
    }
    lines.push(Line {
        name: "nourriture".to_string(),
        color: egui::Color32::from_gray(150),
        pts: food,
    });

    plot(ui, 110.0, &lines, 0.0, y_max * 1.1);
    legend(ui, &lines);
}

fn draw_traits(ui: &mut egui::Ui, history: &History) {
    if history.samples.is_empty() {
        ui.weak("(en attente de données…)");
        return;
    }
    let series = |get: &dyn Fn(&Sample) -> f32| -> Vec<[f32; 2]> {
        history.samples.iter().map(|s| [s.t, get(s)]).collect()
    };
    let lines = vec![
        Line {
            name: "vitesse".to_string(),
            color: egui::Color32::from_rgb(120, 200, 255),
            pts: series(&|s| s.speed),
        },
        Line {
            name: "vision".to_string(),
            color: egui::Color32::from_rgb(255, 170, 90),
            pts: series(&|s| s.vision),
        },
        Line {
            name: "agilité".to_string(),
            color: egui::Color32::from_rgb(150, 230, 120),
            pts: series(&|s| s.agility),
        },
        Line {
            name: "champ".to_string(),
            color: egui::Color32::from_rgb(220, 140, 230),
            pts: series(&|s| s.fov),
        },
    ];

    // Bornes fixes [0, 1] : la dérive se lit contre l'étendue possible du gène.
    plot(ui, 110.0, &lines, 0.0, 1.0);
    legend(ui, &lines);
}

/// Une légende : une pastille colorée + le nom de chaque courbe.
fn legend(ui: &mut egui::Ui, lines: &[Line]) {
    ui.horizontal_wrapped(|ui| {
        for l in lines {
            ui.colored_label(l.color, format!("● {}", l.name));
        }
    });
}

/// Trace les courbes `lines` dans un cadre de hauteur `height`, l'axe Y borné à
/// `[y_min, y_max]` et l'axe X couvrant l'étendue temporelle des données. Tracé
/// maison au `Painter` : un fond, les polylignes, et les graduations Y.
fn plot(ui: &mut egui::Ui, height: f32, lines: &[Line], y_min: f32, y_max: f32) {
    let width = ui.available_width().max(64.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, egui::CornerRadius::same(2), egui::Color32::from_gray(18));

    let (mut x_min, mut x_max) = (f32::MAX, f32::MIN);
    for l in lines {
        for q in &l.pts {
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

    for l in lines {
        let stroke = egui::Stroke::new(1.5, l.color);
        for w in l.pts.windows(2) {
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
