use sts2_vecenv::{StepResults, VecEnv};
use sts2_wire::WireBatch;
use rayon::prelude::*;
use sts2_sim::obs::CombatObservationContext;
use sts2_sim::run::{RunActionKind, RunPhase};
use sts2_vecenv::sim_lane::SimLane;

fn equal_wire(a: &WireBatch, b: &WireBatch) {
    macro_rules! exact {
        ($($field:ident),* $(,)?) => { $(
            assert_eq!(a.$field, b.$field, stringify!($field));
        )* };
    }
    exact!(n, state_kind, state_tokens, state_values, state_mask, cand_kind,
        cand_tokens, cand_values, choice, model_mask, source_applicable,
        target_applicable, source_known, target_known, source_bits, target_bits, legal);
    macro_rules! floats {
        ($($field:ident),* $(,)?) => { $(
            assert!(a.$field.iter().zip(&b.$field)
                .all(|(x, y)| x.to_bits() == y.to_bits()), stringify!($field));
        )* };
    }
    floats!(state_scales, state_tail, boss, cand_scales, cand_tail);
}

fn recorded_combat_lane() -> SimLane {
    let mut lane = SimLane::reset(3000040, "IRONCLAD");
    let neow = lane.actions().iter().position(|a| a.label == Some("Fishing Rod")).unwrap();
    assert_eq!(lane.step(neow).fault, None);
    let movement = lane.actions().iter().position(|a| a.kind == RunActionKind::Move).unwrap();
    assert_eq!(lane.step(movement).fault, None);
    assert_eq!(lane.run().phase(), RunPhase::Combat);
    lane
}

#[test]
fn native_combat_wires_match_at_1_8_16_threads() {
    const M: usize = 4;
    const K: usize = 64;
    let mut runs: Vec<_> = [1, 8, 16].into_iter().map(|threads| {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
        let lanes = (0..M).map(|_| recorded_combat_lane()).collect::<Vec<_>>();
        (pool, lanes)
    }).collect();
    let mut resets = 0;
    for _ in 0..K {
        let batches: Vec<_> = runs.iter_mut().map(|(pool, lanes)| pool.install(|| {
            lanes.par_iter_mut().map(|lane| {
                let reset = lane.fault().is_some() || lane.run().phase() != RunPhase::Combat;
                if reset { *lane = recorded_combat_lane(); }
                let context = CombatObservationContext {
                    public_act1_boss: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0], choice_prompt: "",
                };
                let (wire, bindings) = lane.encode(&context).unwrap();
                assert!(!bindings.is_empty(), "must compare actual combat decisions");
                let action = lane.actions().iter().position(|a|
                    a.kind == RunActionKind::EndTurn && a.enabled)
                    .or_else(|| lane.legal_mask().iter().position(|&legal| legal)).unwrap();
                (wire, lane.step(action), reset)
            }).collect::<Vec<_>>()
        })).collect();
        resets += batches[0].iter().filter(|(_, _, reset)| *reset).count();
        for other in 1..batches.len() {
            for lane in 0..M {
                equal_wire(&batches[0][lane].0, &batches[other][lane].0);
                assert_eq!(batches[0][lane].1, batches[other][lane].1);
                assert_eq!(batches[0][lane].2, batches[other][lane].2);
            }
        }
    }
    assert!(resets > 0, "exercise repeated native resets");
}


#[test]
#[cfg(feature = "native")]
fn native_distinct_seed_vecenv_wires_match_with_auto_resets() {
    seeded_vecenv::<sts2_vecenv::DefaultSim>();
}

#[test]
#[cfg(feature = "native")]
fn native_reason_survives_reset_into_fixture_and_counts() {
    use sts2_vecenv::LaneSim;
    let mut sim=sts2_vecenv::DefaultSim::new();
    sim.reset_device(100,9);
    let code=sim.first_reason();
    assert!(code > 1);
    let text=sim.reason_text().unwrap();
    let fixture=sts2_vecenv::Refusal {seed:100,reason:code,reason_text:Some(text),actions:vec![0]};
    assert!(fixture.fixture_json().contains(text));
    assert!(fixture.fixture_json().contains(&format!("\"reason\":{code}")));
    let mut env: VecEnv=VecEnv::new(1,1,100,false).unwrap();
    let mut wire=WireBatch::zeroed(1);
    env.reset(&mut wire).unwrap();
    env.step(&[-1],&mut wire,&mut StepResults::new(1)).unwrap();
    assert_eq!(env.refusal_counts.get(&(-1,Some("illegal action"))),Some(&1));
    assert_eq!(env.refusals[0].reason_text,Some("illegal action"));
}

fn seeded_vecenv<S: sts2_vecenv::LaneSim>() {
    const M: usize = 4;
    const K: usize = 64;
    let mut runs: Vec<(VecEnv<S>, WireBatch, StepResults)> = [1, 8, 16].into_iter()
        .map(|threads| {
            let mut env = VecEnv::new(M, threads, 100, false).unwrap();
            let mut wire = WireBatch::zeroed(M);
            env.reset(&mut wire).unwrap();
            (env, wire, StepResults::new(M))
        }).collect();
    let mut ended = 0;
    for decision in 0..K {
        for other in 1..runs.len() {
            equal_wire(&runs[0].1, &runs[other].1);
        }
        let actions: Vec<i32> = (0..M).map(|lane| {
            // Force repeated resets, both staggered and simultaneous.
            if decision % 11 == lane || decision % 17 == 0 {
                -1
            } else {
                runs[0].1.model_mask[lane * 256..(lane + 1) * 256]
                    .iter().position(|&legal| legal == 1).unwrap() as i32
            }
        }).collect();
        for (env, wire, results) in &mut runs {
            env.step(&actions, wire, results).unwrap();
        }
        ended += runs[0].2.done.iter().filter(|&&x| x).count();
        for other in 1..runs.len() {
            let a = &runs[0].2;
            let b = &runs[other].2;
            assert_eq!(a.episode_seed, b.episode_seed);
            assert_eq!(a.episode_len, b.episode_len);
            assert_eq!(a.refusal, b.refusal);
            assert_eq!(a.done, b.done);
            assert_eq!(a.terminated, b.terminated);
            assert_eq!(a.truncated, b.truncated);
            assert_eq!(a.reward, b.reward);
            assert_eq!(a.episode_return, b.episode_return);
        }
    }
    assert!(ended > M, "test must exercise auto-reset");
    for other in 1..runs.len() {
        equal_wire(&runs[0].1, &runs[other].1);
    }
}

#[test]
#[cfg(feature = "native")]
fn native_vecenv_encodes_neow_and_map_at_all_thread_counts() {
    for threads in [1, 8, 16] {
        let mut env: VecEnv = VecEnv::new(1, threads, 3000040, false).unwrap();
        let mut wire = WireBatch::zeroed(1);
        env.reset(&mut wire).unwrap();
        assert!(wire.model_mask.chunks(256).all(|mask| mask.iter().any(|&x| x == 1)));
        let mut results = StepResults::new(1);
        assert_eq!(env.step(&[0], &mut wire, &mut results).unwrap(), 0);
        assert!(!results.done[0]);
        assert!(wire.model_mask.iter().any(|&x| x == 1));
    }
}
