//! Frozen-r7 `map/room_generation.py`.

use crate::rng::Rng;
use crate::identity::EncounterId;

use super::catalog::{ActDef, EncounterDef};

#[derive(Clone, Debug)]
pub(crate) struct GeneratedRooms {
    pub events: Vec<crate::identity::EventId>,
    pub normal: Vec<EncounterId>,
    pub elite: Vec<EncounterId>,
    pub boss: EncounterId,
    pub public_boss: crate::identity::PublicBossId,
    pub ancient: Option<crate::identity::EventId>,
    pub second_boss: Option<EncounterId>,
}

fn encounter_sequence(
    rng: &mut Rng,
    pool: &'static [EncounterDef],
    count: usize,
    out: &mut Vec<EncounterId>,
    previous_pool: &'static [EncounterDef],
) {
    let mut bag: Vec<EncounterDef> = Vec::with_capacity(pool.len());
    for _ in 0..count {
        if bag.is_empty() {
            bag.extend_from_slice(pool);
        }
        // C# ActModel.AddWithoutRepeatingTags compares against the last encounter
        // in the shared output list, including across the weak/regular pool switch.
        let last = out.last().and_then(|id| {
            pool.iter()
                .chain(previous_pool.iter())
                .find(|row| row.id == *id)
                .copied()
        });
        let eligible = |row: &EncounterDef| {
            last.is_none_or(|prev| row.id != prev.id && row.tags & prev.tags == 0)
        };
        let reject = bag.iter().any(eligible);
        loop {
            // r7 samples the whole bag before rejecting a same/tag-overlap row.
            let roll = rng.next_double(0.0, 1.0) * bag.len() as f64;
            let index = roll.floor() as usize;
            if index >= bag.len() {
                break;
            }
            if reject && !eligible(&bag[index]) {
                continue;
            }
            out.push(bag.remove(index).id);
            break;
        }
    }
}

pub(crate) fn generate_rooms(
    act: &'static ActDef,
    rng: &mut Rng,
    shared_ancients: &[crate::identity::EventId],
    double_boss: bool,
) -> GeneratedRooms {
    generate_rooms_for_party(act, rng, shared_ancients, double_boss, false)
}

pub(crate) fn generate_rooms_for_party(
    act: &'static ActDef, rng: &mut Rng, shared_ancients: &[crate::identity::EventId],
    double_boss: bool, multiplayer: bool,
) -> GeneratedRooms {
    let mut events = act.events.to_vec();
    rng.shuffle(&mut events);

    let mut normal = Vec::with_capacity(act.num_rooms);
    encounter_sequence(rng, act.weak, act.weak_count, &mut normal, act.weak);
    encounter_sequence(
        rng,
        act.normal,
        act.num_rooms - usize::from(multiplayer) - act.weak_count,
        &mut normal,
        act.weak,
    );

    let mut elite = Vec::with_capacity(15);
    encounter_sequence(rng, act.elite, 15, &mut elite, act.elite);

    let boss = act.bosses[rng.choice_index(act.bosses.len())];
    let ancient_count = act.ancients.len() + shared_ancients.len();
    let ancient = if ancient_count == 0 {
        None
    } else {
        let i = rng.choice_index(ancient_count);
        if i < act.ancients.len() {
            Some(act.ancients[i])
        } else {
            Some(shared_ancients[i - act.ancients.len()])
        }
    };

    let second_boss = if double_boss {
        let remaining: Vec<EncounterId> = act
            .bosses
            .iter()
            .copied()
            .filter(|id| *id != boss)
            .collect();
        Some(remaining[rng.choice_index(remaining.len())])
    } else {
        None
    };

    GeneratedRooms {
        events,
        normal,
        elite,
        boss,
        public_boss: crate::identity::PublicBossId::from_name(boss.name()).expect("source-backed public boss identity"),
        ancient,
        second_boss,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regular_encounters_reject_tags_from_the_last_weak_encounter() {
        static WEAK: &[EncounterDef] = &[EncounterDef { id: EncounterId::FuzzyWurmCrawlerWeak, tags: 1 }];
        static NORMAL: &[EncounterDef] = &[
            EncounterDef { id: EncounterId::NibbitsNormal, tags: 1 },
            EncounterDef { id: EncounterId::MawlerNormal, tags: 2 },
        ];
        for seed in 0..32 {
            let mut rng = Rng::new(seed);
            let mut out = vec![EncounterId::FuzzyWurmCrawlerWeak];
            encounter_sequence(&mut rng, NORMAL, 1, &mut out, WEAK);
            assert_eq!(out, [EncounterId::FuzzyWurmCrawlerWeak, EncounterId::MawlerNormal], "seed {seed}");
        }
    }

    #[test]
    fn overlapping_tags_are_allowed_when_the_entire_bag_overlaps() {
        static WEAK: &[EncounterDef] = &[EncounterDef { id: EncounterId::FuzzyWurmCrawlerWeak, tags: 1 }];
        static NORMAL: &[EncounterDef] = &[EncounterDef { id: EncounterId::NibbitsNormal, tags: 1 }];
        let mut rng = Rng::new(17);
        let mut out = vec![EncounterId::FuzzyWurmCrawlerWeak];
        encounter_sequence(&mut rng, NORMAL, 1, &mut out, WEAK);
        assert_eq!(out, [EncounterId::FuzzyWurmCrawlerWeak, EncounterId::NibbitsNormal]);
    }
}
