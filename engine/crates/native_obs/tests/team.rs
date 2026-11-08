use sts2_native_obs::*;
use sts2_sim::run::{Coord, mp::{Character, MpPhase, MpRun, PlayerSpec}};
use sts2_sim::tables::MapPointType;

fn combat(seats: usize) -> MpRun {
    let characters = [Character::Ironclad, Character::Necrobinder, Character::Defect, Character::Regent];
    let specs: Vec<_> = (0..seats).map(|seat| PlayerSpec {
        net_id: 700 + 19 * seat as u64, character: characters[seat],
    }).collect();
    let mut run = MpRun::new("TEAM-OBS-V2", &specs, 0).unwrap();
    run.phase = MpPhase::Map;
    let (x, y) = run.world.run.coord.unwrap();
    let child = run.world.map.point(Coord::new(x,y)).unwrap().children[0] as usize;
    run.world.map.points[child].point_type = MapPointType::Monster;
    let coord = run.world.map.points[child].coord;
    for seat in 0..seats { run.vote(specs[seat].net_id, seat, coord).unwrap(); }
    run
}

#[test]
fn joint_support_authenticates_each_owners_cards_and_shares_state_only_once() {
    for seats in 2..=4 {
        let run = combat(seats);
        let before = format!("{:?}", run.combat());
        let decision = DecisionId { episode: 31, generation: 7, actor: 0 };
        let (mut encoder, mut scratch, mut team) = (NativeEncoder::new(), NativeObsV1::new(), TeamObsV2::new());
        encoder.encode_multiplayer(&run, decision, 700, &mut scratch).unwrap();
        let solo_contract = scratch.view().unwrap().identity;
        encoder.encode_team(&run, decision, &mut scratch, &mut team).unwrap();
        let view = team.view().unwrap();
        assert_eq!(view.scope, DecisionScope::TeamCombat);
        assert_eq!(view.identity, TEAM_MANIFEST_IDENTITY);
        assert_ne!(view.identity, solo_contract);
        assert_eq!(view.rows.kind.iter().filter(|&&kind| kind == RowKind::Seat).count(), seats);
        let mut ends = vec![0; seats];
        for i in 0..view.candidate_len {
            if !view.candidates.sampleable(i,view.candidate_len) { continue; }
            let binding = team.binding(i,decision).unwrap();
            assert_eq!(binding.net_id, run.roster.players()[binding.actor as usize].net_id);
            assert_eq!(view.candidates.actor[i], binding.actor);
            if view.candidates.action[i] == ActionKind::EndTurn { ends[binding.actor as usize] += 1; }
            if view.candidates.action[i] == ActionKind::PlayCard {
                let row = view.candidates.source[i].index().unwrap();
                let owner = view.rows.owner[row].index().unwrap();
                assert_eq!(view.rows.kind[owner], RowKind::Seat);
                assert_eq!(view.rows.integers[owner][0], binding.actor as i32);
            }
        }
        assert_eq!(ends, vec![1;seats]);
        for owner in 0..seats {
            let hand = view.rows.kind.iter().enumerate().filter(|(i,kind)| {
                if **kind != RowKind::Card { return false; }
                let row = view.rows.owner[*i].index().unwrap();
                view.rows.integers[row][0] == owner as i32
                    && view.rows.detail[*i].iter().any(|id| *id == ContentId::from_index(
                        Namespace::PileKind, sts2_sim::combat::PileKind::Hand as usize).unwrap())
            }).count();
            assert_eq!(hand, run.combat().unwrap().players[owner as u8].hand.len());
        }
        assert_eq!(format!("{:?}",run.combat()),before);
        encoder.encode_multiplayer(&run,decision,700,&mut scratch).unwrap();
        assert_eq!(scratch.view().unwrap().identity,solo_contract);
        assert_eq!(scratch.view().unwrap().scope,DecisionScope::Seat(0));
    }
}

#[test]
fn chosen_seat_ends_and_unends_without_changing_other_seats_or_reusing_a_stale_joint_menu() {
    let mut run = combat(3);
    let mut decision = DecisionId { episode: 31, generation: 0, actor: 0 };
    let (mut encoder, mut scratch, mut team) = (NativeEncoder::new(), NativeObsV1::new(), TeamObsV2::new());
    encoder.encode_team(&run,decision,&mut scratch,&mut team).unwrap();
    let old = decision;
    let slot = {
        let v = team.view().unwrap();
        (0..v.candidate_len).find(|&i|v.candidates.actor[i]==1 && v.candidates.action[i]==ActionKind::EndTurn).unwrap()
    };
    let hand = run.combat().unwrap().players[1].hand.clone();
    encoder.dispatch_team(&mut run,&mut decision,&mut scratch,&mut team,slot).unwrap();
    assert_eq!(decision.actor,0);
    assert!(team.binding(slot,old).is_err());
    let c = run.combat().unwrap();
    assert!(c.is_player_ready_to_end_turn(1));
    assert!(!c.is_player_ready_to_end_turn(0));
    assert_eq!(c.players[1].hand,hand);
    encoder.encode_team(&run,decision,&mut scratch,&mut team).unwrap();
    let slot = {
        let v = team.view().unwrap();
        assert!(!(0..v.candidate_len).any(|i|v.candidates.actor[i]==1 && v.candidates.action[i]==ActionKind::PlayCard));
        (0..v.candidate_len).find(|&i|v.candidates.actor[i]==1 && v.candidates.action[i]==ActionKind::UnendTurn).unwrap()
    };
    encoder.dispatch_team(&mut run,&mut decision,&mut scratch,&mut team,slot).unwrap();
    assert!(!run.combat().unwrap().is_player_ready_to_end_turn(1));
}

#[test]
fn versioned_action_identity_preserves_full_width_ids_through_browser_json() {
    let decision=DecisionId {episode:u64::MAX,generation:u64::MAX-1,actor:3};
    let identity=ActionIdentity::new(decision,3,17,"native binding");
    let value=serde_json::to_value(&identity).unwrap();
    assert_eq!(value["episode"],"18446744073709551615");
    assert_eq!(value["generation"],"18446744073709551614");
    assert_eq!(serde_json::from_value::<ActionIdentity>(value).unwrap(),identity);
}
