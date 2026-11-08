use super::MpRoster;
use crate::tables::RelicId;

impl MpRoster {
    /// EventModel.BeginEvent's owner slot seed plus the existing audited Neow
    /// offer algorithm. Reuses solo generation without its solo-only filter.
    pub fn neow_offers(&self, seed: &str) -> Vec<[RelicId; 3]> {
        self.neow_events(seed).into_iter().map(|event| event.0).collect()
    }

    pub(super) fn neow_events(&self, seed: &str) -> Vec<([RelicId; 3], crate::rng::Rng)> {
        self.neow_events_from_numeric_seed(super::rng::decoded_run_seed(seed))
    }

    pub(super) fn neow_events_from_numeric_seed(&self, seed: u64) -> Vec<([RelicId; 3], crate::rng::Rng)> {
        (0..self.players().len()).map(|slot|
            super::super::manager::multiplayer_neow_event(
                seed.wrapping_add(slot as u64))).collect()
    }
}

#[test]
fn legacy_neow_uses_decoded_run_seed_plus_slot_not_player_seed() {
    use super::{Character, PlayerSpec};
    let specs = [PlayerSpec { net_id: 1, character: Character::Ironclad },
        PlayerSpec { net_id: 2, character: Character::Necrobinder }];
    let roster = MpRoster::new("old73001", &specs).unwrap();
    let decoded = crate::rng::deterministic_hash_code_old("73001") as u32 as u64;
    assert_ne!(roster.player(0).unwrap().rngs.seed, decoded);
    assert_eq!(roster.neow_offers("old73001"), (0..2).map(|slot|
        super::super::manager::multiplayer_neow_offers(decoded + slot)).collect::<Vec<_>>());
}
