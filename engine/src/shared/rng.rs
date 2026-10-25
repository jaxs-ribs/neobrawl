//! Deterministic RNG with Unity's Random.Range semantics (shared with Square Lights).

/// Unity's Random.Range semantics on top of a small deterministic PRNG.
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Random.Range(float min, float max): inclusive.
    pub fn rangef(&mut self, min: f32, max: f32) -> f32 {
        let u = (self.next() >> 40) as f32 / (1u64 << 24) as f32;
        min + (max - min) * u
    }

    /// Random.Range(int min, int max): max exclusive.
    pub fn range(&mut self, min: i32, max: i32) -> i32 {
        if max <= min {
            return min;
        }
        min + ((self.next() >> 33) % (max - min) as u64) as i32
    }
}
