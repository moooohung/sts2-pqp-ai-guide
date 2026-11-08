use super::*;
use crate::run::replay_view::*;

impl MpRun {
    /// Before act/map generation and Neow. Other seat inventories are borrowed
    /// only while the party is fresh; Neow's later HP reduction is not initial HP.
    pub fn initialization_checkpoint_view(&self,local:usize)->Result<ReplayCheckpointView<'_>,&'static str> {
        if self.phase!=MpPhase::Neow||self.neow_done.iter().any(|done|*done) {
            return Err("initialization view requires a fresh party");
        }
        let mut v=self.checkpoint_view(local)?;
        v.run=&self.world.initial_run;v.combat=None;v.player_combat=None;v.room_loaded=false;
        v.event=None;v.map=None;v.room_model=None;v.act_floor=0;
        for p in &mut v.seats {p.hp=p.max_hp;}
        Ok(v)
    }
    /// Room-entry checkpoint before opening-turn hooks/dealing. Re-executes
    /// the shared native setup prefix on copies of the retained pre-combat
    /// world/roster, never on live RNGs. Call at room entry, before any input.
    pub fn room_entry_checkpoint_combat(&self)->Result<crate::combat::Combat,&'static str> {
        let live=self.combat.as_ref().ok_or("combat missing")?;
        let encounter=self.world.current_encounter().ok_or("encounter missing")?;
        let mut copy=self.roster.prepare_combat_view_id(self.world.run.clone(),live.rng.base_seed,
            encounter,self.world.run.room_type,true)?;
        copy.prepare_combat_start();
        Ok(copy)
    }
    /// Immutable, actor-independent offline inspection of this shared party.
    /// The subscriber seat affects only which event/shop the game projected.
    pub fn checkpoint_view(&self, local_seat:usize) -> Result<ReplayCheckpointView<'_>,&'static str> {
        if local_seat>=self.roster.players().len() {return Err("invalid replay local seat");}
        let combat=self.combat.as_ref();
        let run=combat.map_or(&self.world.run,|c|&c.run);
        let seats=self.roster.players().iter().enumerate().map(|(seat,p)| {
            let owner=seat as u8;
            ReplaySeatView {
                character:p.character,
                hp:combat.map_or(p.hp,|c|c.cr(owner).hp),
                max_hp:combat.map_or(p.max_hp,|c|c.cr(owner).max_hp),
                gold:combat.map_or(p.gold,|c|if seat==0 {c.run.gold}
                    else{c.players[owner].gold.expect("attached multiplayer player gold")}),
                deck:combat.map_or(p.deck.as_slice(),|c|c.player_deck(owner)),
                relics:combat.map_or_else(||p.relics.iter().collect(),
                    |c|c.run.relics.iter().filter(|r|r.owner==owner).collect()),
                potions:combat.map_or(p.potions.as_slice(),
                    |c|c.potion_slots_for_player(owner).expect("attached multiplayer potion slots")),
                potion_slots:p.max_potion_slots,
                rng:ReplayPlayerRngs{rewards:&p.rngs.rewards,shops:&p.rngs.shops,
                    transformations:&p.rngs.transformations},
                card_rarity_odds:p.card_rarity_offset,potion_reward_odds:p.potion_reward_odds,
                relic_bag:&self.world.player_bags[seat],
            }
        }).collect();
        let event=if self.world.current_room==Some("NEOW") {
            Some(ReplayEventView {id:crate::identity::EventId::Neow,
                rng:self.neow_rngs[local_seat].clone(),
                // Original EventOption.Chosen awaits its pickup selector before
                // clearing CurrentOptions. Native completion flags are eager;
                // retained pickup state identifies that suspended interval.
                options:if self.neow_done[local_seat]&&!self.owner_pickup_pending(local_seat) {Vec::new()}else{
                    self.neow[local_seat].iter().enumerate().map(|(slot,&relic)|ReplayEventOption {
                        identity:crate::run::EventOptionIdentity::neow(slot as u16),
                        enabled:true,offer:Some(crate::run::NativeEventOffer::Relic(relic)),
                    }).collect()
                }})
        }else if run.room_type==RoomType::Event {self.events[local_seat].as_ref().map(|event|ReplayEventView {
            id:event.id,rng:event.rng.clone(),
            options:event.options.iter().map(|o|ReplayEventOption {
                identity:o.option_id,enabled:o.enabled,
                offer:event.native_offer(o.option_id,&self.roster.players()[local_seat].potions),
            }).collect(),
        })}else{None};
        let shops=if self.phase==MpPhase::Shop {
            (0..self.roster.players().len()).map(|seat|
                self.native_shop_offers(seat).map(|offers|offers.collect())).collect::<Result<_,_>>()?
        }else{Vec::new()};
        Ok(ReplayCheckpointView {run,combat,player_combat:combat,seats,room_loaded:true,
            shared_relic_bag:&self.world.shared_bags,map:Some(&self.world.map),
            initial_shared_auxiliary:initial_shared_auxiliary(run.rngs.seed),
            act_floor:self.world.act_floor,act_order:self.world.act_defs.iter().map(|a|a.name).collect(),
            room_model:self.world.current_room,event,shops,
            rewards:ReplayRewards::Party(self.rewards.as_deref()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn party_checkpoint_view_is_read_only_for_every_local_seat() {
        let specs=[PlayerSpec{net_id:17,character:crate::run::mp::Character::Ironclad},
            PlayerSpec{net_id:53,character:crate::run::mp::Character::Necrobinder}];
        let game=MpRun::from_numeric_seed(7,&specs,10).unwrap();
        let before=format!("{game:?}");
        for seat in 0..specs.len() {
            let initial=game.initialization_checkpoint_view(seat).unwrap();
            assert_eq!(initial.run.total_floor,0);
            assert!(!initial.room_loaded);
            let view=game.checkpoint_view(seat).unwrap();
            assert_eq!(view.seats.len(),2);
            assert_eq!(view.event.as_ref().unwrap().rng,game.neow_rngs[seat]);
            assert_eq!(view.seats[seat].relic_bag,&game.world.player_bags[seat]);
            assert_eq!(view.shared_relic_bag,&game.world.shared_bags);
        }
        assert!(game.checkpoint_view(2).is_err());
        assert_eq!(format!("{game:?}"),before);
    }
}
