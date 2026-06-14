//! PRNG déterministe minimal (SplitMix64).
//!
//! Auto-contenu : chaque agent peut posséder son propre flux reproductible sans
//! contention inter-systèmes (pas de `Res` RNG mutable partagée qui sérialise
//! tout). On garde la graine pour rejouer une config, pas pour le bit-à-bit.

#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Flottant uniforme dans `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        // 24 bits de mantisse.
        (self.next_u64() >> 40) as f32 / (1u32 << 24) as f32
    }

    /// Flottant uniforme dans `[-1, 1)`.
    pub fn next_signed(&mut self) -> f32 {
        self.next_f32() * 2.0 - 1.0
    }

    /// Tirage gaussien centré réduit (moyenne 0, écart-type 1), par Box-Muller.
    /// Sert à perturber les gènes lors d'une mutation.
    pub fn next_gaussian(&mut self) -> f32 {
        // `max` écarte le log de zéro ; u2 fournit la phase.
        let u1 = self.next_f32().max(1e-7);
        let u2 = self.next_f32();
        (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
    }
}
