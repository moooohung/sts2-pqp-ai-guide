use super::*;
use crate::run::replay_view::*;

impl RunManager {
    pub fn checkpoint_generated_gold(&self)->&[i32] {&self.reward.replay_gold}
    /// Construction-prefix snapshot, before act/map generation and Neow's heal.
    /// Valid only before the first dispatched input; inventory/bags are borrowed.
    pub fn initialization_checkpoint_view(&self)->Result<ReplayCheckpointView<'_>,&'static str> {
        if self.native_decision_started {return Err("initialization view requires a fresh run");}
        let mut v=self.checkpoint_view();
        let r=&self.initial_run;
        v.run=r;v.combat=None;v.player_combat=None;v.room_loaded=false;v.event=None;v.map=None;
        v.room_model=None;v.act_floor=0;
        let p=&mut v.seats[0];
        p.hp=r.hp;p.max_hp=r.max_hp;p.gold=r.gold;p.deck=&r.deck;
        p.relics=r.relics.iter().collect();p.potions=&r.potions;
        p.potion_slots=r.max_potion_slots as usize;
        p.rng=ReplayPlayerRngs{rewards:&r.rngs.rewards,shops:&r.rngs.shops,transformations:&r.rngs.transformations};
        Ok(v)
    }
    /// Immutable offline replay view. Never use this hidden-state view as a
    /// policy observation; NativeEncoder remains the learned-input contract.
    pub fn checkpoint_view(&self) -> ReplayCheckpointView<'_> {
        let run=self.run_ctx();
        // The solo manager retains the previous combat arena after leaving it;
        // Capture observes the current room, not that retained allocation.
        let combat=self.combat.as_ref().filter(|_|matches!(run.room_type,
            RoomType::Monster|RoomType::Elite|RoomType::Boss));
        let event=if let Some(event)=self.current_event.as_ref().or(self.replay_event.as_ref())
            && run.room_type==RoomType::Event {
            Some(ReplayEventView {id:event.id,rng:event.rng.clone(),
                options:if self.phase==RunPhase::MapChoice {Vec::new()}else{event.options.iter().map(|o|ReplayEventOption {
                    identity:o.option_id,enabled:o.enabled,
                    offer:event.native_offer(o.option_id,&run.potions),
                }).collect()}})
        } else if run.total_floor==1 && run.room_type==RoomType::Event {
            // Neow's option-generation RNG is not retained by the solo manager.
            // Reuse its existing pure generator on a fresh local RNG, rather
            // than advancing any live stream or duplicating the generation.
            let (_,rng)=Self::generate_neow_event_for_party(run.rngs.seed,false);
            Some(ReplayEventView {id:crate::identity::EventId::Neow,rng,
                options:if self.phase==RunPhase::Event||self.native_reward_indices().next().is_some() {
                    self.neow.iter().map(|o|ReplayEventOption {
                        identity:o.option_id,enabled:true,
                        offer:Some(super::super::NativeEventOffer::Relic(o.relic)),
                    }).collect()
                }else{Vec::new()}})
        }else{None};
        let room_model=event.as_ref().map(|e|e.id.name())
            .or_else(||combat.and(self.encounter_id.map(crate::identity::EncounterId::name)));
        ReplayCheckpointView {
            run,combat,player_combat:self.combat.as_ref(),room_loaded:true,
            seats:vec![ReplaySeatView {
                character:self.native_character,
                hp:combat.map_or(run.hp,|c|c.cr(0).hp),
                max_hp:combat.map_or(run.max_hp,|c|c.cr(0).max_hp),
                gold:run.gold,deck:&run.deck,relics:run.relics.iter().collect(),
                potions:&run.potions,potion_slots:run.max_potion_slots.max(0) as usize,
                rng:ReplayPlayerRngs {rewards:&run.rngs.rewards,shops:&run.rngs.shops,
                    transformations:&run.rngs.transformations},
                card_rarity_odds:self.card_rarity_offset,potion_reward_odds:self.potion_reward_odds,
                relic_bag:&self.relic_bags.player[0],
            }],
            shared_relic_bag:&self.relic_bags.shared,
            initial_shared_auxiliary:initial_shared_auxiliary(run.rngs.seed),
            map:self.visible_map(),act_floor:self.act_floor,
            act_order:self.acts.iter().map(|a|a.def.name).collect(),
            room_model,event,
            shops:if self.phase==RunPhase::Shop {vec![self.native_shop_offers().collect()]}else{Vec::new()},
            rewards:ReplayRewards::Solo((0..self.native_reward_count())
                .filter_map(|i|self.native_reward(i)).collect()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn solo_checkpoint_retains_completed_event_and_generated_gold_without_advancing(){
        let mut game=RunManager::from_numeric_seed(7,RunOptions::default(),
            crate::run::mp::Character::Ironclad);
        game.replay_event=Some(EventState::new(crate::identity::EventId::Wellspring,Rng::new(91)));
        game.reward.replay_gold=vec![17];
        game.phase=RunPhase::MapChoice;
        let before=format!("{game:?}");
        let v=game.checkpoint_view();
        assert_eq!(v.room_model,Some("WELLSPRING"));
        assert_eq!(v.event.as_ref().unwrap().rng,Rng::new(91));
        assert!(v.event.as_ref().unwrap().options.is_empty());
        assert_eq!(game.checkpoint_generated_gold(),&[17]);
        assert_eq!(format!("{game:?}"),before);
    }
    #[test]
    fn solo_checkpoint_view_exposes_exact_private_state_without_mutation() {
        let mut game=RunManager::from_numeric_seed(7,RunOptions::default(),
            crate::run::mp::Character::Ironclad);
        game.card_rarity_offset=0.125;
        game.potion_reward_odds=0.7;
        game.current_event=Some(EventState::new(crate::identity::EventId::Wellspring,Rng::new(91)));
        let before=format!("{game:?}");
        for _ in 0..2 {
            let view=game.checkpoint_view();
            assert_eq!(view.seats[0].card_rarity_odds,game.card_rarity_offset);
            assert_eq!(view.seats[0].potion_reward_odds,game.potion_reward_odds);
            assert_eq!(view.shared_relic_bag,&game.relic_bags.shared);
            assert_eq!(view.seats[0].relic_bag,&game.relic_bags.player[0]);
            assert_eq!(view.event.as_ref().unwrap().rng,game.current_event.as_ref().unwrap().rng);
        }
        assert_eq!(format!("{game:?}"),before);
    }
}
