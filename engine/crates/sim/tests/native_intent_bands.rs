use sts2_sim::{combat::Combat, rng::Rng, run::{RunManager,RunOptions}, tables::MonsterKind};

#[test]
fn dynamic_public_intents_cover_every_exported_ascension_band() {
    for (ascension,band) in [(0,0),(8,2),(10,1)] {
        let mut run=RunManager::new(1,RunOptions::default()).run_ctx().clone();
        run.ascension=ascension;
        run.deck.clear();
        let mut combat=Combat::new(run,1);
        combat.setup_encounter("WATERFALL_GIANT_BOSS",&mut Rng::new(1));
        let enemy=combat.enemies[0];
        for (kind,state) in [
            (MonsterKind::TheForgotten,"DREAD"),
            (MonsterKind::WaterfallGiant,"PRESSURE_GUN_MOVE"),
            (MonsterKind::WaterfallGiant,"EXPLODE_MOVE"),
            (MonsterKind::TestSubject,"MULTI_CLAW_MOVE"),
        ] {
            let (_,mut ai)=combat.create_monster(kind,0);
            assert_eq!(ai.band,band);
            ai.current=ai.def().states.iter().position(|s|s.id==state).unwrap() as u8;
            ai.next_unset=false;
            combat.cr_mut(enemy).ai=Some(ai);
            let intent=combat.public_intent(enemy,0,0).unwrap().expect("move exposes an intent");
            assert!(intent.damage>=0);
            assert!(intent.hits>0);
        }
    }
}
