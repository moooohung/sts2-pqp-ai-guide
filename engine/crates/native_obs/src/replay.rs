//! Shared dashboard v2 projection for native re-simulation and browser WASM.
#[path = "replay/between.rs"]
pub mod between;
use super::*;
use serde_json::{Value,json};
use sts2_sim::{card::{CId, KRef}, combat::Combat, creature::{Creature, Side}};
use sts2_sim::run::{RunManager,mp::MpRun};
mod public_state;
pub enum Game<'a> { Solo(&'a RunManager), Party(&'a MpRun) }
/// Run-level facts every replay consumer (AI play, human replays) projects the same way.
impl Game<'_> {
    pub fn combat(&self)->Option<&Combat>{match self{Game::Solo(g)=>g.combat(),Game::Party(g)=>g.combat()}}
    pub fn act(&self)->u64{match self{Game::Solo(g)=>g.current_act_index() as u64+1,Game::Party(g)=>g.world.run.act_index as u64+1}}
    pub fn floor(&self)->u32{match self{Game::Solo(g)=>g.run_ctx().total_floor,Game::Party(g)=>g.world.run.total_floor}.max(0) as u32}
    /// Seat inventories, the act and the killers of a lost combat.
    pub fn inventory(&self)->Value{
        let inventory=|hp,max_hp,gold,deck:&[sts2_sim::card::Card],relics:Vec<&sts2_sim::relic::Relic>,
            potions:&[Option<sts2_sim::tables::PotionId>],capacity:usize,combat:Option<&Combat>|json!({
            "hp":hp,"max_hp":max_hp,"mhp":max_hp,"gold":gold,
            "deck":deck.iter().map(deck_card).collect::<Vec<_>>(),
            "relics":relics.iter().map(|r|r.id.name()).collect::<Vec<_>>(),
            "relic_state":relics.iter().map(|r|public_state::relic(r,combat)).collect::<Vec<_>>(),
            "pot":potions.iter().flatten().map(|p|p.name()).collect::<Vec<_>>(),"ps":capacity,
            "pv":(0..capacity).map(|i|potions.get(i).copied().flatten().map(|p|p.name())).collect::<Vec<_>>()});
        let seats=match self{
            Game::Solo(g)=>{let r=g.run_ctx();let active=g.combat().filter(|_|g.phase()==sts2_sim::run::RunPhase::Combat);
                let(hp,mhp)=active.map(|c|(c.player().hp,c.player().max_hp)).unwrap_or((r.hp,r.max_hp));
                vec![inventory(hp,mhp,r.gold,&r.deck,r.relics.iter().collect(),
                    &r.potions,r.max_potion_slots as usize,active)]},
            Game::Party(g)=>g.roster.players().iter().enumerate().map(|(seat,p)|{
                if let Some(c)=g.combat().filter(|_|g.phase==sts2_sim::run::mp::MpPhase::Combat){let owner=seat as u8;let cr=c.cr(owner);
                    let potions=c.potion_slots_for_player(owner).expect("attached player potion slots");
                    inventory(cr.hp,cr.max_hp,if seat==0{c.run.gold}else{c.players[owner].gold.unwrap_or(p.gold)},
                        c.player_deck(owner),c.run.owned_relics(owner).collect(),potions,combat_potion_capacity(c,owner,potions),Some(c))}
                else{inventory(p.hp,p.max_hp,p.gold,&p.deck,p.relics.iter().collect(),
                    &p.potions,p.max_potion_slots,None)}
            }).collect::<Vec<_>>(),
        };
        let mut result=seats[0].clone();result["seats"]=json!(seats);result["act"]=json!(self.act());
        result["killer"]=json!(self.combat().filter(|c|c.is_over&&!c.won).map(|c|c.enemies.iter().map(|&id|c.cr(id).monster_id).collect::<Vec<_>>()).unwrap_or_default());
        result
    }
}
pub struct RealSource<'a> {
    pub game:Game<'a>,pub obs:&'a NativeObsV1,pub decision:DecisionId,
    pub team:Option<&'a TeamObsV2>,
    pub seats:usize,pub inventory:Value,pub floor:u32,
}
fn room(kind:sts2_sim::tables::MapPointType)->&'static str {
    match kind.name() {
        "Monster"=>"MONSTER","Elite"=>"ELITE","Boss"=>"BOSS","Shop"=>"SHOP",
        "RestSite"=>"REST_SITE","Treasure"=>"TREASURE","Unknown"=>"UNKNOWN","Ancient"=>"ANCIENT",
        other=>other,
    }
}
fn name(id:ContentId)->Option<&'static str> {
    use sts2_sim::tables::{CardId,RelicId,PotionId,MapPointType,CardType,VarKey};
    let i=id.value.checked_sub(1)? as usize;
    match id.namespace {
        Namespace::CardId=>CardId::ALL.get(i).map(|id|id.name()),
        Namespace::RelicId=>RelicId::ALL.get(i).map(|id|id.name()),
        Namespace::PotionId=>PotionId::ALL.get(i).map(|id|id.name()),
        Namespace::MapPointType=>MapPointType::ALL.get(i).map(|id|room(*id)),
        Namespace::CardType=>CardType::ALL.get(i).map(|id|id.name()),
        Namespace::VarKey=>VarKey::ALL.get(i).map(|id|id.name()),
        _=>id.namespace.registry()?.names.get(i).copied(),
    }
}
fn powers(c: &Combat, creature: &Creature) -> Vec<Value> {
    creature.power_list().iter().map(|&p| {
        let p = &c.powers[p as usize]; json!([p.id.name(), p.amount])
    }).collect()
}
fn card(c: &Combat, k: KRef) -> Value {
    card_snapshot(&c.cards[k as usize])
}
pub(super) fn card_snapshot(card:&sts2_sim::card::Card)->Value {
    let mut tuple = vec![json!(card.id.name()), json!(u8::from(card.upgraded)), json!(card.cost),
        json!(card.original_cost), json!(card.damage), json!(card.block)];
    if let Some(runtime) = card_runtime(card) { tuple.push(runtime); }
    json!(tuple)
}

/// Episode inventories and replay decks share the same per-instance projection.
pub fn deck_card(card: &sts2_sim::card::Card) -> Value {
    let mut tuple = vec![json!(card.id.name()), json!(u8::from(card.upgraded))];
    if let Some(runtime) = card_runtime(card) { tuple.push(runtime); }
    json!(tuple)
}

fn runtime_config(id: &str, card_type: &str, vars: impl Iterator<Item = (&'static str, i32)>) -> Option<Value> {
    if id != "MAD_SCIENCE" { return None; }
    let vars: serde_json::Map<_,_> = vars.map(|(key,value)| (key.to_string(),json!(value))).collect();
    let rider = vars.get("rider")?.as_i64()?;
    Some(json!({"tinker_type":card_type,"tinker_rider":rider,
        "generated_fields":{"effect_vars":vars}}))
}

fn card_runtime(card: &sts2_sim::card::Card) -> Option<Value> {
    runtime_config(card.id.name(), card.card_type.name(), card.vars.iter().map(|(key,value)| (key.name(),*value)))
}

// Offered cards have the same retained vars as live cards. Read their existing
// source row, not the card model ID (two Mad Science instances may differ).
fn row_runtime(view: &ObservationView<'_>, row: usize) -> Option<Value> {
    let model = name(view.rows.model[row])?;
    let card_type = name(view.rows.detail[row][0])?;
    runtime_config(model,card_type,(0..view.state_len).filter(|&r|
        view.rows.kind[r] == RowKind::Modifier && view.rows.parent[r].index() == Some(row))
        .filter_map(|r| {
            let var = view.rows.detail[r][0];
            (var.namespace == Namespace::VarKey).then(|| name(var).map(|key| (key,view.rows.integers[r][0]))).flatten()
        }))
}
fn target(c: &Combat, id: u32) -> Result<Value, String> {
    let creature = c.creatures.get(id as usize).ok_or("replay target missing")?;
    Ok(json!({"id":id,"combat_id":creature.combat_id,
        "kind":if creature.side == Side::Enemy {"enemy"} else if creature.is_player {"player"}
            else if creature.is_pet {"pet"} else {"ally"},
        "owner":if creature.is_player {Some(id)} else {creature.pet_owner.map(u32::from)}}))
}
fn pets(c: &Combat, owner: CId) -> Vec<Value> {
    c.creatures.iter().enumerate().filter(|(_, cr)| !cr.is_player && cr.side == Side::Player
        && cr.pet_owner == Some(owner)).map(|(id, cr)| json!({
            "id":id,"combat_id":cr.combat_id,"owner":owner,"monster_id":cr.monster_id,
            "hp":cr.hp,"mhp":cr.max_hp,"blk":cr.block,"pw":powers(c,cr),
            "alive":cr.is_alive(),"in_combat":cr.in_combat,"escaped":cr.escaped,"slot":cr.slot,
        })).collect()
}
fn combat_entities(c: &Combat) -> Vec<Value> {
    c.creatures.iter().enumerate().map(|(id, creature)| {
        let cid = id as CId;
        let kind = if creature.is_player {"player"} else if creature.is_pet {"pet"}
            else if creature.side == Side::Enemy {"enemy"} else {"ally"};
        let display_slot = if creature.is_player {Some(id)}
            else if creature.side == Side::Enemy {c.enemies.iter().position(|&e| e == cid)}
            else {c.allies.iter().position(|&e| e == cid)};
        json!({"combat_id":c.between_instance,"id":id,"kind":kind,
            "owner_id":if creature.is_player {Some(cid)} else {creature.pet_owner},
            "model":creature.monster_id,"display_slot":display_slot,"slot":creature.slot})
    }).collect()
}
/// Solo combat keeps player zero's potions compact (only grown by procurement),
/// so its slot count is the run's capacity, as in the policy encoder; MP seats
/// carry explicit slot vectors.
fn combat_potion_capacity(c: &Combat, owner: CId, slots: &[Option<sts2_sim::tables::PotionId>]) -> usize {
    if owner == sts2_sim::combat::PLAYER { c.run.max_potion_slots.max(0) as usize } else { slots.len() }
}
fn combat_player_public(c: &Combat, owner: CId, player: &mut Value) {
    let p = &c.players[owner]; let cr = c.cr(owner);
    player["hp"] = json!(cr.hp); player["mhp"] = json!(cr.max_hp);
    player["blk"] = json!(cr.block); player["en"] = json!(p.energy);
    player["men"] = json!(p.base_max_energy); player["stars"] = json!(p.stars);
    player["pw"] = json!(powers(c, cr));
    let slots=c.potion_slots_for_player(owner).expect("attached player potion slots");
    let capacity=combat_potion_capacity(c,owner,slots);
    player["pot"] = json!(slots.iter().flatten().map(|p|p.name()).collect::<Vec<_>>());
    player["ps"] = json!(capacity);
    player["pv"] = json!((0..capacity).map(|i|slots.get(i).copied().flatten().map(|p|p.name())).collect::<Vec<_>>());
    player["piles"] = json!([p.draw.len(), p.discard.len(), p.exhaust.len()]);
    player["orbs"] = json!(p.orbs.orbs.iter().map(|&orb| json!({
        "orb_id":orb.instance_id,"kind":orb.kind.name(),"val":orb.val,"passive":c.orb_passive_value(owner,orb),
        "evoke":c.orb_evoke_value(owner,orb),
    })).collect::<Vec<_>>());
    player["orb_slots"] = json!(p.orbs.capacity);
    player["base_orb_slots"] = json!(p.orbs.base_slots);
    player["pets"] = json!(pets(c,owner));
    player["turn"] = json!(p.turn_number);
    player["ready"] = json!(c.is_player_ready_to_end_turn(owner));
    player["participating"] = json!(c.is_player_taking_turn(owner));
}
fn combat_player(c: &Combat, owner: CId, player: &mut Value) {
    combat_player_public(c,owner,player);
    let p = &c.players[owner];
    player["hand"] = json!(p.hand.iter().map(|&k| card(c,k)).collect::<Vec<_>>());
    player["hand_preview"] = json!(p.hand.iter().map(|&k| {
        let card = &c.cards[k as usize];
        let mut preview = json!({"instance_id":card.instance_id,"owner":card.owner});
        match c.hand_card_preview(k) {
            Ok(vars) => preview["vars"] = json!(vars.into_iter().map(|v|
                (v.name.to_string(),json!({"value":v.value,"enchanted":v.enchanted}))).collect::<serde_json::Map<_,_>>()),
            Err(reason) => preview["error"] = json!(reason),
        }
        preview
    }).collect::<Vec<_>>());
    // The game greys a hand card exactly when it cannot be played now.
    player["hand_playable"] = json!(p.hand.iter().map(|&k| c.can_play_card_readonly(k).unwrap_or(false)).collect::<Vec<_>>());
    player["hand_instances"] = json!(p.hand.iter().map(|&k| {
        let card = &c.cards[k as usize];
        json!({"instance_id":card.instance_id,"combat_card_id":k,"owner":card.owner,
            "card_id":card.id.name(),"upgraded":card.upgraded})
    }).collect::<Vec<_>>());
    player["pile_cards"] = json!({"draw":p.draw.iter().map(|&k| card(c,k)).collect::<Vec<_>>(),
        "discard":p.discard.iter().map(|&k| card(c,k)).collect::<Vec<_>>(),
        "exhaust":p.exhaust.iter().map(|&k| card(c,k)).collect::<Vec<_>>()});
}

#[cfg(test)]
mod hand_preview_tests {
    use super::*;
    use sts2_sim::{combat::Combat, relic::Relic, rng::Rng,
        run::{RunManager, RunOptions}, tables::{CardId, PowerId, RelicId}};

    #[test]
    fn generated_card_configuration_follows_the_instance_through_hand_and_every_pile() {
        use sts2_sim::tables::{CardType, VarKey};
        let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run,7);
        let k = c.new_card(0,CardId::MadScience,false).unwrap();
        c.cards[k as usize].card_type = CardType::Skill;
        c.cards[k as usize].vars.retain(|(key,_)| *key != VarKey::Rider);
        c.cards[k as usize].vars.push((VarKey::Rider,5));
        let expected = deck_card(&c.cards[k as usize])[2].clone();
        assert_eq!(expected["tinker_type"],"SKILL");
        assert_eq!(expected["tinker_rider"],5);
        for pile in 0..4 {
            c.players[0].hand.clear(); c.players[0].draw.clear();
            c.players[0].discard.clear(); c.players[0].exhaust.clear();
            match pile {
                0 => c.players[0].hand.push(k), 1 => c.players[0].draw.push(k),
                2 => c.players[0].discard.push(k), _ => c.players[0].exhaust.push(k),
            }
            let mut player = json!({});
            combat_player(&c,0,&mut player);
            let cards = match pile {
                0 => &player["hand"],1 => &player["pile_cards"]["draw"],
                2 => &player["pile_cards"]["discard"],_ => &player["pile_cards"]["exhaust"],
            };
            assert_eq!(cards[0][6],expected);
        }
        let clone = c.cards[k as usize].clone_with_id(999);
        assert_eq!(deck_card(&clone)[2],expected);
    }

    #[test]
    fn hand_card_previews_use_each_owners_hooks_without_an_enemy_target_or_mutation() {
        for seats in [1, 2, 4] {
            let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
            let mut c = Combat::new(run, 7);
            for _ in 1..seats { c.add_player(80, 80, 3, 0, &[]); }
            c.run.relics.clear();
            c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(7));
            for owner in 0..seats as u8 {
                c.apply_power_to(owner, PowerId::Strength, 2 + i32::from(owner), None);
                c.apply_power_to(owner, PowerId::Dexterity, 1 + i32::from(owner), None);
                c.apply_power_to(owner, PowerId::Vigor, 3, None);
                c.apply_power_to(owner, PowerId::Weak, 1, None);
                c.apply_power_to(owner, PowerId::Frail, 1, None);
                let strike = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
                assert!(c.cards[strike as usize].apply_static_enchantment("Sharp", 2));
                let defend = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
                assert!(c.cards[defend as usize].apply_static_enchantment("Nimble", 2));
                let instinct = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
                assert!(c.cards[instinct as usize].apply_static_enchantment("Instinct", 1));
                let corrupted = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
                assert!(c.cards[corrupted as usize].apply_static_enchantment("Corrupted", 1));
                let burn = c.new_card(owner, CardId::Burn, false).unwrap();
                c.players[owner].hand.clear();
                c.players[owner].hand.extend([strike, defend, instinct, corrupted, burn]);
            }
            let mut nib = Relic::new(RelicId::PenNib);
            nib.owner = seats as u8 - 1;
            nib.counter = 9;
            c.run.relics.push(nib);
            for enemy in c.enemies.clone() {
                c.apply_power_to(enemy, PowerId::Vulnerable, 99, None);
                c.apply_power_to(enemy, PowerId::Weak, 99, None);
                c.apply_power_to(enemy, PowerId::Intangible, 1, None);
            }
            let before = format!("{c:?}");
            let mut players = Vec::new();
            for owner in 0..seats as u8 {
                let mut player = json!({"actor":owner});
                combat_player(&c, owner, &mut player);
                let previews = &player["hand_preview"];
                let expected = if usize::from(owner) == seats - 1 { [19,21,22,24] } else { [9,10,11,12] };
                assert_eq!(previews[0]["vars"]["Damage"], json!({"value":expected[owner as usize],"enchanted":8}),
                    "enchantment then Strength/Vigor then self Weak/Pen Nib, never enemy Vulnerable/Intangible");
                let expected_block = [6,6,7,8][owner as usize];
                assert_eq!(previews[1]["vars"]["Block"], json!({"value":expected_block,"enchanted":7}));
                let instinct = if usize::from(owner) == seats - 1 { [25,27,28,30] } else { [12,13,14,15] };
                let corrupted = if usize::from(owner) == seats - 1 { [21,22,24,25] } else { [10,11,12,12] };
                assert_eq!(previews[2]["vars"]["Damage"], json!({"value":instinct[owner as usize],"enchanted":12}));
                assert_eq!(previews[3]["vars"]["Damage"], json!({"value":corrupted[owner as usize],"enchanted":9}));
                assert_eq!(previews[4]["vars"]["Damage"], json!({"value":2,"enchanted":2}),"Unpowered is not modified by Strength, Vigor or Weak");
                assert_eq!(previews[0]["instance_id"], player["hand_instances"][0]["instance_id"]);
                assert_eq!(previews[0]["owner"], owner);
                assert_eq!(player["hand"][0][4], 6, "legacy tuple remains raw and backward compatible");
                players.push(player);
            }
            assert_eq!(format!("{c:?}"), before, "projection cannot consume Vigor/Pen Nib or change RNG/fault");
            if seats == 2 {
                if let Ok(path) = std::env::var("STS2_HAND_PREVIEW_FRAME") {
                    std::fs::write(path, serde_json::to_vec_pretty(&json!({
                        "ph":"COMBAT","actor":1,"players":players
                    })).unwrap()).unwrap();
                }
            }
            c.apply_power_to(0,PowerId::Strength,-200,None);
            c.apply_power_to(0,PowerId::Dexterity,-200,None);
            let mut zero = json!({});
            combat_player(&c,0,&mut zero);
            assert_eq!(zero["hand_preview"][0]["vars"]["Damage"]["value"],0);
            assert_eq!(zero["hand_preview"][1]["vars"]["Block"]["value"],0);
        }
    }
}
fn foes(c:&Combat,actor:CId)->Vec<Value>{
    c.enemies.iter().map(|&id|{
        let enemy=c.cr(id);let mut kinds=vec![];let(mut damage,mut hits)=(0,0);
        if let Some(ai)=&enemy.ai {if let Some(next)=ai.peek_move() {
            for i in 0..ai.intents(next).len() {
                if let Ok(Some(intent))=c.public_intent(id,actor,i) {
                    kinds.push(format!("{:?}",intent.kind));
                    if intent.damage>0 {damage=intent.damage;hits=intent.hits;}
                }
            }
        }}
        json!([enemy.monster_id,enemy.hp,enemy.max_hp,enemy.block,kinds,damage,hits,
            enemy.hp<=0||enemy.escaped,powers(c,enemy)])
    }).collect()
}
impl RealSource<'_> {
    /// The seat's network id as its run was created (AI play 100+seat, human recordings seat+1).
    fn net(&self,actor:u8)->u64{
        match &self.game {Game::Solo(_)=>1,Game::Party(g)=>g.roster.players()[actor as usize].net_id}
    }
    fn phase(&self,actor:u8)->&'static str{
        match &self.game {
            Game::Solo(g)=>g.phase().name(),
            Game::Party(g)=>{
                if g.scroll_box_bundles[actor as usize].is_some() {return "CARD_REWARD";}
                use sts2_sim::run::mp::MpPhase::*;
                match g.phase {Neow|Event=>"EVENT",Map=>"MAP_CHOICE",Combat=>"COMBAT",
                    Rewards=>"CARD_REWARD",Shop=>"SHOP",RestSite=>"REST_SITE",Treasure=>"TREASURE",RunOver=>"RUN_OVER"}
            }
        }
    }
    fn players(&self,combat:Option<&Combat>)->Result<Value,String>{
        Ok(json!(self.inventory["seats"].as_array().ok_or("missing replay seats")?.iter().enumerate().map(|(actor,seat)|{
            let mut player=seat.clone();
            player["actor"]=json!(actor);
            player["character"]=json!(match &self.game {
                Game::Solo(g)=>g.native_character().name(),Game::Party(g)=>g.roster.players()[actor].character.name(),
            });
            player["deck_cards"]=seat["deck"].clone();
            if let Some(c)=combat {combat_player(c,actor as CId,&mut player);}
            player
        }).collect::<Vec<_>>()))
    }
    pub fn queue_cursor(&self)->usize{
        match &self.game {Game::Solo(_)=>0,Game::Party(g)=>g.queue_events.len()}
    }
    pub fn queue_events_since(&self,from:usize)->Result<Value,String>{
        let events=match &self.game {Game::Solo(_)=>&[][..],Game::Party(g)=>g.queue_events.as_slice()};
        let events=events.get(from..).ok_or("queue event cursor is stale")?;
        Ok(json!(events.iter().map(|row|{
            let e=&row.event;
            let mut event=json!({"seq":e.sequence,"combat":row.combat,"action_id":e.id,
                "original_action_id":e.original_id,"parent_action_id":e.parent,"actor":e.owner,
                "event":e.event,"action":e.action.map(|a|a.name()),"turn":e.turn,"round":e.round});
            match e.action {
                Some(sts2_sim::action_queue::CombatAction::PlayCard{card,target})=>{
                    event["combat_card_id"]=json!(card);event["target_id"]=json!(target);
                }
                Some(sts2_sim::action_queue::CombatAction::UsePotion{slot,target})=>{
                    event["potion_slot"]=json!(slot);event["target_id"]=json!(target);
                }
                _=>{}
            }
            event
        }).collect::<Vec<_>>()))
    }
    pub fn replay_snapshot(&self,actor:CId)->Result<Value,String>{
        let phase=self.phase(actor);
        let combat=match &self.game {Game::Solo(g)=>g.combat(),Game::Party(g)=>g.combat()}.filter(|_|phase=="COMBAT");
        let mut state=json!({"ph":phase,"fl":self.floor,"act":self.inventory["act"],
            "party_size":self.seats,"actor":actor,"players":self.players(combat)?});
        if let Some(c)=combat {
            state["turn"]=json!(c.round);state["foes"]=json!(foes(c,actor));
            state["foe_ids"]=json!(c.enemies.as_slice());
            state["combat_id"]=json!(c.between_instance);
            state["entity_table"]=json!(combat_entities(c));
            state["summons"]=json!((0..self.seats).flat_map(|owner|pets(c,owner as CId)).collect::<Vec<_>>());
        }
        Ok(state)
    }
    pub fn replay_map(&self)->Value {
        let map=match &self.game {
            Game::Solo(g)=>g.visible_map(),Game::Party(g)=>Some(&g.world.map),
        };
        json!(map.map(|m|m.points.iter().map(|p|json!([p.coord.col,p.coord.row,
            room(p.point_type),p.children.iter().map(|&i|{
                let c=m.points[i as usize].coord;[c.col,c.row]
            }).collect::<Vec<_>>()])).collect::<Vec<_>>()).unwrap_or_default())
    }
    /// Snapshot the real simulator state before applying the original candidate slot.
    /// Cold-path replay consumers must re-simulate seed/actions, not call this in rollouts.
    pub fn replay_frame(&self,slot:i32,index:usize)->Result<Value,String> {
        let view=match self.team {Some(team)=>team.view(),None=>self.obs.view()}.map_err(|e|format!("{e:?}"))?;
        if slot<0 || !view.candidates.sampleable(slot as usize,view.candidate_len) {
            return Err("replay action not legal".into());
        }
        let actor=view.candidates.actor[slot as usize];
        let inventory=&self.inventory;
        let seat=&inventory["seats"][actor as usize];
        let (phase,event,combat)=match &self.game {
            Game::Solo(g)=>(self.phase(actor),if g.native_neow_offers().is_some() {Some("Neow".into())}
                else {g.current_event_id().map(str::to_string)},g.combat()),
            Game::Party(g)=>{
                use sts2_sim::run::mp::MpPhase::*;
                (self.phase(actor),if g.phase==Neow {Some("Neow".into())}else {
                    g.native_event_id(actor as usize).and_then(|e|name(e.into())).map(str::to_string)
                },g.combat())
            },
        };
        let mut options=Vec::new();
        let combat=combat.filter(|_|phase=="COMBAT");
        let mut chosen=None;
        for i in 0..view.candidate_len {
            if !view.candidates.sampleable(i,view.candidate_len) {continue;}
            let net=self.net(actor);
            let binding=match self.team {
                Some(team)=>match team.binding(i,self.decision).map_err(|e|format!("{e:?}"))?.action {
                    TeamAction::Seat {binding,..}=>Some(binding),TeamAction::UnendTurn=>None,
                },
                None=>Some(self.obs.binding(i,self.decision,net).map_err(|e|format!("{e:?}"))?),
            };
            let kind=view.candidates.action[i];
            use ActionKind as A;
            let action=match kind {
                A::ChooseNeow|A::EventChoice=>"event_choice",A::Move=>"move",
                A::PlayCard=>"play_card",A::EndTurn|A::ReadyForEnemyTurn=>"end_turn",A::UnendTurn=>"unend_turn",
                A::UsePotion=>"use_potion",A::DiscardPotion=>"discard_potion",
                A::PickCard|A::PickSpecialCard=>"pick_card",A::PickCardBundle=>"pick_card_bundle",
                A::PickPotion=>"pick_potion",A::SkipPotion=>"skip_potion",
                A::PickRelicReward=>"pick_relic_reward",A::SkipRelic=>"skip_relic",
                A::BuyCard=>"buy_card",A::BuyRelic=>"buy_relic",A::BuyPotion=>"buy_potion",
                A::LeaveShop=>"leave_shop",A::RestOption=>"rest_option",
                A::Collect|A::TreasureOpen|A::TreasureVote=>"collect",A::TreasureSkip|A::Skip=>"skip",
                A::Choose=>"choose",A::ConfirmChoice=>"confirm_choice",
                A::SkipRewards|A::TakeGoldSkipRemaining=>"skip_rewards",A::OpenReward=>"open_reward",
                A::BeginRemoval|A::RemoveCard=>"remove_card",A::CancelRemoval=>"cancel_removal",
                A::RerollCardReward=>"reroll_card_reward",A::SacrificeCardReward=>"sacrifice_card_reward",
                A::Absent=>return Err("absent replay action".into()),
            };
            let mut option=json!({"action":action,"slot":i,"actor":view.candidates.actor[i]});
            let source=view.candidates.source[i].index();
            let model=if name(view.candidates.model[i]).is_some() {view.candidates.model[i]}
                else {source.map(|r|view.rows.model[r]).unwrap_or(ContentId::ABSENT)};
            let key=match model.namespace {Namespace::CardId=>"card_id",Namespace::RelicId=>"relic_id",
                Namespace::PotionId=>"potion_id",Namespace::EventOptionId=>"option_id",_=>""};
            if !key.is_empty() {if let Some(n)=name(model) {option[key]=json!(n);}}
            if let Some(r)=source {
                let offered=view.rows.model[r];
                let offered_key=match offered.namespace {Namespace::CardId=>"card_id",
                    Namespace::RelicId=>"relic_id",Namespace::PotionId=>"potion_id",_=>""};
                if !offered_key.is_empty() {if let Some(n)=name(offered) {
                    option[offered_key]=json!(n);option["label"]=json!(n);
                }}
                if view.rows.kind[r]==RowKind::Offer {option["price"]=json!(view.rows.integers[r][0]);}
                if view.rows.kind[r]==RowKind::Card {
                    option["upgraded"]=json!(view.rows.flags[r]&Flag::Upgraded.bit()!=0);
                    if let Some(runtime) = row_runtime(&view,r) { option["runtime_vars"] = runtime; }
                }
                if kind==A::PickCardBundle {
                    option["index"]=json!(view.rows.integers[r][1]);
                    let cards=(0..view.state_len).filter(|&row|view.rows.kind[row]==RowKind::Card
                        && view.rows.parent[row]==view.candidates.source[i]).collect::<Vec<_>>();
                    option["card_ids"]=json!(cards.iter().map(|&row|name(view.rows.model[row]))
                        .collect::<Vec<_>>());
                    option["upgraded"]=json!(cards.iter().map(|&row|
                        view.rows.flags[row]&Flag::Upgraded.bit()!=0).collect::<Vec<_>>());
                    let runtimes = cards.iter().map(|&row|row_runtime(&view,row)).collect::<Vec<_>>();
                    if runtimes.iter().any(Option::is_some) { option["card_runtime_vars"] = json!(runtimes); }
                }
                for id in view.rows.detail[r] {
                    if let Some(n)=name(id) {
                        match id.namespace {
                            Namespace::EventOptionId=>option["option_id"]=json!(n),
                            Namespace::RestOptionKind=>option["option_id"]=json!(n),
                            Namespace::MapPointType=>option["point_type"]=json!(n),
                            _=>{},
                        }
                    }
                }
            }
            if let Some(binding)=binding {
            match binding.command {
                Command::Solo(SoloCommand::RestOption{index})=>{
                    if let Game::Solo(g)=&self.game {
                        if let Some(action)=g.native_rest_action(index as usize) {
                            option["option_id"]=json!(action.option_id);
                        }
                    }
                },
                Command::MpRestOption{index}=>{
                    if let Game::Party(g)=&self.game {
                        if let Ok(options)=g.rest_options(self.net(actor),actor as usize) {
                            option["option_id"]=json!(options.get(index as usize).map(|o|o.0));
                        }
                    }
                },
                Command::MpChooseNeow{option:index}=>{
                    if let Game::Party(g)=&self.game {
                        option["option_id"]=json!(g.native_neow_option_id(actor as usize,index as usize));
                    }
                },
                Command::MpEventOption{option:index}|Command::MpBridgeOption{option:index}=>{
                    if let Game::Party(g)=&self.game {
                        if let Ok(options)=g.event_options(self.net(actor),actor as usize) {
                            option["option_id"]=json!(options.get(index as usize));
                        }
                        if let Some(entry)=g.native_event_options(self.net(actor),actor as usize)?
                            .and_then(|options|options.iter().filter(|o|o.enabled).nth(index as usize)) {
                            public_state::event_offer(g.native_event_offer(actor as usize,entry.option_id),
                                g.native_event_offer_detail(actor as usize,entry.option_id),&mut option);
                        }
                    }
                },
                Command::Solo(SoloCommand::Move{x,y})|Command::MpMove{x,y}=>option["coord"]=json!([x,y]),
                Command::Solo(SoloCommand::BuyCard{index}|SoloCommand::BuyRelic{index}|SoloCommand::BuyPotion{index})|
                Command::MpMerchant(MerchantCommand::BuyCard{index}|MerchantCommand::BuyRelic{index}|MerchantCommand::BuyPotion{index})=>{
                    option["index"]=json!(index);
                },
                Command::Solo(SoloCommand::PlayCard{hand,target})|Command::MpPlayCard{hand,target}=>{
                    option["hand_index"]=json!(hand);
                    if let Some(target)=target {
                        option["target_index"]=json!(target);
                    }
                },
                Command::Solo(SoloCommand::EventChoice{..})|
                Command::Solo(SoloCommand::EventChoiceAt{..})=>{
                    if let (Game::Solo(g),Some(r))=(&self.game,source) {
                        if let Some(action)=g.native_event_action(view.rows.integers[r][0] as usize) {
                            option["option_id"]=json!(action.option_id);
                            if let Some(id)=action.event_option_id {
                                public_state::event_offer(g.native_event_offer(id),g.native_event_offer_detail(id),&mut option);
                            }
                        }
                    }
                },
                _=>{},
            }
            if matches!(kind, A::PlayCard | A::UsePotion) {
                if let Some(id) = binding.target_instance {
                    option["target"] = target(combat.ok_or("action target outside combat")?, id)?;
                }
            }
            }
            if i==slot as usize {chosen=Some(option.clone());}
            options.push(option);
        }
        let mut frame=json!({"i":index,"ph":phase,"fl":self.floor,"act":inventory["act"],
            "actor":actor,"character":match &self.game {
                Game::Solo(g)=>g.native_character().name(),
                Game::Party(g)=>g.roster.player(actor as usize).unwrap().character.name()},
            "hp":seat["hp"],"mhp":seat["max_hp"],"gold":seat["gold"],
            "deck":seat["deck"].as_array().map(Vec::len),"deck_cards":seat["deck"],"rel":seat["relics"],
            "ev":event,"opts":options,"a":chosen.ok_or("replay action not legal")?});
        frame["party_size"]=json!(self.seats);
        frame["players"]=self.players(combat)?;
        for field in ["pot","ps","pv","relic_state"] {frame[field]=frame["players"][actor as usize][field].clone();}
        if phase=="REST_SITE" {
            let heal=match &self.game {Game::Solo(g)=>Some(g.native_rest_heal_amount()),
                Game::Party(g)=>g.native_rest_heal_amount(actor as usize)};
            if let Some(heal)=heal {frame["rest_text_vars"]=json!({"Heal":heal});}
        }
        if phase=="EVENT" {
            let bridge=match &self.game {Game::Solo(g)=>g.native_bridge_offer(),
                Game::Party(g)=>g.native_bridge_offer(actor as usize)};
            if let Some((damage,card))=bridge {
                frame["event_text_vars"]=json!({"HpLoss":damage});
                frame["event_model_vars"]=json!({"RandomCard":{"kind":"cards","id":card.id.name(),"upgraded":card.upgraded}});
            }
            if let Game::Solo(g)=&self.game {
                if let Some((potion,relic))=g.observation_ranwid_offers() {
                    let mut models=json!({});
                    if let Some((_,id))=potion {models["Potion"]=json!({"kind":"potions","id":id.name()});}
                    if let Some((_,id))=relic {models["Relic"]=json!({"kind":"relics","id":id.name()});}
                    frame["event_model_vars"]=models;
                }
            }
            let event_id=match &self.game {Game::Solo(g)=>g.native_event_id(),
                Game::Party(g)=>g.native_event_id(actor as usize)};
            if event_id==Some(sts2_sim::identity::EventId::LostWisp) {
                frame["event_model_vars"]=json!({
                    "Relic":{"kind":"relics","id":sts2_sim::tables::RelicId::LostWisp.name()},
                    "Curse":{"kind":"cards","id":sts2_sim::tables::CardId::Decay.name()}});
            }
            let featured=match &self.game {Game::Solo(g)=>g.native_wongo_featured(),
                Game::Party(g)=>g.native_wongo_featured(actor as usize)};
            if let Some(id)=featured {
                frame["event_model_vars"]=json!({"RandomRelic":{"kind":"relics","id":id.name()}});
            }
        }
        if phase=="SHOP" {
            use sts2_sim::run::NativeShopKind as K;
            let offers:Vec<_>=match &self.game {
                Game::Solo(g)=>g.native_shop_offers().collect(),
                Game::Party(g)=>g.native_shop_offers(actor as usize)?.collect(),
            };
            frame["shop_stock"]=json!(offers.into_iter().map(|o|{
                let mut stock=json!({"action":match o.kind {K::Card=>"buy_card",K::Relic=>"buy_relic",
                    K::Potion=>"buy_potion",K::Removal=>"remove_card"},"actor":actor,"index":o.index,
                    "price":o.price,"on_sale":o.on_sale,"sold":o.sold,"enabled":o.enabled,
                    "affordable":seat["gold"].as_i64().is_some_and(|gold|gold>=i64::from(o.price))});
                if let Some(card)=o.card {stock["card_id"]=json!(card.id.name());stock["upgraded"]=json!(card.upgraded);}
                if let Some(relic)=o.relic {stock["relic_id"]=json!(relic.name());}
                if let Some(potion)=o.potion {stock["potion_id"]=json!(potion.name());}
                stock
            }).collect::<Vec<_>>());
        }
        if let Game::Party(g)=&self.game {
            frame["map_votes"]=json!(g.voting.votes().iter().map(|v|v.as_ref().map(|v|
                json!({"coord":[v.coord.col,v.coord.row],"generation":v.generation}))).collect::<Vec<_>>());
        }
        if let Some(c)=combat {
            for field in ["blk","en","men","stars","pw","pot","hand","hand_instances","hand_playable","hand_preview","piles",
                "pile_cards","orbs","orb_slots","base_orb_slots","pets","ready","participating"] {
                frame[field]=frame["players"][actor as usize][field].clone();
            }
            frame["turn"]=json!(c.round);
            frame["summons"]=json!((0..self.seats).flat_map(|owner|pets(c,owner as CId)).collect::<Vec<_>>());
            frame["foes"]=json!(foes(c,actor));
            frame["foe_ids"]=json!(c.enemies.as_slice());
            frame["combat_id"]=json!(c.between_instance);
            frame["entity_table"]=json!(combat_entities(c));
        }
        Ok(frame)
    }
}
