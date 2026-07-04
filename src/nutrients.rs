//! **Component fields**: environmental concentration fields (the "substrate").
//!
//! A *component* is any diffusible substance laid over the arena — a nutrient, a
//! toxin, a pheromone, biomass — differentiated **only by its relations** (Law 11),
//! not by a type. Each is a [`Field`] (a concentration grid); the set lives in
//! [`Fields`]. The engine treats them uniformly; the scenario declares, per
//! (species, component), how a species relates to each (absorb / emit / sense /
//! affect / reproduce). See [`docs/component-emission-plan.md`].
//!
//! The historical first use (T2, `docs/nutrients-t2-plan.md`) is the **nutrient
//! axis**: plant reproduction is bounded by a *finite* nutrient (Liebig's law of the
//! minimum), not by sunlight alone (infinite → carpeting). Two axes: **energy** (the
//! [`Reserve`](crate::components::Reserve), sun-fed) governs *survival*; a **nutrient**
//! component governs *reproduction only* — so a plant with no nutrient simply does not
//! reproduce (it lives on the sun → no death spiral).
//!
//! The fields are the **environment**, not life forms: **outside SIM Law 11** (they
//! run no agent system) and **not** spatial-query structures (no §5 conflict — a
//! `pos → cell` is a direct hash, never a neighbour search).

use crate::components::{Agent, Species};
use crate::config::SimConfig;
use bevy::prelude::*;

/// A concentration field for **one** component: a square `res × res` grid of `f32`
/// concentrations laid over the arena (`[-half_extent, half_extent]²`), row-major
/// (`index = y * res + x`).
///
/// Conservation is the contract: [`add`](Self::add) deposits, [`take`](Self::take)
/// removes *exactly* what it returns, and [`diffuse`](Self::diffuse) preserves the
/// total mass (a graph-Laplacian relaxation with reflecting boundaries). The one
/// non-conservative operation is [`decay_step`](Self::decay_step) — a *deliberate*
/// dissipation (a pheromone fading, detritus decomposing), inert when `decay == 0`.
#[derive(Clone, Debug)]
pub struct Field {
    /// `res * res` concentrations, row-major (`y * res + x`).
    cells: Vec<f32>,
    /// Cells per side.
    res: usize,
    /// Arena half-extent, the same world span the agents live in (for `pos → cell`).
    half_extent: f32,
    /// Rebalance fraction per [`diffuse`](Self::diffuse) step, in `[0, 1]` — the
    /// *local vs global* limitation knob. `0` → the field never spreads (inert).
    diffusion: f32,
    /// Per-tick fractional decay (`c *= 1 - decay`), in `[0, 1]`: a component that
    /// **dissipates** (a pheromone fading, detritus decomposing). `0` → the field
    /// never decays (a conserved nutrient). Applied by [`decay_step`](Self::decay_step).
    decay: f32,
    /// Double-buffer for [`diffuse`](Self::diffuse) (a relaxation reads the whole
    /// field then writes the new one; an in-place update would bias the stencil).
    scratch: Vec<f32>,
}

impl Field {
    /// A fresh, empty field of `res × res` cells over `[-half_extent, half_extent]²`.
    /// `res` is forced to at least 1 (a degenerate single cell rather than a panic
    /// on an empty `Vec`).
    pub fn new(res: usize, half_extent: f32, diffusion: f32, decay: f32) -> Self {
        let res = res.max(1);
        Self {
            cells: vec![0.0; res * res],
            res,
            half_extent,
            diffusion,
            decay,
            scratch: vec![0.0; res * res],
        }
    }

    /// Side of one cell in world units (`2 * half_extent / res`).
    pub fn cell_size(&self) -> f32 {
        2.0 * self.half_extent / self.res as f32
    }

    /// Map one world coordinate to its grid index along an axis, **clamped** to
    /// `[0, res)`. The reproduction clamp already keeps agents in-arena, but a
    /// source or a drifting body could sit on the very edge — we clamp anyway so a
    /// `pos → cell` never indexes out of bounds.
    fn axis_index(&self, coord: f32) -> usize {
        let cell = ((coord + self.half_extent) / self.cell_size()).floor();
        cell.clamp(0.0, self.res as f32 - 1.0) as usize
    }

    /// The cell index for a world position (clamped, cf. [`axis_index`](Self::axis_index)).
    pub fn cell_index(&self, pos: Vec2) -> usize {
        let ix = self.axis_index(pos.x);
        let iy = self.axis_index(pos.y);
        iy * self.res + ix
    }

    /// The concentration in the cell containing `pos`.
    pub fn sample(&self, pos: Vec2) -> f32 {
        self.cells[self.cell_index(pos)]
    }

    /// Deposit `amount` into the cell containing `pos` (source or agent emission,
    /// recycling). The single point that *creates* concentration.
    pub fn add(&mut self, pos: Vec2, amount: f32) {
        let i = self.cell_index(pos);
        self.cells[i] += amount;
    }

    /// Remove up to `amount` from the cell containing `pos`, returning the amount
    /// **actually** taken (`min(amount, cell)`). Conservation: an absorber gains
    /// exactly what the cell loses.
    pub fn take(&mut self, pos: Vec2, amount: f32) -> f32 {
        let i = self.cell_index(pos);
        let taken = amount.min(self.cells[i]).max(0.0);
        self.cells[i] -= taken;
        taken
    }

    /// Total concentration mass in the field (the conserved quantity under
    /// add/take/diffuse — used by the tests and diagnostics).
    pub fn total(&self) -> f32 {
        self.cells.iter().sum()
    }

    /// Cells per side (for the heatmap layer / diagnostics).
    pub fn resolution(&self) -> usize {
        self.res
    }

    /// Read-only view of the row-major concentrations (`index = y * resolution() +
    /// x`) — for the heatmap rendering layer and diagnostics.
    pub fn cells(&self) -> &[f32] {
        &self.cells
    }

    /// One relaxation step toward the neighbour average, using a 4-neighbour
    /// graph-Laplacian stencil with **reflecting (Neumann) boundaries**:
    ///
    /// `new[i] = cells[i] + diffusion * (Σ_{j∼i} cells[j] − deg(i)·cells[i]) / 4`
    ///
    /// where `deg(i)` is the count of in-grid neighbours (2 at a corner, 3 on an
    /// edge, 4 inside). This **conserves total mass exactly** (each undirected edge
    /// contributes `+(cⱼ−cᵢ)` and `−(cⱼ−cᵢ)`, cancelling) and stays stable for
    /// `diffusion ≤ 1` (the centre keeps weight `1 − diffusion·deg/4 ≥ 0`). Writes
    /// into [`scratch`](Self::scratch), then swaps. Inert (early return) when
    /// `diffusion == 0`.
    pub fn diffuse(&mut self) {
        if self.diffusion == 0.0 {
            return;
        }
        let res = self.res;
        // **Interior** cells (all four neighbours in-grid, `deg = 4`): a *branchless*
        // fast path — the overwhelming bulk of the grid (`(res-2)²` of `res²`). The
        // expression is identical to [`stencil_general`](Self::stencil_general) for an
        // interior cell (`0.0 + a == a`, `deg == 4.0`), so the result is **byte-for-byte**
        // the same as the old per-cell branchy loop (a `diffuse_matches_general_stencil`
        // test pins this).
        for y in 1..res.saturating_sub(1) {
            let base = y * res;
            for x in 1..res - 1 {
                let i = base + x;
                let c = self.cells[i];
                let sum = self.cells[i - 1]
                    + self.cells[i + 1]
                    + self.cells[i - res]
                    + self.cells[i + res];
                self.scratch[i] = c + self.diffusion * (sum - 4.0 * c) / 4.0;
            }
        }
        // **Border ring** (fewer neighbours): the general stencil with in-grid degree
        // checks. Top/bottom rows, then the left/right columns between them.
        for x in 0..res {
            let v = self.stencil_general(x, 0);
            self.scratch[x] = v;
            if res > 1 {
                let v = self.stencil_general(x, res - 1);
                self.scratch[(res - 1) * res + x] = v;
            }
        }
        for y in 1..res.saturating_sub(1) {
            let v = self.stencil_general(0, y);
            self.scratch[y * res] = v;
            let v = self.stencil_general(res - 1, y);
            self.scratch[y * res + res - 1] = v;
        }
        std::mem::swap(&mut self.cells, &mut self.scratch);
    }

    /// One **decay** step: every cell loses the fraction `decay` (`c *= 1 - decay`).
    /// Unlike diffusion this is **not** mass-conserving — it is the deliberate
    /// dissipation a fading pheromone / decomposing detritus needs. Inert (early
    /// return) when `decay == 0` (a conserved nutrient → byte-identical).
    pub fn decay_step(&mut self) {
        if self.decay == 0.0 {
            return;
        }
        let factor = 1.0 - self.decay;
        for c in &mut self.cells {
            *c *= factor;
        }
    }

    /// The general 4-neighbour diffusion stencil for cell `(x, y)`, counting only
    /// in-grid neighbours (`deg` = 2 at a corner, 3 on an edge, 4 inside). This is the
    /// exact per-cell computation the old [`diffuse`](Self::diffuse) ran for **every**
    /// cell; [`diffuse`](Self::diffuse) now uses it only on the border ring and a
    /// branchless equivalent on the interior.
    fn stencil_general(&self, x: usize, y: usize) -> f32 {
        let res = self.res;
        let i = y * res + x;
        let c = self.cells[i];
        let mut sum = 0.0;
        let mut deg = 0.0;
        if x > 0 {
            sum += self.cells[i - 1];
            deg += 1.0;
        }
        if x + 1 < res {
            sum += self.cells[i + 1];
            deg += 1.0;
        }
        if y > 0 {
            sum += self.cells[i - res];
            deg += 1.0;
        }
        if y + 1 < res {
            sum += self.cells[i + res];
            deg += 1.0;
        }
        c + self.diffusion * (sum - deg * c) / 4.0
    }
}

/// The scenario's **component fields**, one [`Field`] per declared component (indexed
/// like [`crate::config::SimConfig::components`]). Built at [`SimPlugin`](crate::SimPlugin)
/// build + at hot reset. Empty (no component declared) → every field system is a no-op
/// → byte-identical for a scenario without a substrate.
#[derive(Resource, Default)]
pub struct Fields(pub Vec<Field>);

impl std::ops::Deref for Fields {
    type Target = Vec<Field>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Fields {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Fields {
    /// Build the fields from the scenario's component configs (one grid each, the
    /// shared `resolution`, per-component `diffusion`/`decay`). The single source for
    /// the plugin build and the hot reset.
    pub fn from_config(config: &SimConfig) -> Self {
        Self(
            config
                .components
                .iter()
                .map(|c| {
                    Field::new(
                        config.field_resolution,
                        config.arena_half_extent,
                        c.diffusion,
                        c.decay,
                    )
                })
                .collect(),
        )
    }
}

/// A per-agent **nutrient store** (the second axis of T2): filled by
/// [`absorb_nutrients`] from the nutrient [`Field`], spent at reproduction
/// ([`crate::ecology::reproduce`]) to pay for a child. Attached to **every** agent
/// at spawn; with the nutrient genes at `0` it is inert (`max == 0`, nothing
/// absorbed, nothing paid) → byte-identical for existing scenarios.
///
/// Deliberately distinct from [`Reserve`](crate::components::Reserve) (energy,
/// sun-/food-fed → *survival*): a missing nutrient stops **reproduction**, it never
/// causes death — the two-axis design that fixes the T1 death spiral.
///
/// **NB (component-emission plan, Phase 2):** this single store becomes a per-component
/// `Stores` when the declarative `FieldRelation` table lands; Phase 1 keeps it, and the
/// nutrient is the field of index `0` by convention.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Nutrients {
    /// Current amount stored.
    pub current: f32,
    /// Capacity (the `nutrient_capacity` gene at spawn).
    pub max: f32,
}

impl Nutrients {
    /// Fill fraction in `[0, 1]` (`0` if `max` is zero — an entity outside the
    /// nutrient axis). Mirrors [`Reserve::fraction`](crate::components::Reserve::fraction)
    /// so the inspector can show the nutrient store as a second reservoir bar.
    pub fn fraction(&self) -> f32 {
        if self.max > 0.0 {
            (self.current / self.max).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// Emission of a substrate **source** (e.g. a submarine volcanic vent): deposits
/// `rate` per second of component `component` into the field cell under it (cf.
/// [`emit_nutrients`]). Carried by a **non-`Agent`** entity (spawned by
/// [`crate::spawn::spawn_sources`]) → the whole life machinery (every system queries
/// `With<Agent>`) ignores it *by construction*: no metabolism, death, reproduction
/// or decision.
#[derive(Component, Clone, Copy, Debug)]
pub struct Emits {
    /// Component index (into [`Fields`] / `SimConfig::components`).
    pub component: usize,
    /// Emission per second of simulated time.
    pub rate: f32,
}

/// EMIT: each substrate source deposits `rate · dt` into its component's field cell.
/// The source is **not** an `Agent`; only this system reads [`Emits`]. A scenario
/// with no source has an empty query → no-op (byte-identical); an out-of-range
/// component index is skipped.
pub fn emit_nutrients(
    time: Res<Time>,
    mut fields: ResMut<Fields>,
    sources: Query<(&Transform, &Emits)>,
) {
    let dt = time.delta_secs();
    for (transform, emits) in &sources {
        if let Some(field) = fields.get_mut(emits.component) {
            field.add(transform.translation.truncate(), emits.rate * dt);
        }
    }
}

/// DIFFUSE: one relaxation step of **every** field toward the neighbour average
/// ([`Field::diffuse`]) — this is what turns point emission into **gradients** (life
/// clusters around sources). Mass-conserving; each field inert (early return inside
/// `diffuse`) when its `diffusion == 0`.
pub fn diffuse_nutrients(mut fields: ResMut<Fields>) {
    for field in fields.iter_mut() {
        field.diffuse();
    }
}

/// DECAY: one dissipation step of **every** field ([`Field::decay_step`]) — a fading
/// pheromone / decomposing detritus. Each field inert (early return) when its
/// `decay == 0` (a conserved nutrient) → byte-identical for T2 scenarios.
pub fn decay_nutrients(mut fields: ResMut<Fields>) {
    for field in fields.iter_mut() {
        field.decay_step();
    }
}

/// ABSORB: each agent pulls nutrient from the **nutrient field** (component `0`, by
/// the Phase-1 convention) into its [`Nutrients`] store, capped by its **absorb**
/// [`FieldRelation`](crate::config::FieldRelation) rate and remaining capacity.
/// Conservation: the store gains exactly what the cell loses ([`Field::take`]). An
/// agent whose nutrient relation has `absorb == 0` is skipped; no component `0` →
/// no-op → byte-identical.
pub fn absorb_nutrients(
    time: Res<Time>,
    config: Res<SimConfig>,
    mut fields: ResMut<Fields>,
    mut agents: Query<(&Transform, &Species, &mut Nutrients), With<Agent>>,
) {
    let Some(field) = fields.get_mut(0) else {
        return;
    };
    let dt = time.delta_secs();
    for (transform, species, mut store) in &mut agents {
        // The absorption rate is the species' nutrient FieldRelation (component 0); the
        // store cap (`store.max`) was set at spawn from the same table.
        let absorb = config.nutrient_of(species.0).0;
        if absorb <= 0.0 {
            continue;
        }
        let want = (absorb * dt).min(store.max - store.current);
        if want <= 0.0 {
            continue;
        }
        let got = field.take(transform.translation.truncate(), want);
        store.current += got;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4×4 grid over `[-10, 10]²` (cell size 5), no diffusion, no decay by default.
    fn field() -> Field {
        Field::new(4, 10.0, 0.0, 0.0)
    }

    /// A position far outside the arena still maps to a *valid* edge cell, and the
    /// opposite corners map to opposite cells (the clamp, not a wrap or a panic).
    #[test]
    fn cell_index_clamps_out_of_bounds() {
        let f = field(); // res = 4 → 16 cells
        assert!(f.cell_index(Vec2::new(1e6, 1e6)) < 16);
        assert!(f.cell_index(Vec2::new(-1e6, -1e6)) < 16);
        // bottom-left corner → cell 0; top-right → cell 15 (iy=3, ix=3).
        assert_eq!(f.cell_index(Vec2::new(-1e6, -1e6)), 0);
        assert_eq!(f.cell_index(Vec2::new(1e6, 1e6)), 15);
        // the origin lands in the interior: (0 + 10) / 5 = 2 → ix=iy=2 → 10.
        assert_eq!(f.cell_index(Vec2::ZERO), 10);
    }

    /// `add` then `take` conserve: the field holds exactly what was deposited, and
    /// taking more than present empties it returning only what was there.
    #[test]
    fn add_then_take_conserves() {
        let mut f = field();
        let p = Vec2::new(3.0, -2.0);
        f.add(p, 5.0);
        assert!((f.total() - 5.0).abs() < 1e-6);
        assert!((f.sample(p) - 5.0).abs() < 1e-6);

        let got = f.take(p, 8.0); // ask for more than present
        assert!((got - 5.0).abs() < 1e-6, "take returns only what was there");
        assert!(f.total().abs() < 1e-6, "the field is emptied");
        assert!(f.sample(p).abs() < 1e-6);
    }

    /// A partial `take` removes exactly what it returns from the cell (gain == loss).
    #[test]
    fn take_removes_exactly_what_it_returns() {
        let mut f = field();
        let p = Vec2::ZERO;
        f.add(p, 10.0);
        let got = f.take(p, 4.0);
        assert!((got - 4.0).abs() < 1e-6);
        assert!((f.sample(p) - 6.0).abs() < 1e-6);
    }

    /// `diffuse` **conserves total mass** at every step and **relaxes toward
    /// uniform**: a spike spreads, its peak drops, the field flattens.
    #[test]
    fn diffuse_conserves_mass_and_relaxes() {
        let mut f = Field::new(8, 10.0, 0.5, 0.0);
        f.add(Vec2::ZERO, 100.0);
        let before = f.total();
        let center = f.cell_index(Vec2::ZERO);
        let peak0 = f.cells[center];

        for _ in 0..50 {
            f.diffuse();
            assert!(
                (f.total() - before).abs() < 1e-3,
                "mass must be conserved across diffusion"
            );
        }

        assert!(f.cells[center] < peak0, "the peak must relax downward");
        let max = f.cells.iter().cloned().fold(f32::MIN, f32::max);
        let min = f.cells.iter().cloned().fold(f32::MAX, f32::min);
        assert!(max - min < peak0, "the field must flatten toward uniform");
    }

    /// With `diffusion == 0` the field never spreads — the byte-identical guarantee
    /// for existing scenarios (the field is allocated but inert).
    #[test]
    fn diffuse_is_inert_when_diffusion_zero() {
        let mut f = field(); // diffusion 0
        f.add(Vec2::new(1.0, 1.0), 7.0);
        let snapshot = f.cells.clone();
        f.diffuse();
        assert_eq!(f.cells, snapshot, "diffusion 0 → no change");
    }

    /// `decay_step` removes the fraction `decay` from every cell (a fading pheromone),
    /// and is **inert** when `decay == 0` (a conserved nutrient, byte-identical).
    #[test]
    fn decay_scales_cells_and_is_inert_at_zero() {
        // decay 0.25: a cell of 8 → 6 after one step; total scales likewise.
        let mut f = Field::new(4, 10.0, 0.0, 0.25);
        f.add(Vec2::ZERO, 8.0);
        f.add(Vec2::new(3.0, 3.0), 4.0);
        let before = f.total();
        f.decay_step();
        assert!(
            (f.total() - before * 0.75).abs() < 1e-6,
            "each cell loses 25%"
        );

        // decay 0 → no change (the conserved-nutrient path).
        let mut g = field();
        g.add(Vec2::ZERO, 5.0);
        let snap = g.cells.clone();
        g.decay_step();
        assert_eq!(g.cells, snap, "decay 0 → inert");
    }

    /// The optimised `diffuse` (branchless interior + border ring, B4) must produce the
    /// **exact same** field, cell for cell, as applying the general stencil to every
    /// cell (the old whole-grid path) — the byte-identical guarantee of the split.
    #[test]
    fn diffuse_matches_general_stencil() {
        // A non-uniform, non-negative field so every cell has a distinct neighbourhood.
        let mut f = Field::new(7, 10.0, 0.37, 0.0);
        for (i, c) in f.cells.iter_mut().enumerate() {
            *c = (i as f32 * 1.3).sin().abs() * 10.0;
        }
        // Reference = the general stencil applied to every cell, in row-major order,
        // read from the *pre-diffuse* field.
        let reference: Vec<f32> = (0..f.res)
            .flat_map(|y| (0..f.res).map(move |x| (x, y)))
            .map(|(x, y)| f.stencil_general(x, y))
            .collect();
        f.diffuse();
        assert_eq!(
            f.cells, reference,
            "the interior fast path must match the general stencil bit-for-bit"
        );
    }
}
