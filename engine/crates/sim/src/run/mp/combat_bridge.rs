//! One combat, all persistent seats. Shared world state is moved, never copied
//! into a separate solo manager per player.
use super::MpRoster;
use crate::{combat::{Combat, RunCtx}, identity::EncounterId, rng::Rng, tables::RoomType};

impl MpRoster {
    /// EventCombatSynchronizer reuses the layout's enemies, but its Player
    /// objects still refer to the live run inventory when combat begins.
    pub(super) fn refresh_prepared_combat_inventory(&self, c: &mut Combat) {
        for (seat,p) in self.players().iter().enumerate() {
            let owner=seat as u8;
            let cr=c.cr_mut(owner);
            cr.hp=p.hp;
            cr.max_hp=p.max_hp;
            c.players[owner].gold=Some(p.gold);
            if seat==0 {
                c.run.hp=p.hp;
                c.run.max_hp=p.max_hp;
                c.run.gold=p.gold;
                c.run.max_potion_slots=p.max_potion_slots as i32;
                c.run.potions=p.potions.clone();
            } else {
                c.players[owner].potions=Some(p.potions.clone());
            }
        }
    }

    pub(super) fn prepare_combat(&self, shared: RunCtx, combat_seed: u64,
        encounter: &str, room: RoomType) -> Result<Combat, &'static str> {
        let id=EncounterId::from_name(encounter).ok_or("unknown encounter")?;
        self.prepare_combat_id(shared,combat_seed,id,room)
    }

    pub(super) fn prepare_combat_id(&self, shared: RunCtx, combat_seed: u64,
        encounter: EncounterId, room: RoomType) -> Result<Combat, &'static str> {
        self.prepare_combat_view_id(shared,combat_seed,encounter,room,false)
    }

    pub(super) fn prepare_combat_view(&self, mut shared: RunCtx, combat_seed: u64,
        encounter: &str, room: RoomType, room_entry:bool) -> Result<Combat, &'static str> {
        let id=EncounterId::from_name(encounter).ok_or("unknown encounter")?;
        self.prepare_combat_view_id(shared,combat_seed,id,room,room_entry)
    }

    pub(super) fn prepare_combat_view_id(&self, mut shared: RunCtx, combat_seed: u64,
        encounter: EncounterId, room: RoomType, room_entry:bool) -> Result<Combat, &'static str> {
        let p0 = &self.players()[0];
        shared.hp = p0.hp;
        shared.max_hp = p0.max_hp;
        shared.gold = p0.gold;
        shared.max_energy = p0.max_energy;
        shared.max_potion_slots = p0.max_potion_slots as i32;
        shared.base_orb_slots = p0.base_orb_slots as i32;
        shared.deck = p0.deck.clone();
        shared.paels_tooth_cards=self.players().iter().flat_map(|p|p.paels_tooth_cards.clone()).collect();
        shared.relics = self.players().iter().flat_map(|p| p.relics.clone()).collect();
        shared.potions = p0.potions.clone();
        shared.next_card_id = self.next_card_id;
        shared.rngs.rewards = p0.rngs.rewards.clone();
        shared.rngs.shops = p0.rngs.shops.clone();
        shared.rngs.transformations = p0.rngs.transformations.clone();
        shared.room_type = room;
        let encounter_seed = shared.rngs.seed.wrapping_add(shared.total_floor as u64)
            .wrapping_add(crate::rng::deterministic_hash64(encounter.name()));
        let mut c = Combat::new(shared, combat_seed);
        c.replay_room_entry=room_entry;
        for p in &self.players()[1..] {
            c.add_player(p.max_hp, p.hp, p.max_energy, p.base_orb_slots as i32, &p.deck);
        }
        for (seat, p) in self.players().iter().enumerate() {
            let owner = seat as u8;
            c.players[owner].character = Some(p.character);
            c.set_character_pool(owner, p.character)?;
            c.players[owner].gold = Some(p.gold);
            if seat>0{c.players[owner].rewards_rng=Some(p.rngs.rewards.clone());}
            c.player_damage_dealt[seat] = p.extra_fields.damage_dealt;
            c.player_debuffs_applied[seat] = p.extra_fields.debuffs_applied;
            if seat > 0 { c.players[owner].potions = Some(p.potions.clone()); }
        }
        c.setup_encounter_id(encounter, &mut Rng::new(encounter_seed));
        if let Some(fault) = c.fault { return Err(fault); }
        Ok(c)
    }

    /// The caller owns shared run RNG/world progression. Only personal fields are
    /// projected into RunCtx; personal rewards/shop/transform streams stay per seat.
    pub fn enter_combat(&self, shared: RunCtx, combat_seed: u64,
        encounter: &str, room: RoomType) -> Result<Combat, &'static str> {
        let id=EncounterId::from_name(encounter).ok_or("unknown encounter")?;
        self.enter_combat_id(shared,combat_seed,id,room)
    }

    pub fn enter_combat_id(&self, shared: RunCtx, combat_seed: u64,
        encounter: EncounterId, room: RoomType) -> Result<Combat, &'static str> {
        let mut c = self.prepare_combat_id(shared, combat_seed, encounter, room)?;
        c.start_combat();
        if let Some(fault) = c.fault { return Err(fault); }
        Ok(c)
    }

    /// Commit permanent state after the engine has completed the original
    /// revival/after-combat hooks. Combat-generated cards remain arena-only.
    pub fn leave_combat(&mut self, c: Combat) -> Result<RunCtx, &'static str> {
        if c.players.len() != self.players().len() { return Err("multiplayer combat roster changed"); }
        if let Some(fault) = c.fault { return Err(fault); }
        if !c.is_over { return Err("multiplayer combat still running"); }
        for seat in 0..self.players().len() {
            let owner = seat as u8;
            let p = self.player_mut(seat).unwrap();
            p.hp = c.cr(owner).hp;
            p.extra_fields.damage_dealt = c.player_damage_dealt[seat];
            p.extra_fields.debuffs_applied = c.player_debuffs_applied[seat];
            p.max_hp = c.cr(owner).max_hp;
            p.max_energy = c.players[owner].base_max_energy;
            p.gold = if seat == 0 { c.run.gold } else {
                c.players[owner].gold.ok_or("multiplayer combat gold missing")?
            };
            p.potions = c.potion_slots_for_player(owner).ok_or("multiplayer combat potions missing")?.to_vec();
            p.max_potion_slots = if seat == 0 { c.run.max_potion_slots as usize } else { p.potions.len() };
            p.deck = c.player_deck(owner).to_vec();
            p.paels_tooth_cards=c.run.paels_tooth_cards.iter().filter(|card|card.owner==owner).cloned().collect();
            if seat>0{
                p.rngs.rewards=c.players[owner].rewards_rng.clone().ok_or("multiplayer owner rewards RNG missing")?;
            }
            p.relics = c.run.relics.iter().filter(|r| r.owner == owner).cloned().collect();
        }
        let p0 = self.player_mut(0).unwrap();
        p0.rngs.rewards = c.run.rngs.rewards.clone();
        p0.rngs.shops = c.run.rngs.shops.clone();
        p0.rngs.transformations = c.run.rngs.transformations.clone();
        self.next_card_id = c.run.next_card_id;
        Ok(c.run)
    }
}

#[cfg(test)]
mod prepared_inventory_tests {
    use super::*;
    use crate::{run::mp::{Character,MpRun,MpPhase,PlayerSpec},tables::PotionId};

    fn vote_all(run:&mut MpRun,option:&str) {
        for seat in 0..run.roster.players().len() {
            let net=run.roster.player(seat).unwrap().net_id;
            let index=run.event_options(net,seat).unwrap().iter().position(|&id|id==option).unwrap();
            run.choose_event_option(net,seat,index).unwrap();
        }
    }

    #[test]
    fn combat_layout_event_after_a_fight_has_no_stale_room_entry_encounter() {
        let specs:Vec<_>=(0..2).map(|seat|PlayerSpec{net_id:100+seat as u64,character:Character::Ironclad}).collect();
        let mut run=MpRun::new("73003",&specs,0).unwrap();
        run.enter_monster().unwrap();
        assert!(run.room_entry_checkpoint_combat().is_ok());
        run.phase=MpPhase::Event;run.world.current_room=Some(crate::identity::EventId::TheLanternKey.name());
        run.begin_generic_event(crate::identity::EventId::TheLanternKey).unwrap();
        assert!(run.combat().is_some());
        assert_eq!(run.world.current_encounter(), None);
        assert!(run.room_entry_checkpoint_combat().is_err());
    }

    #[test]
    fn event_combats_use_current_owner_potions_after_layout_or_page_changes() {
        for seats in 2..=4 {
            for id in [crate::identity::EventId::TheLanternKey,crate::identity::EventId::PunchOff,crate::identity::EventId::BattlewornDummy,crate::identity::EventId::DenseVegetation] {
                let specs:Vec<_>=(0..seats).map(|seat|PlayerSpec{
                    net_id:100+seat as u64,character:Character::Ironclad}).collect();
                let mut run=MpRun::new("73003",&specs,0).unwrap();
                for seat in 0..seats {
                    run.roster.player_mut(seat).unwrap().potions[0]=Some(PotionId::Firepotion);
                }
                run.phase=MpPhase::Event;run.world.current_room=Some(id.name());
                run.begin_generic_event(id).unwrap();
                let layout=run.combat().map(|c|c.enemies.iter().map(|&e|c.cr(e).hp).collect::<Vec<_>>());
                for seat in 0..seats {run.discard_potion(100+seat as u64,seat,0).unwrap();}
                match id {
                    crate::identity::EventId::TheLanternKey=>{vote_all(&mut run,"keep_key");vote_all(&mut run,"fight");}
                    crate::identity::EventId::PunchOff=>{vote_all(&mut run,"take_them");vote_all(&mut run,"fight");}
                    crate::identity::EventId::DenseVegetation=>{vote_all(&mut run,"rest");vote_all(&mut run,"fight");}
                    _=>{
                        let option=run.event_options(100,0).unwrap()[0];
                        vote_all(&mut run,option);
                    }
                }
                assert_eq!(run.phase,MpPhase::Combat,"{id}");
                let c=run.combat().unwrap();
                for seat in 0..seats {
                    assert!(c.potion_slots_for_player(seat as u8).unwrap()[0].is_none(),"{id}: owner {seat}");
                }
                if let Some(hp)=layout {
                    assert_eq!(c.enemies.iter().map(|&e|c.cr(e).hp).collect::<Vec<_>>(),hp,
                        "refresh must retain the prepared enemies");
                }
                run.debug_win_combat().unwrap();
                run.offer_rewards().unwrap();
                for seat in 0..seats {
                    assert!(run.roster.player(seat).unwrap().potions[0].is_none(),"{id}: carryback owner {seat}");
                }
            }
        }
    }
}
