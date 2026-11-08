//! Public sts2-between-decision-v1 DTOs; shared by trainer and WASM.
use serde_json::{Value,json};
use sts2_sim::between::{CreatureRef,PublicState,Recorder,Source,Details,MAX_JSON_INTEGER};
use super::{Game,RealSource};

pub const SCHEMA: &str = "sts2-between-decision-v1";
pub struct Origin { index:usize, endpoint:Value }

fn creature(c:&CreatureRef)->Value {json!({"kind":c.kind,"owner":c.owner,
    "creature_id":c.creature_id,"combat_instance":c.combat_instance})}
fn state(s:PublicState)->Value {json!({"model":s.model,"hp":s.hp,"max_hp":s.max_hp,"block":s.block,
    "alive":s.alive,"in_combat":s.in_combat,"escaped":s.escaped})}
fn endpoint(game:&Game<'_>)->Value {
    let active=match game {
        Game::Solo(g)=>g.phase()==sts2_sim::run::RunPhase::Combat,
        Game::Party(g)=>g.phase==sts2_sim::run::mp::MpPhase::Combat,
    };
    json!({"act":game.act(),"floor":game.floor(),
        "combat_instance":game.combat().filter(|_|active).map(|c|c.between_instance).filter(|&id|id!=0)})
}

pub fn origin(game:&Game<'_>,index:usize)->Origin {
    let mut endpoint=endpoint(game);endpoint["decision"]=json!(index);
    Origin {index,endpoint}
}

pub fn events(recorder:&Recorder)->Result<Value,String> {
    events_for_actor(recorder,None)
}
pub fn orb_data(recorder:&Recorder)->Result<Value,String> {
    let activations=recorder.orb_activations();
    let traces=recorder.orb_traces();
    if activations.iter().any(|e|e.sequence>MAX_JSON_INTEGER||e.combat_id>MAX_JSON_INTEGER
        ||e.action_id>MAX_JSON_INTEGER||e.orb_id>MAX_JSON_INTEGER)
        ||traces.iter().any(|e|e.sequence>MAX_JSON_INTEGER||e.combat_id>MAX_JSON_INTEGER) {
        return Err("orb event exceeds JSON safe integer range".into());
    }
    let mut events:Vec<Value>=activations.iter().map(|e|json!({
        "event":"activation","combat_id":e.combat_id,"action_id":e.action_id,
        "sequence":e.sequence,"activation_id":e.activation_id,
        "parent_activation_id":e.parent_activation_id,"owner_id":e.owner_id,
        "orb_id":e.orb_id,"orb_kind":e.orb_kind,"phase":e.phase,
        "activation_index":e.activation_index,"targets":e.targets,
        "display_slots":e.display_slots.iter().map(|(id,slot)|
            json!({"id":id,"display_slot":slot})).collect::<Vec<_>>(),
        "dequeued":e.dequeued,
    })).chain(traces.iter().map(|e|json!({
        "event":"trace","combat_id":e.combat_id,"action_id":e.action_id,
        "sequence":e.sequence,"activation_id":e.activation_id,"span_id":e.span_id,
        "parent_span_id":e.parent_span_id,"operation":e.operation,
        "transition":e.transition,"target_id":e.target_id,
    }))).collect();
    events.sort_by_key(|e|e["sequence"].as_u64().unwrap());
    let entities=recorder.orb_entities().iter().map(|e|json!({
        "combat_id":e.combat_id,"id":e.id,"kind":e.kind,"owner_id":e.owner_id,
        "model":e.model,"display_slot":e.display_slot,"slot":e.slot,
    })).collect::<Vec<_>>();
    Ok(json!({"events":events,"entities":entities}))
}
pub fn events_for_actor(recorder:&Recorder,actor:Option<u8>)->Result<Value,String> {
    let (events,_,_)=recorder.snapshot();
    if events.iter().any(|e|e.seq>MAX_JSON_INTEGER||e.combat_instance>MAX_JSON_INTEGER) {
        return Err("between event exceeds JSON safe integer range".into());
    }
    Ok(json!(events.iter().map(|e| {
        let source=match &e.source {
            Some(Source::Creature(c))=>creature(c),
            Some(Source::Model{kind,owner,model})=>json!({"kind":kind,"owner":owner,"model":model}),
            None=>Value::Null,
        };
        let mut event=json!({"seq":e.seq,"kind":e.kind,"op":e.op,"combat_instance":e.combat_instance,
            "round":e.round,"side":e.side,"stage":e.stage,"owner":e.owner,"source":source,
            "target":e.target.as_ref().map(creature),"cause":{"span":e.span,"queue_seq":e.queue_seq},
            "span":e.span,"parent_span":null,"before":state(e.before),"after":state(e.after),
            "cue":null});
        match &e.details {
            Details::Lifecycle=>event["cue"]=json!({"profile":"osty","trigger":if e.op=="death"{"Dead"}else{"Revive"}}),
            Details::Enemy {powers,roster,slot}=>{
                event["after"]["powers"]=json!(powers);
                event["roster"]=json!(roster);
                event["slot"]=json!(slot);
                event["cue"]=json!({"profile":"enemy","trigger":e.op});
            }
            Details::Damage {requested,blocked,hp_lost,overkill,props,intended_target,block_owner,block_before,block_after}=>{
                event["result"]=json!({"requested":requested,"blocked":blocked,"hp_lost":hp_lost,
                    "overkill":overkill,"props":props.0,"powered_attack":props.is_powered_attack(),
                    "intended_target":creature(intended_target),"block_owner":creature(block_owner),
                    "block_before":block_before,"block_after":block_after});
            }
            Details::Power {model,amount_before,amount_after,powers,flash}=>{
                event["power"]=json!({"model":model,"amount_before":amount_before,"amount_after":amount_after});
                event["after"]["powers"]=json!(powers);
                event["cue"]=json!({"profile":"power","flash":flash});
            }
            Details::Card {card,from,to,hand_count,piles,count}=>{
                use sts2_sim::combat::PileKind;
                let pile=|p:&PileKind|match p {PileKind::None=>"none",PileKind::Hand=>"hand",
                    PileKind::Draw=>"draw",PileKind::Discard=>"discard",PileKind::Exhaust=>"exhaust",PileKind::Play=>"play"};
                let face_up=|p:&PileKind|matches!(p,PileKind::Play|PileKind::Discard|PileKind::Exhaust);
                let disclosed=face_up(from)||face_up(to)||actor==e.owner&&
                    (from==&PileKind::Hand||to==&PileKind::Hand);
                let card=card.as_ref().filter(|_|disclosed);
                event["movement"]=json!({"from":pile(from),"to":pile(to),"count":count});
                event["card"]=card.map(|c|super::card_snapshot(c)).unwrap_or(Value::Null);
                event["instance_id"]=json!(card.map(|c|c.instance_id));
                event["after"]["hand_count"]=json!(hand_count);
                event["after"]["piles"]=json!(piles);
            }
            Details::Hp {requested,hp_gained}=>{
                event["result"]=json!({"requested":requested,"hp_gained":hp_gained});
                event["cue"]=json!({"profile":"heal","trigger":e.op});
            }
            Details::Combat {won,players}=>{
                event["result"]=json!({"won":won});
                event["after"]["players"]=json!(players.iter().map(|p|{
                    let mut player=state(p.state);
                    player["target"]=creature(&p.target);player["powers"]=json!(p.powers);
                    player["piles"]=json!(p.piles);player["hand_count"]=json!(p.hand_count);
                    player
                }).collect::<Vec<_>>());
            }
            Details::Block|Details::Boundary=>{}
        }
        event
    }).collect::<Vec<_>>()))
}

impl RealSource<'_> {
    pub fn between(&self,recorder:&Recorder,origin:Origin,frame:&Value,terminal:bool)->Result<Value,String> {
        if origin.index as u64>=MAX_JSON_INTEGER {return Err("between edge exceeds JSON safe integer range".into());}
        let actor=u8::try_from(frame["actor"].as_u64().ok_or("between actor missing")?)
            .map_err(|_|"between actor out of range")?;
        if actor as usize>=self.seats {return Err("between actor out of range".into());}
        let events=events_for_actor(recorder,Some(actor))?;
        let list=events.as_array().unwrap();
        let (_,fault,last)=recorder.snapshot();
        let mut to=endpoint(&self.game);
        if terminal {to["terminal"]=json!(true);}else{to["decision"]=json!(origin.index+1);}
        let mut edge=json!({"v":1,"edge":origin.index,"from":origin.endpoint,"to":to,
            "seq_first":list.first().map(|e|&e["seq"]),"seq_last":list.last().map(|e|&e["seq"]),
            "complete":false,"last_recorded_seq":last,"reason":fault.unwrap_or("public-events-partial"),
            "coverage":["pet_lifecycle","enemy_lifecycle","damage","block","boundary","power_apply_remove","card_moves","hp","combat_end"],
            "events":events});
        if frame.get("after").is_none() {
            edge["settled"]=super::public_state::settlement(self,actor)?;
        }
        Ok(edge)
    }
}

#[cfg(test)]
mod orb_tests {
    use super::*;
    use sts2_sim::{combat::Combat, orbs::OrbKind, rng::Rng,
        run::{RunManager, RunOptions}, tables::PowerId};

    #[test]
    fn orb_payload_serializes_targets_and_ordered_hook_spans() {
        let run=RunManager::new(7,RunOptions::default()).run_ctx().clone();
        let mut combat=Combat::new(run,7);
        combat.add_player(80,80,3,0,&[]);
        combat.setup_encounter("INKLETS_NORMAL",&mut Rng::new(7));
        let enemies=combat.enemies.clone();
        for &enemy in &enemies {combat.cr_mut(enemy).block=50;}
        combat.apply_power_to(1,PowerId::Thunder,3,Some(1));
        let recorder=Recorder::default();
        recorder.record_combat(&mut combat);
        let capture=recorder.begin(7);
        combat.channel_orb(1,OrbKind::Lightning);
        combat.evoke_front_orb(1);
        drop(capture);
        let data=orb_data(&recorder).unwrap();
        let events=data["events"].as_array().unwrap();
        assert!(events.windows(2).all(|w|w[0]["sequence"].as_u64()<w[1]["sequence"].as_u64()));
        let evoke=events.iter().find(|e|e["phase"]=="evoke").unwrap();
        assert_eq!(evoke["owner_id"],1);
        assert_eq!(evoke["action_id"],7);
        assert_eq!(evoke["orb_kind"],"LIGHTNING");
        let target=evoke["targets"][0].as_u64().unwrap();
        assert!(enemies.iter().any(|&enemy|u64::from(enemy)==target));
        assert!(events.iter().any(|e|e["operation"]=="after_orb_evoked"&&e["transition"]=="begin"));
        assert!(events.iter().any(|e|e["operation"]=="after_orb_evoked"&&e["transition"]=="end"));
        assert!(events.iter().any(|e|e["operation"]=="damage"&&e["target_id"]==target));
        assert!(data["entities"].as_array().unwrap().iter().any(|e|
            e["id"]==target&&!e["display_slot"].is_null()&&e["combat_id"]==evoke["combat_id"]));
    }

    #[test]
    fn orb_target_entity_keeps_its_display_slot_after_removal() {
        let run=RunManager::new(7,RunOptions::default()).run_ctx().clone();
        let mut combat=Combat::new(run,7);
        combat.setup_encounter("INKLETS_NORMAL",&mut Rng::new(7));
        let enemies=combat.enemies.clone();
        for &enemy in &enemies {combat.cr_mut(enemy).hp=1;}
        let recorder=Recorder::default();
        recorder.record_combat(&mut combat);
        let capture=recorder.begin(2);
        combat.channel_orb(0,OrbKind::Lightning);
        combat.evoke_front_orb(0);
        drop(capture);
        let data=orb_data(&recorder).unwrap();
        let evoke=data["events"].as_array().unwrap().iter().find(|e|e["phase"]=="evoke").unwrap();
        let target=evoke["targets"][0].as_u64().unwrap();
        let expected_slot=enemies.iter().position(|&enemy|u64::from(enemy)==target).unwrap();
        assert!(!combat.enemies.iter().any(|&enemy|u64::from(enemy)==target));
        let entity=data["entities"].as_array().unwrap().iter().find(|e|e["id"]==target).unwrap();
        assert_eq!(entity["display_slot"],expected_slot);
        assert_eq!(entity["combat_id"],evoke["combat_id"]);
    }

    #[test]
    fn activation_slots_remain_pre_removal_when_enemies_shift() {
        let run=RunManager::new(7,RunOptions::default()).run_ctx().clone();
        let mut combat=Combat::new(run,7);
        combat.setup_encounter("INKLETS_NORMAL",&mut Rng::new(7));
        let enemies=combat.enemies.clone();
        combat.cr_mut(enemies[0]).hp=1;
        combat.cr_mut(enemies[1]).hp=50;
        combat.cr_mut(enemies[2]).hp=60;
        combat.channel_orb(0,OrbKind::Dark);
        let recorder=Recorder::default();
        recorder.record_combat(&mut combat);
        let capture=recorder.begin(2);
        combat.evoke_front_orb(0);
        drop(capture);
        let data=orb_data(&recorder).unwrap();
        let event=data["events"].as_array().unwrap().iter().find(|e|e["phase"]=="evoke").unwrap();
        assert_eq!(event["targets"],json!([enemies[0]]));
        assert_eq!(event["display_slots"],json!([
            {"id":enemies[0],"display_slot":0},
            {"id":enemies[1],"display_slot":1},
            {"id":enemies[2],"display_slot":2}
        ]));
        assert_eq!(combat.enemies.as_slice(),&[enemies[1],enemies[2]]);
        let after=super::super::combat_entities(&combat);
        let survivor=after.iter().find(|row|row["id"]==enemies[1]).unwrap();
        assert_eq!(survivor["display_slot"],0);
    }
}
