//! RestSiteRoom.EnterInternal and RestSiteSynchronizer.ChooseOption: one
//! retained screen/completion source per owner, with a party exit barrier.
use super::{MpRun, MpPhase};
use crate::{card::Card, combat::RunCtx, tables::{MapPointType, PotionId, RelicId, RoomType}};
use super::super::rest;
#[path = "cook.rs"]
mod cook;

#[derive(Clone, Debug, Default)]
pub(super) struct SeatRest {
    options: Vec<(&'static str, bool)>,
    native_options: Vec<(rest::RestOptionKind, bool)>,
    smith: Option<Vec<u32>>,
    cook: Option<Vec<u32>>,
    mend: Option<Vec<usize>>,
    heal_pending: bool,
    heal_cards: Vec<Vec<Card>>,
    heal_potions: Vec<PotionId>,
    completed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{relic::Relic, run::mp::{Character, PlayerSpec}, tables::RelicId};

    fn party() -> MpRun {
        let specs: Vec<_> = (0..4).map(|seat| PlayerSpec {
            net_id: 100 + seat, character: Character::Ironclad,
        }).collect();
        MpRun::new("73003", &specs, 0).unwrap()
    }

    #[test]
    fn kindle_reactivates_only_the_owners_candle_and_finishes_the_option() {
        for owner in 0..4 {
            for initial in [0,2] {
                let mut run=party();
                for seat in 0..4 {
                    let mut candle=Relic::new(RelicId::PumpkinCandle);
                    candle.owner=seat as u8;candle.counter=initial;candle.enabled=initial>0;
                    run.roster.player_mut(seat).unwrap().relics.push(candle);
                }
                run.enter_rest_site().unwrap();
                let net=100+owner as u64;
                let index=run.rest_options(net,owner).unwrap().iter().position(|o|o.0=="KINDLE").unwrap();
                let rng=run.world.run.rngs.niche.counter;
                run.choose_rest_option(net,owner,index).unwrap();
                for seat in 0..4 {
                    let candle=run.roster.player(seat).unwrap().relics.iter().find(|r|r.id==RelicId::PumpkinCandle).unwrap();
                    assert_eq!(candle.counter,initial+if seat==owner {5}else{0});
                    assert_eq!(candle.enabled,seat==owner||initial>0);
                }
                assert!(run.rest_options(net,owner).unwrap().is_empty());
                assert_eq!(run.world.run.rngs.niche.counter,rng);
            }
        }
    }

    #[test]
    fn lift_and_dig_apply_only_to_the_rest_option_owner() {
        for (id,option) in [(RelicId::Girya,"LIFT"),(RelicId::Shovel,"DIG")] {
            let mut run=party();
            let mut relic=Relic::new(id);relic.owner=1;
            run.roster.player_mut(1).unwrap().relics.push(relic);
            run.enter_rest_site().unwrap();
            let index=run.rest_options(101,1).unwrap().iter().position(|o|o.0==option).unwrap();
            let before=run.roster.player(1).unwrap().relics.len();
            run.choose_rest_option(101,1,index).unwrap();
            if option=="LIFT" {
                assert_eq!(run.roster.player(1).unwrap().relics.iter().find(|r|r.id==id).unwrap().counter,1);
            } else {
                assert_eq!(run.roster.player(1).unwrap().relics.len(),before+1);
            }
            assert!(run.rest_options(101,1).unwrap().is_empty());
            assert_eq!(run.roster.player(0).unwrap().relics.len(),1);
        }
    }

    #[test]
    fn hatch_transforms_only_owners_eggs_and_finishes_the_option() {
        let mut run=party();
        let mut egg=crate::card::Card::from_factory(crate::tables::CardId::ByrdonisEgg,false,900).unwrap();
        egg.owner=1;
        run.roster.player_mut(1).unwrap().deck.push(egg);
        run.roster.next_card_id=900;
        run.enter_rest_site().unwrap();
        let index=run.rest_options(101,1).unwrap().iter().position(|o|o.0=="HATCH").unwrap();
        run.choose_rest_option(101,1,index).unwrap();
        let p=run.roster.player(1).unwrap();
        assert!(p.relics.iter().any(|r|r.id==RelicId::Byrdpip && r.owner==1));
        assert!(!p.deck.iter().any(|c|c.id==crate::tables::CardId::ByrdonisEgg));
        assert!(p.deck.iter().any(|c|c.id==crate::tables::CardId::ByrdSwoop && c.owner==1));
        assert!(run.rest_options(101,1).unwrap().is_empty());
        assert!(!run.roster.player(0).unwrap().relics.iter().any(|r|r.id==RelicId::Byrdpip));
    }

    #[test]
    fn rest_healing_and_room_entry_relic_hooks_are_owned_by_each_remote_seat() {
        for owner in 1..4 {
            let mut run = party();
            run.roster.player_mut(owner).unwrap().hp = 12;
            for id in [RelicId::VenerableTeaSet, RelicId::RegalPillow] {
                let mut relic = Relic::new(id);
                relic.owner = owner as u8;
                run.roster.player_mut(owner).unwrap().relics.push(relic);
            }
            run.enter_rest_site().unwrap();
            assert!(run.roster.player(owner).unwrap().relics.iter()
                .find(|r| r.id == RelicId::VenerableTeaSet).unwrap().flag1);
            assert_eq!(run.rest_options(100 + owner as u64, owner).unwrap()[2].0, "MEND");
            assert!(run.choose_rest_option(100, owner, 0).is_err());
            assert_eq!(run.roster.player(owner).unwrap().hp, 12);
            run.choose_rest_option(100 + owner as u64, owner, 0).unwrap();
            assert_eq!(run.roster.player(owner).unwrap().hp, 12 + 24 + 15);
            assert_eq!(run.phase, MpPhase::RestSite);
            for other in 0..4 {
                if other != owner { assert_eq!(run.roster.player(other).unwrap().hp, 80); }
                run.skip_rest_remaining(100 + other as u64, other).unwrap();
            }
            assert_eq!(run.phase, MpPhase::Map);
        }
    }

    #[test]
    fn smith_cancel_retains_the_option_and_selection_cannot_cross_owners() {
        let mut run = party();
        let owner = 2;
        run.enter_rest_site().unwrap();
        run.choose_rest_option(102, owner, 1).unwrap();
        let id = run.rest_smith_choices(102, owner).unwrap().unwrap()[0];
        assert!(run.select_rest_smith(100, owner, Some(id)).is_err());
        assert!(run.skip_rest_remaining(102, owner).is_err());
        run.select_rest_smith(102, owner, None).unwrap();
        assert!(run.rest_options(102, owner).unwrap()[1].1);
        run.choose_rest_option(102, owner, 1).unwrap();
        run.select_rest_smith(102, owner, Some(id)).unwrap();
        for (seat, player) in run.roster.players().iter().enumerate() {
            assert_eq!(player.deck.iter().filter(|c| c.upgraded).count(), usize::from(seat == owner));
            assert!(player.deck.iter().all(|c| c.owner as usize == seat));
        }
    }

    #[test]
    fn mend_heals_recipient_without_offering_heal_option_rewards() {
        let mut run = party();
        let mut relic = Relic::new(RelicId::DreamCatcher);
        relic.owner = 1;
        run.roster.player_mut(1).unwrap().relics.push(relic);
        run.roster.player_mut(1).unwrap().hp = 10;
        run.enter_rest_site().unwrap();
        let next_id = run.roster.next_card_id;
        run.choose_rest_option(100, 0, 2).unwrap();
        run.select_rest_mend(100, 0, Some(1)).unwrap();
        assert_eq!(run.roster.player(1).unwrap().hp, 34);
        assert_eq!(run.roster.next_card_id, next_id);
        assert!(run.rest_options(101, 1).unwrap()[0].1);
        assert!(run.rest_mend_targets(100, 0).unwrap().is_none());
        assert!(run.rest_card_reward(101,1).unwrap().is_none());
    }

    #[test]
    fn dream_catcher_retains_owner_rewards_until_selection_finishes() {
        // DreamCatcher.cs adds a three-card Monster reward; HealRestSiteOption
        // awaits its owner before completing the HEAL option.
        let mut run = party();
        let mut relic = Relic::new(RelicId::DreamCatcher);
        relic.owner = 1;
        run.roster.player_mut(1).unwrap().relics.push(relic);
        run.roster.player_mut(1).unwrap().hp = 10;
        run.enter_rest_site().unwrap();
        let next_id = run.roster.next_card_id;
        assert!(run.choose_rest_option(100, 1, 0).is_err());
        assert_eq!(run.roster.player(1).unwrap().hp, 10);
        assert_eq!(run.roster.next_card_id, next_id);
        let untouched = run.roster.player(0).unwrap().rngs.rewards.counter;
        run.choose_rest_option(101, 1, 0).unwrap();
        assert_eq!(run.roster.player(1).unwrap().hp, 34);
        let cards = run.rest_card_reward(101, 1).unwrap().unwrap().to_vec();
        assert_eq!(cards.len(), 3);
        assert!(cards.iter().all(|c| c.owner == 1));
        assert_eq!(run.roster.player(0).unwrap().rngs.rewards.counter, untouched);
        assert!(run.skip_rest_remaining(101, 1).is_err());
        assert!(run.choose_rest_option(101, 1, 0).is_err());
        let before = (run.roster.player(1).unwrap().deck.len(),
            run.roster.player(1).unwrap().rngs.rewards.counter, run.roster.next_card_id);
        assert!(run.select_rest_card_reward(100, 1, Some(0)).is_err());
        assert!(run.select_rest_card_reward(101, 1, Some(3)).is_err());
        assert_eq!((run.roster.player(1).unwrap().deck.len(),
            run.roster.player(1).unwrap().rngs.rewards.counter, run.roster.next_card_id), before);
        run.select_rest_card_reward(101, 1, Some(0)).unwrap();
        assert_eq!(run.roster.player(1).unwrap().deck.last().unwrap().id, cards[0].id);
        assert_eq!(run.roster.player(1).unwrap().rngs.rewards.counter, before.1);
        assert!(run.rest_options(101, 1).unwrap().is_empty());
        assert_eq!(run.phase, MpPhase::RestSite);
    }

    #[test]
    fn dream_catcher_with_frozen_egg_offers_upgraded_power_options_after_heal() {
        for seat in 0..4 {
            let mut run = party();
            for id in [RelicId::DreamCatcher, RelicId::FrozenEgg] {
                let mut relic = Relic::new(id);
                relic.owner = seat as u8;
                run.roster.player_mut(seat).unwrap().relics.push(relic);
            }
            run.roster.player_mut(seat).unwrap().hp = 10;
            run.enter_rest_site().unwrap();
            run.choose_rest_option(100+seat as u64,seat,0).unwrap();
            assert_eq!(run.roster.player(seat).unwrap().hp,34);
            let cards=run.rest_card_reward(100+seat as u64,seat).unwrap().unwrap();
            assert_eq!(cards.len(),3);
            assert!(cards.iter().filter(|c|c.card_type==crate::tables::CardType::Power).all(|c|c.upgraded));
            run.select_rest_card_reward(100+seat as u64,seat,None).unwrap();
            assert!(!run.rest_sites[seat].heal_pending);
        }
    }

    #[test]
    fn tiny_mailbox_keeps_full_inventory_rewards_and_allows_explicit_skips() {
        // TinyMailbox.cs appends two independent PotionRewards for its owner.
        let mut run = party();
        let mut relic = Relic::new(RelicId::TinyMailbox);
        relic.owner = 2;
        run.roster.player_mut(2).unwrap().relics.push(relic);
        run.enter_rest_site().unwrap();
        run.choose_rest_option(102, 2, 0).unwrap();
        assert_eq!(run.rest_sites[2].heal_potions.len(), 2);
        let potion = run.rest_potion_reward(102, 2).unwrap().unwrap();
        run.roster.player_mut(2).unwrap().potions.fill(Some(potion));
        let rng = run.roster.player(2).unwrap().rngs.rewards.counter;
        assert!(run.select_rest_potion_reward(102, 2, true).is_err());
        assert_eq!(run.rest_sites[2].heal_potions.len(), 2);
        assert_eq!(run.roster.player(2).unwrap().rngs.rewards.counter, rng);
        run.select_rest_potion_reward(102, 2, false).unwrap();
        let remaining = run.rest_potion_reward(102, 2).unwrap().unwrap();
        run.roster.player_mut(2).unwrap().potions[0] = None;
        run.select_rest_potion_reward(102, 2, true).unwrap();
        assert_eq!(run.roster.player(2).unwrap().potions[0], Some(remaining));
        assert!(run.rest_options(102, 2).unwrap().is_empty());
        assert_eq!(run.roster.player(2).unwrap().rngs.rewards.counter, rng);
    }

    #[test]
    fn mend_targets_only_other_players_and_heals_the_selected_owner() {
        // MendRestSiteOption.cs:68-143 targets AnyPlayer except the owner and
        // applies the target's normal rest-site heal amount.
        let mut run = party();
        run.roster.player_mut(2).unwrap().hp = 20;
        run.enter_rest_site().unwrap();
        run.choose_rest_option(101, 1, 2).unwrap();
        let targets = run.rest_mend_targets(101, 1).unwrap().unwrap().to_vec();
        assert!(!targets.contains(&1));
        assert!(targets.contains(&2));
        assert!(run.select_rest_mend(100, 1, Some(2)).is_err());
        run.select_rest_mend(101, 1, Some(2)).unwrap();
        assert_eq!(run.roster.player(2).unwrap().hp, 44);
        assert_eq!(run.roster.player(1).unwrap().hp, 80);
    }
}

impl MpRun {
    pub(super) fn rest_context(&self, seat: usize) -> RunCtx {
        let p = self.roster.player(seat).unwrap();
        let mut run = self.world.run.clone();
        run.hp = p.hp;
        run.max_hp = p.max_hp;
        run.gold = p.gold;
        run.deck = p.deck.clone();
        run.paels_tooth_cards=p.paels_tooth_cards.clone();
        for card in run.deck.iter_mut().chain(&mut run.paels_tooth_cards){card.owner=0;}
        run.max_energy=p.max_energy;
        run.max_potion_slots=p.max_potion_slots as i32;
        run.base_orb_slots=p.base_orb_slots as i32;
        run.potions=p.potions.clone();
        run.rngs.rewards=p.rngs.rewards.clone();
        run.rngs.shops=p.rngs.shops.clone();
        run.rngs.transformations=p.rngs.transformations.clone();
        run.relics = p.relics.clone();
        // Adapt the existing synchronous owner-zero hooks, not a solo manager.
        for relic in &mut run.relics { relic.owner = 0; }
        run.next_card_id = self.roster.next_card_id;
        run
    }

    pub(super) fn carry_rest_context(&mut self, seat: usize, mut run: RunCtx) {
        for relic in &mut run.relics { relic.owner = seat as u8; }
        for card in run.deck.iter_mut().chain(&mut run.paels_tooth_cards){card.owner=seat as u8;}
        let p = self.roster.player_mut(seat).unwrap();
        p.hp = run.hp;
        p.max_hp = run.max_hp;
        p.gold = run.gold;
        p.deck = run.deck;
        p.paels_tooth_cards=run.paels_tooth_cards;
        p.max_energy=run.max_energy;
        p.max_potion_slots=run.max_potion_slots as usize;
        p.base_orb_slots=run.base_orb_slots as usize;
        p.potions=run.potions;
        p.rngs.rewards=run.rngs.rewards;
        p.rngs.shops=run.rngs.shops;
        p.rngs.transformations=run.rngs.transformations;
        p.relics = run.relics;
        self.roster.next_card_id = run.next_card_id;
        self.world.run.next_card_id = run.next_card_id;
        self.world.run.ref_cache = run.ref_cache;
    }

    pub(super) fn enter_rest_site(&mut self) -> Result<(), &'static str> {
        self.world.run.room_type = RoomType::RestSite;
        self.world.current_room = None;
        self.rest_sites.clear();
        // BeginRestSite generates every owner's options BEFORE AfterRoomEntered.
        for seat in 0..self.roster.players().len() {
            let mut run = self.rest_context(seat);
            let mut state = rest::RestState::default();
            rest::generate(&mut state, &mut run);
            let mut options: Vec<_> = state.options.iter().map(|o| (o.kind.legacy_id(), o.enabled)).collect();
            let mut native_options: Vec<_> = state.options.iter().map(|o| (o.kind, o.enabled)).collect();
            // RestSiteOption.Generate adds Mend before card/relic modifiers.
            options.insert(2, ("MEND", true));
            native_options.insert(2, (rest::RestOptionKind::Mend, true));
            self.rest_sites.push(SeatRest { options, native_options, ..SeatRest::default() });
            self.carry_rest_context(seat, run);
        }
        for seat in 0..self.roster.players().len() {
            let mut run = self.rest_context(seat);
            rest::after_room_entered(&mut run, RoomType::RestSite, MapPointType::RestSite);
            self.carry_rest_context(seat, run);
        }
        self.phase = MpPhase::RestSite;
        Ok(())
    }

    pub fn rest_options(&self, net_id: u64, seat: usize) -> Result<&[(&'static str, bool)], &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::RestSite { return Err("not at rest site"); }
        Ok(&self.rest_sites[seat].options)
    }

    pub fn native_rest_options(&self, seat: usize) -> Option<&[(rest::RestOptionKind, bool)]> {
        self.rest_sites.get(seat).map(|state| state.native_options.as_slice())
    }
    pub fn native_rest_completed(&self, seat: usize) -> bool {
        self.rest_sites.get(seat).is_some_and(|state| state.completed)
    }
    pub fn native_rest_menu_available(&self, seat: usize) -> bool {
        self.rest_sites.get(seat).is_some_and(|state| !state.completed && state.smith.is_none()
            && state.mend.is_none() && !state.heal_pending)
    }

    pub fn rest_smith_choices(&self, net_id: u64, seat: usize) -> Result<Option<&[u32]>, &'static str> {
        self.roster.actor(net_id, seat)?;
        Ok(self.rest_sites.get(seat).and_then(|s| s.smith.as_deref()))
    }

    pub fn rest_card_reward(&self, net_id: u64, seat: usize) -> Result<Option<&[Card]>, &'static str> {
        self.roster.actor(net_id, seat)?;
        Ok(self.rest_sites.get(seat).and_then(|s| s.heal_cards.first()).map(Vec::as_slice))
    }

    pub fn rest_potion_reward(&self, net_id: u64, seat: usize) -> Result<Option<PotionId>, &'static str> {
        self.roster.actor(net_id, seat)?;
        Ok(self.rest_sites.get(seat).and_then(|s| s.heal_potions.first()).copied())
    }

    pub fn select_rest_card_reward(&mut self, net_id: u64, seat: usize, index: Option<usize>)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::RestSite { return Err("not at rest reward"); }
        let cards = self.rest_card_reward(net_id, seat)?.ok_or("no rest card reward")?;
        if index.is_some_and(|i| i >= cards.len()) { return Err("invalid rest reward card index"); }
        let mut next = self.clone();
        let cards = next.rest_sites[seat].heal_cards.remove(0);
        if let Some(index) = index {
            super::cards::add(&mut next.roster, &mut next.world.run, seat, cards[index].clone())?;
        }
        next.advance_rest_heal(seat);
        *self = next;
        Ok(())
    }

    pub fn select_rest_potion_reward(&mut self, net_id: u64, seat: usize, accept: bool)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::RestSite { return Err("not at rest reward"); }
        let potion = self.rest_potion_reward(net_id, seat)?.ok_or("no rest potion reward")?;
        let mut next = self.clone();
        if accept {
            let player = next.roster.player_mut(seat).unwrap();
            if !player.relics.iter().any(|r| r.id == RelicId::Sozu && r.enabled) {
                let slot = player.potions.iter_mut().find(|p| p.is_none())
                    .ok_or("rest potion reward inventory full")?;
                *slot = Some(potion);
                next.mark_local_potion_seen(seat, potion);
            }
        }
        next.rest_sites[seat].heal_potions.remove(0);
        next.advance_rest_heal(seat);
        *self = next;
        Ok(())
    }

    fn begin_rest_heal_rewards(&mut self, seat: usize) -> Result<(), &'static str> {
        // HealRestSiteOption.ExecuteRestSiteHeal invokes owner relic hooks in
        // order, then RewardsSet populates that list before sorting its UI.
        let relics: Vec<_> = self.roster.player(seat).unwrap().relics.iter()
            .filter(|r| r.enabled).map(|r| r.id).collect();
        self.rest_sites[seat].heal_pending = true;
        for relic in relics {
            match relic {
                RelicId::DreamCatcher => {
                    let ascension = self.world.run.ascension;
                    let act = self.world.run.act_index;
                    let mut next_id = self.roster.next_card_id;
                    let cards = super::rewards::generate_card_reward(
                        self.roster.player_mut(seat).unwrap(), seat as u8,
                        ascension, act, &mut next_id, 3, crate::tables::RoomType::Monster,&mut self.world.run.rngs.niche)?;
                    self.roster.next_card_id = next_id;
                    self.rest_sites[seat].heal_cards.push(cards);
                }
                RelicId::TinyMailbox => {
                    let potions = super::rewards::generate_potions(self.roster.player_mut(seat).unwrap(), 2)?;
                    self.rest_sites[seat].heal_potions.extend(potions);
                }
                _ => {}
            }
        }
        self.advance_rest_heal(seat);
        Ok(())
    }

    fn advance_rest_heal(&mut self, seat: usize) {
        let state = &mut self.rest_sites[seat];
        if state.heal_pending && state.heal_cards.is_empty() && state.heal_potions.is_empty() {
            state.heal_pending = false;
            self.finish_rest_choice(seat, rest::RestOptionKind::Heal);
        }
    }

    pub fn rest_mend_targets(&self, net_id: u64, seat: usize) -> Result<Option<&[usize]>, &'static str> {
        self.roster.actor(net_id, seat)?;
        Ok(self.rest_sites.get(seat).and_then(|s| s.mend.as_deref()))
    }

    pub fn choose_rest_option(&mut self, net_id: u64, seat: usize, index: usize) -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::RestSite { return Err("not at rest site"); }
        let state = &self.rest_sites[seat];
        if state.completed || state.smith.is_some() || state.cook.is_some() || state.mend.is_some() || state.heal_pending {
            return Err("rest choice unavailable");
        }
        let &(kind, enabled) = state.native_options.get(index).ok_or("invalid rest option")?;
        if !enabled { return Err("rest option disabled"); }
        // DreamCatcher creates a Monster-source CardReward, so CardFactory's
        // creation/option hooks and CardReward alternatives still apply at a
        // rest site. Only SilkenTress is implemented by the MP reward factory.
        // LastingCandy's early hook requires a combat room, unlike these hooks.
        // Refuse unsupported owner combinations before healing or drawing RNG.
        let relics = &self.roster.player(seat).unwrap().relics;
        if kind == rest::RestOptionKind::Heal && relics.iter().any(|r| r.enabled && r.id == RelicId::DreamCatcher)
            && relics.iter().any(|r| r.enabled && matches!(r.id,
                RelicId::DingyRug
                | RelicId::Glitter | RelicId::LavaLamp
                | RelicId::PaelsWing | RelicId::PrismaticGem | RelicId::SilverCrucible
                ))
        {
            return Err("unported multiplayer rest card reward modifier");
        }
        let mut next = self.clone();
        let mut run = next.rest_context(seat);
        match kind {
            rest::RestOptionKind::Cook => {
                let choices = rest::removable_indices(&run).into_iter()
                    .map(|i| run.deck[i].instance_id).collect::<Vec<_>>();
                if choices.len() < 2 { return Err("not enough removable cards to cook"); }
                next.rest_sites[seat].cook = Some(choices);
            }
            rest::RestOptionKind::Heal => {
                rest::execute_heal(&mut run);
                next.carry_rest_context(seat, run);
                next.begin_rest_heal_rewards(seat)?;
            }
            rest::RestOptionKind::Smith => {
                let choices = rest::upgradable_indices(&mut run).into_iter()
                    .map(|i| run.deck[i].instance_id).collect::<Vec<_>>();
                if choices.is_empty() { return Err("no upgradable rest card"); }
                next.carry_rest_context(seat, run);
                next.rest_sites[seat].smith = Some(choices);
            }
            rest::RestOptionKind::Lift => {
                if !rest::execute_lift(&mut run) {return Err("rest lift unavailable");}
                next.carry_rest_context(seat,run);
                next.finish_rest_choice(seat,rest::RestOptionKind::Lift);
            }
            rest::RestOptionKind::Kindle => {
                // KindleRestSiteOption.OnSelect -> Owner's PumpkinCandle.Rekindle.
                if !rest::execute_kindle(&mut run) {return Err("rest kindle unavailable");}
                next.carry_rest_context(seat,run);
                next.finish_rest_choice(seat,rest::RestOptionKind::Kindle);
            }
            rest::RestOptionKind::Dig => {
                let relic=next.pull_event_relic_from_front(seat)?;
                next.roster.obtain_inventory_relic(&mut next.world.run,seat,relic)?;
                next.record_relic_seen(seat,relic);
                next.finish_rest_choice(seat,rest::RestOptionKind::Dig);
            }
            rest::RestOptionKind::Mend => {
                let targets: Vec<_> = (0..next.roster.players().len())
                    .filter(|target| *target != seat && next.roster.player(*target).unwrap().hp > 0)
                    .collect();
                if targets.is_empty() { return Err("no multiplayer Mend target"); }
                next.rest_sites[seat].mend = Some(targets);
            }
            rest::RestOptionKind::Hatch => {
                // HatchRestSiteOption.OnSelect -> Byrdpip.AfterObtained.
                let mut relic=crate::relic::Relic::new(RelicId::Byrdpip);
                relic.owner=seat as u8;
                relic.floor_added_to_deck=next.world.run.total_floor;
                next.roster.player_mut(seat).unwrap().relics.push(relic);
                let eggs:Vec<_>=next.roster.player(seat).unwrap().deck.iter()
                    .filter(|c|c.id==crate::tables::CardId::ByrdonisEgg).map(|c|c.instance_id).collect();
                for egg in eggs {
                    next.roster.player_mut(seat).unwrap().retain_deck_cards(|c|c.instance_id!=egg);
                    next.roster.next_card_id+=1;
                    let mut card=crate::card::Card::from_factory(crate::tables::CardId::ByrdSwoop,
                        false,next.roster.next_card_id).ok_or("Byrd Swoop factory missing")?;
                    card.owner=seat as u8;
                    super::cards::add(&mut next.roster,&mut next.world.run,seat,card)?;
                }
                next.finish_rest_choice(seat,rest::RestOptionKind::Hatch);
            }
            rest::RestOptionKind::Clone => return Err("unported multiplayer rest option"),
        }
        *self = next;
        Ok(())
    }

    pub fn select_rest_mend(&mut self, net_id: u64, seat: usize, target: Option<usize>)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::RestSite { return Err("not at rest mend"); }
        let targets = self.rest_sites[seat].mend.as_ref().ok_or("rest mend selection missing")?;
        if target.is_some_and(|target| !targets.contains(&target)) {
            return Err("invalid rest mend target");
        }
        let mut next = self.clone();
        next.rest_sites[seat].mend = None;
        if let Some(target) = target {
            let mut run = next.rest_context(target);
            // MendRestSiteOption invokes Heal and AfterRestSiteHeal only.
            // ModifyRestSiteHealRewards belongs exclusively to HEAL's helper.
            rest::execute_heal(&mut run);
            next.carry_rest_context(target, run);
            next.finish_rest_choice(seat, rest::RestOptionKind::Mend);
        }
        *self = next;
        Ok(())
    }

    pub fn select_rest_smith(&mut self, net_id: u64, seat: usize, card_id: Option<u32>) -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::RestSite { return Err("not at rest smith"); }
        let choices = self.rest_sites[seat].smith.as_ref().ok_or("rest smith selection missing")?;
        if card_id.is_some_and(|id| !choices.contains(&id)) { return Err("invalid rest smith card"); }
        let mut next = self.clone();
        next.rest_sites[seat].smith = None;
        if let Some(id) = card_id {
            let mut run = next.rest_context(seat);
            let index = run.deck.iter().position(|c| c.instance_id == id).ok_or("rest smith card missing")?;
            run.upgrade_deck_card(index);
            next.carry_rest_context(seat, run);
            next.finish_rest_choice(seat, rest::RestOptionKind::Smith);
        }
        // A cancelled Smith.OnSelect returns false; retain the enabled option.
        *self = next;
        Ok(())
    }

    fn finish_rest_choice(&mut self, seat: usize, kind: rest::RestOptionKind) {
        let keep = rest::miniature_tent(&self.rest_context(seat));
        if keep {
            self.rest_sites[seat].options.retain(|(option, _)| *option != kind.legacy_id());
            self.rest_sites[seat].native_options.retain(|(candidate, _)| *candidate != kind);
        } else {
            self.rest_sites[seat].options.clear();
            self.rest_sites[seat].native_options.clear();
        }
        if self.rest_sites[seat].options.is_empty() { self.rest_sites[seat].completed = true; }
        self.advance_rest();
    }

    pub fn skip_rest_remaining(&mut self, net_id: u64, seat: usize) -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if !matches!(self.phase, MpPhase::RestSite | MpPhase::Map) ||
            self.world.run.room_type != RoomType::RestSite || self.rest_sites.is_empty() {
            return Err("not at rest site");
        }
        if self.rest_sites[seat].smith.is_some() { return Err("rest smith selection pending"); }
        if self.rest_sites[seat].cook.is_some() { return Err("rest cook selection pending"); }
        if self.rest_sites[seat].heal_pending { return Err("rest heal rewards pending"); }
        self.rest_sites[seat].options.clear();
        self.rest_sites[seat].native_options.clear();
        self.rest_sites[seat].completed = true;
        self.advance_rest();
        Ok(())
    }

    fn advance_rest(&mut self) {
        if self.rest_sites.iter().all(|s| s.completed) { self.finish_noncombat(); }
    }
}
