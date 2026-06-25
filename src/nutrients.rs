//! The **nutrient field**: an environmental concentration field (the "substrate").
//!
//! Plant reproduction (T2) is bounded by a *finite* nutrient, not by sunlight
//! alone (which is infinite → carpeting). This is the resource-limitation bound of
//! **Liebig's law of the minimum** (ROADMAP §9 "Generic nutrients layer";
//! `docs/nutrients-t2-plan.md`). Two axes (the T2 design): **energy** (the existing
//! [`Reserve`](crate::components::Reserve), sun-fed) governs *survival*; this
//! **nutrient** axis governs *reproduction only* — so a plant with no nutrient
//! simply does not reproduce (it lives on the sun → no death spiral), the fix to
//! the T1 fragility (`scenarios/minerals.ron`).
//!
//! The field is the **environment**, not a life form: it is **outside SIM Law 11**
//! (it runs none of the agent systems) and it is **not** a spatial-query structure
//! (no §5 conflict — it never searches neighbours; position → cell is a direct
//! hash). One nutrient in T2; the shape (a grid of `f32`) generalizes to a
//! `Vec<NutrientField>` in T3.
//!
//! This module is **step 1** of the T2 plan: a pure-data resource with its own
//! unit tests, wired into no system yet (emission / diffusion / absorption come in
//! later steps). Every existing scenario stays byte-identical: with no source and
//! `diffusion = 0`, the field is allocated but never touched.

use crate::components::Agent;
use crate::genotype::Genotype;
use bevy::prelude::*;

/// A concentration field for **one** nutrient: a square `res × res` grid of `f32`
/// concentrations laid over the arena (`[-half_extent, half_extent]²`), row-major
/// (`index = y * res + x`).
///
/// Conservation is the contract: [`add`](Self::add) deposits, [`take`](Self::take)
/// removes *exactly* what it returns, and [`diffuse`](Self::diffuse) preserves the
/// total mass (a graph-Laplacian relaxation with reflecting boundaries). Nothing
/// here creates or destroys nutrient outside `add`.
#[derive(Resource, Clone, Debug)]
pub struct NutrientField {
    /// `res * res` concentrations, row-major (`y * res + x`).
    cells: Vec<f32>,
    /// Cells per side.
    res: usize,
    /// Arena half-extent, the same world span the agents live in (for `pos → cell`).
    half_extent: f32,
    /// Rebalance fraction per [`diffuse`](Self::diffuse) step, in `[0, 1]` — the
    /// *local vs global* limitation knob. `0` → the field never spreads (inert).
    diffusion: f32,
    /// Double-buffer for [`diffuse`](Self::diffuse) (a relaxation reads the whole
    /// field then writes the new one; an in-place update would bias the stencil).
    scratch: Vec<f32>,
}

impl NutrientField {
    /// A fresh, empty field of `res × res` cells over `[-half_extent, half_extent]²`.
    /// `res` is forced to at least 1 (a degenerate single cell rather than a panic
    /// on an empty `Vec`).
    pub fn new(res: usize, half_extent: f32, diffusion: f32) -> Self {
        let res = res.max(1);
        Self {
            cells: vec![0.0; res * res],
            res,
            half_extent,
            diffusion,
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

    /// Deposit `amount` into the cell containing `pos` (source emission, later
    /// recycling). The single point that *creates* nutrient.
    pub fn add(&mut self, pos: Vec2, amount: f32) {
        let i = self.cell_index(pos);
        self.cells[i] += amount;
    }

    /// Remove up to `amount` from the cell containing `pos`, returning the amount
    /// **actually** taken (`min(amount, cell)`). Conservation: a plant gains exactly
    /// what the cell loses.
    pub fn take(&mut self, pos: Vec2, amount: f32) -> f32 {
        let i = self.cell_index(pos);
        let taken = amount.min(self.cells[i]).max(0.0);
        self.cells[i] -= taken;
        taken
    }

    /// Total nutrient mass in the field (the conserved quantity — used by the tests
    /// and, later, by diagnostics).
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
        for y in 0..res {
            for x in 0..res {
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
                self.scratch[i] = c + self.diffusion * (sum - deg * c) / 4.0;
            }
        }
        std::mem::swap(&mut self.cells, &mut self.scratch);
    }
}

/// A per-agent **nutrient store** (the second axis of T2): filled by
/// [`absorb_nutrients`] from the [`NutrientField`], spent at reproduction
/// ([`crate::ecology::reproduce`]) to pay for a child. Attached to **every** agent
/// at spawn; with the nutrient genes at `0` it is inert (`max == 0`, nothing
/// absorbed, nothing paid) → byte-identical for existing scenarios.
///
/// Deliberately distinct from [`Reserve`](crate::components::Reserve) (energy,
/// sun-/food-fed → *survival*): a missing nutrient stops **reproduction**, it never
/// causes death — the two-axis design that fixes the T1 death spiral.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Nutrients {
    /// Current amount stored.
    pub current: f32,
    /// Capacity (the `nutrient_capacity` gene at spawn).
    pub max: f32,
}

/// Emission of a substrate **source** (e.g. a submarine volcanic vent): deposits
/// `rate` per second of nutrient `nutrient` into the field cell under it (cf.
/// [`emit_nutrients`]). Carried by a **non-`Agent`** entity (spawned by
/// [`crate::spawn::spawn_sources`]) → the whole life machinery (every system queries
/// `With<Agent>`) ignores it *by construction*: no metabolism, death, reproduction
/// or decision. T2 uses a single field, so `nutrient` is always `0` (reserved for
/// the multi-nutrient T3).
#[derive(Component, Clone, Copy, Debug)]
pub struct Emits {
    /// Nutrient index (T2: always `0`).
    pub nutrient: usize,
    /// Emission per second of simulated time.
    pub rate: f32,
}

/// EMIT: each substrate source deposits `rate · dt` into the field cell under it.
/// The source is **not** an `Agent`; only this system reads [`Emits`]. A scenario
/// with no source has an empty query → no-op (byte-identical).
pub fn emit_nutrients(
    time: Res<Time>,
    mut field: ResMut<NutrientField>,
    sources: Query<(&Transform, &Emits)>,
) {
    let dt = time.delta_secs();
    for (transform, emits) in &sources {
        field.add(transform.translation.truncate(), emits.rate * dt);
    }
}

/// DIFFUSE: one relaxation step of the field toward the neighbour average
/// ([`NutrientField::diffuse`]) — this is what turns point emission into
/// **gradients** (life clusters around sources). Mass-conserving; inert (early
/// return inside `diffuse`) when `diffusion == 0`.
pub fn diffuse_nutrients(mut field: ResMut<NutrientField>) {
    field.diffuse();
}

/// ABSORB: each agent pulls nutrient from the field cell under it into its
/// [`Nutrients`] store, capped by its absorption rate and its remaining capacity.
/// Conservation: the store gains exactly what the cell loses
/// ([`NutrientField::take`]). An agent with `nutrient_absorption == 0` (every
/// existing scenario) is skipped → byte-identical.
pub fn absorb_nutrients(
    time: Res<Time>,
    mut field: ResMut<NutrientField>,
    mut agents: Query<(&Transform, &Genotype, &mut Nutrients), With<Agent>>,
) {
    let dt = time.delta_secs();
    for (transform, genotype, mut store) in &mut agents {
        if genotype.nutrient_absorption <= 0.0 {
            continue;
        }
        let want = (genotype.nutrient_absorption * dt).min(store.max - store.current);
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

    /// A 4×4 grid over `[-10, 10]²` (cell size 5), no diffusion by default.
    fn field() -> NutrientField {
        NutrientField::new(4, 10.0, 0.0)
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
        let mut f = NutrientField::new(8, 10.0, 0.5);
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
}
