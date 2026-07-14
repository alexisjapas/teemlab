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

use crate::components::{Agent, Reserve, Species};
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

/// A per-agent **component store**: how much of each scenario component
/// ([`SimConfig::components`](crate::config::SimConfig::components)) the agent holds.
/// Filled by [`absorb_nutrients`] from each component's [`Field`], carried up the food
/// chain by predation ([`crate::interaction`]), spent at reproduction
/// ([`crate::ecology::reproduce`]) and returned to the field at death
/// ([`crate::ecology::reap`]). Attached to **every** agent at spawn, sized to the
/// scenario's component count; with no capacity it is inert → byte-identical for a
/// scenario off the resource axis.
///
/// **Component `0` is the nutrient** (the T2 axis): [`fraction`](Self::fraction) — the
/// proprioceptive channel — and the reproduction gate read it, so existing single-axis
/// scenarios are unchanged. Higher indices are other components (any of which a scenario
/// may give a store). This generalizes the former single `{current, max}` nutrient store
/// (the "becomes a per-component `Stores`" note of the component-emission plan, now
/// realized — the MVP prerequisite for emergent digestibility,
/// `docs/emergent-trophics.md`).
///
/// Deliberately distinct from [`Reserve`](crate::components::Reserve) (energy,
/// sun-/food-fed → *survival*): a missing nutrient stops **reproduction**, it never
/// causes death — the two-axis design that fixes the T1 death spiral.
#[derive(Component, Clone, Debug, Default)]
pub struct Nutrients {
    /// Amount of each component currently held, indexed like
    /// [`SimConfig::components`](crate::config::SimConfig::components).
    current: Vec<f32>,
    /// Capacity for each component (`0` = holds none), same indexing — the `capacity`
    /// verb of each [`FieldRelation`](crate::config::FieldRelation), read at spawn.
    capacity: Vec<f32>,
}

impl Nutrients {
    /// A store with the given per-component capacities, holding nothing.
    pub fn new(capacity: Vec<f32>) -> Self {
        Self {
            current: vec![0.0; capacity.len()],
            capacity,
        }
    }

    /// Amount of component `c` held (`0` off the axis / out of range).
    pub fn current(&self, c: usize) -> f32 {
        self.current.get(c).copied().unwrap_or(0.0)
    }

    /// Capacity for component `c` (`0` if none / out of range).
    pub fn capacity(&self, c: usize) -> f32 {
        self.capacity.get(c).copied().unwrap_or(0.0)
    }

    /// Number of component slots (the scenario's component count at spawn).
    pub fn len(&self) -> usize {
        self.current.len()
    }

    /// No component slots — an agent in a scenario without any component.
    pub fn is_empty(&self) -> bool {
        self.current.is_empty()
    }

    /// Apply a signed `delta` to component `c`, clamped to `[0, capacity]` (surplus is
    /// lost, mirroring energy beyond `Reserve::max`). The single mutation point for
    /// absorption and trophic transfer.
    pub fn apply_delta(&mut self, c: usize, delta: f32) {
        if let Some(cur) = self.current.get_mut(c) {
            let cap = self.capacity.get(c).copied().unwrap_or(0.0);
            *cur = (*cur + delta).clamp(0.0, cap);
        }
    }

    /// Remove up to `amount` of component `c`; returns what was actually taken
    /// (`≤ amount`, `≤ held`). Used to spend the reproduction cost.
    pub fn take(&mut self, c: usize, amount: f32) -> f32 {
        if let Some(cur) = self.current.get_mut(c) {
            let taken = amount.min(cur.max(0.0));
            *cur -= taken;
            taken
        } else {
            0.0
        }
    }

    /// Set component `c` to `v`, clamped to `[0, capacity]`. A seeding helper (tests,
    /// tooling); the running sim mutates via [`apply_delta`](Self::apply_delta) /
    /// [`take`](Self::take).
    pub fn set(&mut self, c: usize, v: f32) {
        if let Some(cur) = self.current.get_mut(c) {
            let cap = self.capacity.get(c).copied().unwrap_or(0.0);
            *cur = v.clamp(0.0, cap);
        }
    }

    /// Fill fraction of the **nutrient** (component `0`) in `[0, 1]` — the second
    /// reservoir shown in the inspector and the proprioceptive `self_state` channel
    /// (`0` off the axis). Mirrors
    /// [`Reserve::fraction`](crate::components::Reserve::fraction).
    pub fn fraction(&self) -> f32 {
        let (cur, cap) = (self.current(0), self.capacity(0));
        if cap > 0.0 {
            (cur / cap).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// True if this agent has capacity for **any** component (on the resource axis).
    pub fn on_axis(&self) -> bool {
        self.capacity.iter().any(|&c| c > 0.0)
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

/// EMIT (agents): each agent whose species has an `emit` [`FieldRelation`] deposits
/// `emit · dt` of that component into the field cell under it — the **symmetric of
/// absorption** (`docs/component-emission-plan.md` §3): the agent→environment write
/// (organic waste / pheromone / toxin). Runs alongside the source emission
/// ([`emit_nutrients`]), before diffusion/decay. A scenario with no `emit` relation is
/// a no-op (early return) → byte-identical.
pub fn emit_components(
    time: Res<Time>,
    config: Res<SimConfig>,
    mut fields: ResMut<Fields>,
    agents: Query<(&Transform, &Species), With<Agent>>,
) {
    if !config.field_relations.iter().any(|f| f.emit > 0.0) {
        return;
    }
    let dt = time.delta_secs();
    for (transform, species) in &agents {
        let pos = transform.translation.truncate();
        for fr in config
            .field_relations
            .iter()
            .filter(|f| f.species == species.0 && f.emit > 0.0)
        {
            if let Some(field) = fields.get_mut(fr.component) {
                field.add(pos, fr.emit * dt);
            }
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

/// ABSORB: each agent pulls **each component it absorbs**
/// ([`FieldRelation`](crate::config::FieldRelation) `absorb > 0`) from that component's
/// [`Field`] into its [`Nutrients`] store, capped by the `absorb` rate and the remaining
/// capacity. Conservation: the store gains exactly what the cell loses ([`Field::take`]).
/// A scenario with no absorbing relation is a no-op (early return); before the
/// per-component store only component `0` (the nutrient) was absorbed, so existing
/// single-axis scenarios are unchanged.
pub fn absorb_nutrients(
    time: Res<Time>,
    config: Res<SimConfig>,
    mut fields: ResMut<Fields>,
    mut agents: Query<(&Transform, &Species, &mut Nutrients), With<Agent>>,
) {
    if !config.field_relations.iter().any(|f| f.absorb > 0.0) {
        return;
    }
    let dt = time.delta_secs();
    for (transform, species, mut store) in &mut agents {
        let pos = transform.translation.truncate();
        for fr in config
            .field_relations
            .iter()
            .filter(|f| f.species == species.0 && f.absorb > 0.0)
        {
            let Some(field) = fields.get_mut(fr.component) else {
                continue;
            };
            let want =
                (fr.absorb * dt).min(store.capacity(fr.component) - store.current(fr.component));
            if want <= 0.0 {
                continue;
            }
            let got = field.take(pos, want);
            store.apply_delta(fr.component, got);
        }
    }
}

/// AFFECT: a component whose [`FieldRelation`](crate::config::FieldRelation) has
/// `affect != 0` changes the agent's [`Reserve`] by `affect · concentration · dt` at its
/// cell — a **toxin** (`affect < 0`, drains energy) or a boon (`affect > 0`). This is the
/// field→agent effect that makes an emitted component **harmful**: a toxin and a pheromone
/// differ *only* by their relation (Law 11 — no per-kind code). Paired with `emit` on the
/// same (species, component), it is **self-poisoning** — an endogenous collapse mode
/// (`docs/persistent-ecosystems.md` §1/§3). A scenario with no `affect` relation is a
/// no-op (early return) → byte-identical. Death at zero is left to [`crate::ecology::reap`]
/// (next tick, as for the metabolic drain).
pub fn affect_agents(
    time: Res<Time>,
    config: Res<SimConfig>,
    fields: Res<Fields>,
    mut agents: Query<(&Transform, &Species, &mut Reserve), With<Agent>>,
) {
    if !config.field_relations.iter().any(|f| f.affect != 0.0) {
        return;
    }
    let dt = time.delta_secs();
    for (transform, species, mut reserve) in &mut agents {
        let pos = transform.translation.truncate();
        let mut delta = 0.0;
        for fr in config
            .field_relations
            .iter()
            .filter(|f| f.species == species.0 && f.affect != 0.0)
        {
            if let Some(field) = fields.get(fr.component) {
                delta += fr.affect * field.sample(pos);
            }
        }
        if delta != 0.0 {
            reserve.current = (reserve.current + delta * dt).clamp(0.0, reserve.max);
        }
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
