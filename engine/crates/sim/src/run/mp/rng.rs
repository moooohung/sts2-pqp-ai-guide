use crate::rng::{Rng, deterministic_hash64};

/// RunRngSet(string): legacy prefix applies only to shared/event seeds.
/// Player.InitializeSeed still hashes the original string independently.
pub(super) fn decoded_run_seed(seed: &str) -> u64 {
    if let Some(legacy) = seed.strip_prefix("old") {
        crate::rng::deterministic_hash_code_old(legacy) as u32 as u64
    } else {
        deterministic_hash64(seed)
    }
}

/// Player.cs:239 + Random/PlayerRngSet.cs:13-35. Slot, NOT network ID,
/// offsets the seed. The run's shared RNGs do not belong in this object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerRngs {
    pub seed: u64,
    pub rewards: Rng,
    pub shops: Rng,
    pub transformations: Rng,
}

impl PlayerRngs {
    pub fn new(run_seed: &str, slot: usize) -> Self {
        Self::from_numeric_seed(deterministic_hash64(run_seed), slot)
    }

    /// Player.InitializeSeed (Player.cs:239): derived seed plus roster slot.
    pub fn from_numeric_seed(run_seed: u64, slot: usize) -> Self {
        let seed = run_seed.wrapping_add(slot as u64);
        Self {
            seed,
            rewards: Rng::named(seed, "rewards"),
            shops: Rng::named(seed, "shops"),
            transformations: Rng::named(seed, "transformations"),
        }
    }
}
