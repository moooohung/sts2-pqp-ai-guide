//! C# act 2/3 then room boss: unchanged inventory and HP, ordinary boss entry.
use sts2_sim::run::{RunManager,RunOptions,mp::*};
#[test]
fn coordinate_free_double_boss_win_enters_architect_not_the_map(){
    for character in [Character::Ironclad,Character::Silent,Character::Defect,Character::Regent,Character::Necrobinder] {
        let mut run=RunManager::new_debug_late_boss(51,
            RunOptions{ascension:10,..Default::default()},character,2).unwrap();
        run.debug_win_combat().unwrap();
        assert_eq!(run.run_ctx().coord,None);
        assert_eq!(run.current_event_id(),Some("THE_ARCHITECT"));
        assert_eq!(run.phase(),sts2_sim::run::RunPhase::Event);
    }
}
#[test]
fn debug_act_begins_with_unvisited_ancient_and_no_boss_or_inventory_grants(){
    for act in 1..=2{
        let mut solo=RunManager::new_debug_act(51,RunOptions::default(),Character::Silent,act).unwrap();
        assert!(solo.combat().is_none());assert_eq!(solo.run_ctx().coord,None);
        let actions=solo.available_actions();
        assert_eq!(actions.iter().filter(|a|a.kind==sts2_sim::run::RunActionKind::Move).count(),1);
        assert!(actions.iter().any(|a|a.point_type==Some(sts2_sim::tables::MapPointType::Ancient)));
        let specs=[PlayerSpec{net_id:100,character:Character::Silent},PlayerSpec{net_id:101,character:Character::Regent}];
        let mut mp=MpRun::new_debug_act("51",&specs,10,act).unwrap();
        assert_eq!(mp.world.room_id,Some(0));
        assert_eq!(mp.world.run.coord,None);assert!(mp.combat().is_none());
        let menu=mp.census_menu().unwrap();
        assert_eq!(menu.len(),2);
        assert!(menu.iter().all(|d|d.decision.kind=="vote"&&d.decision.index==mp.world.map.start as usize));
        mp.census_take(&menu[0].decision,&[]).unwrap();
        assert_eq!(mp.world.run.coord,None);
        assert_eq!(mp.phase,MpPhase::Map);
    }
}
#[test]
fn solo_debug_act_and_boss_do_not_grant_inventory_or_skip_combat() {
    for character in [Character::Ironclad,Character::Silent,Character::Defect,Character::Regent,Character::Necrobinder] {
        for act in 1..=2 {
            let base=RunManager::new_for_character(41,RunOptions::default(),character);
            let trial=RunManager::new_debug_late_boss(41,RunOptions::default(),character,act).unwrap();
            assert_eq!(trial.run_ctx().deck.iter().map(|c|c.id).collect::<Vec<_>>(),base.run_ctx().deck.iter().map(|c|c.id).collect::<Vec<_>>());
            assert_eq!(trial.run_ctx().hp,base.run_ctx().hp);
            assert_eq!(trial.run_ctx().max_hp,base.run_ctx().max_hp);
            assert_eq!(trial.run_ctx().total_floor,base.run_ctx().total_floor+1);
            assert_eq!(trial.run_ctx().act_index,act as i32);
            assert_eq!(trial.run_ctx().coord,None);
            assert!(trial.combat().unwrap().enemies.len()>0);
            assert_eq!(trial.combat().unwrap().fault,None);
        }
    }
}
#[test]
fn mp_debug_boss_preserves_every_seats_character_and_starting_inventory() {
    let chars=[Character::Ironclad,Character::Silent,Character::Defect,Character::Regent,Character::Necrobinder];
    for seats in 2..=4 {for act in 1..=2 {
        let specs:Vec<_>=(0..seats).map(|s|PlayerSpec{net_id:100+s as u64,character:chars[(s+act)%5]}).collect();
        let base=MpRun::new("41",&specs,10).unwrap();
        let trial=MpRun::new_debug_late_boss("41",&specs,10,act).unwrap();
        assert_eq!(trial.world.act_floor,0);
        assert_eq!(trial.world.run.coord,None);
        assert_eq!(trial.world.run.total_floor,base.world.run.total_floor+1);
        for seat in 0..seats {
            assert_eq!(format!("{:?}",trial.roster.player(seat).unwrap()),format!("{:?}",base.roster.player(seat).unwrap()));
            assert_eq!(trial.combat().unwrap().players[seat as u8].character,Some(specs[seat].character));
        }
        assert_eq!(trial.combat().unwrap().fault,None);
    }}
}
