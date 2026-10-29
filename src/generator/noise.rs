//! General-purpose seeded 2-D noise: smooth value noise plus fractal (fBm)
//! stacking. A utility for the whole generator — the town shape uses it now,
//! props / roads / detail effects will reuse it later. Nothing in here knows
//! about maps or towns.

/// A seeded 2-D noise field. Every method is pure: same field + same inputs
/// always give the same value, on every machine and every run.
pub struct NoiseField {
    seed: u64,
}

impl NoiseField {
    pub fn new(seed: u64) -> Self {
        Self { seed }
    }

    /// Hash one integer lattice corner into [0, 1). `salt` selects an
    /// independent plane (used to decorrelate the fBm octaves).
    fn corner(&self, xi: i64, yi: i64, salt: u64) -> f32 {
        let mut h = self.seed
            ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ 0x632B_E599_66D0_24A5;
        h ^= (xi as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93);
        h = (h ^ (h >> 31)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= (yi as u64).wrapping_mul(0xA24B_AED4_963E_E407);
        h ^= h >> 29;
        h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
        h ^= h >> 32;
        // Keep 24 bits so the division by 2^24 is exact in f32.
        (h & 0x00FF_FFFF) as f32 / 16_777_216.0
    }

    /// Smooth value noise in [0, 1] at an arbitrary point.
    pub fn value(&self, x: f32, y: f32) -> f32 {
        self.value_salted(x, y, 0)
    }

    fn value_salted(&self, x: f32, y: f32, salt: u64) -> f32 {
        let (xf, yf) = (x.floor(), y.floor());
        let (tx, ty) = (fade(x - xf), fade(y - yf));
        let v00 = self.corner(xf as i64, yf as i64, salt);
        let v10 = self.corner(xf as i64 + 1, yf as i64, salt);
        let v01 = self.corner(xf as i64, yf as i64 + 1, salt);
        let v11 = self.corner(xf as i64 + 1, yf as i64 + 1, salt);
        let a = v00 + (v10 - v00) * tx;
        let b = v01 + (v11 - v01) * tx;
        a + (b - a) * ty
    }

    /// Fractal noise: `octaves` value-noise layers stacked at doubling
    /// frequency and halving amplitude, normalized back to [0, 1]. Low
    /// octaves make broad shapes, high octaves add grain.
    pub fn fbm(&self, x: f32, y: f32, octaves: u32) -> f32 {
        let octaves = octaves.max(1);
        let mut sum = 0.0f32;
        let mut norm = 0.0f32;
        let (mut amp, mut freq) = (1.0f32, 1.0f32);
        for o in 0..octaves {
            let salt = 0x51ED_2701 ^ (o as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            sum += amp * self.value_salted(
                x * freq + o as f32 * 17.31,
                y * freq + o as f32 * 43.7,
                salt,
            );
            norm += amp;
            amp *= 0.5;
            freq *= 2.0;
        }
        (sum / norm).clamp(0.0, 1.0)
    }
}

/// Quintic smoothstep between lattice corners: C2-continuous, so the field
/// has no visible creases along the grid axes.
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_stay_in_unit_range() {
        let field = NoiseField::new(7);
        let mut y = -50.0;
        while y <= 50.0 {
            let mut x = -50.0;
            while x <= 50.0 {
                let v = field.fbm(x, y, 4);
                assert!((0.0..=1.0).contains(&v), "fbm({x},{y}) = {v}");
                let v = field.value(x * 0.37, y * 0.11);
                assert!((0.0..=1.0).contains(&v), "value({x},{y}) = {v}");
                x += 3.13;
            }
            y += 2.71;
        }
    }

    #[test]
    fn same_seed_same_values_different_seed_differ() {
        let a = NoiseField::new(123);
        let b = NoiseField::new(123);
        let c = NoiseField::new(124);
        let mut differs = false;
        for i in 0..200 {
            let (x, y) = (i as f32 * 0.731, i as f32 * 1.193);
            assert_eq!(a.fbm(x, y, 4), b.fbm(x, y, 4), "same seed must match");
            if a.fbm(x, y, 4) != c.fbm(x, y, 4) {
                differs = true;
            }
        }
        assert!(differs, "seeds 123 and 124 produced identical fields");
    }

    #[test]
    fn neighboring_samples_are_close() {
        let field = NoiseField::new(9);
        for i in 0..500 {
            let (x, y) = (i as f32 * 0.137, i as f32 * 0.291);
            let d = (field.fbm(x, y, 4) - field.fbm(x + 0.01, y + 0.01, 4)).abs();
            assert!(d < 0.02, "noise jumps {d} between nearby samples at {x},{y}");
        }
    }
}
