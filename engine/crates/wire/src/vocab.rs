//! Append-only r8 vocabulary extension v2, after frozen r7 vocabulary v13.
//! Never renumber the r7 prefix. New policy weights copy documented rows; Adam
//! moments are zero, not copied. This is an initialization, not wire parity.
pub const R7_TOKEN_VOCAB: usize = 2124;
pub const R8_VOCAB_VERSION: u32 = 2;
pub const PICK_SPECIAL_CARD: u16 = R7_TOKEN_VOCAB as u16;
/// Candidate token slot 4 on Choose/ConfirmChoice: optional discard-to-hand.
/// Dredge is mandatory and its legacy purpose is "discard", not retrieval.
pub const OPTIONAL_DISCARD_TO_HAND: u16 = R7_TOKEN_VOCAB as u16 + 1;
pub const R8_TOKEN_VOCAB: usize = R7_TOKEN_VOCAB + 2;
pub const PICK_CARD: usize = 1423;
/// Initializer only, not a claim of frozen-wire equivalence.
pub const CHOOSE: usize = 1442;
pub const SPECIAL_CARD_EVIDENCE: &str = "extra_reward_csharp_r8_v1";
pub const NEOWS_FURY_EVIDENCE: &str = "combat_choice_csharp_r8_v2";
