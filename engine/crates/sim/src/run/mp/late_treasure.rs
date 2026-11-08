use super::{Character, MpRoster, MpWorld};
use crate::tables::RelicId;

pub(super) fn relic_allowed(roster: &MpRoster, floor: i32, id: RelicId) -> bool {
    let first_ironclad_run = roster.players().iter().any(|player|
        player.character == Character::Ironclad && player.unlock_state.number_of_runs == 0);
    !matches!(id, RelicId::SilverCrucible | RelicId::WingedBoots)
        && !(id == RelicId::LastingCandy && first_ironclad_run)
        && super::super::manager::RunManager::relic_allowed_at_floor(id, floor, true)
}

impl MpWorld {
    pub(super) fn pull_shared_relic_from_front(&mut self, roster: &MpRoster, rarity: usize) -> RelicId {
        // RelicGrabBag.GetAvailableDeque prunes every rarity before escalating,
        // without touching the recipient bags or consuming another RNG draw.
        let floor = self.run.total_floor;
        for bag in &mut self.shared_bags { bag.retain(|id| relic_allowed(roster, floor, *id)); }
        (rarity..3).find(|&bucket| !self.shared_bags[bucket].is_empty())
            .map(|bucket| self.shared_bags[bucket].remove(0)).unwrap_or(RelicId::Circlet)
    }
}

#[cfg(test)]
mod tests {
    use crate::{run::mp::*, tables::RelicId};
    #[test]
    fn late_treasure_prunes_all_shared_rarities_without_owner_rng_draws() {
        for floor in [37, 38, 50] {
            let specs = [PlayerSpec { net_id: 1, character: Character::Silent },
                PlayerSpec { net_id: 2, character: Character::Silent }];
            let mut run = MpRun::new("83003", &specs, 0).unwrap();
            run.world.run.total_floor = floor;
            run.world.shared_bags = [
                vec![RelicId::FrozenEgg, RelicId::Vajra],
                vec![RelicId::ToxicEgg, RelicId::Kunai],
                vec![RelicId::MoltenEgg, RelicId::Shuriken],
                vec![RelicId::FrozenEgg],
            ];
            let owner_rng: Vec<_> = run.roster.players().iter().map(|p| p.rngs.rewards.counter).collect();
            let rng = run.world.run.rngs.treasure_room.counter;
            run.enter_treasure().unwrap();
            assert_eq!(run.treasure_relics.len(), 2);
            assert_eq!(run.world.run.rngs.treasure_room.counter, rng + 2);
            if floor >= 38 {
                assert!(run.world.shared_bags.iter().flatten().chain(run.treasure_relics.iter())
                    .all(|id| !matches!(id, RelicId::FrozenEgg | RelicId::ToxicEgg | RelicId::MoltenEgg)));
            }
            for (seat, p) in run.roster.players().iter().enumerate() {
                assert_eq!(p.rngs.rewards.counter, owner_rng[seat]);
            }
        }
    }
}
