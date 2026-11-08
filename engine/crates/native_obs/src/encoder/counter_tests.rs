use super::*;

#[test]
fn displayed_relic_counters_follow_original_public_values() {
    // Expected values from each C# DisplayAmount, not encoder output.
    let c = Combat::new(RunManager::new(7, sts2_sim::run::RunOptions::default()).run_ctx().clone(), 7);
    for (id, used, outside, inside) in [
        (RelicId::SilverCrucible, 0, Some(3), Some(3)),
        (RelicId::SilverCrucible, 1, Some(2), Some(2)),
        (RelicId::SilverCrucible, 2, Some(1), Some(1)),
        (RelicId::SilverCrucible, 3, None, None),
        (RelicId::WingedBoots, 2, Some(1), Some(1)),
        (RelicId::WingedBoots, 3, None, None),
        (RelicId::FishingRod, 5, Some(2), Some(2)),
        (RelicId::BookOfFiveRings, 6, Some(1), Some(1)),
        (RelicId::JossPaper, 7, Some(2), Some(2)),
        (RelicId::GalacticDust, 12, Some(2), Some(2)),
        (RelicId::Nunchaku, 19, Some(9), Some(9)),
        (RelicId::PenNib, 19, Some(9), Some(9)),
        (RelicId::TuningFork, 4, Some(4), Some(4)),
        (RelicId::PumpkinCandle, 4, Some(4), Some(4)),
        (RelicId::SwordOfStone, 4, Some(4), Some(4)),
        (RelicId::LastingCandy, 3, Some(1), Some(1)),
        (RelicId::PaelsWing, 3, Some(1), Some(1)),
        (RelicId::WongosMysteryTicket, 4, Some(1), Some(1)),
        (RelicId::WongosMysteryTicket, 5, None, None),
        (RelicId::ToyBox, 7, Some(1), Some(1)),
        (RelicId::ToyBox, 15, None, None),
        (RelicId::PaelsTooth, 4, Some(4), Some(4)),
        (RelicId::PaelsTooth, 0, None, None),
        (RelicId::BrilliantScarf, 4, None, Some(4)),
        (RelicId::BrilliantScarf, 5, None, None),
        (RelicId::Metronome, 6, None, Some(6)),
        (RelicId::Metronome, 7, None, None),
        (RelicId::PaelsLegion, 2, None, Some(2)),
        (RelicId::PaelsLegion, 0, None, None),
        (RelicId::Pocketwatch, 2, None, Some(2)),
        (RelicId::VelvetChoker, 2, None, Some(2)),
        (RelicId::LetterOpener, 5, None, Some(2)),
        (RelicId::IronClub, 5, Some(1), Some(1)),
        (RelicId::IronClub, 0, Some(0), Some(0)),
        (RelicId::IronClub, 3, Some(3), Some(3)),
        (RelicId::IronClub, 4, Some(0), Some(0)),
    ] {
        let mut relic = Relic::new(id); relic.counter = used;
        assert_eq!(relic_counter(&relic, None), outside, "{id:?} outside");
        assert_eq!(relic_counter(&relic, Some(&c)), inside, "{id:?} combat");
    }
}
