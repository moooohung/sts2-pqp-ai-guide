use super::*;

impl RunManager {
    pub(super) fn prepare_punch_off(&mut self) -> Result<(), &'static str> {
        self.prepare_event_combat(crate::identity::EncounterId::PunchOffEventEncounter)
    }

    pub(super) fn prepare_event_combat(&mut self,encounter:crate::identity::EncounterId) -> Result<(), &'static str> {
        // C# EventCombatSynchronizer.InitializeForEvent constructs the layout
        // and its monsters before options, even if the player chooses Nab.
        let seed = self.run_ctx().rngs.seed;
        let floor = self.run_ctx().total_floor;
        let run = self.take_run_from_owner();
        let mut combat = Combat::new_event_layout(run, seed, self.character);
        combat.set_character_pool(crate::combat::PLAYER, self.character)?;
        let mut rng = Rng::new(seed.wrapping_add(floor as u64)
            .wrapping_add(deterministic_hash64(encounter.name())));
        combat.setup_encounter_id(encounter, &mut rng);
        self.combat = Some(combat);
        Ok(())
    }

    pub(super) fn start_prepared_event_combat(&mut self,encounter:crate::identity::EncounterId)->Result<(), &'static str> {
        let combat=self.combat.as_mut().ok_or("event combat layout missing")?;
        let (hp,max_hp)=(combat.run.hp,combat.run.max_hp);
        let player=combat.cr_mut(crate::combat::PLAYER);
        player.max_hp=max_hp;player.hp=hp;
        combat.run.room_type=RoomType::Monster;
        combat.initialize_prepared_player_deck();
        combat.start_combat();
        self.encounter_id=Some(encounter);
        self.phase=RunPhase::Combat;
        Ok(())
    }

    pub(super) fn choose_punch_off(&mut self, mut event: EventState, option: EventOptionIdentity) -> Result<(), &'static str> {
        match option.id {
            crate::identity::EventOptionId::PunchOffInitialNab => {
                // PunchOff.Nab: curse enters the deck before RelicReward.Populate.
                self.add_card_to_deck(CardId::from_name("INJURY").ok_or("Injury missing")?)?;
                self.reward = RewardState::default();
                let relic = self.pull_player_relic(None, false);
                self.reward.items.push(RewardItem {
                    kind: RewardItemKind::Relic(relic), consumed: false, skippable: true,
                    skip_completes: true, add_injury_after: false, neows_bones_final: false,
                });
                self.reward.stay_in_container = true;
                self.phase = RunPhase::CardReward;
            }
            crate::identity::EventOptionId::PunchOffInitialICanTakeThem => {
                event.options.clear();
                event.options.set_page(crate::identity::EventPageId::PunchOffICanTakeThem,
                    super::super::events::native_identity::EventParameter::None);
                event.options.push(EventOption::new("fight", "Fight", "Fight for relic + potion"));
                self.current_event = Some(event);
            }
            crate::identity::EventOptionId::PunchOffICanTakeThemFight => {
                // EventCombatSynchronizer.EnterCombat reuses CombatStateForLayout;
                // it neither rolls a manager combat seed nor rebuilds deck copies.
                // C# CombatStateForLayout.AddPlayer retains the shared Player
                // creature. Our layout copied it: carry event-time potion/relic
                // HP changes back before combat hooks can read or overwrite them.
                self.start_prepared_event_combat(crate::identity::EncounterId::PunchOffEventEncounter)?;
                self.punch_off_combat = true;
                self.phase = RunPhase::Combat;
            }
            _ => return Err("invalid Punch Off option"),
        }
        Ok(())
    }

    pub(super) fn append_punch_off_rewards(&mut self) {
        // PunchOff.Fight supplies [RelicReward, PotionReward]. C# RewardsSet.
        // WithRewardsFromRoom appends these AFTER ordinary Monster rewards;
        // r7's suppress_default_rewards is not present in C#.
        let relic = self.pull_player_relic(None, false);
        self.reward.items.push(RewardItem {
            kind: RewardItemKind::Relic(relic), consumed: false, skippable: true,
            skip_completes: true, add_injury_after: false, neows_bones_final: false,
        });
        let Some(potion) = self.roll_potion() else { return };
        self.reward.items.push(RewardItem {
            kind: RewardItemKind::Potion(potion), consumed: false, skippable: true,
            skip_completes: true, add_injury_after: false, neows_bones_final: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deferred_event_deck_installs_enthralled_should_play_listener_for_both_events() {
        for (event,encounter,pages) in [
            (crate::identity::EventId::TheLanternKey,crate::identity::EncounterId::MysteriousKnightEventEncounter,["keep_key","fight"]),
            (crate::identity::EventId::PunchOff,crate::identity::EncounterId::PunchOffEventEncounter,["take_them","fight"]),
        ] {
            let mut mgr=RunManager::new(7,RunOptions::default());
            mgr.unported=None;
            mgr.run_ctx_mut().deck.clear();
            for (i,id) in [CardId::Enthralled,CardId::StrikeIronclad].into_iter().enumerate() {
                mgr.run_ctx_mut().deck.push(Card::from_factory(id,false,100+i as u32).unwrap());
            }
            let run=mgr.run_ctx();
            mgr.current_event=super::super::super::events::initialize(event,
                run.rngs.seed,run.gold,24,run.hp,run.max_hp);
            mgr.prepare_event_combat(encounter).unwrap();
            mgr.take_event_choice(pages[0]).unwrap();
            mgr.take_event_choice(pages[1]).unwrap();
            let c=mgr.combat.as_mut().unwrap();
            let enthralled=c.cards.iter().position(|card|card.id==CardId::Enthralled).unwrap() as crate::card::KRef;
            let strike=c.cards.iter().position(|card|card.id==CardId::StrikeIronclad).unwrap() as crate::card::KRef;
            c.players[0].hand.clear();c.players[0].hand.push(enthralled);c.players[0].hand.push(strike);
            assert!(!c.can_play_card(strike),"{event}: missing Enthralled listener");
            c.players[0].hand.remove(0);
            assert!(c.can_play_card(strike),"{event}: strike must become playable after curse leaves hand");
        }
    }

    #[test]
    fn returning_key_never_allocates_combat_cards_or_changes_the_next_generated_id() {
        let mut mgr=RunManager::new(7,RunOptions::default());
        mgr.unported=None;
        let run=mgr.run_ctx();
        mgr.current_event=super::super::super::events::initialize(crate::identity::EventId::TheLanternKey,
            run.rngs.seed,run.gold,24,run.hp,run.max_hp);
        let mut without_layout=mgr.clone();
        mgr.prepare_event_combat(crate::identity::EncounterId::MysteriousKnightEventEncounter).unwrap();
        mgr.take_event_choice("return_key").unwrap();
        without_layout.take_event_choice("return_key").unwrap();
        mgr.add_card_to_deck(CardId::Anger).unwrap();
        without_layout.add_card_to_deck(CardId::Anger).unwrap();
        assert_eq!(mgr.run_ctx().deck.last().unwrap().instance_id,
            without_layout.run_ctx().deck.last().unwrap().instance_id);
        assert_eq!(mgr.run_ctx().next_card_id,without_layout.run_ctx().next_card_id);
        assert!(mgr.combat.as_ref().unwrap().cards.is_empty());
    }

    #[test]
    fn lantern_layout_hp_and_arena_survive_pages_and_fight_without_any_new_draw() {
        let mut mgr=RunManager::new(7,RunOptions::default());
        mgr.unported=None;
        let run=mgr.run_ctx();
        mgr.current_event=super::super::super::events::initialize(crate::identity::EventId::TheLanternKey,
            run.rngs.seed,run.gold,24,run.hp,run.max_hp);
        let before=mgr.run_ctx().rngs.niche.counter;
        let next_card_id=mgr.run_ctx().next_card_id;
        let deck_len=mgr.run_ctx().deck.len();
        mgr.prepare_event_combat(crate::identity::EncounterId::MysteriousKnightEventEncounter).unwrap();
        let prepared=mgr.combat.as_ref().unwrap();
        let hp=prepared.cr(prepared.enemies[0]).hp;
        let monster=prepared.enemies[0];
        assert!(prepared.cards.is_empty());
        assert_eq!(prepared.run.next_card_id,next_card_id);
        let counter=prepared.run.rngs.niche.counter;
        assert_eq!(counter,before+1); // CombatState.AddMonster's unique HP.
        let manager_counter=mgr.manager_rng.counter;
        mgr.take_event_choice("keep_key").unwrap();
        assert_eq!(mgr.run_ctx().rngs.niche.counter,counter);
        mgr.take_event_choice("fight").unwrap();
        let combat=mgr.combat.as_ref().unwrap();
        assert_eq!(combat.enemies[0],monster);
        assert_eq!(combat.cr(monster).hp,hp);
        assert_eq!(combat.cards.len(),deck_len);
        assert_eq!(mgr.lantern_key_reward.as_ref().unwrap().instance_id,next_card_id+1);
        assert_eq!(combat.run.next_card_id,next_card_id+1+deck_len as u32);
        assert_eq!(combat.run.rngs.niche.counter,counter);
        assert_eq!(mgr.manager_rng.counter,manager_counter);
        assert_eq!(mgr.phase,RunPhase::Combat);
    }

    fn manager() -> RunManager {
        let mut mgr = RunManager::new(4100109, RunOptions::default());
        // The fixture jumps straight from construction to Punch Off, skipping
        // Neow; drop its pending EVENT:Neow marker so fail-closed reward
        // population (relic_nr::make_nr_card_reward) sees a clean run.
        mgr.unported = None;
        mgr.phase = RunPhase::Event;
        let run = mgr.run_ctx();
        mgr.current_event = super::super::super::events::initialize(crate::identity::EventId::PunchOff,
            run.rngs.seed, run.gold, 24, run.hp, run.max_hp);
        mgr.prepare_punch_off().unwrap();
        mgr
    }

    #[test]
    fn punch_off_layout_precedes_nab_and_nab_adds_injury_then_offers_relic() {
        let mut mgr = manager();
        let c = mgr.combat.as_ref().unwrap();
        let enemies = c.alive_enemies();
        assert_eq!(enemies.len(), 2);
        let first = c.cr(enemies[0]).ai.as_ref().unwrap();
        let second = c.cr(enemies[1]).ai.as_ref().unwrap();
        assert_eq!(first.state_id(first.current), "FAST_PUNCH_MOVE");
        // C# FAST has a multi-attack and a debuff intent, unlike r7 STRONG.
        assert_eq!(first.intents(first.current).len(), 2);
        assert_eq!(first.intents(first.current)[0].kind, IntentType::MultiAttack);
        assert_eq!(first.intents(first.current)[0].hits, 2);
        assert_eq!(first.intents(first.current)[1].kind, IntentType::Debuff);
        assert_eq!(second.state_id(second.current), "READY_MOVE");
        for e in enemies {
            let cr = c.cr(e);
            assert!((2..10).contains(&(cr.max_hp - cr.hp)));
        }
        let cards = mgr.run_ctx().deck.len();
        mgr.take_event_choice("nab").unwrap();
        assert_eq!(mgr.run_ctx().deck.len(), cards + 1);
        assert_eq!(mgr.run_ctx().deck.last().unwrap().id, CardId::from_name("INJURY").unwrap());
        assert_eq!(mgr.phase, RunPhase::CardReward);
        assert!(matches!(mgr.reward.items[0].kind, RewardItemKind::Relic(_)));
        mgr.skip_reward_set().unwrap();
        assert_eq!(mgr.phase, RunPhase::MapChoice);
    }

    #[test]
    fn punch_off_preserves_potion_management_and_inventory_across_pages() {
        let mut mgr=manager();
        // The event is never the floor-one Neow page.
        mgr.run_ctx_mut().total_floor=6;
        let potion=PotionId::Bloodpotion;
        mgr.run_ctx_mut().potions=vec![Some(potion),Some(PotionId::Energypotion)];
        mgr.run_ctx_mut().hp=20;
        let hp=mgr.run_ctx().max_hp;
        assert!(mgr.available_actions().iter().any(|a|a.kind==RunActionKind::UsePotion));
        mgr.take_event_choice("take_them").unwrap();
        assert_eq!(mgr.run_ctx().potions,vec![Some(potion),Some(PotionId::Energypotion)]);
        let drink=mgr.available_actions().into_iter().find(|a|
            a.kind==RunActionKind::UsePotion && a.slot_index==Some(0)).unwrap();
        mgr.take_action(drink).unwrap();
        assert_eq!(mgr.run_ctx().hp,20+hp/5);
        let discard=mgr.available_actions().into_iter().find(|a|
            a.kind==RunActionKind::DiscardPotion && a.slot_index==Some(1)).unwrap();
        mgr.take_action(discard).unwrap();
        assert!(mgr.run_ctx().potions.iter().all(Option::is_none));
        mgr.take_event_choice("fight").unwrap();
        assert_eq!(mgr.phase,RunPhase::Combat);
        assert!(mgr.run_ctx().potions.iter().all(Option::is_none));
    }

    #[test]
    fn punch_off_fight_reuses_layout_and_awards_default_plus_extra_rewards() {
        let mut mgr = manager();
        let ids = mgr.run_ctx().next_card_id;
        let deck_len=mgr.run_ctx().deck.len() as u32;
        let counter = mgr.manager_rng.counter;
        mgr.take_event_choice("take_them").unwrap();
        assert_eq!(mgr.phase, RunPhase::Event);
        assert_eq!(mgr.current_event.as_ref().unwrap().options.len(), 1);
        mgr.take_event_choice("fight").unwrap();
        assert_eq!(mgr.phase, RunPhase::Combat);
        assert_eq!(mgr.manager_rng.counter, counter);
        // PlayerCombatState initializes once on actual combat entry, not layout entry.
        assert_eq!(mgr.run_ctx().next_card_id, ids+deck_len);
        let c = mgr.combat.as_mut().unwrap();
        for e in c.alive_enemies() { c.kill_creature(e); }
        c.check_combat_end();
        mgr.after_combat_action().unwrap();
        assert_eq!(mgr.phase, RunPhase::CardReward);
        let card = mgr.reward.items.iter().position(|r| matches!(r.kind, RewardItemKind::Card {..})).unwrap();
        let relic = mgr.reward.items.iter().position(|r| matches!(r.kind, RewardItemKind::Relic(_))).unwrap();
        assert!(relic > card);
        assert!(matches!(mgr.reward.items[relic + 1].kind, RewardItemKind::Potion(_)));
        assert!(!mgr.punch_off_combat);
    }
}
