use sts2_native_obs::*;
use sts2_sim::run::{RunManager, RunOptions, mp::{Character, MpPhase, MpRun, PlayerSpec}};

fn decision() -> DecisionId { DecisionId { episode: 42, generation: 0, actor: 0 } }
fn candidate(obs: &NativeObsV1) -> usize {
    let view = obs.view().unwrap();
    (0..view.candidate_len).find(|&i| view.candidates.sampleable(i, view.candidate_len)).unwrap()
}
fn multiplayer() -> MpRun {
    MpRun::new("DISPATCH-VALIDATION", &[
        PlayerSpec { net_id: 10, character: Character::Ironclad },
        PlayerSpec { net_id: 11, character: Character::Silent },
    ], 0).unwrap()
}

#[test]
fn direct_dispatch_rejects_epoch_generation_seat_and_overflow_without_mutation() {
    for multiplayer in [false, true] {
        for failure in 0..5 {
            let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new();
            let mut current = decision();
            if failure == 4 { current.generation = u64::MAX; }
            let mut solo = RunManager::new(7, RunOptions::default()); let mut mp = self::multiplayer();
            if multiplayer { encoder.encode_multiplayer(&mp, current, 10, &mut obs).unwrap(); }
            else { encoder.encode_solo(&solo, current, &mut obs).unwrap(); }
            let index = candidate(&obs);
            match failure {
                0 => current.episode += 1,
                1 => current.generation += 1,
                2 => current.actor = 1,
                _ => {}
            }
            let net = if failure == 3 { 11 } else { 10 };
            let before_solo = format!("{solo:?}"); let before_mp = format!("{mp:?}");
            let before_decision = current;
            let result = if multiplayer {
                encoder.dispatch_multiplayer(&mut mp, &mut current, net, &mut obs, index)
            } else if failure == 3 {
                // A malformed (but structurally valid) payload must be rejected
                // against the native command stream, not passed to the engine.
                obs.clear(current); obs.push_row(Row { kind: RowKind::Global, ..Row::default() }).unwrap();
                obs.push_candidate(Candidate { action: ActionKind::PlayCard, present: true, legal: true, ..Candidate::default() },
                    Binding { decision: current, net_id: 1,
                        command: Command::Solo(SoloCommand::PlayCard { hand: u32::MAX, target: None }),
                        source_instance: Some(u32::MAX), ..Binding::default() }).unwrap();
                obs.finish(DecisionStatus::Actionable).unwrap();
                encoder.dispatch_solo(&mut solo, &mut current, &mut obs, 0)
            } else { encoder.dispatch_solo(&mut solo, &mut current, &mut obs, index) };
            let expected = if failure == 4 { DispatchError::GenerationOverflow }
                else if failure == 2 || failure == 3 && multiplayer { DispatchError::Contract(ContractError::WrongOwner) }
                else { DispatchError::Contract(ContractError::StaleBinding) };
            assert_eq!(result, Err(expected), "multiplayer={multiplayer} failure={failure}");
            assert_eq!(current, before_decision);
            assert_eq!(format!("{solo:?}"), before_solo); assert_eq!(format!("{mp:?}"), before_mp);
        }
    }
}

#[test]
fn direct_dispatch_rejects_changed_native_content_and_phase_without_a_new_generation() {
    for change_phase in [false, true] {
        let mut run = multiplayer(); let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new();
        let mut current = decision();
        encoder.encode_multiplayer(&run, current, 10, &mut obs).unwrap();
        let index = candidate(&obs);
        if change_phase { run.phase = MpPhase::RunOver; } else { run.neow[0].rotate_left(1); }
        let before = format!("{run:?}");
        assert_eq!(encoder.dispatch_multiplayer(&mut run, &mut current, 10, &mut obs, index),
            Err(DispatchError::Contract(ContractError::StaleBinding)));
        assert_eq!(current, decision()); assert_eq!(format!("{run:?}"), before);
        // A failed validation never replaces the published rows or sidecars.
        assert!(obs.view().is_ok());
    }
}

#[test]
fn dispatch_capacity_rejections_match_full_encode_and_never_discard_or_advance() {
    use sts2_sim::{relic::Relic, tables::{PotionId, RelicId}};
    // Reviewer's exact probe, plus the skipped potion rows, graph edges and
    // candidate overflow. All changes happen AFTER a valid publication.
    for growth in 0..4 {
        let mut run = multiplayer(); let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new();
        run.roster.player_mut(0).unwrap().potions[0] = Some(PotionId::Firepotion);
        let mut current = decision();
        encoder.encode_multiplayer(&run, current, 10, &mut obs).unwrap();
        let view = obs.view().unwrap();
        let index = (0..view.candidate_len).find(|&i| view.candidates.sampleable(i, view.candidate_len)
            && obs.binding(i, current, 10).unwrap().command == Command::MpDiscardPotion { slot: 0 }).unwrap();
        let buffer = match growth {
            0 => {
                let mut relic = Relic::new(RelicId::Circlet); relic.owner = 1;
                run.roster.player_mut(1).unwrap().relics = vec![relic; STATE_CAPACITY];
                None
            }
            1 => {
                run.roster.player_mut(1).unwrap().max_potion_slots = STATE_CAPACITY;
                None
            }
            2 => {
                let (x, y) = run.world.run.coord.unwrap();
                let coord = sts2_sim::run::Coord::new(x, y);
                let prototype = run.world.map.point(coord).unwrap().clone();
                let target = run.world.map.points.iter().position(|point| point.coord == coord).unwrap() as u8;
                run.world.map.points.resize(256, prototype);
                for (index, point) in run.world.map.points.iter_mut().enumerate() {
                    point.coord = sts2_sim::run::Coord::new(index as i32, index as i32);
                    point.children = vec![target; 256];
                }
                Some(BufferKind::Edges)
            }
            3 => {
                run.roster.player_mut(0).unwrap().potions = vec![Some(PotionId::Firepotion); CANDIDATE_CAPACITY + 1];
                Some(BufferKind::Candidates)
            }
            _ => unreachable!(),
        };
        let mut full_encoder = NativeEncoder::new(); let mut full_obs = NativeObsV1::new();
        let expected = full_encoder.encode_multiplayer(&run, current, 10, &mut full_obs).unwrap_err();
        let EncodeError::Contract(ContractError::Capacity { buffer: actual, needed, capacity }) = expected
            else { panic!("growth must fail at an observation capacity: {expected:?}"); };
        if let Some(buffer) = buffer { assert_eq!(actual, buffer); }
        else { assert!(matches!(actual, BufferKind::Rows | BufferKind::Edges)); }
        let cap = match actual { BufferKind::Rows => STATE_CAPACITY, BufferKind::Edges => EDGE_CAPACITY,
            BufferKind::Candidates => CANDIDATE_CAPACITY, BufferKind::Selection => SELECTION_CAPACITY };
        assert_eq!((needed, capacity), (cap + 1, cap));
        let before = format!("{run:?}");
        let binding = obs.binding(index, current, 10).unwrap(); let lengths = obs.lengths();
        assert_eq!(encoder.dispatch_multiplayer(&mut run, &mut current, 10, &mut obs, index),
            Err(DispatchError::Encode(expected)));
        assert_eq!(current, decision());
        assert_eq!(run.roster.player(0).unwrap().potions[0], Some(PotionId::Firepotion));
        assert_eq!(format!("{run:?}"), before);
        assert_eq!(obs.binding(index, current, 10).unwrap(), binding);
        assert_eq!(obs.lengths(), lengths);
    }
}
