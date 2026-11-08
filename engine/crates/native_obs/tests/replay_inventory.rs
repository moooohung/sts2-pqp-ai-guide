use serde_json::{Value, json};
use sts2_native_obs::{replay::{Game, RealSource}, *};
use sts2_sim::run::{Coord, RunManager, RunOptions, RunPhase, RunActionKind,
    mp::{Character, MpPhase, MpRun, PlayerSpec}};
use sts2_sim::tables::{MapPointType, PotionId};

fn solo_frame(run: &RunManager) -> Value {
    let decision = DecisionId { episode: 1, generation: 0, actor: 0 };
    let mut encoder = NativeEncoder::new();
    let mut obs = NativeObsV1::new();
    encoder.encode_solo(run, decision, &mut obs).unwrap();
    let view = obs.view().unwrap();
    let slot = (0..view.candidate_len).find(|&i|
        view.candidates.sampleable(i, view.candidate_len)).unwrap();
    let game = Game::Solo(run);
    RealSource { inventory: game.inventory(), floor: game.floor(), game, obs: &obs,
        team: None, decision, seats: 1 }.replay_frame(slot as i32, 0).unwrap()
}

#[test]
fn solo_rest_exports_the_public_heal_and_updated_map_hp_not_retained_combat_hp() {
    let mut run = RunManager::new_for_character(2026110201,
        RunOptions { ascension: 10, ..Default::default() }, Character::Silent);
    for _ in 0..500 {
        if run.phase() == RunPhase::Combat {
            // Take real enemy damage before exercising the ordinary carryback.
            let end = run.available_actions().into_iter()
                .find(|a| a.kind == RunActionKind::EndTurn).unwrap();
            run.take_action(end).unwrap();
            run.debug_win_combat().unwrap();
            continue;
        }
        let actions = run.available_actions();
        if run.phase() == RunPhase::RestSite {
            let heal = actions.iter().find(|a| a.option_id == Some("HEAL")).unwrap().clone();
            let before = run.run_ctx().hp;
            let amount = run.native_rest_heal_amount();
            let frame = solo_frame(&run);
            assert_eq!(frame["hp"], before);
            assert_eq!(frame["rest_text_vars"]["Heal"], amount);
            run.take_action(heal).unwrap();
            assert_eq!(run.run_ctx().hp, (before + amount).min(run.run_ctx().max_hp));
            let map = solo_frame(&run);
            assert_eq!(map["hp"], run.run_ctx().hp);
            assert!(map["hp"].as_i64().unwrap() > before as i64);
            return;
        }
        let action = actions.iter().find(|a| a.point_type == Some(MapPointType::RestSite))
            .or_else(|| actions.iter().find(|a| matches!(a.kind,
                RunActionKind::SkipRewards | RunActionKind::Skip | RunActionKind::LeaveShop)))
            .or_else(|| actions.first()).expect("run remains actionable").clone();
        run.take_action(action).unwrap();
    }
    panic!("seeded route did not reach rest");
}

#[test]
fn solo_combat_before_any_potion_exports_the_run_capacity_not_an_empty_belt() {
    let mut run = RunManager::new_for_character(2026110701,
        RunOptions { ascension: 10, ..Default::default() }, Character::Regent);
    for _ in 0..200 {
        if run.phase() == RunPhase::Combat {
            assert!(run.run_ctx().potions.iter().all(Option::is_none), "seed obtained a potion before combat");
            let capacity = run.run_ctx().max_potion_slots as usize;
            assert!(capacity > 0);
            let frame = solo_frame(&run);
            assert_eq!(frame["ph"], "COMBAT");
            for state in [&frame, &frame["players"][0]] {
                assert_eq!(state["ps"], capacity);
                assert_eq!(state["pv"], json!(vec![Value::Null; capacity]));
                assert_eq!(state["pot"], json!([]));
            }
            return;
        }
        let actions = run.available_actions();
        let action = actions.iter().find(|a| a.point_type == Some(MapPointType::Monster))
            .or_else(|| actions.iter().find(|a| matches!(a.kind,
                RunActionKind::SkipRewards | RunActionKind::Skip | RunActionKind::LeaveShop)))
            .or_else(|| actions.first()).expect("run remains actionable").clone();
        run.take_action(action).unwrap();
    }
    panic!("seeded route did not reach combat");
}

#[test]
fn every_party_seat_keeps_physical_potion_slots_and_capacity_outside_combat() {
    let specs = [PlayerSpec { net_id: 100, character: Character::Ironclad },
        PlayerSpec { net_id: 101, character: Character::Silent }];
    let mut run = MpRun::from_numeric_seed(2026110202, &specs, 10).unwrap();
    run.roster.player_mut(0).unwrap().potions = vec![None, Some(PotionId::Firepotion)];
    run.roster.player_mut(1).unwrap().potions = vec![Some(PotionId::Blockpotion), None];
    for phase in [MpPhase::Neow, MpPhase::Map, MpPhase::Rewards, MpPhase::Shop, MpPhase::RestSite] {
        run.phase = phase;
        let inventory = Game::Party(&run).inventory();
        assert_eq!(inventory["seats"][0]["pot"], json!(["FirePotion"]));
        assert_eq!(inventory["seats"][0]["pv"], json!([null, "FirePotion"]));
        assert_eq!(inventory["seats"][1]["pv"], json!(["BlockPotion", null]));
        assert_eq!(inventory["seats"][0]["ps"], 2);
        assert_eq!(inventory["seats"][1]["ps"], 2);
    }
    run.phase = MpPhase::Map;
    let (x, y) = run.world.run.coord.unwrap();
    let child = run.world.map.point(Coord::new(x, y)).unwrap().children[0] as usize;
    run.world.map.points[child].point_type = MapPointType::Monster;
    let dest = run.world.map.points[child].coord;
    for seat in 0..2 { run.vote(100 + seat as u64, seat, dest).unwrap(); }
    let inventory = Game::Party(&run).inventory();
    assert_eq!(inventory["seats"][0]["pv"], json!([null, "FirePotion"]));
}

#[test]
fn shop_frames_keep_stock_separate_from_affordability_and_potion_capacity() {
    let specs = [PlayerSpec { net_id: 100, character: Character::Ironclad },
        PlayerSpec { net_id: 101, character: Character::Silent }];
    let mut run = MpRun::from_numeric_seed(2026110202, &specs, 10).unwrap();
    run.phase = MpPhase::Map;
    let (x, y) = run.world.run.coord.unwrap();
    let child = run.world.map.point(Coord::new(x, y)).unwrap().children[0] as usize;
    run.world.map.points[child].point_type = MapPointType::Shop;
    let dest = run.world.map.points[child].coord;
    for seat in 0..2 { run.vote(100 + seat as u64, seat, dest).unwrap(); }
    for (gold, full) in [(0, false), (10000, true)] {
        let player = run.roster.player_mut(0).unwrap();
        player.gold = gold;
        player.potions.fill(if full {Some(PotionId::Firepotion)} else {None});
        let decision = DecisionId { episode: 1, generation: 0, actor: 0 };
        let mut encoder = NativeEncoder::new();
        let mut obs = NativeObsV1::new();
        encoder.encode_multiplayer(&run, decision, 100, &mut obs).unwrap();
        let view = obs.view().unwrap();
        let slot = (0..view.candidate_len).find(|&i|
            view.candidates.sampleable(i, view.candidate_len)).unwrap();
        let game = Game::Party(&run);
        let before = format!("{run:?}");
        let frame = RealSource { inventory: game.inventory(), floor: game.floor(), game,
            obs: &obs, team: None, decision, seats: 2 }.replay_frame(slot as i32, 0).unwrap();
        let stock = frame["shop_stock"].as_array().expect("public merchant stock");
        for action in ["buy_card", "buy_relic", "buy_potion"] {
            let offers: Vec<_> = stock.iter().filter(|o| o["action"] == action).collect();
            assert!(!offers.is_empty(), "{action} must stay visible");
            assert!(offers.iter().all(|o| o["sold"] == false && o["price"].as_i64().unwrap() > 0));
            if gold == 0 || action == "buy_potion" {
                assert!(offers.iter().all(|o| o["enabled"] == false));
            }
            assert!(offers.iter().all(|o| o["affordable"] == (gold != 0)));
        }
        assert_eq!(format!("{run:?}"), before, "projection must not advance the merchant");
    }
}
