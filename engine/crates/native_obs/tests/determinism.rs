use std::hash::{DefaultHasher, Hash, Hasher};
use sts2_native_obs::*;
use sts2_sim::run::{RunManager, RunOptions, mp::Character};

fn schedules(workers: usize) -> Vec<(u64, u64)> {
    let mut result = std::thread::scope(|scope| {
        let tasks: Vec<_> = (0..workers).map(|worker| scope.spawn(move || {
            let mut out = Vec::new();
            let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new();
            for index in (worker..12).step_by(workers) {
                let seed = 202610310000 + index as u64;
                let characters = [Character::Ironclad, Character::Silent, Character::Defect, Character::Necrobinder, Character::Regent];
                let mut run = RunManager::new_for_character(seed, RunOptions { ascension: if index % 2 == 0 { 0 } else { 10 },
                    saved_wongo_points: Some(0), ..RunOptions::default() }, characters[index % characters.len()]);
                let mut current = DecisionId { episode: seed, generation: 0, actor: 0 };
                let mut rng = seed; let mut hash = DefaultHasher::new();
                for _ in 0..128 {
                    let encoded = encoder.encode_solo(&run, current, &mut obs);
                    format!("{encoded:?}").hash(&mut hash);
                    match encoded { Ok(()) => {}, Err(EncodeError::Gameplay(_)) => break, Err(error) => panic!("{seed}: {error:?}") }
                    let view = obs.view().unwrap();
                    format!("{:?}/{:?}/{:?}/{:?}/{:?}", current, view.status, view.rows, view.candidates, view.edges).hash(&mut hash);
                    if view.status != DecisionStatus::Actionable { break; }
                    rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17;
                    let legal = (0..view.candidate_len).filter(|&i| view.candidates.sampleable(i, view.candidate_len)).count();
                    let selected = (0..view.candidate_len).filter(|&i| view.candidates.sampleable(i, view.candidate_len))
                        .nth(rng as usize % legal).unwrap();
                    format!("{:?}", obs.binding(selected, current, 1).unwrap()).hash(&mut hash);
                    let dispatched = encoder.dispatch_solo(&mut run, &mut current, &mut obs, selected);
                    format!("{dispatched:?}").hash(&mut hash);
                    match dispatched { Ok(()) => {}, Err(DispatchError::Gameplay(_)) => break, Err(error) => panic!("{seed}: {error:?}") }
                }
                format!("{run:?}").hash(&mut hash);
                out.push((seed, hash.finish()));
            }
            out
        })).collect();
        tasks.into_iter().flat_map(|task| task.join().unwrap()).collect::<Vec<_>>()
    });
    result.sort_unstable_by_key(|entry| entry.0); result
}

#[test]
fn episode_assignment_observations_bindings_and_outcomes_are_identical_with_one_two_four_workers() {
    let expected = schedules(1);
    assert_eq!(schedules(2), expected);
    assert_eq!(schedules(4), expected);
}
