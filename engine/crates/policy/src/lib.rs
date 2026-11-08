//! r7 relational candidate policy (PublicBossActor + value head) and PPO, in burn.
//! Structure and names follow sts2_env/training/relational_structured_ppo.py and
//! public_boss_policy.py in the frozen r7 package (main_training_package_r7_165).
pub mod model;
pub mod obs;
pub mod packed;
pub mod native;
pub mod native_batch;

// Flex device seeding is global: RNG-consuming unit tests must not interleave
// with the exact seeded-initialization assertion.
#[cfg(test)]
pub(crate) static RNG_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub const STATE_ROWS: usize = 512;
pub const CANDIDATES: usize = 256;
pub const TOKEN_SLOTS: usize = 6;
pub const SCALAR_SLOTS: usize = 8;
pub const SCALAR_WIDTH: usize = 6;
pub const TAIL_SLOTS: usize = 16;
pub const NUMERIC_DIM: usize = SCALAR_SLOTS * SCALAR_WIDTH + TAIL_SLOTS;
pub const BOSSES: usize = 6;
pub const MASKED_LOGIT: f32 = -1.0e9;
