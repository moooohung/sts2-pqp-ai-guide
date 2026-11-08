use sts2_native_obs::*;
use sts2_sim::{
    run::{RunManager, RunOptions, RunPhase, RunAction, RunActionKind},
    tables::RelicId,
};

#[test]
fn pael_pickup_selectors_retain_the_event_until_the_journaled_run_can_continue() {
    // Native action slots from the three main-37a12fb3 A10 failure journals,
    // ending with EventChoice("relic_2"). No state or inventory injection.
    let journals: &[(u64, &[usize])] = &[
        (20400155, &[
            0,0,0,4,2,3,4,0,1,2,3,1,3,4,0,2,1,0,0,0,4,6,5,1,3,1,2,3,2,2,4,1,7,3,5,5,6,3,6,3,
            2,1,3,1,3,4,3,2,2,2,0,2,3,3,1,1,2,2,4,1,1,0,0,0,7,0,7,9,3,5,4,6,3,5,1,1,3,3,4,0,
            3,2,0,0,0,1,13,1,0,0,0,0,6,4,5,8,5,7,1,1,3,0,0,0,0,5,1,2,6,6,5,4,1,3,4,3,0,1,1,1,
            0,2,3,1,0,0,0,10,11,2,12,11,11,11,11,11,14,0,1,2,1,1,2,6,1,1,5,7,1,0,3,2,1,1,1,1,1,8,
            4,2,0,0,0,4,3,1,0,2,0,0,0,0,4,0,1,1,0,0,1,1,2,0,0,1,2,6,3,0,7,5,2,3,1,0,1,3,1,1,
            0,1,4,4,2,2,0,0,1,
        ]),
        (20400507, &[
            0,0,0,2,4,5,0,1,0,1,4,1,0,2,4,1,2,1,3,0,0,0,0,3,1,1,1,2,1,4,3,0,2,3,2,1,1,1,1,1,
            2,2,2,1,2,0,1,2,4,0,2,1,5,2,2,4,3,1,2,4,2,2,0,0,3,2,2,2,1,3,3,2,1,1,2,1,1,2,2,3,
            0,3,0,3,0,3,0,3,0,3,0,3,0,3,0,3,0,3,1,0,2,3,2,2,1,2,1,0,1,1,2,0,2,3,1,3,3,0,2,0,
            1,3,0,0,0,0,0,0,0,0,2,0,4,0,2,0,3,2,2,1,0,0,0,0,0,0,1,0,0,4,3,3,2,2,2,4,1,2,1,0,
            1,0,0,2,2,7,10,4,1,2,2,2,1,5,5,3,0,3,3,1,1,0,4,1,2,1,1,2,1,3,3,0,3,0,0,0,0,0,0,0,
            2,4,2,3,3,0,1,1,2,2,4,2,2,2,3,1,2,0,1,2,2,2,2,1,2,2,2,1,1,1,0,0,2,3,1,0,1,1,0,0,
            0,0,1,
        ]),
        (20400646, &[
            0,1,4,3,0,2,3,2,3,0,1,2,2,3,0,3,0,0,3,0,3,0,3,0,3,0,3,0,3,0,3,0,3,0,3,0,3,0,3,0,
            3,0,3,0,3,0,3,0,3,0,3,0,3,0,3,0,3,0,3,0,3,0,3,0,3,0,3,0,3,0,0,1,1,0,1,0,1,4,5,2,
            4,4,3,5,0,1,2,4,0,2,2,0,1,0,0,11,7,14,0,4,1,4,2,3,5,3,3,4,0,0,0,0,3,4,1,3,2,5,3,3,
            2,0,1,2,0,4,2,2,1,2,0,0,3,3,2,2,0,0,0,0,0,0,1,1,14,0,0,1,5,1,1,3,11,10,14,5,1,2,2,0,
            0,3,1,1,1,0,2,0,0,0,0,4,5,2,14,0,0,0,4,1,0,0,1,1,1,1,1,2,3,4,0,3,1,3,2,2,1,0,0,2,
            0,1,1,3,2,3,2,0,2,2,0,0,0,1,
        ]),
    ];
    for &(seed, slots) in journals {
        let mut run = RunManager::new(seed, RunOptions {
            ascension: 10, saved_wongo_points: Some(0), ..RunOptions::default()
        });
        let mut encoder = NativeEncoder::new();
        let mut obs = Box::new(NativeObsV1::new());
        let mut current = DecisionId { episode: seed, generation: 0, actor: 0 };
        let mut relic = None;
        for (tick, &slot) in slots.iter().enumerate() {
            encoder.encode_solo(&run, current, &mut obs).unwrap();
            let view=obs.view().unwrap();
            // Old journals include failed TryToProcure clicks. Native policy
            // legality now excludes those no-ops; preserve the recorded engine
            // attempt without weakening the checked observation dispatcher.
            if !view.candidates.sampleable(slot,view.candidate_len)
                && view.candidates.action[slot]==ActionKind::BuyPotion {
                let source=view.candidates.source[slot].index().unwrap();
                assert_eq!(view.rows.kind[source],RowKind::Offer);
                let index=usize::try_from(view.rows.integers[source][1]).unwrap();
                let before=(run.run_ctx().gold,run.run_ctx().potions.clone());
                let stock=|run:&RunManager|run.native_shop_offers()
                    .filter(|o|matches!(o.kind,sts2_sim::run::NativeShopKind::Potion))
                    .map(|o|(o.index,o.potion,o.price,o.sold)).collect::<Vec<_>>();
                let before_stock=stock(&run);
                let mut attempt=RunAction::new(RunActionKind::BuyPotion);attempt.index=Some(index);
                run.take_action(attempt).unwrap();
                assert_eq!((run.run_ctx().gold,run.run_ctx().potions.clone()),before);
                assert_eq!(stock(&run),before_stock);
                continue;
            }
            if tick + 1 == slots.len() {
                assert_eq!(run.current_event_id(), Some("PAEL"), "{seed}");
                assert_eq!(run.run_ctx().total_floor, 18, "{seed}");
                let Command::Solo(SoloCommand::EventChoice { option }) =
                    obs.binding(slot, current, 1).unwrap().command else { panic!("expected event choice"); };
                assert_eq!(option, EventOptionId::PaelPageNameRelicIdEntry);
                relic = run.observation_ancient_relic("relic_2");
                assert!(matches!(relic, Some(RelicId::PaelsGrowth | RelicId::PaelsTooth)));
                println!("{seed}@{}: PAEL {relic:?}", tick + 1);
            }
            encoder.dispatch_solo(&mut run, &mut current, &mut obs, slot)
                .unwrap_or_else(|error|panic!("seed={seed} tick={tick} slot={slot}: {error:?}"));
        }
        // C# awaits the pickup's deck selection inside AncientEventModel.RelicOption.
        // Publishing that event-local selector must preserve its parent identity.
        encoder.encode_solo(&run, current, &mut obs).unwrap();
        assert_eq!(run.phase(), RunPhase::Event);
        assert_eq!(run.current_event_id(), Some("PAEL"));
        let before = run.run_ctx().deck.len();
        let mut selected = 0;
        while run.phase() == RunPhase::Event && selected < 5 {
            let view = obs.view().unwrap();
            let slot = (0..view.candidate_len).find(|&index| {
                view.candidates.sampleable(index, view.candidate_len)
                    && view.candidates.flags[index] & Flag::Selected.bit() == 0
                    && obs.binding(index, current, 1).is_ok_and(|binding|
                        matches!(binding.command, Command::Solo(SoloCommand::Choose { .. })))
            }).unwrap();
            encoder.dispatch_solo(&mut run, &mut current, &mut obs, slot).unwrap();
            selected += 1;
            encoder.encode_solo(&run, current, &mut obs).unwrap();
        }
        assert_eq!(run.phase(), RunPhase::MapChoice, "{seed}");
        assert!(run.current_event_id().is_none());
        assert!(run.native_fault().is_none());
        match relic.unwrap() {
            RelicId::PaelsTooth => {
                assert_eq!(selected, 5);
                assert_eq!(run.run_ctx().deck.len(), before - 5);
                assert_eq!(run.run_ctx().paels_tooth_cards.len(), 5);
            }
            RelicId::PaelsGrowth => {
                assert_eq!(selected, 1);
                assert!(run.run_ctx().deck.iter().any(|card| card.enchantments.iter()
                    .any(|(id, amount)| id.name() == "Clone" && *amount == 4)));
            }
            _ => unreachable!(),
        }
        let view = obs.view().unwrap();
        let slot = (0..view.candidate_len).find(|&index|
            view.candidates.sampleable(index, view.candidate_len)
                && obs.binding(index, current, 1).is_ok_and(|binding|
                    matches!(binding.command, Command::Solo(SoloCommand::Move { .. })))).unwrap();
        encoder.dispatch_solo(&mut run, &mut current, &mut obs, slot).unwrap();
        encoder.encode_solo(&run, current, &mut obs).unwrap();
        assert!(run.run_ctx().total_floor > 18, "{seed}");
        println!("{seed}: selector completed ({selected} cards), continued to floor {}", run.run_ctx().total_floor);
    }
}
