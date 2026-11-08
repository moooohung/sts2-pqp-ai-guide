//! Deterministic whole-run multiplayer first-fault census.
//!
//! This is measurement, not an oracle: it drives only public actor-bound MP
//! actions and reports the first fail-closed refusal together with floor/room.
//! STS2_MP_WHOLE_CENSUS_UNIFORM=1 samples independent character slots and
//! unbiased combat actions (cards, potions, end-turn) plus menu options.
//! Legacy event routing and multi-selector cardinality remain hierarchical;
//! this is not uniform sampling over the Cartesian set of whole-party actions.

use std::collections::BTreeMap;

use sts2_sim::{
    card::CId,
    creature::Side,
    run::mp::{Character, MpPhase, MpRun, PlayerSpec},
    tables::TargetType,
};

#[derive(Clone, Copy)]
struct Policy(u64, bool);

impl Policy {
    fn new(seed: u64) -> Self { Self(seed.max(1), false) }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn index(&mut self, len: usize) -> usize {
        assert!(len > 0);
        if self.1 {
            let n=len as u64;
            let threshold=n.wrapping_neg()%n;
            loop {
                let v=self.next();
                if v>=threshold { return (v%n) as usize; }
            }
        }
        (self.next() as usize) % len
    }

    fn chance(&mut self, numerator: u64, denominator: u64) -> bool {
        self.next() % denominator < numerator
    }

    fn shuffle<T>(&mut self, values: &mut [T]) {
        for i in (1..values.len()).rev() {
            let j = self.index(i + 1);
            values.swap(i, j);
        }
    }
}

fn actor(run: &MpRun, seat: usize) -> u64 {
    run.roster.player(seat).unwrap().net_id
}

fn run_neow(run: &mut MpRun, policy: &mut Policy) -> Result<(), String> {
    for _ in 0..256 {
        if run.phase != MpPhase::Neow { return Ok(()); }
        let mut acted = false;
        for seat in 0..run.roster.players().len() {
            if run.phase != MpPhase::Neow { return Ok(()); }
            let net = actor(run, seat);
            if let Some(choices) = run.new_leaf_choices[seat].as_ref() {
                if choices.is_empty() { return Err("census harness empty New Leaf selector".into()); }
                let instance_id = choices[policy.index(choices.len())];
                run.select_new_leaf_card(net, seat, instance_id).map_err(str::to_string)?;
                acted = true;
                continue;
            }
            if let Some(choices) = run.pomander_choices[seat].as_ref() {
                let instance_id = choices[policy.index(choices.len())];
                run.select_pomander_card(net, seat, instance_id).map_err(str::to_string)?;
                acted = true;
                continue;
            }
            if let Some(choices) = run.precise_scissors_choices[seat].as_ref() {
                let instance_id = choices[policy.index(choices.len())];
                run.select_precise_scissors_card(net, seat, instance_id).map_err(str::to_string)?;
                acted = true;
                continue;
            }
            if let Some(choices) = run.paels_growth_choices[seat].clone() {
                let id = choices[policy.index(choices.len())];
                run.select_paels_growth_card(net, seat, id).map_err(str::to_string)?;
                acted = true;
                continue;
            }
            if let Some(choices) = run.shears_choices[seat].clone() {
                let mut choices = choices;
                policy.shuffle(&mut choices);
                run.select_precarious_shears_cards(net, seat, &choices[..2]).map_err(str::to_string)?;
                acted = true;
                continue;
            }
            if run.relic_rewards[seat].is_some() {
                let accept=!policy.1 || !run.can_skip_relic_reward(net,seat).map_err(str::to_string)? || policy.index(2)==0;
                run.select_relic_reward(net, seat, accept).map_err(str::to_string)?;
                acted = true;
                continue;
            }
            if let Some(bundles) = run.scroll_box_bundles[seat].as_ref() {
                let index = policy.index(bundles.len());
                run.select_scroll_box_bundle(net, seat, index).map_err(str::to_string)?;
                acted = true;
                continue;
            }
            if run.neow_potion_rewards[seat].is_some() {
                let can_take = run.roster.player(seat).unwrap().potions.iter().any(Option::is_none);
                let accept=if policy.1 {policy.index(2)==0}else{policy.chance(3,4)};
                run.select_neow_potion_reward(net, seat, can_take && accept)
                    .map_err(str::to_string)?;
                acted = true;
                continue;
            }
            if let Some(cards) = run.neow_cards[seat].as_ref() {
                let index = policy.index(cards.len()+usize::from(policy.1));
                run.select_neow_card(net, seat, (index<cards.len()).then_some(index)).map_err(str::to_string)?;
                acted = true;
                continue;
            }
            if !run.neow_finished(seat).unwrap_or(false) {
                let option = policy.index(3);
                let relic = run.neow[seat][option];
                run.choose_neow(net, seat, option)
                    .map_err(|error| format!("{error} relic={}", relic.name()))?;
                acted = true;
            }
        }
        if !acted && run.phase == MpPhase::Neow {
            return Err("census harness stalled in Neow".into());
        }
    }
    Err("census harness Neow action budget exceeded".into())
}

fn run_map(run: &mut MpRun, policy: &mut Policy) -> Result<(), String> {
    let (col, row) = run.world.run.coord.ok_or_else(|| "census harness map coord missing".to_string())?;
    let source = sts2_sim::run::Coord::new(col, row);
    let point = run.world.map.point(source).ok_or_else(|| "census harness map point missing".to_string())?;
    if point.children.is_empty() {
        return run.advance_act().map_err(str::to_string);
    }
    let children = point.children.clone();
    for seat in 0..run.roster.players().len() {
        if run.phase != MpPhase::Map { break; }
        let child = children[policy.index(children.len())];
        let destination = run.world.map.points[child as usize].coord;
        run.vote(actor(run, seat), seat, destination).map_err(str::to_string)?;
    }
    Ok(())
}

fn resolve_choice(run: &mut MpRun, policy: &mut Policy) -> Result<bool, String> {
    let Some(choice) = run.combat().and_then(|c| c.pending_choice.clone()) else { return Ok(false); };
    let options = run.combat().unwrap().choice_options.len();
    let seat = choice.player as usize;
    let net = actor(run, seat);
    if choice.is_multi() {
        let min = usize::from(choice.min).min(options);
        let max = usize::from(choice.max).min(options);
        if min > max { return Err("census harness invalid multi-choice bounds".into()); }
        let count = if max == min { min } else { min + policy.index(max - min + 1) };
        let mut indices: Vec<_> = (0..options).collect();
        policy.shuffle(&mut indices);
        for index in indices.into_iter().take(count) {
            run.resolve_combat_choice(net, seat, Some(index)).map_err(|error| {
                format!("{error}: seat={seat} index={index} options={options} count={count} choice={choice:?}")
            })?;
        }
        run.resolve_combat_choice(net, seat, None).map_err(|error| {
            format!("{error}: seat={seat} confirm options={options} count={count} choice={choice:?}")
        })?;
    } else if options == 0 {
        if !choice.allow_skip { return Err("census harness mandatory combat choice has no options".into()); }
        run.resolve_combat_choice(net, seat, None).map_err(str::to_string)?;
    } else {
        let index = if policy.1 {
            let index=policy.index(options+usize::from(choice.allow_skip));
            (index<options).then_some(index)
        } else if choice.allow_skip && policy.chance(1, 8) { None } else { Some(policy.index(options)) };
        run.resolve_combat_choice(net, seat, index).map_err(|error| {
            format!("{error}: seat={seat} index={index:?} options={options} choice={choice:?}")
        })?;
    }
    Ok(true)
}

fn potion_actions(run: &MpRun) -> Vec<(usize, usize, Option<usize>)> {
    let Some(combat) = run.combat() else { return Vec::new(); };
    let mut actions = Vec::new();
    for seat in 0..combat.players.len() {
        let owner = seat as CId;
        let Some(slots) = combat.potion_slots_for_player(owner) else { continue; };
        for slot in 0..slots.len() {
            if slots[slot].is_none() { continue; }
            if combat.can_use_potion_for_player(owner, slot, None) {
                actions.push((seat, slot, None));
            }
            let target_count = combat.players.len().max(combat.enemies.len());
            for target in 0..target_count {
                if combat.can_use_potion_for_player(owner, slot, Some(target)) {
                    actions.push((seat, slot, Some(target)));
                }
            }
        }
    }
    actions
}

fn card_actions(run: &MpRun, ended: &[bool]) -> Vec<(usize, usize, Option<usize>)> {
    let Some(combat) = run.combat() else { return Vec::new(); };
    let mut actions = Vec::new();
    for seat in 0..combat.players.len() {
        if ended[seat] || !combat.cr(seat as CId).is_alive() { continue; }
        for hand in 0..combat.players[seat as CId].hand.len() {
            let card = combat.players[seat as CId].hand[hand];
            let mut probe = combat.clone();
            if !probe.can_play_card(card) { continue; }
            match combat.card_target_type(card) {
                TargetType::AnyEnemy => {
                    for (target, &enemy) in combat.enemies.iter().enumerate() {
                        if combat.can_hit(enemy) { actions.push((seat, hand, Some(target))); }
                    }
                }
                TargetType::AnyAlly => {
                    for target in 0..combat.players.len() {
                        if combat.cr(target as CId).is_alive() {
                            actions.push((seat, hand, Some(target)));
                        }
                    }
                }
                _ => actions.push((seat, hand, None)),
            }
        }
    }
    actions
}

fn run_combat(run: &mut MpRun, policy: &mut Policy) -> Result<(), String> {
    let seats = run.roster.players().len();
    let mut ended = vec![false; seats];
    let mut generation = None;
    for _ in 0..20_000 {
        if run.phase != MpPhase::Combat { return Ok(()); }
        if resolve_choice(run, policy)? { continue; }
        let combat = run.combat().ok_or_else(|| "census harness combat missing".to_string())?;
        let key = (combat.round, combat.turn_count, combat.side);
        if generation != Some(key) {
            generation = Some(key);
            ended.fill(false);
        }
        if combat.side != Side::Player {
            return Err("census harness combat stopped on enemy side without queued progression".into());
        }

        let potions = potion_actions(run);
        if policy.1 {
            let cards=card_actions(run,&ended);
            let ends:Vec<_>=(0..seats).filter(|&seat| !ended[seat]).collect();
            let count=potions.len()+cards.len()+ends.len();
            if count>0 {
                let action=policy.index(count);
                if action<potions.len() {
                    let (seat,slot,target)=potions[action];
                    run.use_potion(actor(run,seat),seat,slot,target).map_err(str::to_string)?;
                } else if action<potions.len()+cards.len() {
                    let (seat,hand,target)=cards[action-potions.len()];
                    let card=run.combat().unwrap().players[seat as CId].hand[hand];
                    let id=run.combat().unwrap().cards[card as usize].id;
                    run.play_card(actor(run,seat),seat,hand,target)
                        .map_err(|error|format!("{error} card={id:?} seat={seat}"))?;
                } else {
                    let seat=ends[action-potions.len()-cards.len()];
                    run.end_turn_for(actor(run,seat),seat).map_err(str::to_string)?;
                    ended[seat]=true;
                }
                continue;
            }
            for seat in 0..seats {
                run.ready_to_begin_enemy_turn_for(actor(run,seat),seat).map_err(str::to_string)?;
                if run.phase!=MpPhase::Combat || run.combat().is_some_and(|c|c.pending_choice.is_some()) {break;}
            }
            continue;
        }
        if !potions.is_empty() && policy.chance(1, 10) {
            let (seat, slot, target) = potions[policy.index(potions.len())];
            run.use_potion(actor(run, seat), seat, slot, target).map_err(str::to_string)?;
            continue;
        }

        let cards = card_actions(run, &ended);
        if !cards.is_empty() && policy.chance(3, 4) {
            let (seat, hand, target) = cards[policy.index(cards.len())];
            run.play_card(actor(run, seat), seat, hand, target).map_err(str::to_string)?;
            continue;
        }

        let mut voted = false;
        for seat in 0..seats {
            if ended[seat] { continue; }
            run.end_turn_for(actor(run, seat), seat).map_err(str::to_string)?;
            ended[seat] = true;
            voted = true;
            if run.phase != MpPhase::Combat || run.combat().is_some_and(|c| c.pending_choice.is_some()) {
                break;
            }
        }
        if run.phase != MpPhase::Combat { continue; }
        if run.combat().is_some_and(|c| c.pending_choice.is_some()) { continue; }
        if ended.iter().all(|done| *done) {
            for seat in 0..seats {
                run.ready_to_begin_enemy_turn_for(actor(run, seat), seat).map_err(str::to_string)?;
                if run.phase != MpPhase::Combat { break; }
                if run.combat().is_some_and(|c| c.pending_choice.is_some()) { break; }
            }
            continue;
        }
        if !voted { return Err("census harness combat made no legal progress".into()); }
    }
    Err("census harness combat action budget exceeded".into())
}

fn run_rewards(run: &mut MpRun, policy: &mut Policy) -> Result<(), String> {
    run.offer_rewards().map_err(str::to_string)?;
    let seats = run.roster.players().len();
    for seat in 0..seats {
        if run.phase != MpPhase::Rewards { break; }
        let net = actor(run, seat);
        if policy.1 {
            loop {
                let reward=run.rewards.as_ref().unwrap()[seat].clone();
                let cards=if reward.card_taken {0}else{reward.cards.len()};
                let potion=usize::from(reward.potion.is_some() && !reward.potion_taken &&
                    run.roster.player(seat).unwrap().potions.iter().any(Option::is_none));
                let extras:Vec<_>=reward.extra_cards.iter().enumerate().flat_map(|(r,cards)|
                    cards.as_ref().into_iter().flat_map(move |cards|(0..cards.len()).map(move |i|(r,i)))).collect();
                let index=policy.index(cards+potion+extras.len()+1);
                if index<cards {
                    run.take_card_reward(net,seat,index).map_err(str::to_string)?;
                } else if index<cards+potion {
                    run.take_potion_reward(net,seat).map_err(str::to_string)?;
                } else if index<cards+potion+extras.len() {
                    let (reward,index)=extras[index-cards-potion];
                    run.take_extra_card_reward(net,seat,reward,index).map_err(str::to_string)?;
                } else {
                    run.take_gold_skip_remaining_rewards(net,seat).map_err(str::to_string)?;
                    break;
                }
            }
            continue;
        }
        let reward = run.rewards.as_ref().unwrap()[seat].clone();
        if reward.potion.is_some() && !reward.potion_taken {
            if !run.take_potion_reward(net, seat).map_err(str::to_string)? {
                let slot = run.roster.player(seat).unwrap().potions.iter().position(Option::is_some)
                    .ok_or_else(|| "census harness full potion inventory has no occupied slot".to_string())?;
                run.discard_potion(net, seat, slot).map_err(str::to_string)?;
                if !run.take_potion_reward(net, seat).map_err(str::to_string)? {
                    return Err("census harness potion retry remained unavailable".into());
                }
            }
        }
        let reward = run.rewards.as_ref().unwrap()[seat].clone();
        if !reward.card_taken && !reward.cards.is_empty() {
            run.take_card_reward(net, seat, policy.index(reward.cards.len())).map_err(str::to_string)?;
        }
        run.take_gold_skip_remaining_rewards(net, seat).map_err(str::to_string)?;
    }
    Ok(())
}

fn run_event(run: &mut MpRun, policy: &mut Policy) -> Result<(), String> {
    let event = run.world.current_room.unwrap_or("UNKNOWN_EVENT");
    for _ in 0..512 {
        if run.phase != MpPhase::Event { return Ok(()); }
        let mut acted = false;
        for seat in 0..run.roster.players().len() {
            if run.phase != MpPhase::Event { return Ok(()); }
            if run.cheese_finished(seat).unwrap_or(false) { continue; }
            let net = actor(run, seat);

            if let Ok((cards, min, max, skippable)) = run.event_card_modal(net, seat) {
                let mut indices: Vec<_> = (0..cards.len()).collect();
                policy.shuffle(&mut indices);
                let count = if skippable && policy.chance(1, 5) {
                    0
                } else if max == min {
                    min
                } else {
                    min + policy.index(max - min + 1)
                };
                let selected: Vec<_> = indices.into_iter().take(count).collect();
                run.select_event_cards(net, seat, &selected).map_err(str::to_string)?;
                acted = true;
                continue;
            }

            if let Ok(rewards) = run.event_potion_rewards(net, seat) {
                let pending: Vec<_> = rewards.iter().enumerate()
                    .filter_map(|(index, reward)| reward.is_some().then_some(index)).collect();
                if let Some(&index) = pending.first() {
                    if !run.take_event_potion_reward(net, seat, index).map_err(str::to_string)? {
                        let slot = run.roster.player(seat).unwrap().potions.iter()
                            .position(Option::is_some)
                            .ok_or_else(|| "census harness full event potion inventory has no occupied slot".to_string())?;
                        run.discard_potion(net, seat, slot).map_err(str::to_string)?;
                        if !run.take_event_potion_reward(net, seat, index).map_err(str::to_string)? {
                            return Err("census harness event potion retry remained unavailable".into());
                        }
                    }
                } else {
                    run.finish_event_potion_rewards(net, seat).map_err(str::to_string)?;
                }
                acted = true;
                continue;
            }

            match event {
                "ROOM_FULL_OF_CHEESE" => {
                    if let Some(cards) = run.cheese_cards[seat].as_ref() {
                        if cards.len() < 2 { return Err("census harness cheese grid too small".into()); }
                        let first = policy.index(cards.len());
                        let mut second = policy.index(cards.len() - 1);
                        if second >= first { second += 1; }
                        run.select_cheese_cards(net, seat, [first, second]).map_err(str::to_string)?;
                    } else {
                        run.choose_cheese(net, seat, 0).map_err(str::to_string)?;
                    }
                }
                "SLIPPERY_BRIDGE" => {
                    let option = if policy.1 {policy.index(2)} else {usize::from(policy.chance(1, 4))};
                    run.choose_bridge_option(net, seat, option).map_err(str::to_string)?;
                }
                "SYMBIOTE" => {
                    match run.symbiote_choices(net, seat) {
                        Ok(choices) if !choices.is_empty() => {
                            let id = choices[policy.index(choices.len())];
                            run.select_symbiote_card(net, seat, id).map_err(str::to_string)?;
                        }
                        Ok(_) => return Err("census harness empty Symbiote selector".into()),
                        Err("event selector missing") => {
                            run.choose_symbiote_option(net, seat, 1).map_err(str::to_string)?;
                        }
                        Err(e) => return Err(e.to_string()),
                    }
                }
                "WHISPERING_HOLLOW" => {
                    match run.whispering_hollow_rewards(net, seat) {
                        Ok(rewards) => {
                            let rewards = *rewards;
                            if let Some(index) = rewards.iter().position(Option::is_some) {
                                if !run.take_whispering_hollow_reward(net, seat, index).map_err(str::to_string)? {
                                    let slot = run.roster.player(seat).unwrap().potions.iter().position(Option::is_some)
                                        .ok_or_else(|| "census harness full event potion inventory has no occupied slot".to_string())?;
                                    run.discard_potion(net, seat, slot).map_err(str::to_string)?;
                                }
                            } else {
                                run.finish_whispering_hollow_option(net, seat).map_err(str::to_string)?;
                            }
                        }
                        Err("Whispering Hollow rewards missing") => {
                            run.offer_whispering_hollow_rewards(net, seat).map_err(str::to_string)?;
                        }
                        Err(e) => return Err(e.to_string()),
                    }
                }
                _ => {
                    let options = run.event_options(net, seat).map_err(str::to_string)?;
                    if options.is_empty() { return Err("census harness event has no enabled option".into()); }
                    run.choose_event_option(net, seat, policy.index(options.len())).map_err(str::to_string)?;
                }
            }
            acted = true;
        }
        if !acted && run.phase == MpPhase::Event {
            return Err("census harness event made no progress".into());
        }
    }
    Err("census harness event action budget exceeded".into())
}

fn run_shop(run: &mut MpRun, policy: &mut Policy) -> Result<(), String> {
    for seat in 0..run.roster.players().len() {
        let net = actor(run, seat);
        let offers = run.merchant_offers(net, seat).map_err(str::to_string)?;
        let enabled: Vec<_> = offers.into_iter().filter(|offer| offer.enabled).collect();
        let selection=if policy.1 {policy.index(enabled.len()+1)} else if !enabled.is_empty() && policy.chance(1,2) {
            policy.index(enabled.len())
        } else {enabled.len()};
        if selection<enabled.len() {
            let action = enabled[selection].action;
            let relic=enabled[selection].relic;
            run.merchant_purchase(net, seat, action)
                .map_err(|error|format!("{error} relic={relic:?} seat={seat}"))?;
        }
        for _ in 0..16 {
            use sts2_sim::run::mp::MerchantPickupChoice;
            let Some(choice)=run.merchant_pickup(net,seat).map_err(str::to_string)?.cloned() else {break;};
            let selected=match choice {
                MerchantPickupChoice::Deck{cards,min,max,..}=>{
                    let count=min+policy.index(max-min+1);
                    let mut indices:Vec<_>=(0..cards.len()).collect();policy.shuffle(&mut indices);
                    indices.truncate(count);indices
                }
                MerchantPickupChoice::Cards(cards)=>{
                    let index=policy.index(cards[0].len()+1);
                    if index==cards[0].len() {vec![]}else{vec![index]}
                }
                MerchantPickupChoice::Potions(_)=>{
                    if run.roster.player(seat).unwrap().potions.iter().any(Option::is_none) && policy.index(2)==0 {
                        vec![0]
                    } else {vec![]}
                }
            };
            run.select_merchant_pickup(net,seat,&selected).map_err(str::to_string)?;
        }
    }
    run.leave_shop(actor(run, 0), 0).map_err(str::to_string)
}

fn run_rest(run: &mut MpRun, policy: &mut Policy) -> Result<(), String> {
    for _ in 0..256 {
        if run.phase != MpPhase::RestSite { return Ok(()); }
        let mut acted = false;
        for seat in 0..run.roster.players().len() {
            if run.phase != MpPhase::RestSite { return Ok(()); }
            let net = actor(run, seat);
            if let Some(cards) = run.rest_card_reward(net, seat).map_err(str::to_string)? {
                let index = policy.index(cards.len()+usize::from(policy.1));
                run.select_rest_card_reward(net, seat, (index<cards.len()).then_some(index)).map_err(str::to_string)?;
                acted = true;
                continue;
            }
            if run.rest_potion_reward(net, seat).map_err(str::to_string)?.is_some() {
                let accept = run.roster.player(seat).unwrap().potions.iter().any(Option::is_none)
                    && (!policy.1 || policy.index(2)==0);
                run.select_rest_potion_reward(net, seat, accept).map_err(str::to_string)?;
                acted = true;
                continue;
            }
            if let Some(targets) = run.rest_mend_targets(net, seat).map_err(str::to_string)?.map(<[_]>::to_vec) {
                if targets.is_empty() { return Err("census harness empty Mend selector".into()); }
                run.select_rest_mend(net, seat, Some(targets[policy.index(targets.len())])).map_err(str::to_string)?;
                acted = true;
                continue;
            }
            if let Some(choices) = run.rest_smith_choices(net, seat).map_err(str::to_string)?.map(<[_]>::to_vec) {
                if choices.is_empty() { return Err("census harness empty smith selector".into()); }
                run.select_rest_smith(net, seat, Some(choices[policy.index(choices.len())])).map_err(str::to_string)?;
                acted = true;
                continue;
            }
            let options = run.rest_options(net, seat).map_err(str::to_string)?.to_vec();
            let enabled: Vec<_> = options.iter().enumerate().filter_map(|(i, (_, enabled))| enabled.then_some(i)).collect();
            if enabled.is_empty() {
                run.skip_rest_remaining(net, seat).map_err(str::to_string)?;
            } else {
                let index=policy.index(enabled.len()+usize::from(policy.1));
                if index==enabled.len() {
                    run.skip_rest_remaining(net,seat).map_err(str::to_string)?;
                } else {
                    run.choose_rest_option(net, seat, enabled[index]).map_err(str::to_string)?;
                }
            }
            acted = true;
        }
        if !acted && run.phase == MpPhase::RestSite {
            return Err("census harness rest site made no progress".into());
        }
    }
    Err("census harness rest-site action budget exceeded".into())
}

fn run_treasure(run: &mut MpRun, policy: &mut Policy) -> Result<(), String> {
    let seats = run.roster.players().len();
    for seat in 0..seats {
        run.open_treasure(actor(run, seat), seat).map_err(str::to_string)?;
    }
    let relics = run.treasure_relics.len();
    for seat in 0..seats {
        let vote = if policy.1 {
            let index=policy.index(relics+1);
            (index<relics).then_some(index)
        } else if relics == 0 || policy.chance(1, 10) { None } else { Some(policy.index(relics)) };
        run.vote_treasure(actor(run, seat), seat, vote).map_err(str::to_string)?;
    }
    Ok(())
}

fn drive_run(run: &mut MpRun, policy: &mut Policy) -> Result<(), String> {
    for _ in 0..512 {
        match run.phase {
            MpPhase::Neow => run_neow(run, policy)?,
            MpPhase::Map => run_map(run, policy)?,
            MpPhase::Combat => run_combat(run, policy)?,
            MpPhase::Rewards => run_rewards(run, policy)?,
            MpPhase::Event => run_event(run, policy)?,
            MpPhase::Shop => run_shop(run, policy)?,
            MpPhase::RestSite => run_rest(run, policy)?,
            MpPhase::Treasure => run_treasure(run, policy)?,
            MpPhase::RunOver => return Ok(()),
        }
    }
    Err("census harness whole-run phase budget exceeded".into())
}

fn room(run: &MpRun) -> String {
    run.world.current_room.map(str::to_string).unwrap_or_else(|| format!("{:?}", run.world.run.room_type))
}

fn assert_held_out_trial_terminal(trial: usize) {
    const CHARACTERS: [Character; 5] = [
        Character::Ironclad,
        Character::Silent,
        Character::Defect,
        Character::Regent,
        Character::Necrobinder,
    ];
    let seats = 2 + trial % 3;
    let ascension = if (trial / 3) % 2 == 0 { 0 } else { 10 };
    let specs: Vec<_> = (0..seats).map(|seat| PlayerSpec {
        net_id: 10_000 + seat as u64,
        character: CHARACTERS[(trial + seat) % CHARACTERS.len()],
    }).collect();
    let seed = (9_100_000u64 + trial as u64).to_string();
    let mut policy = Policy::new(0x9e37_79b9_7f4a_7c15 ^ trial as u64);
    let mut run = MpRun::new(&seed, &specs, ascension).unwrap();
    drive_run(&mut run, &mut policy).unwrap_or_else(|error| {
        panic!("held-out trial {trial} failed at floor={} room={} error={error}",
            run.world.run.total_floor, room(&run));
    });
    assert_eq!(run.phase, MpPhase::RunOver);
}

#[test]
fn held_out_3246_hello_world_card_effect_reaches_terminal() {
    // HelloWorld.cs:19-23 applies HelloWorldPower; this trial previously
    // refused with fault_card=HelloWorldCard.
    assert_held_out_trial_terminal(3246);
}

#[test]
fn held_out_3657_endless_conveyor_always_has_a_real_option() {
    // EndlessConveyor.cs:115-148 always provides Observe/Leave even when
    // the 40-gold grab option is locked.
    assert_held_out_trial_terminal(3657);
}

#[test]
fn held_out_5437_future_of_potions_trade_reaches_terminal() {
    // TheFutureOfPotions.cs:101-138 trades one of the first three held
    // potions for an upgraded same-rarity/type CardReward.
    assert_held_out_trial_terminal(5437);
}

#[test]
fn held_out_11479_dead_sculpting_strike_owner_finishes_open_choice() {
    // CombatManager.HandlePlayerDeath leaves cards in combat until the current
    // card/potion effect ends, so its already-open CardSelectCmd remains valid.
    assert_held_out_trial_terminal(11479);
}

#[test]
#[ignore]
fn whole_multiplayer_first_fault_census() {
    const CHARACTERS: [Character; 5] = [
        Character::Ironclad,
        Character::Silent,
        Character::Defect,
        Character::Regent,
        Character::Necrobinder,
    ];
    let runs = std::env::var("STS2_MP_WHOLE_CENSUS_RUNS").ok()
        .and_then(|value| value.parse().ok()).unwrap_or(3_000usize);
    let offset = std::env::var("STS2_MP_WHOLE_CENSUS_OFFSET").ok()
        .and_then(|value| value.parse().ok()).unwrap_or(0usize);
    let verbose = std::env::var_os("STS2_MP_WHOLE_CENSUS_VERBOSE").is_some();
    let uniform = std::env::var_os("STS2_MP_WHOLE_CENSUS_UNIFORM").is_some();
    let mut faults = BTreeMap::<String, [usize; 3]>::new();
    let mut exposure = [[0usize; 2]; 3];
    let mut terminal = 0usize;
    let mut terminals=[0usize;3];

    for local in 0..runs {
        let trial = offset + local;
        let seats = 2 + trial % 3;
        let ascension = if (trial / 3) % 2 == 0 { 0 } else { 10 };
        exposure[seats - 2][usize::from(ascension != 0)] += 1;
        let mut policy = Policy::new(0x9e37_79b9_7f4a_7c15 ^ trial as u64);
        policy.1=uniform;
        let specs: Vec<_> = (0..seats).map(|seat| PlayerSpec {
            net_id: 10_000 + seat as u64,
            character: CHARACTERS[if uniform {policy.index(CHARACTERS.len())}else{(trial + seat) % CHARACTERS.len()}],
        }).collect();
        let seed = (9_100_000u64 + trial as u64).to_string();
        let mut run = match MpRun::new(&seed, &specs, ascension) {
            Ok(run) => run,
            Err(error) => {
                faults.entry(error.to_string()).or_default()[seats - 2] += 1;
                eprintln!("WHOLE_CENSUS_RUN trial={trial} seed={seed} seats={seats} asc={ascension} floor=0 room=construction fault={error}");
                continue;
            }
        };
        match drive_run(&mut run, &mut policy) {
            Ok(()) => {
                terminal += 1;
                terminals[seats-2]+=1;
                if verbose {
                    eprintln!("WHOLE_CENSUS_RUN trial={trial} seed={seed} seats={seats} asc={ascension} floor={} room={} terminal",
                        run.world.run.total_floor, room(&run));
                }
            }
            Err(error) => {
                faults.entry(error.clone()).or_default()[seats - 2] += 1;
                let fault_card = run.combat().and_then(|combat| combat.fault_card);
                let pending_rewards=run.combat().map(|combat| &combat.end_rewards);
                let relics:Vec<_>=run.roster.players().iter().map(|p|p.relics.iter()
                    .map(|r|r.id).collect::<Vec<_>>()).collect();
                eprintln!("WHOLE_CENSUS_RELICS trial={trial} relics={relics:?}");
                let rest_options:Vec<_>=(0..seats).map(|seat|run.rest_options(actor(&run,seat),seat)
                    .map(|options|options.to_vec())).collect();
                eprintln!("WHOLE_CENSUS_DETAIL trial={trial} rewards={pending_rewards:?} rest={rest_options:?}");
                eprintln!("WHOLE_CENSUS_RUN trial={trial} seed={seed} seats={seats} asc={ascension} floor={} room={} fault={error} fault_card={fault_card:?} roster={specs:?} neow={:?}",
                    run.world.run.total_floor, room(&run), run.neow);
            }
        }
    }

    let first_faults: usize = faults.values().flat_map(|counts| counts.iter()).sum();
    eprintln!("WHOLE_CENSUS attempted={runs} terminal={terminal} first_faults={first_faults}");
    for seats in 2..=4 {
        eprintln!("WHOLE_CENSUS_EXPOSURE seats={seats} a0={} a10={} terminal={} refused={}",
            exposure[seats - 2][0], exposure[seats - 2][1],terminals[seats-2],
            exposure[seats-2].iter().sum::<usize>()-terminals[seats-2]);
    }
    for (reason, counts) in &faults {
        eprintln!("WHOLE_CENSUS_FAULT total={} seats2={} seats3={} seats4={} reason={reason}",
            counts.iter().sum::<usize>(), counts[0], counts[1], counts[2]);
    }
    assert_eq!(exposure.iter().flatten().sum::<usize>(), runs);
}
