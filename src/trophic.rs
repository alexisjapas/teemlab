//! The **derived trophic graph** — one object, rendered three ways (redesign Phase B,
//! `docs/emergent-trophics.md` §6). Because edibility is now *computed* (emergent
//! targeting, Phase A), the whole food web can be derived from a [`SimConfig`] and
//! reasoned about; this module builds it **once** so the screens can skin it:
//!
//! - **Studio** — the *static* reachability validator (this stage, B3): the graph +
//!   the broken-chain flags ([`SimConfig::broken_chains`], §6.1).
//! - **Observe** — the *dynamic* fragility overlay (B7): the same graph annotated with
//!   live population / flow.
//! - **Lab** — a *fragility* metric (B7): an aggregate of the dependency structure.
//!
//! [`derive`](TrophicGraph::derive) is **pure** (no egui, no ECS) and unit-tested;
//! [`paint`](TrophicGraph::paint) draws the node-link diagram. Nodes are the
//! components ∪ the archetypes; edges are **edibility** (archetype→archetype, from
//! [`SimConfig::can_eat`]), **absorption** (component→archetype) and **emission**
//! (archetype→component).

use bevy_egui::egui;
use teemlab::SimConfig;

/// The derived food web of a [`SimConfig`] — a data structure, not a drawing. Indices
/// are into the config's `components` / `archetypes`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrophicGraph {
    /// Component display names, by index.
    pub components: Vec<String>,
    /// Archetype nodes: name, colour, and the trophic **level** (0 = producer, then
    /// `1 + max level of prey`) used to lay the graph out left→right.
    pub archetypes: Vec<ArchetypeNode>,
    /// **Edibility** edges `(actor, prey)` — the actor archetype can eat the prey
    /// archetype ([`SimConfig::can_eat`]).
    pub edibility: Vec<(usize, usize)>,
    /// **Absorption** edges `(component, archetype)` — the archetype pulls the
    /// component out of its field (a `field_relation` with `absorb > 0`).
    pub absorption: Vec<(usize, usize)>,
    /// **Emission** edges `(archetype, component)` — the archetype puts the component
    /// into its field, alive or at death (`emit > 0` or `emit_at_death > 0`).
    pub emission: Vec<(usize, usize)>,
    /// Components a **source** feeds into the world (available at the root).
    pub source_components: Vec<usize>,
    /// **Broken chains**: `(archetype, component)` — the archetype needs the component
    /// but no route reaches it ([`SimConfig::broken_chains`], the §6.1 flag).
    pub broken: Vec<(usize, usize)>,
}

/// One archetype node of the [`TrophicGraph`].
#[derive(Clone, Debug, PartialEq)]
pub struct ArchetypeNode {
    /// Display name.
    pub name: String,
    /// Body colour (linear sRGB `[r, g, b]`), for the node fill.
    pub color: [f32; 3],
    /// Trophic level: `0` for a producer (eats no other archetype), else
    /// `1 + max(level of the archetypes it eats)`. Drives the left→right layout.
    pub level: usize,
}

impl TrophicGraph {
    /// Derive the food web from a scenario. Pure — reads only the config's data.
    pub fn derive(config: &SimConfig) -> Self {
        let n = config.archetypes.len();
        let components: Vec<String> = config.components.iter().map(|c| c.name.clone()).collect();

        // Edibility: every ordered pair the engine would let eat (dominance ∩ digestibility).
        let mut edibility = Vec::new();
        for a in 0..n {
            for b in 0..n {
                if a != b && config.can_eat(a as u16, b as u16) {
                    edibility.push((a, b));
                }
            }
        }

        // Absorption / emission from the field relations (the nutritional profile).
        let mut absorption = Vec::new();
        let mut emission = Vec::new();
        for fr in &config.field_relations {
            let a = fr.species as usize;
            if a >= n {
                continue;
            }
            if fr.absorb > 0.0 {
                absorption.push((fr.component, a));
            }
            if fr.emit > 0.0 || fr.emit_at_death > 0.0 {
                emission.push((a, fr.component));
            }
        }

        // Components a source feeds into the world.
        let mut source_components: Vec<usize> =
            config.sources.iter().map(|s| s.component).collect();
        source_components.sort_unstable();
        source_components.dedup();

        let broken: Vec<(usize, usize)> = config
            .broken_chains()
            .into_iter()
            .map(|(s, c)| (s as usize, c))
            .collect();

        // Trophic levels: relax `level[a] = 1 + max(level[prey])` to a fixpoint (capped
        // at `n` iterations so a cycle — mutual/cannibal edibility — terminates).
        let mut level = vec![0usize; n];
        for _ in 0..n {
            let mut changed = false;
            for &(actor, prey) in &edibility {
                let want = level[prey] + 1;
                if want > level[actor] {
                    level[actor] = want;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        let archetypes = config
            .archetypes
            .iter()
            .enumerate()
            .map(|(i, a)| ArchetypeNode {
                name: a.name.clone(),
                color: a.color,
                level: level[i],
            })
            .collect();

        Self {
            components,
            archetypes,
            edibility,
            absorption,
            emission,
            source_components,
            broken,
        }
    }

    /// Whether the web is viable — no archetype needs an unreachable component.
    pub fn is_viable(&self) -> bool {
        self.broken.is_empty()
    }

    /// Number of edibility edges (a crude "connectance" proxy for the results header).
    pub fn edge_count(&self) -> usize {
        self.edibility.len()
    }

    /// Draw the diagram into `ui` as a fixed-height canvas — the **static** Studio
    /// surface. Components sit in the leftmost column, archetypes in columns by trophic
    /// level; the **flags** the caller draws below carry the authoritative text.
    pub fn paint(&self, ui: &mut egui::Ui, height: f32) {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), height),
            egui::Sense::hover(),
        );
        let painter = ui.painter_at(rect);
        self.paint_into(&painter, rect, None);
    }

    /// Node layout: component column (0) + one column per trophic level (1..). Shared by
    /// the static (Studio) and dynamic (Observe) surfaces.
    fn layout(&self, rect: egui::Rect) -> (Vec<egui::Pos2>, Vec<egui::Pos2>) {
        let max_level = self.archetypes.iter().map(|a| a.level).max().unwrap_or(0);
        let n_cols = max_level + 2;
        let pad = 26.0;
        let col_w = ((rect.width() - 2.0 * pad) / (n_cols.max(1) as f32 - 1.0).max(1.0)).max(1.0);
        let col_x = |col: usize| rect.left() + pad + col as f32 * col_w;
        let slot_y = |i: usize, count: usize| {
            let count = count.max(1);
            let h = rect.height() - 2.0 * pad;
            rect.top() + pad + h * (i as f32 + 0.5) / count as f32
        };
        let comp_pos = (0..self.components.len())
            .map(|i| egui::pos2(col_x(0), slot_y(i, self.components.len())))
            .collect();
        let mut per_level = vec![0usize; max_level + 1];
        for a in &self.archetypes {
            per_level[a.level] += 1;
        }
        let mut idx_in_level = vec![0usize; max_level + 1];
        let arch_pos = self
            .archetypes
            .iter()
            .map(|a| {
                let i = idx_in_level[a.level];
                idx_in_level[a.level] += 1;
                egui::pos2(col_x(a.level + 1), slot_y(i, per_level[a.level]))
            })
            .collect();
        (comp_pos, arch_pos)
    }

    /// Draw the graph over an arbitrary painter / rect, **static** (`live = None`, the
    /// Studio validator) or **dynamic** (`live = Some`, the Observe overlay): node size
    /// ∝ population, edibility-edge colour ∝ **dependency** and thickness ∝ flow — the
    /// §6.2/6.3 fragility surface. A broken need keeps its red ring.
    pub fn paint_into(&self, painter: &egui::Painter, rect: egui::Rect, live: Option<Live>) {
        if self.archetypes.is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "no species",
                egui::FontId::proportional(12.0),
                crate::theme::INK_FAINT,
            );
            return;
        }
        let (comp_pos, arch_pos) = self.layout(rect);

        // Absorption / emission (static grey either way).
        for &(c, a) in &self.absorption {
            if let (Some(&p), Some(&q)) = (comp_pos.get(c), arch_pos.get(a)) {
                painter.line_segment([p, q], egui::Stroke::new(1.0, crate::theme::GRID));
            }
        }
        for &(a, c) in &self.emission {
            if let (Some(&p), Some(&q)) = (arch_pos.get(a), comp_pos.get(c)) {
                painter.line_segment([p, q], egui::Stroke::new(1.0, crate::theme::GRID));
            }
        }
        // Edibility (prey → predator) — dependency-coloured / flow-thick when live.
        for &(actor, prey) in &self.edibility {
            let (Some(&p), Some(&q)) = (arch_pos.get(prey), arch_pos.get(actor)) else {
                continue;
            };
            let (color, width) = match &live {
                Some(l) => {
                    let dep = l.dependency(self, actor, prey);
                    (
                        lerp_color(crate::theme::GRID, crate::theme::ERROR, dep),
                        1.0 + 3.0 * dep,
                    )
                }
                None => (crate::theme::INK_FAINT, 1.5),
            };
            painter.line_segment([p, q], egui::Stroke::new(width, color));
        }
        let broken_arch: std::collections::HashSet<usize> =
            self.broken.iter().map(|&(a, _)| a).collect();

        // Component nodes (small diamonds).
        for (i, &p) in comp_pos.iter().enumerate() {
            let col = if self.source_components.contains(&i) {
                crate::theme::ACCENT
            } else {
                crate::theme::INK_MUTED
            };
            let r = 5.0;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(p.x, p.y - r),
                    egui::pos2(p.x + r, p.y),
                    egui::pos2(p.x, p.y + r),
                    egui::pos2(p.x - r, p.y),
                ],
                col,
                egui::Stroke::NONE,
            ));
            painter.text(
                egui::pos2(p.x, p.y - r - 2.0),
                egui::Align2::CENTER_BOTTOM,
                &self.components[i],
                egui::FontId::monospace(9.0),
                crate::theme::INK_MUTED,
            );
        }
        // Archetype nodes — radius ∝ population (sqrt, so area tracks it) when live.
        let max_pop = live
            .as_ref()
            .and_then(|l| l.population.iter().copied().max())
            .unwrap_or(0);
        for (i, a) in self.archetypes.iter().enumerate() {
            let p = arch_pos[i];
            let fill = color32(a.color);
            let r = match &live {
                Some(l) if max_pop > 0 => {
                    let pop = l.population.get(i).copied().unwrap_or(0);
                    5.0 + 9.0 * (pop as f32 / max_pop as f32).sqrt()
                }
                _ => 8.0,
            };
            painter.circle_filled(p, r, fill);
            if broken_arch.contains(&i) {
                painter.circle_stroke(p, r + 2.0, egui::Stroke::new(2.0, crate::theme::ERROR));
            }
            let label = match &live {
                Some(l) => format!("{} · {}", a.name, l.population.get(i).copied().unwrap_or(0)),
                None => a.name.clone(),
            };
            painter.text(
                egui::pos2(p.x, p.y + r + 2.0),
                egui::Align2::CENTER_TOP,
                label,
                egui::FontId::proportional(10.0),
                crate::theme::INK,
            );
        }
    }
}

/// Live annotations for the **dynamic** overlay (Observe): per-archetype population and
/// the config (for the digestibility that weights edge dependency, §6.3).
pub struct Live<'a> {
    /// Live population per archetype (index-aligned with the graph's archetypes).
    pub population: &'a [usize],
    /// The scenario, read for `digestibility`.
    pub config: &'a SimConfig,
}

impl Live<'_> {
    /// `dependency(P→Q) = digestibility(P,Q)·pop(Q) / Σ_Q' digestibility(P,Q')·pop(Q')` —
    /// the **share** of predator P's nourishment coming from prey Q (§6.3): `1` = a pure
    /// specialist on Q (fragile), `→0` = one of many sources (robust). `0` if P has no
    /// live intake.
    fn dependency(&self, g: &TrophicGraph, actor: usize, prey: usize) -> f32 {
        let intake = |q: usize| {
            self.config.digestibility(actor as u16, q as u16).max(0.0)
                * self.population.get(q).copied().unwrap_or(0) as f32
        };
        let denom: f32 = g
            .edibility
            .iter()
            .filter(|&&(a, _)| a == actor)
            .map(|&(_, q)| intake(q))
            .sum();
        if denom > 0.0 {
            intake(prey) / denom
        } else {
            0.0
        }
    }
}

/// Linear RGB interpolation between two colours (`t` clamped to `[0, 1]`).
fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    egui::Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// A linear-sRGB `[r, g, b]` archetype colour as an egui `Color32`.
fn color32(c: [f32; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(
        (c[0] * 255.0) as u8,
        (c[1] * 255.0) as u8,
        (c[2] * 255.0) as u8,
    )
}

/// **Structural web fragility** (`docs/emergent-trophics.md` §6.4) — how concentrated
/// each predator's diet is, from the digestibility structure alone (no live
/// populations; the *dynamic*, population-weighted version rides with the Observe
/// overlay, B7). Node fragility = the Herfindahl [`concentration`] of a predator's
/// digestibility over the prey it can eat: `1` = a pure specialist (all eggs in one
/// basket → fragile), `→0` = a generalist (robust).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fragility {
    /// Per predator: `(archetype index, node fragility ∈ (0, 1])`. Producers /
    /// non-predators are omitted — they have no diet to concentrate.
    pub per_predator: Vec<(usize, f32)>,
    /// The worst (most specialised) predator's fragility — the pre-collapse signature.
    pub worst: f32,
    /// Mean fragility over predators (0 if none) — a scalar web-robustness proxy.
    pub mean: f32,
}

/// Herfindahl **concentration** of a diet: `Σ (dᵢ / Σd)²`. `1` = all weight on one prey,
/// `1/k` = `k` equally-digestible prey. Pure — the fragility kernel, unit-tested.
fn concentration(digs: &[f32]) -> f32 {
    let total: f32 = digs.iter().sum();
    if total <= 0.0 {
        return 0.0;
    }
    digs.iter().map(|d| (d / total).powi(2)).sum()
}

/// Compute the structural [`Fragility`] of a scenario's food web from its digestibility
/// structure (`SimConfig::digestibility` over what each predator `can_eat`).
pub fn web_fragility(config: &SimConfig) -> Fragility {
    let n = config.archetypes.len();
    let mut per_predator = Vec::new();
    for p in 0..n {
        let digs: Vec<f32> = (0..n)
            .filter(|&q| q != p && config.can_eat(p as u16, q as u16))
            .map(|q| config.digestibility(p as u16, q as u16).max(0.0))
            .collect();
        let frag = concentration(&digs);
        if frag > 0.0 {
            per_predator.push((p, frag));
        }
    }
    let worst = per_predator.iter().map(|&(_, f)| f).fold(0.0, f32::max);
    let mean = if per_predator.is_empty() {
        0.0
    } else {
        per_predator.iter().map(|&(_, f)| f).sum::<f32>() / per_predator.len() as f32
    };
    Fragility {
        per_predator,
        worst,
        mean,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_is_a_viable_empty_web() {
        // A genuinely empty cast/world (note: `SimConfig::empty()` keeps one placeholder
        // agent — the editor canvas — so we build the truly-empty case explicitly).
        let cfg = SimConfig {
            archetypes: vec![],
            components: vec![],
            field_relations: vec![],
            sources: vec![],
            ..SimConfig::default()
        };
        let g = TrophicGraph::derive(&cfg);
        assert!(g.archetypes.is_empty());
        assert!(g.components.is_empty());
        assert!(g.is_viable(), "an empty web has nothing to break");
    }

    #[test]
    fn broken_mirrors_the_config_flag() {
        // The graph's broken list is exactly the config's reachability flag, remapped.
        let cfg = SimConfig::default();
        let g = TrophicGraph::derive(&cfg);
        let want: Vec<(usize, usize)> = cfg
            .broken_chains()
            .into_iter()
            .map(|(s, c)| (s as usize, c))
            .collect();
        assert_eq!(g.broken, want);
        assert_eq!(g.is_viable(), cfg.broken_chains().is_empty());
    }

    #[test]
    fn levels_stay_bounded_under_a_cannibal_cycle() {
        // A self-referential / cyclic edibility must not spin the level relaxation
        // forever; `derive` caps it at `n` iterations. Deriving the default config
        // (whatever its web) must simply terminate and bound levels by the count.
        let cfg = SimConfig::default();
        let g = TrophicGraph::derive(&cfg);
        let n = g.archetypes.len();
        assert!(g.archetypes.iter().all(|a| a.level <= n));
    }

    #[test]
    fn concentration_is_1_for_a_specialist_and_1_over_k_for_k_equal_prey() {
        // The Herfindahl kernel: one prey → 1 (fragile); k equal prey → 1/k (robust).
        assert_eq!(concentration(&[]), 0.0);
        assert_eq!(concentration(&[1.0]), 1.0);
        assert!((concentration(&[0.5, 0.5]) - 0.5).abs() < 1e-6);
        assert!(
            (concentration(&[2.0, 2.0]) - 0.5).abs() < 1e-6,
            "scale-free"
        );
        assert!((concentration(&[1.0, 1.0, 1.0]) - 1.0 / 3.0).abs() < 1e-6);
        // A skewed diet is more concentrated than an even one but below a specialist.
        let skewed = concentration(&[0.9, 0.1]);
        assert!(skewed > 0.5 && skewed < 1.0);
    }

    #[test]
    fn web_fragility_is_bounded_and_worst_dominates_mean() {
        // On any scenario: every node fragility ∈ (0, 1], the worst is the max, and the
        // mean never exceeds it.
        let cfg = SimConfig::default();
        let f = web_fragility(&cfg);
        assert!(
            f.per_predator
                .iter()
                .all(|&(_, x)| x > 0.0 && x <= 1.0 + 1e-6)
        );
        assert!(f.mean <= f.worst + 1e-6);
        assert!(f.worst <= 1.0 + 1e-6);
    }
}
