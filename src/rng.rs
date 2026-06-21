//! Minimal deterministic PRNG (SplitMix64).
//!
//! Self-contained: each agent can own its own reproducible stream without
//! inter-system contention (no shared mutable `Res` RNG that serializes
//! everything). We keep the seed to replay a config, not for bit-for-bit.

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

    /// Uniform float in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        // 24 bits of mantissa.
        (self.next_u64() >> 40) as f32 / (1u32 << 24) as f32
    }

    /// Uniform float in `[-1, 1)`.
    pub fn next_signed(&mut self) -> f32 {
        self.next_f32() * 2.0 - 1.0
    }

    /// Standard normal draw (mean 0, std-dev 1), via Box-Muller.
    /// Used to perturb genes during a mutation.
    pub fn next_gaussian(&mut self) -> f32 {
        // `max` keeps the log away from zero; u2 provides the phase.
        let u1 = self.next_f32().max(1e-7);
        let u2 = self.next_f32();
        (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
    }
}
