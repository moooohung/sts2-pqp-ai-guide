//! TreasureRoomRelicSynchronizer: shared relic pool, owner votes, seeded
//! rock/paper/scissors tie resolution, and source-ordered consolation awards.
use super::{MpPhase, MpRun};
use crate::{relic::Relic, tables::{RelicId, RoomType}};

// C# RelicPickingResultType order, used by AnimateRelicAwards before pickups
// and MoveToFallback. There are at most four results, so ties keep source order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum AwardKind { OnlyOnePlayerVoted, FoughtOver, ConsolationPrize, Skipped }

impl MpRun {
    pub(super) fn enter_treasure(&mut self) -> Result<(), &'static str> {
        if self.roster.players().iter().flat_map(|p| &p.relics)
            .any(|r| r.id == RelicId::SilverCrucible) {
            return Err("unported multiplayer treasure generation modifier");
        }
        self.world.run.room_type = RoomType::Treasure;
        self.world.current_room = None;
        // Keep the actual map point for Planisphere on unknown treasure rooms.
        self.apply_room_entry_hooks(RoomType::Treasure)?;
        self.treasure_relics.clear();
        self.treasure_votes.fill(None);
        self.treasure_opened.fill(false);
        for _ in self.roster.players() {
            let roll = self.world.run.rngs.treasure_room.next_float(1.0) as f32;
            let rarity = if roll < 0.5 { 0 } else if roll < 0.83 { 1 } else { 2 };
            let id = self.world.pull_shared_relic_from_front(&self.roster, rarity);
            self.treasure_relics.push(id);
        }
        self.phase = MpPhase::Treasure;
        Ok(())
    }

    /// OneOffSynchronizer.DoTreasureRoomRewards is an owner action independent
    /// of relic votes. A transport does not open remote chests on their behalf.
    pub fn open_treasure(&mut self, net_id: u64, seat: usize) -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if !matches!(self.phase, MpPhase::Treasure | MpPhase::Map) ||
            self.world.run.room_type != RoomType::Treasure {
            return Err("not at treasure chest");
        }
        if self.treasure_opened[seat] { return Err("treasure chest already opened"); }
        // OneOffSynchronizer.TryHandleSpoilsMap checks the current point's
        // quests first. Merely holding Spoils Map does not trigger a payout:
        // its map registration is in Act 2 (SpoilsMap.AfterCreated).
        let quests = self.world.run.coord.and_then(|(col, row)|
            self.world.map.point(crate::run::Coord::new(col, row)))
            .map(|point|point.spoils_quests.clone()).unwrap_or_default();
        let p = self.roster.player_mut(seat).unwrap();
        let mut gold = p.rngs.rewards.next_int(42, 52) as f64;
        if self.world.run.ascension >= 3 { gold *= 0.75; }
        p.gain_gold(&self.world.run,gold as i32);
        let completed:Vec<_>=p.deck.iter().filter(|c|c.id==crate::tables::CardId::SpoilsMap
            &&quests.contains(&c.instance_id)).map(|c|c.instance_id).collect();
        for instance in &completed {
            p.gain_gold(&self.world.run,600);
            p.retain_deck_cards(|c|c.instance_id!=*instance);
        }
        if let Some((col,row))=self.world.run.coord {
            if let Some(point)=self.world.map.points.iter_mut().find(|p|p.coord==crate::run::Coord::new(col,row)){
                point.spoils_quests.retain(|id|!completed.contains(id));
            }
        }
        self.treasure_opened[seat] = true;
        Ok(())
    }

    pub fn vote_treasure(&mut self, net_id: u64, seat: usize, index: Option<usize>) -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Treasure { return Err("not at treasure relic vote"); }
        if index.is_some_and(|i| i >= self.treasure_relics.len()) { return Err("invalid treasure relic index"); }
        let mut next = self.clone();
        next.treasure_votes[seat] = Some(index);
        if next.treasure_votes.iter().all(Option::is_some) { next.award_treasure()?; }
        *self = next;
        Ok(())
    }

    fn award_treasure(&mut self) -> Result<(), &'static str> {
        let mut awards = Vec::new();
        let mut unused = Vec::new();
        for (index, &id) in self.treasure_relics.iter().enumerate() {
            let players: Vec<_> = self.treasure_votes.iter().enumerate().filter_map(|(seat, vote)|
                (*vote == Some(Some(index))).then_some(seat)).collect();
            if players.is_empty() { unused.push(id); continue; }
            let kind = if players.len() == 1 { AwardKind::OnlyOnePlayerVoted } else { AwardKind::FoughtOver };
            let mut alive = players.clone();
            while alive.len() > 1 {
                let moves: Vec<_> = alive.iter().map(|&seat|
                    (seat, self.world.run.rngs.treasure_room.choice_index(3))).collect();
                let mut distinct = Vec::new();
                for &(_, value) in &moves { if !distinct.contains(&value) { distinct.push(value); } }
                if distinct.len() == 2 {
                    let losing = if (distinct[0] + 1) % 3 == distinct[1] { distinct[0] } else { distinct[1] };
                    alive.retain(|seat| moves.iter().any(|&(who, value)| &who == seat && value != losing));
                }
            }
            awards.push((kind, Some(alive[0]), id));
        }
        let unawarded: Vec<_> = (0..self.roster.players().len()).filter(|seat|
            self.treasure_votes[*seat] != Some(None)
                && !awards.iter().any(|(_, owner, _)| *owner == Some(*seat))).collect();
        // AbstractModel comparison for relics is ModelId order.
        unused.sort_by_key(|id| id.name());
        self.world.run.rngs.treasure_room.shuffle(&mut unused);
        for (index, id) in unused.into_iter().enumerate() {
            let owner = unawarded.get(index).copied();
            let kind = if owner.is_some() { AwardKind::ConsolationPrize } else { AwardKind::Skipped };
            awards.push((kind, owner, id));
        }
        awards.sort_by_key(|(kind, _, _)| *kind);
        for (_, owner, id) in awards {
            // TreasureRoomRelicSynchronizer.AwardRelics selects from the normal
            // Common/Uncommon/Rare grab bags. MpRoster::obtain_inventory_relic
            // covers every C# AfterObtained callback in those bags (Whetstone,
            // War Paint, Potion Belt, Strawberry/Pear/Mango, Old Coin) and is
            // otherwise a direct owner attachment.
            if let Some(seat) = owner {
                self.roster.obtain_inventory_relic(&mut self.world.run, seat, id)?;
                if !crate::tables::RELIC_INFO[id.index()].stackable {
                    for bag in &mut self.world.player_bags[seat] { bag.retain(|other| *other != id); }
                    for bag in &mut self.world.shared_bags { bag.retain(|other| *other != id); }
                }
                self.record_relic_seen(seat, id);
            }
            // Original AnimateRelicAwards also moves skipped prizes, whose
            // owner is null, into every player's fallback queue.
            for seat in 0..self.roster.players().len() {
                if Some(seat) != owner { self.world.move_player_relic_to_fallback(seat, id); }
            }
        }
        self.treasure_relics.clear();
        self.finish_noncombat();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::mp::{Character, PlayerSpec};

    fn chest() -> MpRun {
        let specs: Vec<_> = (0..4).map(|i| PlayerSpec { net_id: 100 + i,
            character: Character::Ironclad }).collect();
        let mut run = MpRun::new("73001", &specs, 0).unwrap();
        run.world.run.total_floor = 9;
        run.world.run.room_type = RoomType::Treasure;
        run.phase = MpPhase::Treasure;
        run.treasure_relics = vec![RelicId::Vajra, RelicId::BagOfMarbles,
            RelicId::BagOfPreparation, RelicId::AmethystAubergine];
        run
    }

    #[test]
    fn remote_treasure_votes_bind_owner_and_award_only_after_all_votes() {
        let mut run = chest();
        let before = run.world.run.rngs.treasure_room.counter;
        assert!(run.vote_treasure(100, 1, Some(1)).is_err());
        assert!(run.vote_treasure(101, 1, Some(4)).is_err());
        assert_eq!(run.world.run.rngs.treasure_room.counter, before);
        for seat in (0..4).rev() {
            run.vote_treasure(100 + seat as u64, seat, Some(seat)).unwrap();
            if seat > 0 { assert_eq!(run.roster.player(seat).unwrap().relics.len(), 1); }
        }
        for (seat, id) in [RelicId::Vajra, RelicId::BagOfMarbles,
            RelicId::BagOfPreparation, RelicId::AmethystAubergine].into_iter().enumerate() {
            let relic = run.roster.player(seat).unwrap().relics.last().unwrap();
            assert_eq!(relic.id, id);
            assert_eq!(relic.owner as usize, seat);
            assert_eq!(relic.floor_added_to_deck, 9);
            assert!(!run.world.player_bags[seat].iter().flatten().any(|other| *other == id));
            for other in 0..4 {
                assert!(!run.world.player_bags[other].iter().flatten().any(|candidate| *candidate == id));
                assert_eq!(run.world.player_fallbacks[other].contains(&id), other != seat);
            }
        }
        assert_eq!(run.phase, MpPhase::Map);
        assert!(run.vote_treasure(101, 1, Some(1)).is_err());
    }

    #[test]
    fn contested_relics_use_seeded_fights_and_skippers_receive_no_consolation() {
        let mut run = chest();
        for seat in 0..3 { run.vote_treasure(100 + seat as u64, seat, Some(0)).unwrap(); }
        run.vote_treasure(103, 3, None).unwrap();
        assert_eq!(run.roster.player(3).unwrap().relics.len(), 1);
        let prizes: Vec<_> = run.roster.players().iter().take(3)
            .flat_map(|p| p.relics.iter().skip(1).map(|r| r.id)).collect();
        assert_eq!(prizes.len(), 3);
        assert_eq!(prizes.iter().filter(|&&id| id == RelicId::Vajra).count(), 1);
        let unique: std::collections::HashSet<_> = prizes.into_iter().collect();
        assert_eq!(unique.len(), 3);
    }

    #[test]
    fn treasure_pickup_routes_normal_bag_after_obtained_effects() {
        let mut run = chest();
        run.treasure_relics[3] = RelicId::Whetstone;
        let before_upgraded = run.roster.player(3).unwrap().deck.iter().filter(|c| c.upgraded).count();
        for seat in 0..3 { run.vote_treasure(100 + seat as u64, seat, Some(seat)).unwrap(); }
        run.vote_treasure(103, 3, Some(3)).unwrap();
        assert!(run.roster.player(3).unwrap().relics.iter().any(|r| r.id == RelicId::Whetstone));
        assert_eq!(run.roster.player(3).unwrap().deck.iter().filter(|c| c.upgraded).count(), before_upgraded + 2);
        assert!(!run.world.player_bags[3].iter().flatten().any(|other| *other == RelicId::Whetstone));
        assert_eq!(run.phase, MpPhase::Map);
    }

    #[test]
    fn chest_gold_is_a_one_off_owner_action_not_a_party_reward() {
        let mut run = chest();
        let before: Vec<_> = run.roster.players().iter().map(|p| (p.gold, p.rngs.rewards.counter)).collect();
        assert!(run.open_treasure(100, 2).is_err());
        run.open_treasure(102, 2).unwrap();
        assert!(run.open_treasure(102, 2).is_err());
        for (seat, p) in run.roster.players().iter().enumerate() {
            if seat == 2 {
                assert!((42..=52).contains(&(p.gold - before[seat].0)));
                assert_eq!(p.rngs.rewards.counter, before[seat].1 + 1);
            } else {
                assert_eq!((p.gold, p.rngs.rewards.counter), before[seat]);
            }
        }
    }

    #[test]
    fn spoils_map_only_pays_and_removes_the_marked_owners_quest() {
        let mut run = chest();
        let instance = run.roster.next_card_id + 1;
        let mut quest = crate::card::Card::from_factory(
            crate::tables::CardId::SpoilsMap, false, instance).unwrap();
        quest.owner = 2;
        run.roster.player_mut(2).unwrap().deck.push(quest);
        run.roster.next_card_id = instance;
        let mut marked = run.clone();
        let (col, row) = marked.world.run.coord.unwrap();
        marked.world.map.points.iter_mut().find(|p|
            p.coord == crate::run::Coord::new(col, row)).unwrap().spoils_quests.push(instance);
        let before = (run.roster.player(2).unwrap().gold,
            run.roster.player(2).unwrap().rngs.rewards.counter);
        run.open_treasure(102, 2).unwrap();
        assert!(run.roster.player(2).unwrap().deck.iter().any(|c| c.instance_id == instance));
        assert!((42..=52).contains(&(run.roster.player(2).unwrap().gold - before.0)));
        assert_eq!(run.roster.player(2).unwrap().rngs.rewards.counter, before.1 + 1);
        marked.open_treasure(102, 2).unwrap();
        assert_eq!((marked.roster.player(2).unwrap().gold,
            marked.roster.player(2).unwrap().rngs.rewards.counter),
            (run.roster.player(2).unwrap().gold+600,before.1+1));
        assert!(marked.treasure_opened[2]);
        assert!(!marked.roster.player(2).unwrap().deck.iter().any(|c|c.instance_id==instance));
    }

    #[test]
    fn treasure_entry_applies_room_hooks_to_their_owner_before_opening() {
        let mut run = chest();
        let mut bank = Relic::new(RelicId::MawBank);
        bank.owner = 2;
        run.roster.player_mut(2).unwrap().relics.push(bank);
        let before: Vec<_> = run.roster.players().iter().map(|p| p.gold).collect();
        run.enter_treasure().unwrap();
        for (seat, player) in run.roster.players().iter().enumerate() {
            assert_eq!(player.gold, before[seat] + if seat == 2 { 12 } else { 0 });
        }
        assert!(run.treasure_opened.iter().all(|opened| !opened));
    }
}
