//! Flat census decision domains. Every native command has weight one; batch
//! selectors have one equally weighted outcome per legal unordered subset.
//! No effect is executed to decide legality: a refusing command stays evidence.
use super::*;
use crate::{tables::*,card::CId,run::Coord};

#[derive(Clone,Debug)]
pub struct Decision {pub seat:usize,pub kind:&'static str,pub index:usize,pub target:Option<usize>}
#[derive(Clone,Debug)]
pub struct Domain {pub decision:Decision,pub n:usize,pub min:usize,pub max:usize,pub skip:bool,pub batch:bool}
fn choose(n:usize,k:usize)->u128 {
    if k>n {return 0;}
    (0..k.min(n-k)).fold(1u128,|v,i|v.checked_mul((n-i) as u128).expect("selector domain exceeds u128")/(i+1) as u128)
}
impl Domain {
    pub fn count(&self)->u128 {
        if !self.batch {return 1;}
        (self.min..=self.max).map(|k|choose(self.n,k)).fold(u128::from(self.skip&&self.min>0),
            |a,b|a.checked_add(b).expect("selector domain exceeds u128"))
    }
    pub fn subset(&self,mut rank:u128)->Vec<usize> {
        assert!(rank<self.count());
        if !self.batch {return Vec::new();}
        if self.skip && self.min>0 {if rank==0 {return Vec::new();}rank-=1;}
        for k in self.min..=self.max {
            let count=choose(self.n,k);
            if rank>=count {rank-=count;continue;}
            let mut result=Vec::new();let mut start=0;
            for remaining in (0..k).rev() {
                for i in start..self.n {
                    let count=choose(self.n-i-1,remaining);
                    if rank<count {result.push(i);start=i+1;break;}rank-=count;
                }
            }
            return result;
        }
        unreachable!()
    }
}
fn add(menu:&mut Vec<Domain>,seat:usize,kind:&'static str,index:usize,target:Option<usize>) {
    menu.push(Domain{decision:Decision{seat,kind,index,target},n:0,min:0,max:0,skip:false,batch:false});
}
fn batch(menu:&mut Vec<Domain>,seat:usize,kind:&'static str,n:usize,min:usize,max:usize,skip:bool) {
    menu.push(Domain{decision:Decision{seat,kind,index:0,target:None},n,min,max:max.min(n),skip,batch:true});
}
impl MpRun {
    /// Diagnostic census metadata, not a policy observation.
    pub fn census_starting_ancient(&self)->Option<&'static str>{
        self.world.encounters.ancient.map(crate::identity::EventId::name)
    }
    pub fn census_menu(&self)->Result<Vec<Domain>,&'static str> {
        let mut menu=Vec::new();
        if self.phase==MpPhase::RunOver {return Ok(menu);}
        if self.phase==MpPhase::Combat {
            let c=self.combat().ok_or("combat missing")?;
            if let Some(choice)=&c.pending_choice {
                let seat=choice.player as usize;
                for i in 0..c.choice_options.len() {
                    if !choice.is_multi() || c.choice_selected.contains(&(i as u16)) || c.choice_selected.len()<choice.max as usize {
                        add(&mut menu,seat,"combat_choice",i,Some(i));
                    }
                }
                if (choice.is_multi() && (c.choice_selected.len()>=choice.min as usize
                    || (choice.allow_skip&&c.choice_selected.is_empty())))
                    || (!choice.is_multi()&&choice.allow_skip) {
                    add(&mut menu,seat,"combat_choice",0,None);
                }
                return Ok(menu);
            }
            for seat in 0..c.players.len() {
                let owner=seat as CId;
                if c.player_turn_phase_one_done {
                    if c.players_ready_to_begin_enemy_turn&(1<<seat)==0 {add(&mut menu,seat,"ready",0,None);}
                    continue;
                }
                if c.players_ready_to_end_turn&(1<<seat)==0 {
                    add(&mut menu,seat,"end",0,None);
                    if !c.cr(owner).is_alive() {continue;}
                    for (hand,&card) in c.players[owner].hand.iter().enumerate() {
                        if !c.clone().can_play_card(card) {continue;}
                        match c.card_target_type(card) {
                            TargetType::AnyEnemy=>for (i,&enemy) in c.enemies.iter().enumerate() {
                                if c.can_hit(enemy) {add(&mut menu,seat,"play",hand,Some(i));}
                            },
                            TargetType::AnyAlly=>for target in 0..c.players.len() {
                                if c.cr(target as CId).is_alive() {add(&mut menu,seat,"play",hand,Some(target));}
                            },
                            _=>add(&mut menu,seat,"play",hand,None),
                        }
                    }
                }
                if let Some(slots)=c.potion_slots_for_player(owner) {
                    for (slot,id) in slots.iter().enumerate() {
                        let Some(id)=id else {continue;};
                        if c.can_discard_potion_for_player(owner,slot){add(&mut menu,seat,"discard",slot,None);}
                        let targets=match POTION_INFO[id.index()].target {
                            PotionTargetType::AnyEnemy=>(0..c.enemies.len()).map(Some).collect::<Vec<_>>(),
                            PotionTargetType::AnyPlayer=>(0..c.players.len()).map(Some).collect(),
                            _=>vec![None],
                        };
                        for target in targets {
                            if c.can_use_potion_for_player(owner,slot,target) {add(&mut menu,seat,"potion",slot,target);}
                        }
                    }
                }
            }
            return Ok(menu);
        }
        for seat in 0..self.roster.players().len() {
            let p=self.roster.player(seat).unwrap();let net=p.net_id;
            let free=p.potions.iter().any(Option::is_none)||p.relics.iter().any(|r|r.id==RelicId::Sozu&&r.enabled);
            if matches!(self.phase,MpPhase::Neow|MpPhase::Map|MpPhase::Rewards|MpPhase::Event) {
                for (i,slot) in p.potions.iter().enumerate() {if slot.is_some(){add(&mut menu,seat,"discard",i,None);}}
            }
            match self.phase {
                MpPhase::Neow=>{
                    let pending=[("new_leaf",&self.new_leaf_choices[seat]),("pomander",&self.pomander_choices[seat]),
                        ("scissors",&self.precise_scissors_choices[seat]),("growth",&self.paels_growth_choices[seat])];
                    if let Some((kind,cards))=pending.into_iter().find_map(|(kind,cards)|cards.as_ref().map(|cards|(kind,cards))) {
                        for &id in cards {add(&mut menu,seat,kind,id as usize,None);}continue;
                    }
                    if let Some(cards)=&self.shears_choices[seat] {batch(&mut menu,seat,"shears",cards.len(),2,2,false);continue;}
                    if self.relic_rewards[seat].is_some() {
                        add(&mut menu,seat,"neow_relic",1,None);
                        if self.can_skip_relic_reward(net,seat)? {add(&mut menu,seat,"neow_relic",0,None);}continue;
                    }
                    if let Some(bundles)=&self.scroll_box_bundles[seat] {
                        for i in 0..bundles.len(){add(&mut menu,seat,"scroll",i,None);}continue;
                    }
                    if self.neow_potion_rewards[seat].is_some() {
                        add(&mut menu,seat,"neow_potion",0,None);if free{add(&mut menu,seat,"neow_potion",1,None);}continue;
                    }
                    if let Some(cards)=&self.neow_cards[seat] {
                        add(&mut menu,seat,"neow_card",0,None);
                        for i in 0..cards.len(){add(&mut menu,seat,"neow_card",i,Some(i));}continue;
                    }
                    if !self.neow_finished(seat).unwrap_or(false) {for i in 0..3 {add(&mut menu,seat,"neow",i,None);}}
                }
                MpPhase::Map=>{
                    if self.world.run.coord.is_none() {
                        add(&mut menu,seat,"vote",self.world.map.start as usize,None);continue;
                    }
                    let (x,y)=self.world.run.coord.ok_or("map coordinate missing")?;
                    let point=self.world.map.point(Coord::new(x,y)).ok_or("map point missing")?;
                    if point.children.is_empty() {
                        if seat==0 {add(&mut menu,seat,"advance",0,None);}
                    } else {
                        for &child in &point.children {
                            let coord=self.world.map.points[child as usize].coord;
                            if self.voting.votes()[seat].is_none_or(|v|v.coord!=coord) {
                                add(&mut menu,seat,"vote",child as usize,None);
                            }
                        }
                    }
                }
                MpPhase::Shop=>{
                    if let Some(choice)=self.merchant_pickup(net,seat)? {
                        match choice {
                            MerchantPickupChoice::Deck{cards,min,max,..}=>batch(&mut menu,seat,"merchant_pick",cards.len(),*min,*max,false),
                            MerchantPickupChoice::Cards(cards)=>batch(&mut menu,seat,"merchant_pick",cards[0].len(),0,1,false),
                            MerchantPickupChoice::Potions(_)=>{
                                batch(&mut menu,seat,"merchant_pick",usize::from(free),0,1,false);
                            }
                        }
                    } else {
                        for (i,offer) in self.merchant_offers(net,seat)?.iter().enumerate() {
                            if offer.enabled {add(&mut menu,seat,"buy",i,None);}
                        }
                        if !self.merchant_pickups.iter().any(Option::is_some) {add(&mut menu,seat,"leave_shop",0,None);}
                    }
                }
                MpPhase::Rewards=>{
                    for relic in self.card_reward_sacrifices(net,seat)?{add(&mut menu,seat,"card_sacrifice",relic,None);}
                    let r=&self.rewards.as_ref().ok_or("rewards not populated")?[seat];
                    if r.completed {continue;}
                    add(&mut menu,seat,"finish_rewards",0,None);
                    for (i,&taken) in r.card_removals.iter().enumerate() {
                        if !taken {
                            let cards=self.card_removal_reward_cards(net,seat,i)?;
                            for j in 0..cards.len(){add(&mut menu,seat,"reward_remove",i,Some(j));}
                        }
                    }
                    if !r.card_taken {for i in 0..r.cards.len(){add(&mut menu,seat,"card",i,None);}}
                    if free && r.potion.is_some()&&!r.potion_taken {add(&mut menu,seat,"reward_potion",0,None);}
                    for (i,cards) in r.extra_cards.iter().enumerate() {if let Some(cards)=cards {
                        for j in 0..cards.len(){add(&mut menu,seat,"extra_card",i,Some(j));}
                    }}
                    for (i,relic) in r.extra_relics.iter().enumerate() {if relic.is_some(){add(&mut menu,seat,"extra_relic",i,None);}}
                    for (i,potion) in r.extra_potions.iter().enumerate(){if potion.is_some()&&free{add(&mut menu,seat,"extra_potion",i,None);}}
                    for i in 0..r.special_cards.len(){if let Some(index)=r.special_card_reward_index(i){add(&mut menu,seat,"special",index,None);}}
                }
                MpPhase::RestSite=>{
                    if let Some(cards)=self.rest_card_reward(net,seat)? {
                        add(&mut menu,seat,"rest_card",0,None);for i in 0..cards.len(){add(&mut menu,seat,"rest_card",i,Some(i));}
                    } else if self.rest_potion_reward(net,seat)?.is_some() {
                        add(&mut menu,seat,"rest_potion",0,None);if free{add(&mut menu,seat,"rest_potion",1,None);}
                    } else if let Some(targets)=self.rest_mend_targets(net,seat)? {
                        add(&mut menu,seat,"mend",0,None);for &target in targets{add(&mut menu,seat,"mend",target,Some(target));}
                    } else if let Some(cards)=self.rest_cook_choices(net,seat)? {
                        batch(&mut menu,seat,"cook",cards.len(),2,2,true);
                    } else if let Some(cards)=self.rest_smith_choices(net,seat)? {
                        add(&mut menu,seat,"smith",0,None);for &id in cards{add(&mut menu,seat,"smith",id as usize,Some(id as usize));}
                    } else {
                        let options=self.rest_options(net,seat)?;
                        if !options.is_empty() {
                            add(&mut menu,seat,"skip_rest",0,None);
                            for (i,(_,enabled)) in options.iter().enumerate(){if *enabled{add(&mut menu,seat,"rest",i,None);}}
                        }
                    }
                }
                MpPhase::Treasure=>{
                    if !self.treasure_opened[seat] {add(&mut menu,seat,"open",0,None);}
                    if self.treasure_votes[seat].is_none() {
                        add(&mut menu,seat,"treasure",0,None);
                        for i in 0..self.treasure_relics.len(){add(&mut menu,seat,"treasure",i,Some(i));}
                    }
                }
                MpPhase::Event=>{
                    if self.event_done[seat] {continue;}
                    if let Some(reward)=self.event_card_reward_queues[seat].first(){
                        add(&mut menu,seat,"event_card_skip",0,None);
                        for i in 0..reward.cards.len(){add(&mut menu,seat,"event_card",i,None);}
                        for relic in self.card_reward_sacrifices(net,seat)?{add(&mut menu,seat,"card_sacrifice",relic,None);}
                        continue;
                    }
                    if self.relic_rewards[seat].is_some() {
                        add(&mut menu,seat,"event_relic",1,None);
                        if self.relic_reward_skippable[seat] {add(&mut menu,seat,"event_relic",0,None);}
                        continue;
                    }
                    if let Some(modal)=&self.event_card_modals[seat] {
                        batch(&mut menu,seat,"event_cards",modal.cards.len(),modal.min,modal.max,modal.skippable);continue;
                    }
                    if let Some(potions)=&self.event_potion_rewards[seat] {
                        let hollow=self.world.current_room==Some("WHISPERING_HOLLOW");
                        if free {for (i,potion) in potions.iter().enumerate(){if potion.is_some(){add(&mut menu,seat,if hollow{"hollow_potion"}else{"event_potion"},i,None);}}}
                        if potions.iter().all(Option::is_none){add(&mut menu,seat,if hollow{"hollow_finish"}else{"event_potion_finish"},0,None);}
                        continue;
                    }
                    match self.world.current_room {
                        Some("ROOM_FULL_OF_CHEESE")=>{
                            if let Some(cards)=&self.cheese_cards[seat]{batch(&mut menu,seat,"cheese_cards",cards.len(),2,2,false);}
                            else {for i in 0..2 {add(&mut menu,seat,"cheese",i,None);}}
                        }
                        Some("SYMBIOTE")=>{
                            let event=self.events[seat].as_ref().ok_or("Symbiote state missing")?;
                            if event.flags==1 {for id in self.symbiote_choices(net,seat)? {add(&mut menu,seat,"symbiote_card",id as usize,None);}}
                            else {for (i,o) in event.options.iter().enumerate(){if o.enabled{add(&mut menu,seat,"symbiote",i,None);}}}
                        }
                        Some("SLIPPERY_BRIDGE")=>{
                            for (i,o) in self.events[seat].as_ref().ok_or("bridge state missing")?.options.iter().enumerate(){
                                if o.enabled {add(&mut menu,seat,"bridge",i,None);}
                            }
                        }
                        _=>for i in 0..self.event_options(net,seat)?.len(){add(&mut menu,seat,"event",i,None);},
                    }
                }
                _=>{}
            }
        }
        Ok(menu)
    }
    pub fn census_take(&mut self,d:&Decision,selected:&[usize])->Result<(),&'static str> {
        let s=d.seat;let net=self.roster.player(s).ok_or("invalid actor")?.net_id;let i=d.index;let t=d.target;
        match d.kind {
            "neow"=>self.choose_neow(net,s,i),"new_leaf"=>self.select_new_leaf_card(net,s,i as u32),
            "pomander"=>self.select_pomander_card(net,s,i as u32),"scissors"=>self.select_precise_scissors_card(net,s,i as u32),
            "growth"=>self.select_paels_growth_card(net,s,i as u32),
            "shears"=>{let ids:Vec<_>=selected.iter().map(|&i|self.shears_choices[s].as_ref().unwrap()[i]).collect();self.select_precarious_shears_cards(net,s,&ids)},
            "neow_relic"=>self.select_relic_reward(net,s,i!=0),"scroll"=>self.select_scroll_box_bundle(net,s,i),
            "neow_potion"=>self.select_neow_potion_reward(net,s,i!=0),"neow_card"=>self.select_neow_card(net,s,t),
            "vote"=>self.vote(net,s,self.world.map.points[i].coord),"advance"=>self.advance_act(),
            "play"=>self.play_card(net,s,i,t),"potion"=>self.use_potion(net,s,i,t),
            "end"=>self.end_turn_for(net,s),"ready"=>self.ready_to_begin_enemy_turn_for(net,s),
            "combat_choice"=>self.resolve_combat_choice(net,s,t),"discard"=>self.discard_potion(net,s,i),
            "buy"=>{let action=self.merchant_offers(net,s)?[i].action;self.merchant_purchase(net,s,action).map(|_|())},
            "merchant_pick"=>self.select_merchant_pickup(net,s,selected),"leave_shop"=>self.leave_shop(net,s),
            "finish_rewards"=>self.take_gold_skip_remaining_rewards(net,s),"card"=>self.take_card_reward(net,s,i),
            "reward_remove"=>self.take_card_removal_reward(net,s,i,t),
            "reward_potion"=>self.take_potion_reward(net,s).map(|_|()),"extra_card"=>self.take_extra_card_reward(net,s,i,t.unwrap()),
            "extra_relic"=>self.take_extra_relic_reward(net,s,i),"extra_potion"=>self.take_extra_potion_reward(net,s,i).map(|_|()),
            "special"=>self.take_special_card_reward(net,s,i),"rest_card"=>self.select_rest_card_reward(net,s,t),
            "rest_potion"=>self.select_rest_potion_reward(net,s,i!=0),"mend"=>self.select_rest_mend(net,s,t),
            "cook"=>self.select_rest_cook(net,s,selected),
            "smith"=>self.select_rest_smith(net,s,t.map(|v|v as u32)),"skip_rest"=>self.skip_rest_remaining(net,s),
            "rest"=>self.choose_rest_option(net,s,i),"open"=>self.open_treasure(net,s),"treasure"=>self.vote_treasure(net,s,t),
            "event_cards"=>self.select_event_cards(net,s,selected),"event"=>self.choose_event_option(net,s,i),
            "card_sacrifice"=>self.sacrifice_card_reward(net,s,i),
            "event_card"=>self.select_event_cards(net,s,&[i]),"event_card_skip"=>self.finish_event_card_reward(net,s),
            "event_relic"=>self.select_event_relic_reward(net,s,i!=0),
            "event_potion"=>self.take_event_potion_reward(net,s,i).map(|_|()),"event_potion_finish"=>self.finish_event_potion_rewards(net,s),
            "hollow_potion"=>self.take_whispering_hollow_reward(net,s,i).map(|_|()),
            "hollow_finish"=>self.finish_whispering_hollow_option(net,s),"cheese"=>self.choose_cheese(net,s,i),
            "cheese_cards"=>self.select_cheese_cards(net,s,[selected[0],selected[1]]),
            "symbiote"=>self.choose_symbiote_option(net,s,i),"symbiote_card"=>self.select_symbiote_card(net,s,i as u32),
            "bridge"=>self.choose_bridge_option(net,s,i),_=>Err("unknown census action"),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn menus_have_one_entry_per_distinct_card_or_potion_action() {
        use std::collections::BTreeSet;
        let specs:Vec<_>=(0..4).map(|i|PlayerSpec{net_id:100+i,character:Character::Ironclad}).collect();
        for info in POTION_INFO {
            let id=PotionId::from_name(info.id).unwrap();
            let mut run=MpRun::new("931001",&specs,0).unwrap();
            for seat in 0..4 {run.roster.player_mut(seat).unwrap().potions=vec![Some(id)];}
            run.combat=Some(run.roster.enter_combat(run.world.run.clone(),1,"CORPSE_SLUGS_NORMAL",RoomType::Monster).unwrap());
            run.phase=MpPhase::Combat;
            for owner in 0..4 {
                let c=run.combat.as_mut().unwrap();
                let card=c.new_card(owner,CardId::Constellation,false).unwrap();
                c.players[owner].hand.push(card);c.gain_stars(owner,2);
            }
            let c=run.combat().unwrap();let menu=run.census_menu().unwrap();
            let mut distinct=BTreeSet::new();
            for domain in menu {
                let d=domain.decision;
                let target=match d.kind {
                    "potion"=>match info.target {
                        PotionTargetType::AnyEnemy=>Some(c.enemies[d.target.unwrap_or(0)] as usize),
                        PotionTargetType::AnyPlayer=>Some(d.target.unwrap_or(d.seat)),
                        PotionTargetType::Self_=>Some(d.seat),
                        _=>None,
                    },
                    "play"=>{
                        let card=c.players[d.seat as u8].hand[d.index];
                        match c.card_target_type(card) {
                            TargetType::AnyEnemy=>Some(c.enemies[d.target.unwrap_or(0)] as usize),
                            TargetType::AnyAlly=>Some(d.target.unwrap_or(d.seat)),_=>None,
                        }
                    },
                    _=>d.target,
                };
                assert!(distinct.insert((d.seat,d.kind,d.index,target)),"duplicate {id:?}: {d:?}");
            }
            for seat in 0..4 {
                let owner=seat as u8;
                let expected=match info.target {
                    PotionTargetType::AnyEnemy=>(0..c.enemies.len()).filter(|&i|c.can_use_potion_for_player(owner,0,Some(i))).count(),
                    PotionTargetType::AnyPlayer=>(0..c.players.len()).filter(|&i|c.can_use_potion_for_player(owner,0,Some(i))).count(),
                    _=>usize::from(c.can_use_potion_for_player(owner,0,None)),
                };
                assert_eq!(distinct.iter().filter(|&&(actor,kind,_,_)|actor==seat&&kind=="potion").count(),expected);
            }
        }
    }
    #[test] fn subset_domains_count_each_legal_combination_once() {
        let d=Domain{decision:Decision{seat:0,kind:"test",index:0,target:None},n:4,min:1,max:3,skip:true,batch:true};
        assert_eq!(d.count(),15);
        let distinct:std::collections::BTreeSet<_>=(0..d.count()).map(|i|d.subset(i)).collect();
        assert_eq!(distinct.len(),15);
        assert_eq!(distinct.iter().filter(|s|s.len()==2).count(),6);
    }
    #[test] fn initial_flat_menu_contains_each_neow_option_for_every_actor() {
        for seats in 2..=4 {
            let specs:Vec<_>=(0..seats).map(|i|PlayerSpec{net_id:100+i as u64,character:Character::Ironclad}).collect();
            let run=MpRun::new("930501",&specs,0).unwrap();
            let menu=run.census_menu().unwrap();
            assert_eq!(menu.iter().map(|d|d.count()).sum::<u128>(),(3*seats) as u128);
            for seat in 0..seats {
                let options:Vec<_>=menu.iter().filter(|d|d.decision.seat==seat).map(|d|d.decision.index).collect();
                assert_eq!(options,vec![0,1,2]);
            }
        }
    }
    #[test] fn readiness_menu_includes_a_dead_network_seat() {
        let mut run=MpRun::new("930701",&[
            PlayerSpec{net_id:100,character:Character::Ironclad},
            PlayerSpec{net_id:101,character:Character::Ironclad},
        ],0).unwrap();
        let mut combat=run.roster.enter_combat(run.world.run.clone(),1,"SLUDGE_SPINNER_WEAK",RoomType::Monster).unwrap();
        combat.cr_mut(1).hp=0;
        run.combat=Some(combat);run.phase=MpPhase::Combat;
        assert!(run.census_menu().unwrap().iter().any(|d|d.decision.kind=="end"&&d.decision.seat==1));
        run.end_turn_for(100,0).unwrap();run.end_turn_for(101,1).unwrap();
        run.ready_to_begin_enemy_turn_for(100,0).unwrap();
        assert!(run.census_menu().unwrap().iter().any(|d|d.decision.kind=="ready"&&d.decision.seat==1));
    }
    #[test] fn hollow_menu_exposes_both_csharp_options_for_every_actor() {
        let mut run=MpRun::new("930801",&[
            PlayerSpec{net_id:100,character:Character::Ironclad},
            PlayerSpec{net_id:101,character:Character::Ironclad},
        ],0).unwrap();
        run.phase=MpPhase::Event;run.world.current_room=Some("WHISPERING_HOLLOW");
        run.begin_whispering_hollow().unwrap();
        let menu=run.census_menu().unwrap();
        for seat in 0..2 {
            assert_eq!(menu.iter().filter(|d|d.decision.seat==seat&&d.decision.kind=="event")
                .map(|d|d.decision.index).collect::<Vec<_>>(),vec![0,1]);
        }
    }
}
