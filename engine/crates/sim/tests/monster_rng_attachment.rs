//! Game 0861bfa1 Combat/CombatState.cs:143,151-152: local RNG uses
//! seed + column + row + act + globally assigned attachment CombatId.
use sts2_sim::{combat::Combat,rng::Rng,run::{RunManager,RunOptions},tables::MonsterKind};

#[test]
fn monster_rng_counts_players_pets_and_prepared_enemies() {
    for seats in 1..=4 {
        let mut run=RunManager::new(7,RunOptions::default()).run_ctx().clone();
        run.coord=Some((2,1));
        run.act_index=1;
        let seed=run.rngs.seed;
        let deck=run.deck.clone();
        let mut combat=Combat::new(run,1);
        for _ in 1..seats {combat.add_player(80,80,3,0,&deck);}
        for ordinal in 0..3 {
            if ordinal==1 {combat.summon_osty(0,1).unwrap();}
            let expected_id=seats+ordinal+usize::from(ordinal>0);
            let (creature,ai)=combat.create_monster(MonsterKind::ShrinkerBeetle,0);
            let enemy=combat.add_enemy(creature,ai);
            assert_eq!(combat.cr(enemy).combat_id as usize,expected_id);
            assert_eq!(combat.cr(enemy).ai.as_ref().unwrap().rng,
                Rng::new(seed+2+1+1+expected_id as u64));
        }
    }
}
