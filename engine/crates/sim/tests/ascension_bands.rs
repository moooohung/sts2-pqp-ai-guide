//! C# ShrinkerBeetle: ToughEnemies changes HP at A8, DeadlyEnemies damage at A9.
use sts2_sim::{combat::Combat,run::{RunManager,RunOptions},tables::{MonsterKind,StateKind,monster_def}};
#[test]
fn every_ascension_uses_independent_tough_and_deadly_thresholds() {
    for ascension in 0..=10 {
        let run=RunManager::new(1,RunOptions{ascension,..Default::default()}).run_ctx().clone();
        let mut c=Combat::new(run,1);
        let (creature,ai)=c.create_monster(MonsterKind::ShrinkerBeetle,0);
        assert_eq!(c.fault,None);
        assert_eq!(creature.hp_range,Some(if ascension>=8 {(40,42)}else{(38,40)}));
        let state=monster_def(ai.kind,ai.band as usize).states.iter().find(|s|s.id=="CHOMP_MOVE").unwrap();
        let StateKind::Move{intents,..}=state.kind else{panic!("Chomp move");};
        assert_eq!(intents[0].damage,if ascension>=9 {8}else{7});
    }
}
