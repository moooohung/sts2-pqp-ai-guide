use super::{MpPhase, MpRun};
impl MpRun {
    /// Public menu legality for an actor's retained relic reward.
    pub fn can_skip_relic_reward(&self, net_id: u64, seat: usize) -> Result<bool, &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Neow { return Err("not at Neow relic reward"); }
        if self.relic_rewards[seat].is_none() { return Err("no owner relic reward"); }
        Ok(self.relic_reward_skippable[seat])
    }

    pub(super) fn begin_small_capsule_reward(&mut self, seat: usize) -> Result<(), &'static str> {
        // SmallCapsule.AfterObtained -> RewardsCmd.OfferCustom(RelicReward).
        // RelicReward.Populate pulls its owner's bag and removes the offer from
        // the shared bag before selection, even when the recipient later skips.
        let roll = self.roster.player_mut(seat).unwrap().rngs.rewards.next_float(1.0);
        let rarity = if roll < 0.5 { 0 } else if roll < 0.83 { 1 } else { 2 };
        let id = *self.world.player_bags[seat][rarity].first().ok_or("empty personal relic bag")?;
        self.world.player_bags[seat][rarity].remove(0);
        for bag in &mut self.world.shared_bags { bag.retain(|other| *other != id); }
        self.relic_rewards[seat] = Some(id);
        self.relic_reward_skippable[seat] = true;
        Ok(())
    }

    /// Answer the retained RelicReward, not an arbitrary client-supplied relic
    /// ID. Acceptance and skipping are independent for each network actor.
    pub fn select_relic_reward(&mut self, net_id: u64, seat: usize, accept: bool)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Neow { return Err("not at Neow relic reward"); }
        let id = self.relic_rewards[seat].ok_or("no owner relic reward")?;
        let skippable = self.relic_reward_skippable[seat];
        if !accept && !skippable { return Err("relic reward cannot be skipped"); }
        let mut next = self.clone();
        next.relic_rewards[seat] = None;
        next.relic_reward_skippable[seat] = true;
        if accept {
            if skippable {
                // SmallCapsule.cs creates a Common/Uncommon/Rare RelicReward.
                next.roster.obtain_inventory_relic(&mut next.world.run, seat, id)?;
                next.record_relic_seen(seat, id);
            } else {
                // NeowsBones.cs awaits each Ancient relic's complete pickup
                // continuation before allowing the next mandatory reward.
                next.obtain_neow_relic_effect(seat, id)?;
            }
        }
        next.maybe_finish_neows_bones(seat)?;
        next.advance_neow();
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod menu_tests {
    use super::*;
    use crate::{run::mp::{Character,PlayerSpec},tables::RelicId};

    #[test]
    fn relic_reward_skip_menu_matches_owner_and_retained_offer() {
        let mut run=MpRun::new("930001",&[
            PlayerSpec {net_id:100,character:Character::Ironclad},
            PlayerSpec {net_id:101,character:Character::Necrobinder},
        ],0).unwrap();
        assert_eq!(run.can_skip_relic_reward(101,1),Err("no owner relic reward"));
        run.relic_rewards[1]=Some(RelicId::Circlet);
        run.relic_reward_skippable[1]=false;
        assert_eq!(run.can_skip_relic_reward(101,1),Ok(false));
        assert_eq!(run.select_relic_reward(101,1,false),Err("relic reward cannot be skipped"));
        assert!(run.can_skip_relic_reward(100,1).is_err());
        run.relic_reward_skippable[1]=true;
        assert_eq!(run.can_skip_relic_reward(101,1),Ok(true));
        run.select_relic_reward(101,1,false).unwrap();
        assert_eq!(run.can_skip_relic_reward(101,1),Err("no owner relic reward"));
    }
}
