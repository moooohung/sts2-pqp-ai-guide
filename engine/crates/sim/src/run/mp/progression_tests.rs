use super::*;
use crate::run::mp::{Character, PlayerSpec};

fn run(ascension: i32) -> MpRun {
    let specs = [
        PlayerSpec { net_id: 7, character: Character::Ironclad },
        PlayerSpec { net_id: 42, character: Character::Necrobinder },
    ];
    MpRun::new("73001", &specs, ascension).unwrap()
}

#[test]
fn map_votes_bind_to_the_current_act_after_transition() {
    let mut run = run(0);
    run.phase = MpPhase::Map;
    run.world.run.act_index = 1;
    let (col, row) = run.world.run.coord.unwrap();
    let source = Coord::new(col, row);
    run.voting.on_location_changed(1, source);
    let child = run.world.map.point(source).unwrap().children[0];
    let destination = run.world.map.points[child as usize].coord;

    run.vote(7, 0, destination).unwrap();
    assert_eq!(run.voting.votes()[0].unwrap().coord, destination);
}

#[test]
fn advancing_act_regenerates_shared_map_and_resets_encounter_cursors() {
    let mut run = run(0);
    run.phase = MpPhase::Map;
    let boss = run.world.map.points[run.world.map.boss as usize].coord;
    assert!(run.world.map.point(boss).unwrap().children.is_empty());
    run.world.run.coord = Some((boss.col, boss.row));
    run.normal_cursor = 3;
    run.elite_cursor = 2;
    run.boss_cursor = 1;
    let generation = run.voting.generation;

    run.advance_act().unwrap();

    assert_eq!(run.world.run.act_index, 1);
    assert_eq!(run.voting.generation, generation + 1);
    assert_eq!(run.normal_cursor, 0);
    assert_eq!(run.elite_cursor, 0);
    assert_eq!(run.boss_cursor, 0);
    let start = run.world.map.points[run.world.map.start as usize].coord;
    assert_eq!(run.world.run.coord, Some((start.col, start.row)));

    let child = run.world.map.point(start).unwrap().children[0];
    let destination = run.world.map.points[child as usize].coord;
    run.vote(7, 0, destination).unwrap();
    assert_eq!(run.voting.votes()[0].unwrap().coord, destination);
}

#[test]
fn spoils_map_transition_registers_each_owners_quest() {
    let mut run=run(0);
    run.roster.next_card_id+=1;
    let instance=run.roster.next_card_id;
    let mut card=Card::from_factory(CardId::SpoilsMap,false,instance).unwrap();
    card.owner=1;
    run.roster.player_mut(1).unwrap().deck.push(card);
    run.phase=MpPhase::Map;
    let boss=run.world.map.points[run.world.map.boss as usize].coord;
    run.world.run.coord=Some((boss.col,boss.row));
    run.advance_act().unwrap();
    assert_eq!(run.world.run.act_index,1);
    let treasures:Vec<_>=run.world.map.points.iter().filter(|p|p.point_type==crate::tables::MapPointType::Treasure).collect();
    assert_eq!(treasures.len(),1);
    assert_eq!(treasures[0].spoils_quests,vec![instance]);
}

#[test]
fn final_act_transition_enters_architect_and_proceeds_to_victory() {
    for seats in 2..=4 {
        let characters=[Character::Ironclad,Character::Silent,Character::Defect,Character::Regent];
        let specs:Vec<_>=(0..seats).map(|seat|PlayerSpec{net_id:100+seat as u64,character:characters[seat]}).collect();
        let mut run=MpRun::new("73001",&specs,0).unwrap();
        for act in 0..3 {
            assert_eq!(run.world.run.act_index,act);
            let boss=run.world.map.points[run.world.map.boss as usize].coord;
            run.world.run.coord=Some((boss.col,boss.row));
            run.enter_boss().unwrap();
            run.debug_win_combat().unwrap();
            assert_eq!(run.phase,MpPhase::Rewards);
            run.offer_rewards().unwrap();
            for seat in 0..seats {
                run.take_gold_skip_remaining_rewards(specs[seat].net_id,seat).unwrap();
                if seat+1<seats {assert_eq!(run.phase,MpPhase::Rewards);}
            }
            run.advance_act().unwrap();
            if act<2 {assert_eq!(run.phase,MpPhase::Map);}
        }
        assert_eq!(run.phase, MpPhase::Event);
        for (seat,spec) in specs.iter().enumerate() {
            let lines=match spec.character {Character::Silent=>1,Character::Ironclad|Character::Necrobinder=>2,_=>3};
            for line in 0..lines {
                assert_eq!(run.event_options(spec.net_id,seat).unwrap(),
                    vec![if line+1<lines{"advance"}else{"proceed"}]);
                run.choose_event_option(spec.net_id,seat,0).unwrap();
            }
        }
        assert_eq!(run.phase, MpPhase::RunOver);
        assert!(run.roster.players().iter().all(|p|p.hp>0));
    }
}
