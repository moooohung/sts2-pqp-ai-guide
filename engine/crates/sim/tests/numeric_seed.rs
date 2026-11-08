//! The public numeric API must preserve seed-text initialization, including
//! private pending choices, maps, bag order and RNG words (Debug snapshots).
use sts2_sim::run::mp::{Character, MpRun, PlayerSpec};

#[test]
fn numeric_seed_matches_modern_text_for_all_rosters() {
    let characters = [Character::Ironclad, Character::Necrobinder,
        Character::Silent, Character::Defect, Character::Regent];
    for text in ["73003", "HUMAN-REPLAY-CONTRACT", "1"] {
        for seats in 2..=4 {
            for offset in 0..characters.len() {
                let specs: Vec<_> = (0..seats).map(|slot| PlayerSpec {
                    net_id: 100 + slot as u64 * 7,
                    character: characters[(slot + offset) % characters.len()],
                }).collect();
                for ascension in [0, 10] {
                    let mut original = MpRun::new(text, &specs, ascension).unwrap();
                    let mut numeric = MpRun::from_numeric_seed(
                        sts2_sim::rng::deterministic_hash64(text), &specs, ascension).unwrap();
                    assert_eq!(original.seen_relics, numeric.seen_relics);
                    // HashSet's randomized iteration is not gameplay state.
                    original.seen_relics.clear();
                    numeric.seen_relics.clear();
                    // Provenance only: the numeric path cannot know private text.
                    numeric.world.string_seed = original.world.string_seed.clone();
                    assert_eq!(format!("{original:?}"), format!("{numeric:?}"),
                        "{text}, {seats} seats, ascension {ascension}");
                }
            }
        }
    }
}
