use serde_json::{Value, json};
use sts2_native_obs::{replay::{Game, RealSource}, *};
use sts2_sim::{card::Card, run::{Coord, mp::{Character, MpPhase, MpRun, PlayerSpec}},
    tables::{CardId, MapPointType}};

fn combat(seats: usize) -> MpRun {
    combat_with_relic(seats, None)
}

fn combat_with_relic(seats: usize, relic: Option<sts2_sim::tables::RelicId>) -> MpRun {
    let characters = [Character::Defect, Character::Necrobinder, Character::Regent, Character::Ironclad];
    let specs: Vec<_> = (0..seats).map(|seat| PlayerSpec {
        net_id: 100 + seat as u64, character: characters[seat],
    }).collect();
    let mut run = MpRun::new("REPLAY-SEATS", &specs, 0).unwrap();
    if let Some(id) = relic {run.roster.player_mut(0).unwrap().relics.push(sts2_sim::relic::Relic::new(id));}
    for seat in 0..seats {
        let player = run.roster.player_mut(seat).unwrap();
        player.deck = player.deck[..6].iter().enumerate().map(|(i, old)| {
            let mut card = Card::from_factory(if i % 2 == 0 { CardId::StrikeIronclad }
                else { CardId::Blaze }, false, old.instance_id).unwrap();
            card.owner = seat as u8;
            card
        }).collect();
    }
    run.phase = MpPhase::Map;
    let (x, y) = run.world.run.coord.unwrap();
    let child = run.world.map.point(Coord::new(x, y)).unwrap().children[0] as usize;
    run.world.map.points[child].point_type = MapPointType::Monster;
    let destination = run.world.map.points[child].coord;
    for seat in 0..seats { run.vote(100 + seat as u64, seat, destination).unwrap(); }
    run
}

fn frame(run: &MpRun) -> Value {
    let decision = DecisionId { episode: 1, generation: 0, actor: 0 };
    let mut encoder = NativeEncoder::new();
    let mut obs = NativeObsV1::new();
    encoder.encode_multiplayer(run, decision, 100, &mut obs).unwrap();
    let view = obs.view().unwrap();
    let slot = (0..view.candidate_len).find(|&i| view.candidates.sampleable(i, view.candidate_len)).unwrap();
    let inventory = Game::Party(run).inventory();
    RealSource { game: Game::Party(run), obs: &obs, team:None, decision, seats: run.roster.players().len(),
        inventory, floor: 1 }.replay_frame(slot as i32, 0).unwrap()
}

#[test]
fn between_settlement_exposes_public_resources_without_private_cards_or_draw_order() {
    use sts2_native_obs::replay::between;
    for seats in [2,4] {
        let run=combat(seats);
        let obs=NativeObsV1::new();
        let projection=RealSource {game:Game::Party(&run),obs:&obs,team:None,
            decision:DecisionId::default(),seats,inventory:Game::Party(&run).inventory(),floor:1};
        let recorder=sts2_sim::between::Recorder::default();
        let edge=projection.between(&recorder,between::origin(&Game::Party(&run),0),
            &json!({"actor":0}),false).unwrap();
        let public=&edge["settled"];
        for owner in 0..seats {
            let player=&public["players"][owner];
            let c=run.combat().unwrap();
            let p=&c.players[owner as u8];
            assert!(!p.draw.is_empty(),"exercise a genuinely hidden ordered pile");
            assert_eq!(player["hp"],c.cr(owner as u8).hp);
            assert_eq!(player["piles"],json!([p.draw.len(),p.discard.len(),p.exhaust.len()]));
            for key in ["pile_cards","hand","hand_instances","hand_preview","hand_playable","deck","deck_cards","rng","ai"] {
                assert!(player.get(key).is_none(),"between disclosed private field {key} for seat {owner}");
            }
            assert!(player["pets"].is_array());
        }
    }
}

#[test]
fn pet_lifecycle_export_preserves_death_then_owner_revival_between_decisions() {
    use sts2_native_obs::replay::between;
    use sts2_sim::{combat::Props, creature::Side, hooks::Ev, relic::Relic, tables::{Hook, RelicId}};
    for seats in 1..=4 {
        let mut c = if seats == 1 {
            let run = sts2_sim::run::RunManager::new(7, Default::default());
            let mut c = sts2_sim::combat::Combat::new(run.run_ctx().clone(), 7);
            c.setup_encounter("TUNNELER_WEAK", &mut sts2_sim::rng::Rng::new(7));
            c.start_combat();
            c
        } else {
            combat(seats).combat().unwrap().clone()
        };
        c.run.relics.clear();
        let pets: Vec<_> = (0..seats as u8).map(|owner| {
            let pet = c.osty(owner).unwrap_or_else(|| c.summon_osty(owner, 1).unwrap());
            let mut relic = Relic::new(RelicId::BoundPhylactery);
            relic.owner = owner;
            c.run.relics.push(relic);
            c.cr_mut(owner).block = 0;
            pet
        }).collect();
        let recorder=sts2_sim::between::Recorder::default();
        assert_eq!(between::events(&recorder).unwrap(),json!([]));
        let capture=recorder.begin(0);
        c.side = Side::Enemy;
        let enemy = c.enemies[0];
        for owner in 0..seats as u8 {
            c.deal_damage(Some(enemy), owner, 5, Props::MOVE);
            assert!(c.cr(pets[owner as usize]).is_dead());
        }
        c.side = Side::Player;
        for owner in 0..seats as u8 {
            c.players[owner].turn_number = 2;
            c.fire_relics(Hook::AfterEnergyResetLate, Ev::Creature(owner));
        }
        let events = between::events(&recorder).unwrap();
        let all = events.as_array().unwrap();
        let events:Vec<_> = all.iter().filter(|e|e["kind"]=="lifecycle").collect();
        assert_eq!(events.len(), seats * 2);
        for owner in 0..seats {
            let death = &events[owner];
            let revival = &events[seats + owner];
            assert_eq!(death["kind"], "lifecycle");
            assert_eq!(death["op"], "death");
            assert_eq!(death["owner"], owner);
            assert_eq!(death["target"]["creature_id"], pets[owner]);
            assert_eq!(death["target"]["owner"], owner);
            assert_eq!(death["after"]["hp"], 0);
            assert_eq!(death["after"]["alive"], false);
            assert_eq!(death["side"], "enemy");
            assert_eq!(revival["op"], "revive");
            assert_eq!(revival["owner"], owner);
            assert_eq!(revival["target"], death["target"]);
            assert_eq!(revival["after"]["hp"], 1);
            assert_eq!(revival["after"]["alive"], true);
            assert_eq!(revival["side"], "player");
            assert_eq!(revival["source"],json!({"kind":"relic","owner":owner,"model":"BOUND_PHYLACTERY"}));
            assert!(death["seq"].as_u64() < revival["seq"].as_u64());
            assert_eq!(c.cr(pets[owner]).hp, 1, "decision endpoints conceal the death");
        }
        drop(capture);
        let _capture=recorder.begin(1);
        assert_eq!(between::events(&recorder).unwrap(), json!([]));
        c.kill_creature(pets[0]);
        drop(c);
        let tombstones=between::events(&recorder).unwrap();
        assert!(tombstones[0]["seq"].as_u64().unwrap()>all.last().unwrap()["seq"].as_u64().unwrap(),"sequence belongs to the run, not the edge");
        assert_eq!(tombstones[0]["after"]["hp"],0,"the collector survives combat disposal");
    }
    let recorder=sts2_sim::between::Recorder::default();
    let _capture=recorder.begin(0);
    let run=combat(2);
    let obs=NativeObsV1::new();
    let projection=RealSource {game:Game::Party(&run),obs:&obs,team:None,
        decision:DecisionId::default(),seats:2,inventory:Game::Party(&run).inventory(),floor:1};
    let edge=projection.between(&recorder,between::origin(&Game::Party(&run),0),
        &json!({"actor":0}),false).unwrap();
    assert_eq!(edge["v"],1);
    assert_eq!(edge["edge"],0);
    assert_eq!(edge["from"]["decision"],0);
    assert_eq!(edge["to"]["decision"],1);
    assert_eq!(edge["from"]["combat_instance"],1);
    assert!(edge["events"].as_array().unwrap().iter().any(|e|e["kind"]=="boundary"));
    assert_eq!(edge["complete"],false,"pet-only coverage must not claim a complete game log");
    assert_eq!(edge["reason"],"public-events-partial");
    assert!(edge["settled"]["players"].is_array());
    let edge=projection.between(&recorder,between::origin(&Game::Party(&run),1),
        &json!({"actor":0,"after":projection.replay_snapshot(0).unwrap()}),true).unwrap();
    assert!(edge.get("settled").is_none(),"after is the sole authoritative settlement when present");
    assert_eq!(edge["to"]["terminal"],true);
    assert!(projection.between(&recorder,between::origin(&Game::Party(&run),2),
        &json!({"actor":256}),false).is_err(),"an invalid audience cannot wrap into another owner's private hand");
}

#[test]
fn recorded_hits_preserve_block_absorption_and_stop_after_retaliation_kills_attacker() {
    use sts2_native_obs::replay::between;
    use sts2_sim::{combat::{Combat, Props}, rng::Rng, run::RunManager, tables::PowerId};
    let run=RunManager::new(7,Default::default());
    let mut c=Combat::new(run.run_ctx().clone(),7);
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(7));
    c.start_combat();
    c.run.relics.clear();
    let enemy=c.enemies[0];
    let recorder=sts2_sim::between::Recorder::default();
    let capture=recorder.begin(0);
    c.gain_move_block(0,5);
    c.apply_damage(0,4,Props::MOVE,Some(enemy));
    c.apply_damage(0,4,Props::MOVE,Some(enemy));
    let events=between::events(&recorder).unwrap();
    let hits:Vec<_>=events.as_array().unwrap().iter().filter(|e|e["kind"]=="damage").collect();
    assert_eq!(hits.len(),2,"each resolved hit, not its endpoint delta, is recorded");
    assert_eq!(hits[0]["result"]["blocked"],4);
    assert_eq!(hits[0]["result"]["hp_lost"],0);
    assert_eq!(hits[0]["before"]["block"],5);
    assert_eq!(hits[0]["after"]["block"],1);
    assert_eq!(hits[1]["result"]["blocked"],1);
    assert_eq!(hits[1]["result"]["hp_lost"],3);
    assert_eq!(hits[1]["after"]["block"],0);
    drop(capture);
    c.cr_mut(enemy).hp=3;
    c.apply_power_to(0,PowerId::Thorns,3,Some(0));
    let _capture=recorder.begin(1);
    c.monster_attack_players(enemy,4,3);
    let events=between::events(&recorder).unwrap();
    let hits:Vec<_>=events.as_array().unwrap().iter().filter(|e|e["kind"]=="damage").collect();
    assert_eq!(hits.len(),2,"retaliation and the first attack resolve; remaining intended hits do not");
    assert_eq!(hits[0]["target"]["creature_id"],enemy,"BeforeDamageReceived retaliation resolves first");
    assert_eq!(hits[0]["result"]["hp_lost"],3);
    assert_eq!(hits[1]["target"]["creature_id"],0);
}

#[test]
fn multiplayer_frames_include_every_seats_live_resources_cards_orbs_and_pets() {
    for seats in 2..=4 {
        let run = combat(seats);
        let c = run.combat().unwrap();
        let before = format!("{c:?}");
        let frame = frame(&run);
        assert_eq!(frame["party_size"], seats);
        for owner in 0..seats {
            let p = &frame["players"][owner];
            let live = &c.players[owner as u8];
            assert_eq!(p["actor"], owner);
            assert_eq!(p["character"], run.roster.players()[owner].character.name());
            assert_eq!(p["blk"], c.cr(owner as u8).block);
            assert_eq!(p["en"], live.energy);
            assert_eq!(p["men"], live.base_max_energy);
            assert_eq!(p["stars"], live.stars);
            assert!(p["pw"].is_array());
            assert_eq!(p["hand"].as_array().unwrap().len(), live.hand.len());
            assert_eq!(p["hand_instances"].as_array().unwrap().len(), live.hand.len());
            for (i, &k) in live.hand.iter().enumerate() {
                assert_eq!(p["hand_instances"][i]["instance_id"], c.cards[k as usize].instance_id);
                assert_eq!(p["hand_instances"][i]["owner"], owner);
                // Legality, not the card's original cost (a 0-cost card is still playable).
                assert_eq!(p["hand_playable"][i], c.can_play_card_readonly(k).unwrap());
            }
            assert_eq!(p["piles"], json!([live.draw.len(), live.discard.len(), live.exhaust.len()]));
            assert_eq!(p["orb_slots"], live.orbs.capacity);
            assert_eq!(p["orbs"].as_array().unwrap().len(), live.orbs.orbs.len());
            assert!(p["pets"].is_array());
        }
        assert_eq!(frame["players"][0]["orbs"][0]["kind"], "LIGHTNING");
        assert_eq!(frame["players"][0]["orbs"][0]["passive"], 3);
        assert_eq!(frame["players"][0]["orbs"][0]["evoke"], 8);
        let osty = frame["players"][1]["pets"].as_array().unwrap().iter()
            .find(|pet| pet["monster_id"] == "OSTY").unwrap();
        assert_eq!(osty["owner"], 1);
        assert_eq!(osty["hp"], c.cr(c.osty(1).unwrap()).hp);
        assert!(osty["pw"].is_array());
        for field in ["blk", "en", "men", "stars", "orbs", "orb_slots", "hand", "hand_instances", "hand_playable", "piles", "pw", "pets"] {
            assert_eq!(frame[field], frame["players"][0][field], "{field}");
        }
        assert_eq!(format!("{c:?}"), before, "projection must be read-only");
    }
}

#[test]
fn public_power_events_preserve_consumption_and_retaliation_before_damage() {
    use sts2_native_obs::replay::between;
    use sts2_sim::{combat::{Combat,Props},rng::Rng,run::RunManager,tables::PowerId};
    let run=RunManager::new(7,Default::default());
    let mut c=Combat::new(run.run_ctx().clone(),7);
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(7));
    c.start_combat();
    c.run.relics.clear();
    let enemy=c.enemies[0];
    let recorder=sts2_sim::between::Recorder::default();
    let _capture=recorder.begin(0);
    let artifact=c.apply_power_to(0,PowerId::Artifact,1,Some(0)).unwrap();
    c.powers[artifact as usize].vars=[13791379;4];
    c.powers[artifact as usize].payload=13791379;
    c.apply_power_to(0,PowerId::Weak,2,Some(enemy));
    c.apply_power_to(0,PowerId::Strength,2,Some(0));
    c.apply_power_to(0,PowerId::Strength,-2,Some(0));
    c.apply_power_to(0,PowerId::Thorns,3,Some(0));
    c.apply_damage(0,1,Props::MOVE,Some(enemy));
    let events=between::events(&recorder).unwrap();
    let rows=events.as_array().unwrap();
    let artifact:Vec<_>=rows.iter().filter(|e|e["kind"]=="power"&&e["power"]["model"]=="ARTIFACT").collect();
    assert_eq!(artifact.iter().map(|e|e["op"].as_str().unwrap()).collect::<Vec<_>>(),["apply","change","remove"]);
    assert_eq!(artifact[1]["power"]["amount_after"],0);
    assert!(artifact[2]["after"]["powers"].as_array().unwrap().iter().all(|p|p[0]!="ARTIFACT"));
    let strength:Vec<_>=rows.iter().filter(|e|e["kind"]=="power"&&e["power"]["model"]=="STRENGTH").collect();
    assert_eq!(strength.iter().map(|e|e["op"].as_str().unwrap()).collect::<Vec<_>>(),["apply","change","remove"]);
    let trigger=rows.iter().position(|e|e["kind"]=="power"&&e["op"]=="trigger"&&e["power"]["model"]=="THORNS").unwrap();
    assert_eq!(rows[trigger+1]["kind"],"damage");
    assert_eq!(rows[trigger+1]["target"]["creature_id"],enemy);
    assert_eq!(rows[trigger+2]["target"]["creature_id"],0);
    assert!(!events.to_string().contains("13791379"),"private power payload and variables are not public events");
    drop(_capture);
    let _capture=recorder.begin(1);
    c.apply_power_to(enemy,PowerId::Poison,2,Some(0));
    c.side=sts2_sim::creature::Side::Enemy;
    c.fire(sts2_sim::tables::Hook::AfterSideTurnStart,
        sts2_sim::hooks::Ev::Side(sts2_sim::creature::Side::Enemy),None);
    let events=between::events(&recorder).unwrap();
    let rows=events.as_array().unwrap();
    let trigger=rows.iter().position(|e|e["kind"]=="power"&&e["op"]=="trigger"&&e["power"]["model"]=="POISON").unwrap();
    assert_eq!(rows[trigger+1]["kind"],"damage","poison uses the same real-hit channel");
    assert_eq!(rows[trigger+1]["result"]["hp_lost"],2);
    assert_eq!(rows[trigger+2]["op"],"change");
    assert_eq!(rows[trigger+2]["power"]["amount_after"],1);
    drop(_capture);
    let _capture=recorder.begin(2);
    c.apply_power_to(enemy,PowerId::Vulnerable,2,Some(0));
    c.fire_after_turn_end(sts2_sim::creature::Side::Enemy);
    let events=between::events(&recorder).unwrap();
    assert!(events.as_array().unwrap().iter().any(|e|e["kind"]=="power"&&
        e["power"]["model"]=="VULNERABLE"&&e["op"]=="change"&&e["power"]["amount_after"]==1),
        "turn-end duration decrements are visible before the next decision");
}

#[test]
fn radiance_real_turn_records_energy_reset_before_hand_draw() {
    use sts2_native_obs::replay::between;
    use sts2_sim::{combat::Combat,creature::Side,rng::Rng,run::RunManager,tables::PowerId};
    let run=RunManager::new(7,Default::default());
    let mut c=Combat::new(run.run_ctx().clone(),7);
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(7));
    c.start_combat();
    c.run.relics.clear();
    let ordinary_energy=c.players[0].energy;
    let round=c.round;
    c.apply_power_to(0,PowerId::Radiance,3,Some(0));
    let recorder=sts2_sim::between::Recorder::default();
    let _capture=recorder.begin(0);
    c.end_player_turn();
    assert!(c.fault.is_none(),"{:?}",c.fault);
    assert_eq!(c.side,Side::Player);
    assert_eq!(c.round,round+1);
    assert_eq!(c.power_amount(0,PowerId::Radiance),2);
    assert_eq!(c.players[0].energy,ordinary_energy+1);
    let events=between::events(&recorder).unwrap();
    let rows=events.as_array().unwrap();
    let changes:Vec<_>=rows.iter().enumerate().filter(|(_,e)|
        e["kind"]=="power"&&e["power"]["model"]=="RADIANCE"&&e["op"]=="change").collect();
    assert_eq!(changes.len(),1,"the real energy-reset path must publish exactly one Radiance decrement");
    assert_eq!(changes[0].1["power"]["amount_before"],3);
    assert_eq!(changes[0].1["power"]["amount_after"],2);
    let draw=rows.iter().position(|e|e["kind"]=="card"&&e["movement"]["from"]=="draw"&&
        e["movement"]["to"]=="hand").expect("the new turn must draw a hand");
    assert!(changes[0].0<draw,"AfterEnergyReset precedes the new hand, not AfterSideTurnStart");
}

#[test]
fn public_turn_ticks_update_surviving_power_stacks_before_settlement() {
    use sts2_native_obs::replay::between;
    use sts2_sim::{combat::Combat,creature::Side,rng::Rng,run::RunManager,tables::PowerId};
    let cases=[
        (PowerId::ToricToughness,Side::Player,"player_start","player"),
        (PowerId::LightningRod,Side::Player,"player_start","player"),
        (PowerId::Hibernate,Side::Player,"player_start","player"),
        (PowerId::Plating,Side::Player,"player_start","player"),
        (PowerId::Blur,Side::Player,"player_start","player"),
        (PowerId::Reflect,Side::Player,"player_start","player"),
        (PowerId::Clarity,Side::Player,"player_start","player"),
        (PowerId::Sandpit,Side::Player,"enemy_start","enemy"),
        (PowerId::Poison,Side::Enemy,"enemy_start","enemy"),
        (PowerId::Regen,Side::Player,"turn_end","player"),
        (PowerId::TheBomb,Side::Player,"turn_end","player"),
        (PowerId::EscapeArtist,Side::Enemy,"enemy_end","enemy"),
        (PowerId::Shrink,Side::Enemy,"enemy_end","enemy"),
        (PowerId::Hatch,Side::Enemy,"enemy_end","enemy"),
        (PowerId::BattlewornDummyTimeLimit,Side::Enemy,"enemy_end","enemy"),
        (PowerId::RetainHand,Side::Player,"turn_end","player"),
        (PowerId::Asleep,Side::Enemy,"enemy_end","enemy"),
        (PowerId::Slumber,Side::Enemy,"enemy_end","enemy"),
        (PowerId::NoBlock,Side::Player,"enemy_end","enemy"),
        (PowerId::DoubleDamage,Side::Player,"turn_end","player"),
        (PowerId::Conqueror,Side::Player,"turn_end","player"),
        (PowerId::Debilitate,Side::Enemy,"enemy_end","enemy"),
        (PowerId::Vulnerable,Side::Enemy,"enemy_end","enemy"),
        (PowerId::Weak,Side::Enemy,"enemy_end","enemy"),
        (PowerId::Frail,Side::Enemy,"enemy_end","enemy"),
        (PowerId::Colossus,Side::Enemy,"enemy_end","enemy"),
        (PowerId::Intangible,Side::Enemy,"enemy_end","enemy"),
    ];
    for (id,owner_side,stage,side) in cases {
        let run=RunManager::new(7,Default::default());
        let mut c=Combat::new(run.run_ctx().clone(),7);
        c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(7));
        c.start_combat();
        c.run.relics.clear();
        let round=c.round;
        let owner=if owner_side==Side::Player {0}else{c.enemies[0]};
        c.apply_power_to(owner,id,3,Some(0));
        let recorder=sts2_sim::between::Recorder::default();
        let _capture=recorder.begin(0);
        c.end_player_turn();
        assert!(c.fault.is_none(),"{id:?}: {:?}",c.fault);
        assert_eq!(c.side,Side::Player,"{id:?} must finish the turn");
        assert_eq!(c.round,round+1,"{id:?} must finish the turn");
        assert_eq!(c.power_amount(owner,id),2,"{id:?} must retain the original tick");
        let events=between::events(&recorder).unwrap();
        let changes:Vec<_>=events.as_array().unwrap().iter().filter(|e|
            e["kind"]=="power"&&e["power"]["model"]==id.name()&&e["op"]=="change").collect();
        assert_eq!(changes.len(),1,"{id:?} tick must appear once before settlement: {events}");
        assert_eq!(changes[0]["target"]["creature_id"],owner,"{id:?}");
        assert_eq!(changes[0]["stage"],stage,"{id:?}");
        assert_eq!(changes[0]["side"],side,"{id:?}");
        assert_eq!(changes[0]["power"]["amount_before"],3);
        assert_eq!(changes[0]["power"]["amount_after"],2);
        assert!(changes[0]["after"]["powers"].as_array().unwrap().iter().any(|p|p[0]==id.name()&&p[1]==2));
    }
}

#[test]
fn public_card_moves_record_effect_draw_autoplay_and_exhaust_without_draw_order() {
    use sts2_native_obs::replay::between;
    use sts2_sim::{combat::Combat,rng::Rng,run::RunManager};
    let run=RunManager::new(7,Default::default());
    let mut c=Combat::new(run.run_ctx().clone(),7);
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(7));
    c.start_combat();
    c.run.relics.clear();
    c.players[0].hand.clear();c.players[0].draw.clear();
    let played=c.new_card(0,CardId::Bash,false).unwrap();
    let hidden=c.new_card(0,CardId::DemonFormCard,false).unwrap();
    c.players[0].draw.extend([played,hidden]);
    let recorder=sts2_sim::between::Recorder::default();
    let _capture=recorder.begin(0);
    c.draw_cards(0,1,false);
    assert!(c.auto_play_card(played,Some(c.enemies[0]),true));
    let events=between::events(&recorder).unwrap();
    let moves:Vec<_>=events.as_array().unwrap().iter().filter(|e|e["kind"]=="card"&&e["op"]=="move").collect();
    assert_eq!(moves.len(),3);
    assert_eq!(moves.iter().map(|e|(e["movement"]["from"].as_str().unwrap(),
        e["movement"]["to"].as_str().unwrap())).collect::<Vec<_>>(),
        [("draw","hand"),("hand","play"),("play","exhaust")]);
    assert!(moves[0]["card"].is_null(),"a hand reveal is private without its owner audience");
    assert_eq!(moves[1]["card"][0],"BASH","autoplay reveals the played card");
    assert!(!events.to_string().contains(CardId::DemonFormCard.name()),"remaining draw identities never enter the public log");
    let owner=between::events_for_actor(&recorder,Some(0)).unwrap();
    assert_eq!(owner.as_array().unwrap().iter().find(|e|e["kind"]=="card").unwrap()["card"][0],"BASH");
    assert!(owner.as_array().unwrap().iter().any(|e|e["kind"]=="card"&&e["op"]=="play"&&e["card"][0]=="BASH"));
    let remote=between::events_for_actor(&recorder,Some(1)).unwrap();
    assert!(remote.as_array().unwrap().iter().find(|e|e["kind"]=="card").unwrap()["card"].is_null());
    drop(_capture);
    c.players[0].draw.clear();c.players[0].discard.clear();c.players[0].exhaust.clear();
    c.players[0].discard.extend([played,hidden]);
    let _capture=recorder.begin(1);
    c.shuffle_if_needed(0);
    let shuffled=between::events_for_actor(&recorder,Some(0)).unwrap();
    let cards:Vec<_>=shuffled.as_array().unwrap().iter().filter(|e|e["kind"]=="card").collect();
    assert_eq!(cards.len(),1,"shuffle is one public aggregate, not an ordered card enumeration");
    assert_eq!(cards[0]["op"],"shuffle");
    assert_eq!(cards[0]["movement"]["count"],2);
    assert!(cards[0]["card"].is_null()&&cards[0]["instance_id"].is_null());
}

#[test]
fn enemy_death_reveals_spawned_children_before_parent_removal() {
    use sts2_native_obs::replay::between;
    use sts2_sim::{combat::Combat,rng::Rng,run::RunManager};
    let run=RunManager::new(7,Default::default());
    let mut c=Combat::new(run.run_ctx().clone(),7);
    c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(7));
    c.start_combat();
    let parent=c.enemies[0];
    c.apply_power_to(parent,sts2_sim::tables::PowerId::Infested,1,Some(parent));
    let recorder=sts2_sim::between::Recorder::default();
    let _capture=recorder.begin(0);
    c.kill_creature(parent);
    let events=between::events(&recorder).unwrap();
    let lifecycle:Vec<_>=events.as_array().unwrap().iter().filter(|e|e["kind"]=="lifecycle").collect();
    assert_eq!(lifecycle.len(),6,"death, four public child reveals, then parent removal");
    assert_eq!(lifecycle[0]["op"],"death");
    assert_eq!(lifecycle[0]["target"]["creature_id"],parent);
    for (i,event) in lifecycle[1..5].iter().enumerate() {
        assert_eq!(event["op"],"spawn");
        assert_eq!(event["target"]["creature_id"],c.enemies[i]);
        assert_eq!(event["after"]["model"],"WRIGGLER");
        assert!(event.get("intent").is_none(),"a spawn cannot disclose the next unset move");
    }
    assert_eq!(lifecycle[5]["op"],"remove");
    assert_eq!(lifecycle[5]["target"]["creature_id"],parent);
}

#[test]
fn victory_heal_and_final_public_results_survive_combat_drop() {
    use sts2_native_obs::replay::between;
    use sts2_sim::{combat::Combat,rng::Rng,run::RunManager,relic::Relic,tables::RelicId};
    for seats in [1,2,4] {
        let mut c=if seats==1 {
            let run=RunManager::new(7,Default::default());
            let mut c=Combat::new(run.run_ctx().clone(),7);
            c.setup_encounter("TUNNELER_WEAK",&mut Rng::new(7));
            c.start_combat();c
        }else{combat(seats).combat().unwrap().clone()};
        c.run.relics.clear();
        c.run.relics.push(Relic::new(RelicId::BurningBlood));
        c.cr_mut(0).hp=20;
        if seats>1 {c.kill_creature(1);}
        let recorder=sts2_sim::between::Recorder::default();
        let _capture=recorder.begin(0);
        for enemy in c.enemies.clone() {c.kill_creature(enemy);}
        assert!(c.won);
        drop(c);
        let events=between::events(&recorder).unwrap();
        let rows=events.as_array().unwrap();
        let heal=rows.iter().position(|e|e["kind"]=="hp"&&e["op"]=="heal").expect("victory heal missing");
        assert_eq!(rows[heal]["after"]["hp"],26);
        assert_eq!(rows[heal]["result"]["hp_gained"],6);
        let final_state=rows.last().unwrap();
        assert_eq!(final_state["kind"],"combat");
        assert_eq!(final_state["op"],"end");
        assert_eq!(final_state["result"]["won"],true);
        assert_eq!(final_state["after"]["players"].as_array().unwrap().len(),seats);
        assert_eq!(final_state["after"]["players"][0]["hp"],26);
        if seats>1 {
            assert!(rows.iter().any(|e|e["kind"]=="hp"&&e["op"]=="revive"&&e["owner"]==1&&e["after"]["hp"]==1));
            assert_eq!(final_state["after"]["players"][1]["hp"],1);
        }
        for player in final_state["after"]["players"].as_array().unwrap() {
            for hidden in ["draw","deck","hand","pile_cards","rng","ai"] {
                assert!(player.get(hidden).is_none(),"combat finalization leaked {hidden}");
            }
        }
    }
}

#[test]
fn noncombat_between_origin_never_reuses_a_retained_combat_instance() {
    use sts2_native_obs::replay::between;
    for seats in [2,4] {
        let recorder=sts2_sim::between::Recorder::default();
        let capture=recorder.begin(0);
        let mut run=combat(seats);
        drop(capture);
        assert!(run.combat().unwrap().between_instance>0);
        run.phase=MpPhase::Map;
        let _capture=recorder.begin(1);
        let obs=NativeObsV1::new();
        let projection=RealSource {game:Game::Party(&run),obs:&obs,team:None,
            decision:DecisionId::default(),seats,inventory:Game::Party(&run).inventory(),floor:1};
        let edge=projection.between(&recorder,between::origin(&Game::Party(&run),1),
            &json!({"actor":0}),false).unwrap();
        assert!(edge["from"]["combat_instance"].is_null(),
            "a map action must not bind the next encounter's events to the previous combat");
        assert!(edge["to"]["combat_instance"].is_null());
    }
}

#[test]
fn replay_card_targets_keep_indices_and_distinguish_absolute_enemy_player_and_pet_ids() {
    let run = combat(3);
    let c = run.combat().unwrap();
    let frame = frame(&run);
    let options = frame["opts"].as_array().unwrap();
    let attack = options.iter().find(|o| o["card_id"] == "STRIKE_IRONCLAD").unwrap();
    assert_eq!(attack["target_index"], 0);
    assert_eq!(attack["target"]["kind"], "enemy");
    assert_eq!(attack["target"]["id"], c.enemies[0]);
    let ally = options.iter().find(|o| o["card_id"] == "BLAZE" && o["target"]["kind"] == "player").unwrap();
    assert!(ally["target_index"].is_u64());
    assert_eq!(ally["target"]["owner"], ally["target"]["id"]);
    let pet = options.iter().find(|o| o["card_id"] == "BLAZE" && o["target"]["kind"] == "pet").unwrap();
    assert_eq!(pet["target_index"], 3);
    assert_eq!(pet["target"]["owner"], 1);
    assert_eq!(pet["target"]["id"], c.osty(1).unwrap());
    assert_ne!(pet["target"]["id"], attack["target"]["id"]);
}

#[test]
fn fan_frames_export_live_public_counter_and_hide_it_after_combat() {
    use sts2_sim::tables::RelicId;
    let mut run = combat_with_relic(2, Some(RelicId::OrnamentalFan));
    let fan = |frame: Value| frame["players"][0]["relic_state"].as_array().unwrap().iter()
        .find(|r| r["id"] == "ORNAMENTAL_FAN").unwrap().clone();
    assert_eq!(fan(frame(&run))["display_amount"], 0);
    for count in 1..=2 {
        let c = run.combat().unwrap();
        let hand = c.players[0].hand.iter().position(|&k| c.cards[k as usize].id == CardId::StrikeIronclad).unwrap();
        run.play_card(100, 0, hand, Some(0)).unwrap();
        let state = fan(frame(&run));
        assert_eq!(state["show_counter"], true);
        assert_eq!(state["display_amount"], count);
        assert_eq!(state["status"], if count == 2 {"active"} else {"normal"});
    }
    run.debug_win_combat().unwrap();
    let inventory=Game::Party(&run).inventory();
    let state=inventory["seats"][0]["relic_state"].as_array().unwrap().iter()
        .find(|r|r["id"]=="ORNAMENTAL_FAN").unwrap();
    assert_eq!(state["show_counter"], false);
}

#[test]
fn scroll_boxes_frames_record_both_packs_before_selection_for_solo_and_party() {
    use sts2_sim::{run::{NativeRewardContent, RunManager, RunOptions}, tables::RelicId};
    let mut encoder = NativeEncoder::new();
    let mut obs = NativeObsV1::new();
    let mut decision = DecisionId { episode: 1, generation: 0, actor: 0 };
    let scroll = ContentId::from_index(Namespace::RelicId, RelicId::ScrollBoxes.index()).unwrap();
    let mut solo = (0..128).map(|seed| RunManager::new(seed, RunOptions::default()))
        .find(|run| run.native_neow_offers().unwrap().iter().any(|(_, relic)| *relic == RelicId::ScrollBoxes))
        .expect("seeded Scroll Boxes offer");
    encoder.encode_solo(&solo, decision, &mut obs).unwrap();
    let view = obs.view().unwrap();
    let slot = (0..view.candidate_len).find(|&i| view.candidates.source[i].index()
        .is_some_and(|row| view.rows.model[row] == scroll)).unwrap();
    encoder.dispatch_solo(&mut solo, &mut decision, &mut obs, slot).unwrap();
    encoder.encode_solo(&solo, decision, &mut obs).unwrap();
    let reward = solo.native_reward(solo.native_reward_indices().next().unwrap()).unwrap();
    let NativeRewardContent::Bundles(bundles) = reward.content else { panic!("Scroll Boxes packs"); };
    let expected = bundles.iter().map(|pack| pack.iter().map(|card|
        (card.id.name().to_string(), card.upgraded)).collect::<Vec<_>>()).collect::<Vec<_>>();
    check_bundle_frame(Game::Solo(&solo), &obs, decision, &expected);

    let specs = [Character::Ironclad, Character::Silent].map(|character| PlayerSpec {
        net_id: if character == Character::Ironclad {100} else {101}, character,
    });
    let mut party = MpRun::new("SCROLL-BOXES-REPLAY", &specs, 0).unwrap();
    party.neow[1][0] = RelicId::ScrollBoxes;
    party.choose_neow(101, 1, 0).unwrap();
    let packs = party.scroll_box_bundles[1].as_mut().unwrap();
    packs[1][0].upgraded = true;
    let expected = packs.iter().map(|pack| pack.iter().map(|card|
        (card.id.name().to_string(), card.upgraded)).collect::<Vec<_>>()).collect::<Vec<_>>();
    decision.actor = 1;
    encoder.encode_multiplayer(&party, decision, 101, &mut obs).unwrap();
    check_bundle_frame(Game::Party(&party), &obs, decision, &expected);
}

fn check_bundle_frame(game: Game<'_>, obs: &NativeObsV1, decision: DecisionId, expected: &[Vec<(String, bool)>]) {
    let view = obs.view().unwrap();
    let slots = (0..view.candidate_len).filter(|&i| view.candidates.action[i] == ActionKind::PickCardBundle)
        .collect::<Vec<_>>();
    assert_eq!(slots.len(), 2);
    let source = RealSource { inventory: game.inventory(), floor: game.floor(), game,
        obs, team: None, decision, seats: if decision.actor == 0 {1} else {2} };
    let frame = source.replay_frame(slots[1] as i32, 0).unwrap();
    let packs = frame["opts"].as_array().unwrap().iter().filter(|o| o["action"] == "pick_card_bundle")
        .collect::<Vec<_>>();
    assert_eq!(packs.len(), 2);
    for (index, pack) in packs.iter().enumerate() {
        assert_eq!(pack["index"], index, "pack numbering must survive projection");
        assert_eq!(pack["card_ids"], json!(expected[index].iter().map(|c| &c.0).collect::<Vec<_>>()));
        assert_eq!(pack["upgraded"], json!(expected[index].iter().map(|c| c.1).collect::<Vec<_>>()));
        assert_eq!(pack["card_ids"].as_array().unwrap().len(), 3);
    }
    assert_eq!(frame["a"], *packs[1]);
    assert_eq!(frame["actor"], decision.actor);
    assert_eq!(frame["ph"], "CARD_REWARD", "Neow's party modal is a card-pack reward");
    println!("SCROLL_BOXES_FRAME={frame}");
}

#[test]
fn configured_mad_science_instances_survive_decks_and_reward_options_without_changing_observation_rows() {
    use sts2_sim::tables::{CardType, VarKey};
    let specs = [Character::Ironclad, Character::Silent].map(|character|
        PlayerSpec {net_id:if character == Character::Ironclad {100} else {101},character});
    let mut run = MpRun::new("MAD-SCIENCE-REPLAY", &specs, 0).unwrap();
    let mut variants = Vec::new();
    for (i, (kind, rider)) in [(CardType::Attack,2),(CardType::Skill,5),(CardType::Power,7)].into_iter().enumerate() {
        let mut card = Card::from_factory(CardId::MadScience, false, 900+i as u32).unwrap();
        card.card_type = kind;
        card.owner = 0;
        card.vars.retain(|(key,_)| *key != VarKey::Rider);
        card.vars.push((VarKey::Rider,rider));
        variants.push(card);
    }
    run.roster.player_mut(0).unwrap().deck = variants.clone();
    run.neow_cards[0] = Some(variants);
    let decision = DecisionId {episode:1,generation:0,actor:0};
    let mut encoder = NativeEncoder::new();
    let mut obs = NativeObsV1::new();
    encoder.encode_multiplayer(&run,decision,100,&mut obs).unwrap();
    let before = obs.view().unwrap().rows.integers.to_vec();
    let view = obs.view().unwrap();
    let slot = (0..view.candidate_len).find(|&i|view.candidates.action[i] == ActionKind::PickCard).unwrap();
    let game = Game::Party(&run);
    let frame = RealSource {inventory:game.inventory(),game,obs:&obs,team:None,decision,seats:2,floor:1}
        .replay_frame(slot as i32,0).unwrap();
    let options = frame["opts"].as_array().unwrap().iter().filter(|o|o["action"] == "pick_card").collect::<Vec<_>>();
    for (i,(kind,rider)) in [("ATTACK",2),("SKILL",5),("POWER",7)].into_iter().enumerate() {
        let runtime = &frame["players"][0]["deck_cards"][i][2];
        assert_eq!(runtime["tinker_type"],kind);
        assert_eq!(runtime["tinker_rider"],rider);
        assert_eq!(options[i]["runtime_vars"],*runtime);
        assert_eq!(runtime["generated_fields"]["effect_vars"]["damage"],12);
    }
    assert_eq!(obs.view().unwrap().rows.integers,before);
    if let Ok(path) = std::env::var("STS2_MAD_SCIENCE_FRAME") {
        std::fs::write(path,serde_json::to_vec_pretty(&frame).unwrap()).unwrap();
    }
    run.select_neow_card(100,0,Some(0)).unwrap();
    run.phase = MpPhase::Map;
    let (x,y) = run.world.run.coord.unwrap();
    let child = run.world.map.point(Coord::new(x,y)).unwrap().children[0] as usize;
    run.world.map.points[child].point_type = MapPointType::Monster;
    let destination = run.world.map.points[child].coord;
    for seat in 0..2 { run.vote(100+seat as u64,seat,destination).unwrap(); }
    let combat_frame = self::frame(&run);
    for tuple in combat_frame["hand"].as_array().unwrap() {
        assert_eq!(tuple[0],"MAD_SCIENCE");
        assert!(tuple[6]["tinker_rider"].is_number());
    }
    if let Ok(path) = std::env::var("STS2_MAD_SCIENCE_COMBAT_FRAME") {
        std::fs::write(path,serde_json::to_vec_pretty(&combat_frame).unwrap()).unwrap();
    }
}
