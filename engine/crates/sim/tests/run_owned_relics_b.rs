use sts2_sim::{relic::Relic, run::RunManager, tables::RelicId};

#[test]
fn run_inventory_queries_and_mutations_are_scoped_to_each_owner() {
    let mut run = RunManager::new(1, Default::default()).run_ctx().clone();
    run.relics.clear();
    for owner in 0..4 {
        let mut relic = Relic::new(RelicId::TheCourier);
        relic.owner = owner;
        relic.enabled = owner != 2;
        run.relics.push(relic);
    }
    for owner in 0..4 {
        assert!(run.has_owned_relic(owner, RelicId::TheCourier));
        assert!(!run.has_owned_relic(owner, RelicId::RegalPillow));
        assert_eq!(run.owned_relics(owner).count(), 1);
        for r in run.owned_relics_mut(owner) { r.counter = owner as i32 + 10; }
    }
    for owner in 0..4 {
        let r = run.owned_relics(owner).next().unwrap();
        assert_eq!(r.counter, owner as i32 + 10);
        assert_eq!(r.enabled, owner != 2);
    }
}
