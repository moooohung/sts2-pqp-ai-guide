use sts2_native_obs::*;
use sts2_sim::run::{RunManager, RunOptions, mp::{Character, MpRun, PlayerSpec}};

fn decision() -> DecisionId { DecisionId { episode: 42, generation: 0, actor: 0 } }
fn snapshot(obs: &NativeObsV1) -> String {
    let view = obs.view().unwrap(); format!("{:?}/{:?}/{:?}/{:?}",view.status,view.rows,view.candidates,view.edges)
}
fn first_legal(obs: &NativeObsV1) -> usize {
    let view = obs.view().unwrap(); (0..view.candidate_len).find(|&i|view.candidates.sampleable(i,view.candidate_len)).unwrap()
}

#[test]
fn solo_neow_is_readonly_deterministic_and_dispatch_expires_bindings() {
    for character in [Character::Ironclad, Character::Silent, Character::Defect, Character::Necrobinder, Character::Regent] {
        let mut run = RunManager::new_for_character(20261029, RunOptions::default(), character);
        let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new(); let mut current = decision();
        let before = format!("{run:?}"); encoder.encode_solo(&run,current,&mut obs).unwrap();
        let expected = snapshot(&obs); encoder.encode_solo(&run,current,&mut obs).unwrap();
        assert_eq!(snapshot(&obs),expected); assert_eq!(format!("{run:?}"),before);
        let view = obs.view().unwrap();
        let seat = view.rows.kind.iter().position(|&kind| kind == RowKind::Seat).unwrap();
        assert_eq!(view.rows.detail[seat][0], ContentId::from_index(Namespace::Character, character as usize).unwrap());
        assert!(view.rows.kind.contains(&RowKind::EventOption));
        let candidate = first_legal(&obs); let old = obs.binding(candidate,current,1).unwrap();
        encoder.dispatch_solo(&mut run,&mut current,&mut obs,candidate).unwrap();
        assert_eq!(current.generation,1); assert!(obs.view().is_err());
        encoder.encode_solo(&run,current,&mut obs).unwrap();
        assert!(obs.binding(candidate,old.decision,1).is_err());
    }
}

#[test]
fn solo_first_combat_is_actionable_outside_card_execution() {
    for character in [Character::Ironclad, Character::Silent, Character::Defect, Character::Necrobinder, Character::Regent] {
        let mut run = RunManager::new_for_character(202610290000, RunOptions::default(), character);
        let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new();
        let mut current = decision(); let mut reached = false;
        for _ in 0..32 {
            encoder.encode_solo(&run, current, &mut obs).unwrap();
            let view = obs.view().unwrap();
            assert_eq!(view.status, DecisionStatus::Actionable);
            let seat = view.rows.kind.iter().position(|&kind| kind == RowKind::Seat).unwrap();
            assert_eq!(view.rows.detail[seat][0], ContentId::from_index(Namespace::Character, character as usize).unwrap());
            if let Some(combat) = run.combat() {
                assert!(!combat.in_play_phase, "not executing a card at a decision boundary");
                assert!(view.candidates.action.contains(&ActionKind::EndTurn));
                reached = true; break;
            }
            let candidate = first_legal(&obs);
            encoder.dispatch_solo(&mut run, &mut current, &mut obs, candidate).unwrap();
        }
        assert!(reached, "{character:?} fixture did not reach its first combat");
    }
}

#[test]
fn multiplayer_two_to_four_seats_all_characters_have_typed_private_offers() {
    let characters = [Character::Ironclad,Character::Silent,Character::Defect,Character::Necrobinder,Character::Regent];
    for seats in 2..=4 { for offset in 0..5 {
        let specs: Vec<_> = (0..seats).map(|seat|PlayerSpec {net_id:100+seat as u64*19,character:characters[(seat+offset)%5]}).collect();
        let mut run = MpRun::new("NATIVE-OBS-PHASES",&specs,10).unwrap();
        let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new();
        for seat in 0..seats {
            let current = DecisionId { actor:seat as u8,..decision() };
            encoder.encode_multiplayer(&run,current,specs[seat].net_id,&mut obs).unwrap();
            let expected = snapshot(&obs);
            let view = obs.view().unwrap();
            let rows: Vec<_> = view.rows.kind.iter().enumerate().filter_map(|(i,&kind)|(kind==RowKind::Seat).then_some(i)).collect();
            assert_eq!(rows.len(),seats);
            for (owner,&row) in rows.iter().enumerate() {
                assert_eq!(view.rows.detail[row][0],ContentId::from_index(Namespace::Character,specs[owner].character as usize).unwrap());
            }
            let candidate=first_legal(&obs);assert_eq!(obs.binding(candidate,current,specs[(seat+1)%seats].net_id),Err(ContractError::WrongOwner));
            let other=(seat+1)%seats;let original=run.neow[other];run.neow[other].rotate_left(1);
            encoder.encode_multiplayer(&run,current,specs[seat].net_id,&mut obs).unwrap();
            assert_eq!(snapshot(&obs),expected);run.neow[other]=original;
        }
    } }
}
