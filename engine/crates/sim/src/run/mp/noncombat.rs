//! Original RunManager room dispatch, MerchantRoom.EnterInternal, and
//! RoomFullOfCheese.Gorge. No independent solo run managers or input policy.
use crate::identity::EventId;
use super::{MpRun, MpPhase};
use crate::{card::Card, run::Coord, tables::{MapPointType, RoomType, CardRarity, RelicId}};

impl MpRun {
    /// Read-only owner completion state for the exposed Cheese event menu.
    pub fn cheese_finished(&self, seat: usize) -> Option<bool> {
        self.event_done.get(seat).copied()
    }

    /// CombatRoom/TreasureRoom.EnterInternal invoke AfterRoomEntered before
    /// play begins. Planisphere keys off the map point, not the rolled room.
    pub(super) fn apply_room_entry_hooks(&mut self, room: RoomType) -> Result<(), &'static str> {
        // EnterRoomDebug can enter a room without any visited map coordinate.
        // In that case CurrentMapPoint is null; no Unknown-point relic fires.
        let point = if let Some((col,row))=self.world.run.coord {
            self.world.map.point(Coord::new(col,row)).ok_or("map destination missing")?.point_type
        } else {MapPointType::Unassigned};
        self.world.run.room_type = room;
        for seat in 0..self.roster.players().len() {
            let mut run = self.rest_context(seat);
            super::super::rest::after_room_entered(&mut run, room, point);
            self.carry_rest_context(seat, run);
        }
        Ok(())
    }

    pub(super) fn enter_noncombat(&mut self, kind: MapPointType, coord: Coord) -> Result<(), &'static str> {
        if kind == MapPointType::RestSite { return self.enter_rest_site(); }
        if kind == MapPointType::Treasure { return self.enter_treasure(); }
        let room = if kind == MapPointType::Shop { RoomType::Shop } else {
            // UnknownMapPointOdds.Roll applies party-wide room-type hooks
            // before both the roll and the unrolled-odds increments.
            // GoldenCompass.cs:45-54 restricts the allowed set to Event only
            // in the acquisition act; Roll still consumes its normal draw.
            let golden=self.roster.players().iter().flat_map(|p| &p.relics)
                .any(|r|r.id==RelicId::GoldenCompass&&r.flag1&&r.counter2==self.world.run.act_index);
            let point = self.world.map.point(coord).ok_or("map destination missing")?;
            let no_shop = self.world.run.room_type == RoomType::Shop ||
                (!point.children.is_empty() && point.children.iter().all(|&i|
                    self.world.map.points[i as usize].point_type == MapPointType::Shop));
            // JuzuBracelet.ModifyUnknownMapPointRoomTypes removes Monster
            // for the shared run even when the relic belongs to a remote seat.
            let no_monster = self.roster.players().iter().flat_map(|p| &p.relics)
                .any(|r| r.id == RelicId::JuzuBracelet && r.enabled);
            let allowed = [!golden&&!no_monster, !golden, !golden, !golden&&!no_shop];
            let types = [RoomType::Monster, RoomType::Elite, RoomType::Treasure, RoomType::Shop];
            let base = [0.1f32, -1.0, 0.02, 0.03];
            let roll = self.world.run.rngs.unknown_map_point.next_float(1.0) as f32;
            let mut cumulative = 0.0f32;
            let mut room = RoomType::Event;
            for i in 0..4 {
                if !allowed[i] || self.unknown_odds[i] < 0.0 { continue; }
                cumulative += self.unknown_odds[i];
                if roll <= cumulative { room = types[i]; break; }
            }
            for i in 0..4 {
                if room == types[i] { self.unknown_odds[i] = base[i]; }
                else if allowed[i] { self.unknown_odds[i] += base[i]; }
            }
            room
        };
        match room {
            RoomType::Monster => self.enter_monster()?,
            RoomType::Elite => self.enter_elite()?,
            RoomType::Treasure => return self.enter_treasure(),
            RoomType::Shop => {
                self.enter_merchants()?;
                // MerchantRoom.EnterInternal invokes hooks after every
                // owner's inventory has been generated.
                self.apply_room_entry_hooks(RoomType::Shop)?;
                self.world.current_room = None;
                self.phase = MpPhase::Shop;
            }
            RoomType::Event => {
                // C# RoomSet.EnsureNextEventIsValid:104-116 advances until
                // NextEvent.IsAllowed. EventModel overrides are party predicates.
                loop {
                    let Some(&candidate) = self.world.encounters.events.get(self.event_cursor) else {
                        return Err("events exhausted");
                    };
                    if self.event_is_allowed(candidate) { break; }
                    self.event_cursor += 1;
                }
                let id = *self.world.encounters.events.get(self.event_cursor).ok_or("events exhausted")?;
                self.event_cursor += 1;
                self.event_done.fill(false);
                self.cheese_cards.fill(None);
                self.event_potion_rewards.fill(None);
                self.world.current_room = Some(id.name());
                self.native_event = Some(id);
                self.phase = MpPhase::Event;
                if id == EventId::SlipperyBridge { self.begin_bridge()?; }
                if id == EventId::Symbiote { self.begin_symbiote()?; }
                if id == EventId::WhisperingHollow { self.begin_whispering_hollow()?; }
                if !matches!(id, EventId::RoomFullOfCheese | EventId::SlipperyBridge | EventId::Symbiote | EventId::WhisperingHollow) {
                    self.begin_generic_event(id)?;
                }
                // C# Rooms/EventRoom.cs EventRoom.EnterInternal calls
                // Hook.AfterRoomEntered after BeginEvent, so Unknown-point
                // relics such as Planisphere also trigger on event rooms.
                self.apply_room_entry_hooks(RoomType::Event)?;
            }
            _ => return Err("unported multiplayer unknown-room outcome"),
        }
        self.world.run.room_type = room;
        Ok(())
    }

    pub fn leave_shop(&mut self, net_id: u64, seat: usize) -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Shop { return Err("not in merchant room"); }
        if self.merchant_pickups.iter().any(Option::is_some) {return Err("merchant pickup pending");}
        self.finish_noncombat();
        Ok(())
    }

    pub(super) fn finish_noncombat(&mut self) {
        // EventRoom exit discards CombatStateForLayout, not live player state.
        if self.phase == MpPhase::Event { self.combat = None; }
        self.phase = MpPhase::Map;
        let (col, row) = self.world.run.coord.unwrap();
        self.voting.on_location_changed(self.world.run.act_index as usize, Coord::new(col, row));
    }

    pub fn choose_cheese(&mut self, net_id: u64, seat: usize, option: usize) -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Event || self.world.current_room != Some("ROOM_FULL_OF_CHEESE") {
            return Err("not at Room Full of Cheese");
        }
        if self.event_done[seat] || self.cheese_cards[seat].is_some() { return Err("event choice already begun"); }
        if option>1 {return Err("invalid Cheese option");}
        let mut next = self.clone();
        if option==1 {
            // RoomFullOfCheese.Search: damage before obtaining ChosenCheese.
            let mut ctx=next.rest_context(seat);
            super::super::manager::slippery_bridge::damage_event_player(&mut ctx,14,true)?;
            next.carry_rest_context(seat,ctx);
            let mut relic=crate::relic::Relic::new(RelicId::ChosenCheese);
            relic.owner=seat as u8;relic.floor_added_to_deck=next.world.run.total_floor;
            next.roster.player_mut(seat).unwrap().relics.push(relic);
            next.record_relic_seen(seat,RelicId::ChosenCheese);
            next.event_done[seat]=true;
            if next.event_done.iter().all(|done|*done){next.finish_noncombat();}
            *self=next;
            return Ok(());
        }
        let mut next_id = next.roster.next_card_id;
        let player = next.roster.player_mut(seat).unwrap();
        let mut available: Vec<_> = super::super::card_pools::cards(player.character.into(), true)?
            .into_iter().filter(|id| crate::tables::card_info(*id, false)
                .is_some_and(|info| info.rarity == CardRarity::Common)).collect();
        let mut cards = Vec::new();
        for _ in 0..8.min(available.len()) {
            let id = available.remove(player.rngs.rewards.choice_index(available.len()));
            next_id += 1;
            let mut card = Card::from_factory(id, false, next_id).ok_or("event card factory missing")?;
            card.owner = seat as u8;
            cards.push(card);
        }
        next.roster.next_card_id = next_id;
        next.cheese_cards[seat] = Some(cards);
        *self = next;
        Ok(())
    }

    pub fn select_cheese_cards(&mut self, net_id: u64, seat: usize, indices: [usize; 2]) -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Event { return Err("not at event grid"); }
        let cards = self.cheese_cards[seat].as_ref().ok_or("event grid missing")?;
        if indices[0] == indices[1] || indices.iter().any(|&i| i >= cards.len()) { return Err("invalid event selection"); }
        let chosen = indices.map(|i| cards[i].clone());
        let mut next = self.clone();
        for card in chosen { super::cards::add(&mut next.roster, &mut next.world.run, seat, card)?; }
        next.cheese_cards[seat] = None;
        next.event_done[seat] = true;
        if next.event_done.iter().all(|done| *done) { next.finish_noncombat(); }
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn cheese_search_damages_then_grants_only_the_owners_relic_without_a_grid() {
        use crate::run::mp::{MpRun,MpPhase,Character,PlayerSpec};
        use crate::tables::RelicId;
        for seats in 2..=4 {for owner in 0..seats {for hp in [10,50] {
            let specs:Vec<_>=(0..seats).map(|s|PlayerSpec{net_id:100+s as u64,character:Character::Ironclad}).collect();
            let mut run=MpRun::new("930601",&specs,0).unwrap();
            run.phase=MpPhase::Event;run.world.current_room=Some("ROOM_FULL_OF_CHEESE");
            run.roster.player_mut(owner).unwrap().hp=hp;
            run.choose_cheese(100+owner as u64,owner,1).unwrap();
            assert_eq!(run.roster.player(owner).unwrap().hp,(hp-14).max(1));
            assert!(run.roster.player(owner).unwrap().relics.iter().any(|r|r.id==RelicId::ChosenCheese&&r.owner as usize==owner));
            assert!(run.cheese_cards[owner].is_none());
            assert!(run.event_done[owner]);
            for seat in 0..seats {if seat!=owner {
                assert_eq!(run.roster.player(seat).unwrap().relics.len(),1);
                assert!(!run.event_done[seat]);
            }}
        }}}
    }
    use super::*;
    use crate::run::mp::{Character, PlayerSpec};

    #[test]
    fn symbiote_is_skipped_only_in_the_first_act_before_selecting_an_event() {
        let specs = [PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Necrobinder }];
        for act in [0, 1] {
            let mut run = MpRun::new("73001", &specs, 0).unwrap();
            run.event_cursor = 0;
            run.world.run.act_index = act;
            run.world.encounters.events = vec![EventId::Symbiote, EventId::RoomFullOfCheese];
            run.unknown_odds = [-1.0; 4];
            let coord = run.world.map.points[run.world.map.start as usize].coord;
            let result = run.enter_noncombat(MapPointType::Unknown, coord);
            if act == 0 {
                result.unwrap();
                assert_eq!(run.world.current_room, Some("ROOM_FULL_OF_CHEESE"));
                assert_eq!(run.event_cursor, 2);
            } else {
                result.unwrap();
                assert_eq!(run.world.current_room, Some("SYMBIOTE"));
                assert_eq!(run.event_cursor, 1);
                assert!(run.events.iter().all(Option::is_some));
            }
        }
    }

    #[test]
    fn slippery_bridge_ineligibility_advances_to_the_next_event() {
        let specs = [PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Necrobinder }];
        for no_removable_card in [false, true] {
            let mut run = MpRun::new("73001", &specs, 0).unwrap();
            run.event_cursor = 0;
            run.world.run.act_index = 1;
            run.world.run.total_floor = if no_removable_card { 12 } else { 6 };
            if no_removable_card {
                run.roster.player_mut(1).unwrap().deck.clear();
            }
            run.world.encounters.events = vec![EventId::SlipperyBridge, EventId::RoomFullOfCheese];
            run.unknown_odds = [-1.0; 4];
            let coord = run.world.map.points[run.world.map.start as usize].coord;
            run.enter_noncombat(MapPointType::Unknown, coord).unwrap();
            assert_eq!(run.world.current_room, Some("ROOM_FULL_OF_CHEESE"));
            assert_eq!(run.event_cursor, 2);
            assert!(run.events.iter().all(Option::is_none));
        }
    }

    #[test]
    fn dense_vegetation_ineligibility_advances_to_the_next_event() {
        let specs = [PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Necrobinder }];
        let mut run = MpRun::new("73001", &specs, 0).unwrap();
        run.event_cursor = 0;
        run.roster.player_mut(1).unwrap().hp = 8;
        run.world.encounters.events = vec![EventId::DenseVegetation, EventId::RoomFullOfCheese];
        run.unknown_odds = [-1.0; 4];
        let coord = run.world.map.points[run.world.map.start as usize].coord;
        run.enter_noncombat(MapPointType::Unknown, coord).unwrap();
        assert_eq!(run.world.current_room, Some("ROOM_FULL_OF_CHEESE"));
        assert_eq!(run.event_cursor, 2);
        assert!(run.events.iter().all(Option::is_none));
    }

    #[test]
    fn neow_advances_the_regular_event_cursor() {
        let specs = [PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Necrobinder }];
        let mut run = MpRun::new("73001", &specs, 0).unwrap();
        assert_eq!(run.event_cursor, 1);
        run.world.encounters.events = vec![EventId::DenseVegetation, EventId::RoomFullOfCheese];
        run.unknown_odds = [-1.0; 4];
        let coord = run.world.map.points[run.world.map.start as usize].coord;
        run.enter_noncombat(MapPointType::Unknown, coord).unwrap();
        assert_eq!(run.world.current_room, Some("ROOM_FULL_OF_CHEESE"));
        assert_eq!(run.event_cursor, 2);
    }

    #[test]
    fn unknown_monster_and_treasure_use_the_same_room_entry_as_fixed_points() {
        let specs = [PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Necrobinder }];
        for room in [RoomType::Monster, RoomType::Treasure] {
            let mut run = MpRun::new("73003", &specs, 0).unwrap();
            let start = run.world.map.start as usize;
            run.world.map.points[start].point_type = MapPointType::Unknown;
            let owner = run.roster.player_mut(1).unwrap();
            owner.hp = 20;
            let mut relic = crate::relic::Relic::new(RelicId::Planisphere);
            relic.owner = 1;
            owner.relics.push(relic);
            let mut fixed = run.clone();
            run.unknown_odds = if room == RoomType::Monster {
                [1.0, -1.0, -1.0, -1.0]
            } else { [-1.0, -1.0, 1.0, -1.0] };
            let coord = run.world.map.points[run.world.map.start as usize].coord;
            let before = run.world.run.rngs.unknown_map_point.counter;
            run.enter_noncombat(MapPointType::Unknown, coord).unwrap();
            if room == RoomType::Monster { fixed.enter_monster().unwrap(); }
            else { fixed.enter_treasure().unwrap(); }
            assert_eq!(run.world.run.rngs.unknown_map_point.counter, before + 1);
            assert_eq!(run.phase, fixed.phase);
            assert_eq!(run.world.current_room, fixed.world.current_room);
            assert_eq!(run.world.run.room_type, room);
            assert_eq!(run.roster.player(1).unwrap().hp, 25);
            assert_eq!(run.roster.player(0).unwrap().hp, fixed.roster.player(0).unwrap().hp);
            if room == RoomType::Monster {
                assert_eq!(run.world.current_room, run.world.encounters.normal.first().map(|id| id.name()));
            }
            assert_eq!(run.treasure_relics, fixed.treasure_relics);
            assert_eq!(run.world.shared_bags, fixed.world.shared_bags);
            assert_eq!(run.world.run.rngs.treasure_room.counter, fixed.world.run.rngs.treasure_room.counter);
        }
    }

    #[test]
    fn merchant_entry_meal_ticket_heals_only_its_living_owner() {
        let specs = [PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Necrobinder }];
        for seat in 0..2 {
            for kind in [MapPointType::Shop, MapPointType::Unknown] {
                for hp in [0, 20] {
                    let mut run = MpRun::new("73003", &specs, 0).unwrap();
                    let start = run.world.map.start as usize;
                    run.world.map.points[start].point_type = kind;
                    let coord = run.world.map.points[start].coord;
                    run.world.run.coord = Some((coord.col, coord.row));
                    run.unknown_odds = [-1.0, -1.0, -1.0, 1.0];
                    let other_hp = run.roster.player(1 - seat).unwrap().hp;
                    let owner = run.roster.player_mut(seat).unwrap();
                    owner.hp = hp;
                    let mut relic = crate::relic::Relic::new(RelicId::MealTicket);
                    relic.owner = seat as u8;
                    owner.relics.push(relic);
                    run.enter_noncombat(kind, coord).unwrap();
                    assert_eq!(run.phase, MpPhase::Shop);
                    // MealTicket.AfterRoomEntered heals 15 only when alive.
                    assert_eq!(run.roster.player(seat).unwrap().hp, if hp == 0 { 0 } else { 35 });
                    assert_eq!(run.roster.player(1 - seat).unwrap().hp, other_hp);
                    assert_eq!(run.shops.len(), specs.len());
                }
            }
        }
    }

    #[test]
    fn split_votes_dispatch_the_winning_coordinate_not_the_last_voters_room() {
        let specs = [PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Necrobinder }];
        let mut nonlast_winners = 0;
        for seed in 0..8 {
            let mut run = MpRun::new("73003", &specs, 0).unwrap();
            let source = run.world.map.points.iter().find(|p| p.children.len() >= 2).unwrap().clone();
            let first = source.children[0] as usize;
            let second = source.children[1] as usize;
            run.world.map.points[first].point_type = MapPointType::Monster;
            run.world.map.points[second].point_type = MapPointType::Shop;
            let monster = run.world.map.points[first].coord;
            let shop = run.world.map.points[second].coord;
            run.world.run.coord = Some((source.coord.col, source.coord.row));
            run.phase = MpPhase::Map;
            run.voting.on_location_changed(0, source.coord);
            run.voting.rng = crate::rng::Rng::new(seed);
            run.vote(7, 0, monster).unwrap();
            assert_eq!(run.phase, MpPhase::Map);
            run.vote(42, 1, shop).unwrap();
            if run.world.run.coord == Some((monster.col, monster.row)) {
                nonlast_winners += 1;
                assert_eq!(run.phase, MpPhase::Combat);
                assert_eq!(run.combat().unwrap().run.room_type, RoomType::Monster);
            } else {
                assert_eq!(run.world.run.coord, Some((shop.col, shop.row)));
                assert_eq!(run.phase, MpPhase::Shop);
                assert!(run.combat().is_none());
            }
        }
        assert!(nonlast_winners > 0, "exercise a winner different from the final voter");
    }

    #[test]
    fn cheese_grid_is_owned_and_acquisition_is_atomic_for_remote_seats() {
        let specs: Vec<_> = (1..=4).map(|net_id| PlayerSpec { net_id,
            character: Character::Ironclad }).collect();
        for seat in 1..4 {
            let mut run = MpRun::new("73001", &specs, 0).unwrap();
            run.phase = MpPhase::Event;
            run.world.current_room = Some("ROOM_FULL_OF_CHEESE");
            let initial: Vec<_> = run.roster.players().iter().map(|p|
                (p.deck.len(), p.rngs.rewards.counter)).collect();
            assert!(run.choose_cheese(1, seat, 0).is_err());
            assert_eq!(run.roster.player(seat).unwrap().rngs.rewards.counter, initial[seat].1);
            run.choose_cheese(specs[seat].net_id, seat, 0).unwrap();
            let before = run.roster.next_card_id;
            assert_eq!(run.cheese_cards[seat].as_ref().unwrap().len(), 8);
            assert!(run.select_cheese_cards(1, seat, [0, 1]).is_err());
            assert!(run.select_cheese_cards(specs[seat].net_id, seat, [0, 0]).is_err());
            assert_eq!(run.roster.next_card_id, before);
            run.select_cheese_cards(specs[seat].net_id, seat, [0, 1]).unwrap();
            assert!(run.select_cheese_cards(specs[seat].net_id, seat, [0, 1]).is_err());
            for (other, player) in run.roster.players().iter().enumerate() {
                assert_eq!(player.deck.len(), initial[other].0 + if other == seat { 2 } else { 0 });
                if other != seat { assert_eq!(player.rngs.rewards.counter, initial[other].1); }
                assert!(player.deck.iter().all(|card| card.owner as usize == other));
            }
        }
    }

    #[test]
    fn remote_juzu_excludes_monsters_without_advancing_their_odds() {
        // JuzuBracelet.cs and UnknownMapPointOdds.Roll: excluded room types
        // neither roll nor gain their base odds; the shared RNG still advances.
        let specs = [PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Necrobinder }];
        let mut run = MpRun::new("32767", &specs, 0).unwrap();
        let mut relic = crate::relic::Relic::new(RelicId::JuzuBracelet);
        relic.owner = 1;
        run.roster.player_mut(1).unwrap().relics.push(relic);
        run.world.encounters.events = vec![EventId::Neow, EventId::RoomFullOfCheese];
        run.unknown_odds = [1.0, -1.0, -1.0, -1.0];
        let before = run.world.run.rngs.unknown_map_point.counter;
        let coord = run.world.map.points[run.world.map.start as usize].coord;
        run.enter_noncombat(MapPointType::Unknown, coord).unwrap();
        assert_eq!(run.world.current_room, Some("ROOM_FULL_OF_CHEESE"));
        assert_eq!(run.unknown_odds[0], 1.0);
        assert_eq!(run.world.run.rngs.unknown_map_point.counter, before + 1);
        assert_eq!(run.phase, MpPhase::Event);
    }

    #[test]
    fn shop_navigation_requires_the_bound_actor_and_room_phase() {
        let specs = [PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Necrobinder }];
        let mut run = MpRun::new("73001", &specs, 0).unwrap();
        assert!(run.leave_shop(42, 1).is_err());
        run.enter_noncombat(MapPointType::Shop, Coord::new(0, 0)).unwrap();
        assert_eq!(run.shops.len(), 2);
        assert!(run.leave_shop(7, 1).is_err());
        assert_eq!(run.phase, MpPhase::Shop);
        run.leave_shop(42, 1).unwrap();
        assert_eq!(run.phase, MpPhase::Map);
    }
}
