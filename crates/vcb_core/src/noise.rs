//! Deterministic randomness: coordinate hashes and PCG streams.
//!
//! These are the only sources of randomness in the crate. Nothing reads the system
//! clock or an entropy source, so every simulated frame is reproducible from the
//! preset's seed and the frame index.
//!
//! Two kinds of source cover the physics:
//!
//! - **Spatial** noise (fixed-pattern effects such as photo-response non-uniformity
//!   or hot pixels) is a pure function of `(x, y, salt, seed)`: [`hash_u32()`],
//!   [`unit()`], [`signed()`] and [`gauss()`]. The same photosite always gets the same
//!   value, in any evaluation order and on any thread.
//! - **Temporal** noise (shot noise, read noise, comparator noise) comes from a
//!   [`Pcg32`] stream seeded with the stage seed and the frame index, advanced in
//!   readout order.
//!
//! Each stage derives its own seed with [`stage_seed`] from the preset seed and a
//! [`salt`] of the stage name, so stages never share a stream.
//!
//! # Examples
//!
//! ```
//! use vcb_core::noise::{self, Pcg32};
//!
//! let seed = noise::stage_seed(1, noise::salt("sensor"));
//! assert_eq!(noise::unit(3, 4, 0, seed), noise::unit(3, 4, 0, seed));
//!
//! let mut stream = Pcg32::new(seed, 0);
//! let first = stream.next_u32();
//! assert_eq!(Pcg32::new(seed, 0).next_u32(), first);
//! ```

/// Increment of the `SplitMix64` sequence, `2^64 / phi`, decorrelating absorbed words.
const GOLDEN_GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

/// Multiplier of the 64-bit LCG underlying PCG (O'Neill 2014, `pcg32` reference code).
const PCG_MULTIPLIER: u64 = 6_364_136_223_846_793_005;

/// Scale that maps a 24-bit integer onto `[0, 1)`; 24 bits are exact in an `f32` mantissa.
const UNIT_24: f32 = 1.0 / 16_777_216.0;

/// Mixes 64 bits with the `SplitMix64` finaliser (Steele et al., 2014).
///
/// The map is a bijection with full avalanche.
fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Hashes coordinates, salt and seed into 64 well-mixed bits.
fn hash64(x: u32, y: u32, salt: u32, seed: u64) -> u64 {
    let key = (u64::from(x) << 32) | u64::from(y);
    let h = mix64(seed.wrapping_add(GOLDEN_GAMMA) ^ u64::from(salt));
    mix64(h.wrapping_add(GOLDEN_GAMMA) ^ key)
}

/// Hashes a photosite coordinate, a salt and a seed into 32 random bits.
///
/// For a fixed salt and seed the map from `(x, y)` to the result has no structure
/// that survives into image space; changing any argument changes the result.
#[must_use]
pub fn hash_u32(x: u32, y: u32, salt: u32, seed: u64) -> u32 {
    (hash64(x, y, salt, seed) >> 32) as u32
}

/// Returns a uniform value in `[0, 1)` for a coordinate, salt and seed.
#[must_use]
pub fn unit(x: u32, y: u32, salt: u32, seed: u64) -> f32 {
    (hash64(x, y, salt, seed) >> 40) as f32 * UNIT_24
}

/// Returns a uniform value in `[-1, 1)` for a coordinate, salt and seed.
#[must_use]
pub fn signed(x: u32, y: u32, salt: u32, seed: u64) -> f32 {
    unit(x, y, salt, seed).mul_add(2.0, -1.0)
}

/// Returns a standard normal value for a coordinate, salt and seed.
///
/// Uses the Box-Muller transform on two 24-bit uniforms taken from one 64-bit hash,
/// so the tails are truncated at about 5.8 standard deviations.
#[must_use]
pub fn gauss(x: u32, y: u32, salt: u32, seed: u64) -> f32 {
    let h = hash64(x, y, salt, seed);
    let u1 = ((h >> 40) + 1) as f32 * UNIT_24;
    let u2 = ((h >> 16) & 0x00ff_ffff) as f32 * UNIT_24;
    box_muller(u1, u2)
}

/// Maps `u1` in `(0, 1]` and `u2` in `[0, 1)` to one standard normal value.
fn box_muller(u1: f32, u2: f32) -> f32 {
    (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
}

/// Hashes a stage name into a salt at compile time (32-bit FNV-1a).
///
/// # Examples
///
/// ```
/// const SALT: u32 = vcb_core::noise::salt("readout");
/// assert_ne!(SALT, vcb_core::noise::salt("sensor"));
/// ```
#[must_use]
pub const fn salt(name: &str) -> u32 {
    let bytes = name.as_bytes();
    let mut hash: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u32;
        hash = hash.wrapping_mul(0x0100_0193);
        i += 1;
    }
    hash
}

/// Derives the seed of one stage from the preset seed and the stage salt.
#[must_use]
pub fn stage_seed(seed: u64, salt: u32) -> u64 {
    mix64(mix64(seed ^ GOLDEN_GAMMA).wrapping_add(u64::from(salt)))
}

/// A PCG32 random number stream (XSH-RR output, 64-bit state).
///
/// Streams with different `stream_id` values are statistically independent for the
/// same seed; stages use the frame index as the stream id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pcg32 {
    state: u64,
    increment: u64,
}

impl Pcg32 {
    /// Creates a stream from a seed and a stream id, as PCG's `srandom_r` does.
    ///
    /// Only the low 63 bits of `stream_id` select the stream.
    #[must_use]
    pub fn new(seed: u64, stream_id: u64) -> Self {
        let mut rng = Self {
            state: 0,
            increment: (stream_id << 1) | 1,
        };
        rng.step();
        rng.state = rng.state.wrapping_add(seed);
        rng.step();
        rng
    }

    fn step(&mut self) {
        self.state = self
            .state
            .wrapping_mul(PCG_MULTIPLIER)
            .wrapping_add(self.increment);
    }

    /// Returns the next 32 random bits.
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.step();
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rotation = (old >> 59) as u32;
        xorshifted.rotate_right(rotation)
    }

    /// Returns the next uniform value in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * UNIT_24
    }

    /// Returns the next standard normal value (Box-Muller on two draws).
    pub fn gauss(&mut self) -> f32 {
        let u1 = ((self.next_u32() >> 8) + 1) as f32 * UNIT_24;
        let u2 = self.next_f32();
        box_muller(u1, u2)
    }
}
