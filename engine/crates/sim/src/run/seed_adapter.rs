//! Exact scalar Gymnasium/NumPy reset seed adapter used by frozen r7.
//!
//! The fixture seed is fed to NumPy 2.4.3 PCG64 and one bounded
//! `integers(0, 2**31)` draw becomes the seed passed to `RunManager`.
//! This is the scalar-seed/one-draw subset of NumPy SeedSequence + PCG64.

#[inline]
fn hashmix(mut value: u32, h: &mut u32) -> u32 {
    value ^= *h;
    *h = h.wrapping_mul(0x931e_8875);
    value = value.wrapping_mul(*h);
    value ^ (value >> 16)
}

fn seed_sequence(seed: u64) -> [u64; 4] {
    let mut pool = [0u32; 4];
    let mut h = 0x43b0_d7e5u32;
    for (j, dst) in pool.iter_mut().enumerate() {
        let word = match j {
            0 => seed as u32,
            1 => (seed >> 32) as u32,
            _ => 0,
        };
        *dst = hashmix(word, &mut h);
    }
    for src in 0..4 {
        for dst in 0..4 {
            if src == dst {
                continue;
            }
            let mixed = hashmix(pool[src], &mut h);
            let value = 0xca01_f9ddu32
                .wrapping_mul(pool[dst])
                .wrapping_sub(0x4973_f715u32.wrapping_mul(mixed));
            pool[dst] = value ^ (value >> 16);
        }
    }

    let mut words = [0u64; 4];
    h = 0x8b51_f9ddu32;
    for j in 0..8 {
        let mut value = pool[j & 3] ^ h;
        h = h.wrapping_mul(0x58f3_8ded);
        value = value.wrapping_mul(h);
        value ^= value >> 16;
        if j & 1 == 0 {
            words[j >> 1] = value as u64;
        } else {
            words[j >> 1] |= (value as u64) << 32;
        }
    }
    words
}

#[derive(Clone, Copy)]
struct Pcg64 {
    state: u128,
    inc: u128,
    cached: Option<u32>,
}

impl Pcg64 {
    const MULT: u128 = ((2_549_297_995_355_413_924u128) << 64) | 4_865_540_595_714_422_341u128;

    fn new(seed: u64) -> Self {
        let w = seed_sequence(seed);
        let init_state = ((w[0] as u128) << 64) | w[1] as u128;
        let init_seq = ((w[2] as u128) << 64) | w[3] as u128;
        let mut out = Self {
            state: 0,
            inc: (init_seq << 1) | 1,
            cached: None,
        };
        out.step();
        out.state = out.state.wrapping_add(init_state);
        out.step();
        out
    }

    #[inline]
    fn step(&mut self) {
        self.state = self.state.wrapping_mul(Self::MULT).wrapping_add(self.inc);
    }

    fn next_u32(&mut self) -> u32 {
        if let Some(value) = self.cached.take() {
            return value;
        }
        self.step();
        let hi = (self.state >> 64) as u64;
        let lo = self.state as u64;
        let value = hi ^ lo;
        let rotation = (hi >> 58) as u32;
        let value = value.rotate_right(rotation);
        self.cached = Some((value >> 32) as u32);
        value as u32
    }

    fn bounded_u32(&mut self, high: u32) -> u32 {
        debug_assert!(high > 0);
        loop {
            let product = (self.next_u32() as u64) * (high as u64);
            let leftover = product as u32;
            if leftover < high {
                let threshold = (u32::MAX - (high - 1)) % high;
                if leftover < threshold {
                    continue;
                }
            }
            return (product >> 32) as u32;
        }
    }
}

/// Convert the seed passed to `STS2RunEnv.reset(seed=...)` into the exact
/// scalar seed passed by Gymnasium to frozen-r7 `RunManager`.
pub fn gym_seed_to_run_seed(seed: u64) -> u64 {
    Pcg64::new(seed).bounded_u32(2_147_483_648) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_fixture_seed() {
        assert_eq!(gym_seed_to_run_seed(4_100_000), 1_422_437_085);
    }
}
