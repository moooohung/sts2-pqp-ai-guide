//! r7 `sts2_env/core/rng.py`: the game's seeded `Rng` over xoshiro256** (`MegaRandom`),
//! named streams seeded with xxh64 of the UTF-8 name, and .NET `System.Random`
//! for the few legacy call sites that use it.

pub const INT_MAX: i64 = 2_147_483_647;
pub const RNG_ALGORITHM: &str = "sts2-0861bfa1-xoshiro256ss-xxh64-v1";

const P1: u64 = 11400714785074694791;
const P2: u64 = 14029467366897019727;
const P3: u64 = 1609587929392839161;
const P4: u64 = 9650029242287828579;
const P5: u64 = 2870177450012600261;

#[inline]
fn round64(acc: u64, value: u64) -> u64 {
    acc.wrapping_add(value.wrapping_mul(P2)).rotate_left(31).wrapping_mul(P1)
}

#[inline]
fn read_u64(data: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(data[at..at + 8].try_into().unwrap())
}

/// xxHash64, as `_xxh64` in rng.py.
pub fn xxh64(data: &[u8], seed: u64) -> u64 {
    let length = data.len();
    let mut index = 0usize;
    let mut h;
    if length >= 32 {
        let (mut v1, mut v2, mut v3, mut v4) =
            (seed.wrapping_add(P1).wrapping_add(P2), seed.wrapping_add(P2), seed, seed.wrapping_sub(P1));
        while index + 32 <= length {
            v1 = round64(v1, read_u64(data, index));
            v2 = round64(v2, read_u64(data, index + 8));
            v3 = round64(v3, read_u64(data, index + 16));
            v4 = round64(v4, read_u64(data, index + 24));
            index += 32;
        }
        h = v1.rotate_left(1).wrapping_add(v2.rotate_left(7)).wrapping_add(v3.rotate_left(12)).wrapping_add(v4.rotate_left(18));
        for v in [v1, v2, v3, v4] {
            h ^= round64(0, v);
            h = h.wrapping_mul(P1).wrapping_add(P4);
        }
    } else {
        h = seed.wrapping_add(P5);
    }
    h = h.wrapping_add(length as u64);
    while index + 8 <= length {
        h ^= round64(0, read_u64(data, index));
        h = h.rotate_left(27).wrapping_mul(P1).wrapping_add(P4);
        index += 8;
    }
    if index + 4 <= length {
        let word = u32::from_le_bytes(data[index..index + 4].try_into().unwrap()) as u64;
        h ^= word.wrapping_mul(P1);
        h = h.rotate_left(23).wrapping_mul(P2).wrapping_add(P3);
        index += 4;
    }
    while index < length {
        h ^= (data[index] as u64).wrapping_mul(P5);
        h = h.rotate_left(11).wrapping_mul(P1);
        index += 1;
    }
    h ^= h >> 33;
    h = h.wrapping_mul(P2);
    h ^= h >> 29;
    h = h.wrapping_mul(P3);
    h ^= h >> 32;
    h
}

/// `deterministic_hash64`: xxh64 of the UTF-8 encoding. Rust strings are always valid
/// UTF-8, which matches .NET's replacement of unpaired surrogates for every name r7 uses.
pub fn deterministic_hash64(text: &str) -> u64 {
    xxh64(text.as_bytes(), 0)
}

/// `deterministic_hash_code_old` (legacy StringHelper.GetDeterministicHashCodeOld, UTF-16).
pub fn deterministic_hash_code_old(text: &str) -> i32 {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut num: i32 = 352654597;
    let mut num2: i32 = num;
    let mut index = 0;
    while index < units.len() {
        num = (num << 5).wrapping_add(num) ^ units[index] as i32;
        if index == units.len() - 1 {
            break;
        }
        num2 = (num2 << 5).wrapping_add(num2) ^ units[index + 1] as i32;
        index += 2;
    }
    num.wrapping_add(num2.wrapping_mul(1566083941))
}

/// Round a double to IEEE float32 and back (`_float32`).
#[inline]
pub fn float32(value: f64) -> f64 {
    value as f32 as f64
}

/// xoshiro256** seeded through splitmix64 (`MegaRandom`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MegaRandom {
    pub s: [u64; 4],
    pub counter: u64,
}

impl MegaRandom {
    pub fn new(seed: u64) -> Self {
        let mut x = seed;
        let mut s = [0u64; 4];
        for word in &mut s {
            x = x.wrapping_add(11400714819323198485);
            let mut z = x;
            z = (z ^ (z >> 30)).wrapping_mul(13787848793156543929);
            z = (z ^ (z >> 27)).wrapping_mul(10723151780598845931);
            *word = z ^ (z >> 31);
        }
        Self { s, counter: 0 }
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.counter += 1;
        let [mut s0, mut s1, mut s2, mut s3] = self.s;
        let result = s1.wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s1 << 17;
        s2 ^= s0;
        s3 ^= s1;
        s1 ^= s2;
        s0 ^= s3;
        s2 ^= t;
        s3 = s3.rotate_left(45);
        self.s = [s0, s1, s2, s3];
        result
    }

    /// Double in [0, 1) from the top 53 bits.
    #[inline]
    pub fn sample(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// The game's seeded `Rng` (rng.py `Rng`). `next_int(low, high)` is inclusive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng {
    pub base_seed: u64,
    pub seed: u64,
    pub counter: u64,
    pub inner: MegaRandom,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self::with_counter(seed, None, 0)
    }

    pub fn named(seed: u64, name: &str) -> Self {
        Self::with_counter(seed, Some(name), 0)
    }

    pub fn with_counter(seed: u64, name: Option<&str>, counter: u64) -> Self {
        let derived = match name {
            None => seed,
            Some(n) => seed.wrapping_add(deterministic_hash64(n)),
        };
        let mut rng = Self { base_seed: seed, seed: derived, counter: 0, inner: MegaRandom::new(derived) };
        rng.fast_forward_counter(counter);
        rng
    }

    /// `Rng.for_model(run_seed, player_slot, model_entry, mixin)`; arithmetic wraps like
    /// the Python `& UINT64_MASK` in `Rng.__init__`.
    pub fn for_model(run_seed: u64, player_slot: u64, model_entry: &str, mixin: u64) -> Self {
        Self::new(run_seed.wrapping_add(player_slot).wrapping_add(deterministic_hash64(model_entry)).wrapping_add(mixin))
    }

    pub fn fast_forward_counter(&mut self, target: u64) {
        assert!(self.counter <= target, "cannot fast-forward an Rng counter to a lower number");
        while self.counter < target {
            self.counter += 1;
            self.inner.next_u64();
        }
    }

    /// Inclusive `[low, high]`.
    #[inline]
    pub fn next_int(&mut self, low: i64, high: i64) -> i64 {
        assert!(low <= high, "low must be <= high");
        self.counter += 1;
        let range = (high + 1 - low) as f64;
        (self.inner.sample() * range) as i64 + low
    }

    /// `[low, high)`, C# `Random.Next(min, max)`.
    #[inline]
    pub fn next_int_exclusive(&mut self, low: i64, high: i64) -> i64 {
        assert!(low < high, "low must be < high");
        self.next_int(low, high - 1)
    }

    #[inline]
    pub fn next_bool(&mut self) -> bool {
        self.next_int(0, 1) == 0
    }

    pub fn next_double(&mut self, low: f64, high: f64) -> f64 {
        assert!(low <= high, "low must be <= high");
        self.counter += 1;
        self.inner.sample() * (high - low) + low
    }

    /// `next_float_range(low, high)`: float32 bounds, double mapping, float32 result.
    pub fn next_float_range(&mut self, low: f64, high: f64) -> f64 {
        let (low, high) = (float32(low), float32(high));
        assert!(low <= high, "low must be <= high");
        self.counter += 1;
        float32(self.inner.sample() * float32(high - low) + low)
    }

    pub fn next_float(&mut self, upper: f64) -> f64 {
        self.next_float_range(0.0, upper)
    }

    /// `next_unsigned_int(low, high)` with an explicit `[low, high)` range.
    pub fn next_unsigned_int(&mut self, low: u64, high: u64) -> u64 {
        assert!(low < high && high <= u32::MAX as u64, "uint range must satisfy 0 <= low < high <= uint.MaxValue");
        self.counter += 1;
        low + (self.inner.sample() * (high - low) as f64) as u64
    }

    /// No-argument `next_unsigned_long()`: raw 64 bits.
    pub fn next_unsigned_long_raw(&mut self) -> u64 {
        self.counter += 1;
        self.inner.next_u64()
    }

    /// In-place Fisher-Yates from the end, as `Rng.shuffle`.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for index in (1..items.len()).rev() {
            let swap = self.next_int(0, index as i64) as usize;
            items.swap(index, swap);
        }
    }

    pub fn choice_index(&mut self, len: usize) -> usize {
        assert!(len > 0, "cannot choose from an empty list");
        self.next_int(0, len as i64 - 1) as usize
    }

    /// `Rng.sample(lst, k)`: k distinct elements, popping from a working copy.
    pub fn sample<T: Clone>(&mut self, items: &[T], k: usize) -> Vec<T> {
        assert!(k <= items.len(), "sample larger than population");
        let mut pool = items.to_vec();
        let mut out = Vec::with_capacity(k);
        for _ in 0..k {
            let index = self.next_int(0, pool.len() as i64 - 1) as usize;
            out.push(pool.remove(index));
        }
        out
    }

    /// `next_gaussian_int`: rejection sampling with Python's round-half-to-even.
    pub fn next_gaussian_int(&mut self, mean: f64, stddev: f64, min: i64, max: i64) -> i64 {
        assert!(min <= max && (min as f64) <= mean && mean <= max as f64, "mean must be within [min, max]");
        loop {
            let u1 = 1.0 - self.next_double(0.0, 1.0);
            let u2 = 1.0 - self.next_double(0.0, 1.0);
            let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).sin();
            let value = round_half_even(mean + stddev * z);
            if min <= value && value <= max {
                return value;
            }
        }
    }

    /// `fork()`: a child Rng seeded from `next_int(0, INT_MAX_EXCLUSIVE)`.
    pub fn fork(&mut self) -> Rng {
        Rng::new(self.next_int(0, INT_MAX + 1) as u64)
    }
}

/// Python 3 `round()` on floats: ties go to the even integer.
pub fn round_half_even(value: f64) -> i64 {
    let floor = value.floor();
    let diff = value - floor;
    let base = floor as i64;
    if diff > 0.5 {
        base + 1
    } else if diff < 0.5 {
        base
    } else if base % 2 == 0 {
        base
    } else {
        base + 1
    }
}

/// .NET `System.Random(int)` (`_DotNetCompatRandom`).
#[derive(Clone, Debug)]
pub struct DotNetRandom {
    seed_array: [i32; 56],
    inext: usize,
    inextp: usize,
}

impl DotNetRandom {
    pub fn new(seed: i32) -> Self {
        let mut seed_array = [0i32; 56];
        let subtraction = if seed == i32::MIN { i32::MAX } else { seed.abs() };
        let mut mj = 161_803_398i32.wrapping_sub(subtraction);
        seed_array[55] = mj;
        let mut mk: i32 = 1;
        let mut ii = 0usize;
        for _ in 1..55 {
            ii += 21;
            if ii >= 55 {
                ii -= 55;
            }
            seed_array[ii] = mk;
            mk = mj.wrapping_sub(mk);
            if mk < 0 {
                mk = mk.wrapping_add(i32::MAX);
            }
            mj = seed_array[ii];
        }
        for _ in 1..5 {
            for i in 1..56 {
                let mut n = i + 30;
                if n >= 55 {
                    n -= 55;
                }
                seed_array[i] = seed_array[i].wrapping_sub(seed_array[1 + n]);
                if seed_array[i] < 0 {
                    seed_array[i] = seed_array[i].wrapping_add(i32::MAX);
                }
            }
        }
        Self { seed_array, inext: 0, inextp: 21 }
    }

    pub fn internal_sample(&mut self) -> i32 {
        let mut loc_inext = self.inext + 1;
        if loc_inext >= 56 {
            loc_inext = 1;
        }
        let mut loc_inextp = self.inextp + 1;
        if loc_inextp >= 56 {
            loc_inextp = 1;
        }
        let mut ret = self.seed_array[loc_inext].wrapping_sub(self.seed_array[loc_inextp]);
        if ret == i32::MAX {
            ret -= 1;
        }
        if ret < 0 {
            ret = ret.wrapping_add(i32::MAX);
        }
        self.seed_array[loc_inext] = ret;
        self.inext = loc_inext;
        self.inextp = loc_inextp;
        ret
    }

    pub fn sample(&mut self) -> f64 {
        self.internal_sample() as f64 * (1.0 / INT_MAX as f64)
    }
}
