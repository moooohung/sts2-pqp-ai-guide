use sts2_native_obs::*;
use sts2_sim::tables;

fn decision() -> DecisionId { DecisionId { episode: 81, generation: 7, actor: 2 } }
fn global() -> Row { Row { kind: RowKind::Global, ..Row::default() } }
fn binding(command: Command) -> Binding {
    Binding { decision: decision(), net_id: 9002, command, ..Binding::default() }
}
fn candidate(command: Command) -> Candidate {
    Candidate { action: command.action(), actor: 2, present: true, legal: true, ..Candidate::default() }
}
fn lane() -> Box<NativeObsV1> {
    let mut obs = Box::new(NativeObsV1::new());
    obs.clear(decision());
    obs
}

#[test]
fn native_discriminants_are_the_only_numbering_source() {
    macro_rules! check {
        ($($name:ident),+ $(,)?) => {$({
            let registry = Namespace::$name.registry().unwrap();
            assert_eq!(registry.count, tables::$name::COUNT);
            for id in tables::$name::ALL {
                assert_eq!(registry.names[id.index()], format!("{id:?}"));
                let wire = ContentId::from_index(Namespace::$name, id.index()).unwrap();
                assert_eq!(wire.value as usize, id.index() + 1);
                assert!(wire.is_valid());
            }
            assert!(ContentId::from_index(Namespace::$name, tables::$name::COUNT).is_none());
        })+};
    }
    check!(CardId, RelicId, PowerId, PotionId, MonsterKind, IntentType, OrbType, VarKey,
        CardType, CardRarity, TargetType, CardTag, MapPointType, RoomType,
        PotionRarity, PotionUsageType, PotionTargetType, RelicRarity, RelicPool, PowerType, PowerStackType);
    assert_eq!(MAX_SEATS, sts2_sim::player::MAX_PLAYERS);
    assert_eq!(Namespace::EnchantmentId.registry().unwrap().names, sts2_sim::card::ENCHANTMENT_NAMES);
    for id in EnchantmentId::ALL {
        assert_eq!(ContentId::from(id).value as usize, id.index() + 1);
    }
    assert!(!ContentId { namespace: Namespace::CardId, value: 0 }.is_valid());
    assert!(ContentId::ABSENT.is_valid());
    assert!(ContentId::from_index(Namespace::Absent, 0).is_none());
}

#[test]
fn manifest_records_original_capture_and_every_namespace() {
    let manifest: serde_json::Value = serde_json::from_str(CONTENT_MANIFEST_JSON).unwrap();
    let original: serde_json::Value = serde_json::from_str(include_str!("../data/original_ids.json")).unwrap();
    assert_eq!(ORIGINAL_DLL_CAPTURE, "0861BFA1DF347538D932F22D580E75420F08082792EB914E53B4882764ACDBE9");
    assert_eq!(manifest["original_dll_capture"], original["capture"]);
    assert_eq!(EventId::COUNT, original["events"].as_array().unwrap().len());
    assert_eq!(EventPageId::COUNT, original["pages"].as_array().unwrap().len());
    assert_eq!(EventOptionId::COUNT, original["options"].as_array().unwrap().len());
    assert_eq!(original["events"][EventId::AbyssalBaths.index()]["key"], "ABYSSAL_BATHS");
    assert!(original["pages"].as_array().unwrap().iter().any(|v| v["key"] == "ABYSSAL_BATHS.INITIAL"));
    assert!(original["options"].as_array().unwrap().iter().any(|v| v["key"] == "ABYSSAL_BATHS.INITIAL.IMMERSE"));
    assert_eq!(REGISTRIES.len(), Namespace::COUNT);
    for registry in REGISTRIES {
        assert!(registry.count > 0, "{}", registry.name);
        assert_eq!(registry.count, registry.names.len());
        let row = manifest["registries"].as_array().unwrap().iter()
            .find(|row| row["name"] == registry.name).unwrap();
        assert_eq!(row["embedding_rows"].as_u64().unwrap() as usize, registry.count + 1);
        for (index, member) in row["ids"].as_array().unwrap().iter().enumerate() {
            assert_eq!(member["index"], index);
            assert_eq!(member["wire"], index + 1);
        }
    }
    let schema: serde_json::Value = serde_json::from_str(include_str!("../schema.json")).unwrap();
    assert_eq!(manifest["capacity_status"], schema["capacity_status"]);
    assert!(CAPACITIES_VALIDATED);
    println!("NativeObsV1 schema={} content={} namespaces={} events={} pages={} options={}",
        manifest["schema_hash"], manifest["content_hash"], REGISTRIES.len(),
        EventId::COUNT, EventPageId::COUNT, EventOptionId::COUNT);
}

#[test]
fn checkpoints_reject_independent_schema_and_content_drift() {
    assert_eq!(MANIFEST_IDENTITY.validate(), Ok(()));
    let mut altered = MANIFEST_IDENTITY;
    altered.version += 1;
    assert_eq!(altered.validate(), Err(ContractError::SchemaMismatch));
    altered = MANIFEST_IDENTITY;
    altered.schema[0] ^= 1;
    assert_eq!(altered.validate(), Err(ContractError::SchemaMismatch));
    altered = MANIFEST_IDENTITY;
    altered.content[31] ^= 1;
    assert_eq!(altered.validate(), Err(ContractError::ContentMismatch));
}

#[test]
fn negative_fractional_values_and_known_zero_survive_exactly() {
    let mut obs = lane();
    obs.push_row(global()).unwrap();
    let mut row = Row { kind: RowKind::Modifier, parent: RowHandle(1), ..Row::default() };
    row.numbers.set_float(row_fields::modifier::AMOUNT, -0.375).unwrap();
    row.numbers.set_integer(row_fields::modifier::DURATION, 0).unwrap();
    row.numbers.set_integer(row_fields::modifier::SIGN, -1).unwrap();
    row.flags.set(Flag::Revealed, false);
    obs.push_row(row).unwrap();
    let command = Command::MpEndTurn;
    obs.push_candidate(candidate(command), binding(command)).unwrap();
    obs.finish(DecisionStatus::Actionable).unwrap();
    let view = obs.view().unwrap();
    assert_eq!(view.rows.floats[1][row_fields::modifier::AMOUNT].to_bits(), (-0.375f32).to_bits());
    assert_eq!(view.rows.integer_present[1] & (1 << row_fields::modifier::DURATION), 1 << row_fields::modifier::DURATION);
    assert_eq!(view.rows.integers[1][row_fields::modifier::SIGN], -1);
    assert_eq!(view.rows.flags[1] & Flag::Revealed.bit(), 0);
    assert_ne!(view.rows.flags_present[1] & Flag::Revealed.bit(), 0);
    assert_eq!(view.rows.flags_present[1] & Flag::Alive.bit(), 0);
}

#[test]
fn nonfinite_and_untyped_payloads_never_publish_partial_observations() {
    let mut obs = lane();
    obs.push_row(global()).unwrap();
    let mut row = Row { kind: RowKind::Card, ..Row::default() };
    row.numbers.floats[0] = f32::NAN;
    row.numbers.float_present = 1;
    assert_eq!(obs.push_row(row), Err(ContractError::NonFinite));
    assert!(matches!(obs.view(), Err(ContractError::NonFinite)));
    assert_eq!(obs.finish(DecisionStatus::Waiting), Err(ContractError::NonFinite));
    obs.clear(decision());
    let mut row = global();
    row.numbers.set_float(0, 0.5).unwrap();
    assert_eq!(obs.push_row(row), Err(ContractError::InvalidNumericField));
    obs.clear(decision());
    let mut row = global();
    row.numbers.integers[0] = 19; // No presence bit: could leak stale private data.
    assert_eq!(obs.push_row(row), Err(ContractError::InvalidNumericField));
}

#[test]
fn candidate_presence_and_legality_intersect_at_word_boundaries() {
    let mut obs = lane();
    obs.push_row(global()).unwrap();
    let command = Command::MpEndTurn;
    for index in 0..CANDIDATE_CAPACITY {
        let mut candidate = candidate(command);
        candidate.present = index != 63;
        candidate.legal = index != 64;
        obs.push_candidate(candidate, binding(command)).unwrap();
    }
    obs.finish(DecisionStatus::Actionable).unwrap();
    let view = obs.view().unwrap();
    assert!(!view.candidates.sampleable(63, view.candidate_len));
    assert!(!view.candidates.sampleable(64, view.candidate_len));
    assert!(view.candidates.sampleable(65, view.candidate_len));
    assert!(view.candidates.sampleable(CANDIDATE_CAPACITY - 1, view.candidate_len));
    assert!(!view.candidates.sampleable(CANDIDATE_CAPACITY, view.candidate_len));
    assert_eq!(obs.binding(63, decision(), 9002), Err(ContractError::InvalidBinding));
    assert_eq!(obs.binding(64, decision(), 9002), Err(ContractError::InvalidBinding));
}

#[test]
fn stale_generation_episode_and_wrong_seat_are_distinct_errors() {
    let command = Command::MpTakeSpecialCard { reward_index: 3 };
    let mut obs = lane();
    obs.push_row(global()).unwrap();
    let mut exact = binding(command);
    exact.source_instance = Some(0);
    exact.offer_instance = Some(0);
    obs.push_candidate(candidate(command), exact).unwrap();
    obs.finish(DecisionStatus::Actionable).unwrap();
    assert_eq!(obs.binding(0, decision(), 9002).unwrap(), exact);
    let mut changed = decision();
    changed.generation += 1;
    assert_eq!(obs.binding(0, changed, 9002), Err(ContractError::StaleBinding));
    changed = decision(); changed.episode += 1;
    assert_eq!(obs.binding(0, changed, 9002), Err(ContractError::StaleBinding));
    changed = decision(); changed.actor = 1;
    assert_eq!(obs.binding(0, changed, 9002), Err(ContractError::WrongOwner));
    assert_eq!(obs.binding(0, decision(), 17), Err(ContractError::WrongOwner));
}

#[test]
fn ordered_multi_selection_retains_option_and_instance_identity() {
    let mut obs = lane();
    obs.push_row(global()).unwrap();
    let selected = [
        SelectionItem { option_index: 7, instance_id: Some(0) },
        SelectionItem { option_index: 3, instance_id: Some(29) },
    ];
    let span = obs.push_selection(&selected).unwrap();
    let event = Command::MpEventCards { selection: span };
    let shears = Command::MpPrecariousShearsCards { selection: span };
    obs.push_candidate(candidate(event), binding(event)).unwrap();
    obs.push_candidate(candidate(shears), binding(shears)).unwrap();
    obs.finish(DecisionStatus::Actionable).unwrap();
    assert_eq!(obs.selection(span).unwrap(), selected);
    assert_ne!(obs.binding(0, decision(), 9002).unwrap().command,
        obs.binding(1, decision(), 9002).unwrap().command);
    assert_eq!(event.action(), shears.action());
}

#[test]
fn exact_capacity_and_capacity_plus_one_never_truncate() {
    let mut obs = lane();
    for _ in 0..STATE_CAPACITY { obs.push_row(global()).unwrap(); }
    assert_eq!(obs.lengths().0, STATE_CAPACITY);
    assert_eq!(obs.push_row(global()), Err(ContractError::Capacity {
        buffer: BufferKind::Rows, needed: STATE_CAPACITY + 1, capacity: STATE_CAPACITY }));
    assert!(matches!(obs.view(), Err(ContractError::Capacity { .. })));
    obs.clear(decision());
    obs.push_row(global()).unwrap();
    for _ in 0..CANDIDATE_CAPACITY {
        obs.push_candidate(candidate(Command::MpEndTurn), binding(Command::MpEndTurn)).unwrap();
    }
    assert_eq!(obs.push_candidate(candidate(Command::MpEndTurn), binding(Command::MpEndTurn)),
        Err(ContractError::Capacity { buffer: BufferKind::Candidates, needed: CANDIDATE_CAPACITY + 1, capacity: CANDIDATE_CAPACITY }));
    obs.clear(decision());
    obs.push_row(global()).unwrap();
    let edge = Edge { from: RowHandle(1), to: RowHandle(1), relation: Relation::Member };
    for _ in 0..EDGE_CAPACITY { obs.push_edge(edge).unwrap(); }
    assert_eq!(obs.push_edge(edge), Err(ContractError::Capacity {
        buffer: BufferKind::Edges, needed: EDGE_CAPACITY + 1, capacity: EDGE_CAPACITY }));
    obs.clear(decision());
    let item = SelectionItem { option_index: 2, instance_id: Some(0) };
    for _ in 0..SELECTION_CAPACITY { obs.push_selection(&[item]).unwrap(); }
    assert_eq!(obs.push_selection(&[item]), Err(ContractError::Capacity {
        buffer: BufferKind::Selection, needed: SELECTION_CAPACITY + 1, capacity: SELECTION_CAPACITY }));
}

#[test]
fn clear_invalidates_lengths_masks_and_old_publication() {
    let mut obs = lane();
    obs.push_row(global()).unwrap();
    obs.push_candidate(candidate(Command::MpEndTurn), binding(Command::MpEndTurn)).unwrap();
    obs.finish(DecisionStatus::Actionable).unwrap();
    let next = DecisionId { generation: 8, ..decision() };
    obs.clear(next);
    assert_eq!(obs.lengths(), (0, 0, 0, 0));
    assert!(matches!(obs.view(), Err(ContractError::Incomplete)));
    obs.finish(DecisionStatus::Waiting).unwrap();
    let view = obs.view().unwrap();
    assert_eq!(view.decision, next);
    assert!(view.candidates.candidate_present.iter().all(|word| *word == 0));
    assert!(view.candidates.legal.iter().all(|word| *word == 0));
}

#[test]
fn empty_actionable_is_an_error_and_waiting_has_no_distribution() {
    let mut obs = lane();
    obs.push_row(global()).unwrap();
    assert_eq!(obs.finish(DecisionStatus::Actionable), Err(ContractError::EmptyActionable));
    for status in [DecisionStatus::Waiting, DecisionStatus::Terminal, DecisionStatus::GameplayFault] {
        obs.clear(decision());
        obs.finish(status).unwrap();
        assert_eq!(obs.view().unwrap().candidate_len, 0);
    }
    obs.clear(decision());
    obs.push_candidate(candidate(Command::MpEndTurn), binding(Command::MpEndTurn)).unwrap();
    assert_eq!(obs.finish(DecisionStatus::Terminal), Err(ContractError::NonActionableCandidates));
}

#[test]
fn invalid_handle_namespace_and_owner_fail_at_the_write_boundary() {
    let mut obs = lane();
    assert_eq!(obs.push_row(Row { parent: RowHandle(1), ..global() }), Err(ContractError::InvalidHandle));
    obs.clear(decision());
    assert_eq!(obs.push_row(Row { model: ContentId { namespace: Namespace::CardId, value: u16::MAX }, ..global() }),
        Err(ContractError::InvalidContentId));
    obs.clear(decision());
    obs.push_row(global()).unwrap();
    let mut foreign = candidate(Command::MpEndTurn); foreign.actor = 0;
    assert_eq!(obs.push_candidate(foreign, binding(Command::MpEndTurn)), Err(ContractError::WrongOwner));
}

#[test]
fn switching_seats_exposes_only_current_rows_candidates_and_masks() {
    let mut obs = lane();
    obs.push_row(global()).unwrap();
    let secret = ContentId::from_index(Namespace::CardId, 0).unwrap();
    let mut hand = Row { kind: RowKind::Card, model: secret, ..Row::default() };
    hand.flags.set(Flag::Upgraded, true);
    for _ in 0..65 {
        let source = obs.push_row(hand).unwrap();
        let command = Command::MpPlayCard { hand: 0, target: None };
        let mut offered = candidate(command);
        offered.source = source;
        offered.model = secret;
        offered.flags = hand.flags;
        let mut exact = binding(command);
        exact.source_instance = Some(0);
        obs.push_candidate(offered, exact).unwrap();
    }
    obs.push_edge(Edge { from: RowHandle(1), to: RowHandle(2), relation: Relation::Member }).unwrap();
    let old_selection = obs.push_selection(&[SelectionItem { option_index: 0, instance_id: Some(0) }]).unwrap();
    obs.finish(DecisionStatus::Actionable).unwrap();
    assert_eq!(obs.view().unwrap().rows.model[65], secret);

    let next = DecisionId { actor: 1, generation: 8, ..decision() };
    obs.clear(next);
    obs.push_row(global()).unwrap();
    let command = Command::MpEndTurn;
    let mut offered = candidate(command);
    offered.actor = next.actor;
    let mut exact = binding(command);
    exact.decision = next;
    exact.net_id = 9001;
    obs.push_candidate(offered, exact).unwrap();
    obs.finish(DecisionStatus::Actionable).unwrap();
    let view = obs.view().unwrap();
    macro_rules! bounded {
        ($columns:expr, $len:expr; $($field:ident),+ $(,)?) => {$({
            assert_eq!($columns.$field.len(), $len, stringify!($field));
            assert!($columns.$field.get($len).is_none(), stringify!($field));
        })+};
    }
    bounded!(view.rows, 1; kind, model, detail, owner, parent, flags, flags_present,
        integers, floats, integer_present, float_present);
    bounded!(view.candidates, 1; action, model, detail, source, target, actor, flags,
        flags_present, integers, floats, integer_present, float_present);
    assert!(!view.rows.model.contains(&secret));
    assert!(!view.candidates.model.contains(&secret));
    assert_eq!(view.rows.flags_present, &[0]);
    assert_eq!(view.candidates.flags_present, &[0]);
    assert_eq!(view.candidates.candidate_present, &[1]);
    assert_eq!(view.candidates.legal, &[1]);
    assert!(!view.candidates.sampleable(1, CANDIDATE_CAPACITY));
    assert!(view.edges.is_empty());
    assert_eq!(obs.binding(1, next, 9001), Err(ContractError::InvalidBinding));
    assert_eq!(obs.selection(old_selection), Err(ContractError::InvalidBinding));

    obs.clear(DecisionId { generation: 9, ..next });
    obs.finish(DecisionStatus::Waiting).unwrap();
    let view = obs.view().unwrap();
    bounded!(view.rows, 0; kind, model, detail, owner, parent, flags, flags_present,
        integers, floats, integer_present, float_present);
    bounded!(view.candidates, 0; action, model, detail, source, target, actor, flags,
        flags_present, integers, floats, integer_present, float_present, candidate_present, legal);
}

#[test]
fn malformed_solo_bindings_cannot_publish_or_round_trip() {
    let mut obs = lane();
    let command = Command::Solo(SoloCommand::PlayCard { hand: 0, target: None });
    obs.push_row(global()).unwrap();
    assert_eq!(obs.push_candidate(candidate(command), binding(command)), Err(ContractError::InvalidBinding));
    assert!(matches!(obs.view(), Err(ContractError::InvalidBinding)));
    assert_eq!(binding(command).validate_owner(decision(), 9002), Err(ContractError::InvalidBinding));

    obs.clear(decision());
    obs.push_row(global()).unwrap();
    let mut exact = binding(command);
    exact.source_instance = Some(0);
    obs.push_candidate(candidate(command), exact).unwrap();
    obs.finish(DecisionStatus::Actionable).unwrap();
    assert_eq!(obs.binding(0, decision(), 9002), Ok(exact));

    obs.clear(decision());
    obs.push_row(global()).unwrap();
    let mut mismatched = candidate(command);
    mismatched.action = ActionKind::EndTurn;
    assert_eq!(obs.push_candidate(mismatched, exact), Err(ContractError::InvalidBinding));

    obs.clear(decision());
    obs.push_row(global()).unwrap();
    let invalid_target = Command::Solo(SoloCommand::UsePotion {
        slot: 0, target: PotionRecipient::Player(MAX_SEATS as u8),
    });
    assert_eq!(obs.push_candidate(candidate(invalid_target), binding(invalid_target)),
        Err(ContractError::InvalidBinding));
}

#[test]
fn solo_variants_cover_run_actions_with_required_payloads() {
    use ActionKind as A;
    use SoloCommand as S;
    let option = EventOptionId::ALL[0];
    let commands = [
        (S::EventChoice { option }, A::EventChoice),
        (S::EventChoiceAt { option, x: 2, y: 3 }, A::EventChoice),
        (S::Move { x: 0, y: 1 }, A::Move),
        (S::LeaveShop, A::LeaveShop),
        (S::BuyCard { index: 0 }, A::BuyCard),
        (S::BuyRelic { index: 1 }, A::BuyRelic),
        (S::BuyPotion { index: 2 }, A::BuyPotion),
        (S::RemoveCard, A::RemoveCard),
        (S::RestOption { index: 0 }, A::RestOption),
        (S::EndTurn, A::EndTurn),
        (S::PlayCard { hand: 1, target: Some(2) }, A::PlayCard),
        (S::UsePotion { slot: 0, target: PotionRecipient::None }, A::UsePotion),
        (S::UsePotion { slot: 1, target: PotionRecipient::Creature(2) }, A::UsePotion),
        (S::UsePotion { slot: 2, target: PotionRecipient::Player(0) }, A::UsePotion),
        (S::DiscardPotion { slot: 0 }, A::DiscardPotion),
        (S::Choose { index: 3 }, A::Choose),
        (S::ConfirmChoice, A::ConfirmChoice),
        (S::Skip, A::Skip),
        (S::PickCard { index: 1 }, A::PickCard),
        (S::PickSpecialCard { reward_index: 2 }, A::PickSpecialCard),
        (S::RerollCardReward, A::RerollCardReward),
        (S::SacrificeCardReward, A::SacrificeCardReward),
        (S::PickCardBundle { index: 2 }, A::PickCardBundle),
        (S::SkipRewards, A::SkipRewards),
        (S::SkipPotion, A::SkipPotion),
        (S::PickPotion { reward_index: None }, A::PickPotion),
        (S::PickPotion { reward_index: Some(0) }, A::PickPotion),
        (S::SkipRelic, A::SkipRelic),
        (S::PickRelicReward { reward_index: None }, A::PickRelicReward),
        (S::PickRelicReward { reward_index: Some(0) }, A::PickRelicReward),
        (S::OpenReward { reward_index: 0 }, A::OpenReward),
        (S::Collect, A::Collect),
    ];
    let mut obs = lane();
    obs.push_row(global()).unwrap();
    for (solo, expected) in commands {
        let command = Command::Solo(solo);
        assert_eq!(command.action(), expected);
        let mut exact = binding(command);
        if matches!(solo, S::PlayCard { .. }) { exact.source_instance = Some(0); }
        obs.push_candidate(candidate(command), exact).unwrap();
    }
    obs.finish(DecisionStatus::Actionable).unwrap();
    for (index, (solo, _)) in commands.iter().enumerate() {
        assert_eq!(obs.binding(index, decision(), 9002).unwrap().command, Command::Solo(*solo));
    }
    for name in Namespace::RunActionKind.registry().unwrap().names {
        assert!(commands.iter().any(|(_, action)| format!("{action:?}") == *name), "{name}");
    }
}
