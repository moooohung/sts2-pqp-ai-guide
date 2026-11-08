use super::*;
use sts2_sim::combat::Props;
use sts2_sim::{choice::{PendingChoice, Resolver}, rng::Rng, run::{EventDescription,
    EventOptionIdentity, EventParameter, RunOptions, RunPhase, mp::PlayerSpec}};

fn decision(actor: u8) -> DecisionId { DecisionId { episode: 7, generation: 0, actor } }
#[test]
fn non_event_popup_and_reward_modals_have_their_own_rows_and_typed_buttons() {
    use sts2_sim::{identity::EventPageId,run::EventPageIdentity};
    for (kind, (page,parameter)) in [
        (EventPageId::TrialReject,EventParameter::AbandonRunConfirmation(true)),
        (EventPageId::CrystalSphereCrystalSphereMinigame,EventParameter::SphereReward{offer:0,action:1}),
    ].into_iter().enumerate() {
        let mut encoder=NativeEncoder::new();
        let mut obs=NativeObsV1::new();
        encoder.begin(&mut obs,decision(0),1).unwrap();
        let global = NativeEncoder::row(&mut obs, Row { kind: RowKind::Global, ..Row::default() }).unwrap();
        encoder.seat_rows[0] = NativeEncoder::row(&mut obs, Row { kind: RowKind::Seat, parent: global, ..Row::default() }).unwrap();
        let modal = encoder.event_page(&mut obs,page.event(),EventPageIdentity{id:page,parameter}).unwrap();
        if kind == 0 {
            for (index, confirm) in [true, false].into_iter().enumerate() {
                let option = EventOptionIdentity { id: EventOptionId::TrialRejectDoubleDown,
                    parameter: EventParameter::AbandonRunConfirmation(confirm) };
                let source = encoder.event_option(&mut obs, option, option.id, None, index, modal, true, None, &[]).unwrap();
                encoder.event_action(&mut obs, option, index, source, true).unwrap();
            }
        }
        obs.finish(if kind == 0 { DecisionStatus::Actionable } else { DecisionStatus::Waiting }).unwrap();
        let view = obs.view().unwrap();
        assert_eq!(view.rows.kind[modal.index().unwrap()], RowKind::Modal);
        assert_eq!(view.rows.model[modal.index().unwrap()], id(Namespace::PublicModalKind, kind));
        assert!(!view.rows.kind.contains(&RowKind::EventPage));
        if kind == 0 {
            assert_eq!(view.candidates.action, &[ActionKind::ConfirmChoice, ActionKind::Skip]);
            for (i, confirm) in [true, false].into_iter().enumerate() {
                let source = view.candidates.source[i].index().unwrap();
                assert_eq!(view.rows.kind[source], RowKind::ModalOption);
                assert_eq!(view.rows.parent[source], modal);
                assert_eq!(view.candidates.target[i], encoder.seat_rows[0]);
                assert_eq!(obs.binding(i, decision(0), 1).unwrap().command,
                    Command::Solo(SoloCommand::AbandonConfirmation { confirm }));
            }
        }
    }
}
#[test]
fn every_catalog_page_preserves_concrete_offer_identity() {
    use sts2_sim::{identity::{EventPageId, EventOptionId}, run::NativeEventOffer as O};
    for page in EventPageId::ALL {
        let event = page.event();
        let Some(option) = EventOptionId::ALL.into_iter().find(|id| id.event() == event) else { continue };
        for (a, b) in [(O::Relic(RelicId::HappyFlower), O::Relic(RelicId::Girya)),
            (O::Potion(PotionId::Firepotion), O::Potion(PotionId::Blockpotion)),
            (O::Pool(Character::Ironclad), O::Pool(Character::Defect))] {
            let encode = |offer| {
                let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new();
                encoder.begin(&mut obs, decision(0), 1).unwrap();
                let run = RunManager::new(7, RunOptions::default());
                encoder.global(&mut obs, run.run_ctx(), id(Namespace::RunPhase, RunPhase::Event as usize),
                    1, None, None).unwrap();
                let parent = encoder.event_page(&mut obs, event,
                    sts2_sim::run::EventPageIdentity { id: page, parameter: EventParameter::None }).unwrap();
                let source = encoder.event_option(&mut obs, EventOptionIdentity { id: option,
                    parameter: EventParameter::Offer(0) }, option, None, 0, parent, true,
                    event_offer_model(Some(offer)), &[]).unwrap();
                encoder.simple(&mut obs, Command::Solo(SoloCommand::EndTurn), source, true).unwrap();
                obs.finish(DecisionStatus::Actionable).unwrap();
                state(&obs)
            };
            assert_ne!(encode(a), encode(b), "{page:?}");
        }
    }
}
fn state(obs: &NativeObsV1) -> String {
    let v = obs.view().unwrap(); format!("{:?}/{:?}/{:?}/{:?}", v.status, v.rows, v.candidates, v.edges)
}
fn fixture(seats: usize) -> Combat {
    let mut ctx = RunManager::new(7, RunOptions::default()).run_ctx().clone();
    ctx.deck.clear(); ctx.relics.clear(); ctx.potions = vec![Some(PotionId::Firepotion), None];
    ctx.max_potion_slots = 2; ctx.room_type = RoomType::Monster;
    let mut c = Combat::new(ctx, 7);
    for _ in 1..seats { c.add_player(50, 50, 3, 0, &[]); }
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    for owner in 0..seats as u8 {
        c.players[owner].energy = 3;
        let card = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        c.players[owner].hand.push(card);
    }
    c.side = Side::Player;
    c
}
fn project(c: &Combat, actor: u8) -> NativeObsV1 {
    let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new();
    encoder.begin(&mut obs, decision(actor), 1 + u64::from(actor)).unwrap();
    let global = encoder.global(&mut obs, &c.run, id(Namespace::RunPhase, RunPhase::Combat as usize),
        c.players.len(), Some(c), None).unwrap();
    let status = encoder.combat(&mut obs, c, global, c.players.len() > 1, |_| Character::Ironclad,
        |slot| c.can_discard_potion_for_player(actor, slot)).unwrap();
    encoder.finish(&mut obs, status).unwrap(); obs
}

#[test]
fn binding_only_validation_authenticates_selection_contents_and_preserves_publication() {
    let selected = [
        SelectionItem { option_index: 0, instance_id: Some(0) },
        SelectionItem { option_index: 1, instance_id: Some(7) },
    ];
    for replace in [false, true] {
        let mut obs = NativeObsV1::new(); obs.clear(decision(0));
        obs.push_row(Row { kind: RowKind::Global, ..Row::default() }).unwrap();
        let span = obs.push_selection(&selected).unwrap();
        let binding = Binding { decision: decision(0), net_id: 1,
            command: Command::MpMerchantPickupCards { selection: span }, ..Binding::default() };
        let candidate = Candidate { action: ActionKind::ConfirmChoice, present: true, legal: true, ..Candidate::default() };
        obs.push_candidate(candidate, binding).unwrap(); obs.finish(DecisionStatus::Actionable).unwrap();
        let before = state(&obs);
        let mut live = selected;
        if replace { live[0].instance_id = Some(99); }
        obs.begin_binding_check(0, binding);
        obs.push_row(Row { kind: RowKind::Global, ..Row::default() }).unwrap();
        let result = obs.push_selection(&live);
        if replace {
            assert_eq!(result, Err(ContractError::StaleBinding));
            assert_eq!(obs.end_binding_check(), Err(ContractError::StaleBinding));
        } else {
            assert_eq!(result, Ok(span));
            obs.push_candidate(candidate, binding).unwrap(); obs.finish(DecisionStatus::Actionable).unwrap();
            obs.end_binding_check().unwrap();
        }
        assert_eq!(state(&obs), before);
        assert_eq!(obs.selection(span).unwrap(), selected);
    }
}

#[test]
fn count_only_validation_has_identical_capacity_and_row_validation_boundaries() {
    let mut obs = NativeObsV1::new(); obs.clear(decision(0));
    let global = Row { kind: RowKind::Global, ..Row::default() };
    obs.push_row(global).unwrap();
    let binding = Binding { decision: decision(0), net_id: 1, command: Command::MpEndTurn, ..Binding::default() };
    let candidate = Candidate { action: ActionKind::EndTurn, present: true, legal: true, ..Candidate::default() };
    obs.push_candidate(candidate, binding).unwrap(); obs.finish(DecisionStatus::Actionable).unwrap();
    let before = state(&obs);
    for buffer in [BufferKind::Rows, BufferKind::Edges, BufferKind::Candidates] {
        obs.begin_binding_check(0, binding);
        let cap = match buffer {
            BufferKind::Rows => STATE_CAPACITY,
            BufferKind::Edges => EDGE_CAPACITY,
            BufferKind::Candidates => CANDIDATE_CAPACITY,
            _ => unreachable!(),
        };
        if buffer != BufferKind::Rows { obs.push_row(global).unwrap(); }
        let mut push = |obs: &mut NativeObsV1| match buffer {
            BufferKind::Rows => obs.push_row(global).map(|_| ()),
            BufferKind::Edges => obs.push_edge(Edge { from: RowHandle(1), to: RowHandle(1), relation: Relation::Source }),
            BufferKind::Candidates => obs.push_candidate(candidate, binding).map(|_| ()),
            _ => unreachable!(),
        };
        for _ in 0..cap { push(&mut obs).unwrap(); }
        assert_eq!(push(&mut obs), Err(ContractError::Capacity { buffer, needed: cap + 1, capacity: cap }));
        let _ = obs.end_binding_check();
        assert_eq!(state(&obs), before);
    }
    for bad in [
        Row { model: ContentId { namespace: Namespace::CardId, value: u16::MAX }, ..global },
        Row { parent: RowHandle(u16::MAX), ..global },
        { let mut row = global; row.numbers.integer_present = !0; row },
    ] {
        let mut reference = NativeObsV1::new(); reference.clear(decision(0));
        let expected = reference.push_row(bad);
        assert!(expected.is_err());
        obs.begin_binding_check(0, binding);
        assert_eq!(obs.push_row(bad), expected);
        assert!(obs.end_binding_check().is_err());
        assert_eq!(state(&obs), before);
    }
}

#[test]
fn candidate_only_validation_matches_full_projection_for_live_combat_changes() {
    let mut base = fixture(2);
    let duplicate = base.new_card(0, CardId::StrikeIronclad, false).unwrap();
    base.players[0].hand.push(duplicate);
    for mutation in 0..9 {
        let mut current = base.clone();
        match mutation {
            0 => {},
            1 => current.players[0].energy = 0,
            2 => current.players[0].hand.reverse(),
            3 => { current.players[0].hand.pop(); },
            4 => { current.creatures[current.enemies[0] as usize].hp = 0; },
            5 => { current.enemies.clear(); },
            6 => { let key = current.players[0].hand[0]; current.cards[key as usize].instance_id += 100; },
            7 => { let key = current.players[0].hand[0]; current.cards[key as usize].damage = Some(42); },
            8 => current.is_over = true,
            _ => unreachable!(),
        }
        let full = project(&current, 0);
        let original = project(&base, 0);
        for index in 0..original.view().unwrap().candidate_len {
            if !original.view().unwrap().candidates.sampleable(index, original.view().unwrap().candidate_len) { continue; }
            let mut published = project(&base, 0);
            let binding = published.binding(index, decision(0), 1).unwrap();
            let expected = full.binding(index, decision(0), 1).is_ok_and(|live| live == binding)
                && full.view().unwrap().candidates.model[index] == published.view().unwrap().candidates.model[index];
            let before = state(&published);
            let mut validator = NativeEncoder::new();
            published.begin_binding_check(index, binding);
            validator.begin(&mut published, decision(0), 1).unwrap();
            let global = validator.global(&mut published, &current.run,
                id(Namespace::RunPhase, RunPhase::Combat as usize), 2, Some(&current), None).unwrap();
            let status = validator.combat(&mut published, &current, global, true, |_| Character::Ironclad,
                |slot| current.can_discard_potion_for_player(0, slot)).unwrap();
            published.finish(status).unwrap();
            assert_eq!(published.end_binding_check().is_ok(), expected, "mutation={mutation} candidate={index}");
            assert_eq!(state(&published), before, "validation must not rewrite the published snapshot");
        }
    }
}

#[test]
fn combat_projection_is_readonly_and_hides_rng_draw_order_and_other_hands() {
    let mut c = fixture(4);
    for id_ in [CardId::DefendIronclad, CardId::Bash, CardId::StrikeIronclad] {
        let k = c.new_card(0, id_, false).unwrap(); c.players[0].draw.push(k);
    }
    let before = format!("{c:?}"); let expected = state(&project(&c, 0));
    let observed = project(&c, 0); let rows = observed.view().unwrap().rows;
    for kind in [RowKind::CardPreview, RowKind::PileSummary, RowKind::EffectParameter] {
        assert!(rows.kind.contains(&kind), "privacy comparison must include {kind:?}");
    }
    assert_eq!(format!("{c:?}"), before);
    c.players[0].draw.reverse(); c.run.rngs.shuffle.next_int(0, 100);
    c.run.rngs.monster_ai.next_int(0, 100);
    let hidden = c.players[1].hand[0];
    c.cards[hidden as usize] = Card::from_factory(CardId::Bash, true, 999).unwrap();
    c.cards[hidden as usize].owner = 1;
    assert_eq!(state(&project(&c, 0)), expected);
    let visible = c.players[0].hand[0]; c.cards[visible as usize].damage = Some(42);
    assert_ne!(state(&project(&c, 0)), expected);
}

#[test]
fn hidden_pile_permutations_preserve_multisets_and_fixed_seeker_bindings() {
    fn snapshot(obs: &NativeObsV2) -> String {
        let view = obs.view().unwrap();
        let bindings: Vec<_> = (0..view.candidate_len).map(|slot|
            obs.binding(slot, view.decision, 1).ok()).collect();
        format!("{:?}/{:?}/{:?}/{:?}/{:?}/{bindings:?}", view.status, view.rows,
            view.candidates, view.edges, view.streamed_piles)
    }
    for character in [Character::Ironclad, Character::Silent, Character::Defect,
        Character::Necrobinder, Character::Regent] {
        let mut run = RunManager::new_for_character(202610290000, RunOptions::default(), character);
        let mut encoder = NativeEncoder::new();
        let mut obs = NativeObsV2::new();
        let mut tick = decision(0);
        for _ in 0..32 {
            if run.combat().is_some() { break; }
            encoder.encode_solo(&run, tick, &mut obs).unwrap();
            let view = obs.view().unwrap();
            let slot = (0..view.candidate_len).find(|&i| view.candidates.sampleable(i, view.candidate_len)).unwrap();
            encoder.dispatch_solo(&mut run, &mut tick, &mut obs, slot).unwrap();
        }
        let real = run.combat().expect("a real starting encounter").clone();
        for (size, unique, permutations) in [(8, false, 32), (4000, false, 32), (4000, true, 8)] {
            let mut c = real.clone();
            c.players[0].draw.clear();
            for i in 0..size {
                let k = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
                assert!(c.cards[usize::from(k)].apply_static_enchantment(EnchantmentId::Sharp,
                    if unique { i + 1 } else { 1 }));
                c.players[0].draw.push(k);
            }
            for pile in [PileKind::Discard, PileKind::Exhaust] {
                for i in 0..8 {
                    let k = c.new_card(0, CardId::DefendIronclad, i % 2 == 0).unwrap();
                    match pile {
                        PileKind::Discard => c.players[0].discard.push(k),
                        PileKind::Exhaust => c.players[0].exhaust.push(k),
                        _ => unreachable!(),
                    }
                }
            }
            let hand = c.players[0].hand.len();
            let seeker = c.new_card(0, CardId::SeekerStrike, false).unwrap();
            c.players[0].hand.push(seeker);
            c.players[0].energy = 3;
            assert!(c.play_card(0, hand, Some(c.players.len())));
            assert_eq!(c.pending_choice.as_ref().unwrap().resolver.src, ChoiceSrc::Card(CardId::SeekerStrike));
            let options = c.choice_options.clone();
            encoder.encode_combat_snapshot(&c, &[character], decision(0), 1, &mut obs).unwrap();
            let expected = snapshot(&obs);
            let mut rng = Rng::new(0x5eed_17);
            for permutation in 0..permutations {
                rng.shuffle(&mut c.players[0].draw);
                rng.shuffle(&mut c.players[0].discard);
                rng.shuffle(&mut c.players[0].exhaust);
                rng.shuffle(&mut c.players[0].play);
                assert_eq!(c.choice_options, options);
                encoder.encode_combat_snapshot(&c, &[character], decision(0), 1, &mut obs).unwrap();
                assert!(obs.view().unwrap().state_len <= STATE_CAPACITY);
                assert!(snapshot(&obs) == expected,
                    "{character:?} size={size} unique={unique} permutation={permutation}: hidden pile order leaked");
                let view = obs.view().unwrap();
                let draw = (0..view.state_len).find(|&i| view.rows.kind[i] == RowKind::PileSummary
                    && view.rows.detail[i][0] == id(Namespace::PileKind, PileKind::Draw as usize)).unwrap();
                let handle = RowHandle((draw + 1) as u16);
                let count = view.edges.iter().filter(|e| e.from == handle && e.relation == Relation::Member)
                    .map(|e| u64::from(view.rows.multiplicity[e.to.index().unwrap()])).sum::<u64>()
                    + view.streamed_piles.groups.iter().filter(|g| g.pile == handle)
                        .map(|g| u64::from(g.multiplicity)).sum::<u64>();
                assert_eq!(count, size as u64, "candidate details must not change the public multiset");
            }
        }
    }
}

#[test]
fn duplicate_draw_cards_with_distinct_live_costs_do_not_reveal_order() {
    let mut c = fixture(1);
    for cost in [0, 2, 1, 0, 1, 2] {
        let k = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
        c.cards[k as usize].cv.turn_star_cost_override = Some(cost);
        c.players[0].draw.push(k);
    }
    let expected = state(&project(&c, 0));
    c.players[0].draw.reverse();
    assert_eq!(state(&project(&c, 0)), expected);
    c.players[0].draw.rotate_left(2);
    assert_eq!(state(&project(&c, 0)), expected);
}

#[test]
fn combat_masks_bind_exact_cards_targets_and_inventory_holes() {
    let c = fixture(1); let obs = project(&c, 0); let v = obs.view().unwrap();
    assert_eq!(v.status, DecisionStatus::Actionable);
    let mut play = 0; let mut discard = 0;
    for i in 0..v.candidate_len {
        if !v.candidates.sampleable(i, v.candidate_len) { continue; }
        let binding = obs.binding(i, decision(0), 1).unwrap();
        match binding.command {
            Command::Solo(SoloCommand::PlayCard { hand, target }) => {
                play += 1; let card = &c.cards[c.players[0].hand[hand as usize] as usize];
                assert_eq!(binding.source_instance, Some(card.instance_id));
                assert_eq!(binding.target_instance, Some(u32::from(c.enemies[target.unwrap() as usize])));
                let mut stepped = c.clone(); assert!(stepped.play_card(0, hand as usize, target.map(usize::from)));
            }
            Command::Solo(SoloCommand::DiscardPotion { slot }) => { discard += 1; assert_eq!(slot, 0); }
            _ => {}
        }
    }
    assert!(play > 0); assert_eq!(discard, 1);
    assert!(v.rows.kind.iter().enumerate().any(|(i, &kind)| kind == RowKind::Potion
        && v.rows.model[i] == ContentId::ABSENT));
}

#[test]
fn every_public_child_retains_typed_parent_owner_and_power_target() {
    let mut c = fixture(1);
    let target = c.enemies[0];
    c.apply_power_to(target, PowerId::Vulnerable, 2, Some(0));
    let k = c.players[0].hand[0];
    assert!(c.cards[k as usize].apply_static_enchantment(EnchantmentId::TezcatarasEmber, 1));
    let obs = project(&c, 0); let v = obs.view().unwrap();
    for i in 0..v.state_len {
        let from = RowHandle((i + 1) as u16);
        for (to, relation) in [(v.rows.parent[i], Relation::Parent), (v.rows.owner[i], Relation::Owner)] {
            if to != RowHandle::ABSENT {
                assert!(v.edges.contains(&Edge { from, to, relation }), "{:?}", v.rows.kind[i]);
            }
        }
        if matches!(v.rows.kind[i], RowKind::Intent | RowKind::Power) {
            assert_eq!(v.rows.kind[v.rows.parent[i].index().unwrap()], RowKind::Creature);
        }
        if v.rows.kind[i] == RowKind::Modifier && v.rows.model[i].namespace == Namespace::EnchantmentId {
            assert_eq!(v.rows.kind[v.rows.parent[i].index().unwrap()], RowKind::Card);
        }
    }
}

#[test]
fn damage_and_block_previews_preserve_gameplay_and_match_original_arithmetic() {
    let mut c = fixture(1); let k = c.players[0].hand[0]; let target = c.enemies[0];
    c.apply_power_to(0, PowerId::Strength, 5, Some(0));
    c.apply_power_to(target, PowerId::Vulnerable, 2, Some(0));
    assert!(c.cards[k as usize].apply_static_enchantment(EnchantmentId::TezcatarasEmber, 1));
    let before = format!("{c:?}");
    // Original-DLL TezcatarasEmber example: floor((6 + 5 + 3) * 1.5) = 21.
    assert_eq!(c.preview_damage(6, Some(0), Some(target), Props::MOVE, Some(k)), Ok(21));
    assert_eq!(format!("{c:?}"), before);
    c.apply_power_to(0, PowerId::Dexterity, 2, Some(0));
    c.apply_power_to(0, PowerId::Frail, 1, Some(0));
    let block = c.new_card(0, CardId::DefendIronclad, false).unwrap();
    let before = format!("{c:?}");
    assert_eq!(c.preview_block(5, 0, Props::MOVE, Some(block)), Ok(5));
    assert_eq!(format!("{c:?}"), before);
    let mut actual = c.clone(); assert_eq!(actual.modify_block(5, 0, Props::MOVE, Some(block)), 5);
}

#[test]
fn v2_body_slam_uses_the_original_display_calculation() {
    // C# BodySlam.CanonicalVars: CalculationBase=0, ExtraDamage=1,
    // multiplier=Owner.Creature.Block; NCard.UpdateVisuals runs the preview.
    let mut c = fixture(1);
    c.players[0].hand.clear();
    let k = c.new_card(0, CardId::BodySlam, false).unwrap();
    c.players[0].hand.push(k);
    c.cr_mut(0).block = 17;
    let obs = project(&c, 0); let v = obs.view().unwrap();
    let play = v.candidates.action.iter().position(|&a| a == ActionKind::PlayCard).unwrap();
    assert_eq!(v.candidates.integers[play][11], 17);
}


#[test]
fn v2_non_damage_power_previews_use_original_given_hooks_without_artifact_or_mutation() {
    // DeadlyPoison.cs: 5, upgraded +2. SneckoSkull.cs: owner-only +1.
    // UnsettlingLamp.cs: double only its selected source; PowerVar.cs never
    // calls ModifyPowerAmountReceived, so target Artifact cannot erase a preview.
    for owner in 0..2u8 { for (upgraded, base) in [(false, 5), (true, 7)] {
        let mut c = fixture(2); c.players[owner].hand.clear();
        let k = c.new_card(owner, CardId::DeadlyPoison, upgraded).unwrap();
        c.players[owner].hand.push(k);
        let mut skull = Relic::new(RelicId::SneckoSkull); skull.owner = owner;
        c.run.relics.push(skull);
        let mut other_skull = Relic::new(RelicId::SneckoSkull); other_skull.owner = 1 - owner;
        c.run.relics.push(other_skull);
        let mut lamp = Relic::new(RelicId::UnsettlingLamp); lamp.owner = owner; lamp.card = Some(k);
        c.run.relics.push(lamp);
        c.apply_power_to(c.enemies[0], PowerId::Artifact, 1, Some(c.enemies[0]));
        let before = format!("{c:?}");
        let obs = project(&c, owner); let v = obs.view().unwrap();
        let source = (0..v.state_len).find(|&i| v.rows.kind[i] == RowKind::Card
            && v.rows.model[i] == id(Namespace::CardId, CardId::DeadlyPoison.index())
            && v.rows.detail[i][3] == id(Namespace::PileKind, PileKind::Hand as usize)).unwrap();
        let rows: Vec<_> = (0..v.state_len).filter(|&i| v.rows.kind[i] == RowKind::CardPreview
            && v.rows.parent[i] == RowHandle((source + 1) as u16)
            && v.rows.detail[i][0] == v2::public_variable("PoisonPower").unwrap()).collect();
        assert!(!rows.is_empty());
        for i in rows { assert_eq!(v.rows.integers[i][0], (base + 1) * 2); assert_eq!(v.rows.integers[i][1], base); }
        assert_eq!(format!("{c:?}"), before);
        assert_eq!(c.cards[usize::from(k)].native_card_preview().unwrap().iter().find(|v| v.name == "PoisonPower").unwrap().value, base);
    } }
}

#[test]
fn v2_non_damage_power_descriptor_coverage_matches_every_original_card_constructor() {
    let source: serde_json::Value = serde_json::from_str(include_str!("../../../sim/data/v2/card.json")).unwrap();
    let native: serde_json::Value = serde_json::from_str(include_str!("../../../sim/data/cards.json")).unwrap();
    for model in source["models"].as_array().unwrap() {
        // Original test doubles have no game factory identity or reachable row.
        if model["class"].as_str().unwrap().starts_with("Cards.Mocks.") { continue; }
        let canonical = model["members"].as_array().unwrap().iter()
            .filter_map(|m| m["csharp"].as_str()).find(|s| s.contains("CanonicalVars")).unwrap_or("");
        let mut expected = std::collections::BTreeSet::new();
        for constructor in canonical.split("new PowerVar<").skip(1) {
            let (power, rest) = constructor.split_once(">(").unwrap();
            let args = rest.split_once(')').unwrap().0;
            let name = if args.starts_with('"') { args.split('"').nth(1).unwrap() } else { power };
            expected.insert(name);
        }
        if expected.is_empty() { continue; }
        let entry = model["entry"].as_str().unwrap();
        let ids: Vec<_> = CardId::ALL.into_iter().filter(|id| native["cards"][id.name()]["base"]["entry"] == entry).collect();
        assert!(!ids.is_empty(), "{entry}: original model has no native identity");
        for id in ids {
            let card = Card::from_factory(id, false, 1).expect(entry);
            let variables = card.native_card_preview().unwrap();
            let actual: std::collections::BTreeSet<_> = variables.iter().filter(|v| !v.damage && !v.block && expected.contains(v.name))
                .map(|v| v.name).collect();
            assert_eq!(actual, expected, "{entry}");
            for variable in actual { assert!(v2::public_variable(variable).is_some(), "{entry}: {variable}"); }
        }
    }
}

#[test]
fn v2_barrage_displays_orb_dependent_hits() {
    // Barrage.cs CanonicalVars: CalculatedHits = 0 + 1 * OrbQueue.Orbs.Count.
    let mut c = fixture(1); c.players[0].hand.clear();
    let k = c.new_card(0, CardId::Barrage, false).unwrap(); c.players[0].hand.push(k);
    c.players[0].orbs.open_base_slots(3);
    c.channel_orb(0, sts2_sim::orbs::OrbKind::Lightning);
    c.channel_orb(0, sts2_sim::orbs::OrbKind::Frost);
    let obs = project(&c, 0); let v = obs.view().unwrap();
    let rows: Vec<_> = (0..v.state_len).filter(|&i| v.rows.kind[i] == RowKind::CardPreview
        && v.rows.detail[i][0] == v2::public_variable("CalculatedHits").unwrap()).collect();
    assert!(!rows.is_empty());
    for i in rows { assert_eq!(v.rows.integers[i][0], 2); assert_eq!(v.rows.integers[i][2], 2); }
}

#[test]
fn v2_multiplayer_voltaic_counts_only_its_owners_successful_lightning_channels() {
    let mut c = fixture(2);
    for owner in 0..2u8 {
        c.players[owner].hand.clear();
        c.players[owner].orbs.open_base_slots(3);
        let k = c.new_card(owner, CardId::Voltaic, false).unwrap();
        c.players[owner].hand.push(k);
    }
    use sts2_sim::orbs::OrbKind;
    c.channel_orb(0, OrbKind::Lightning);
    c.channel_orb(1, OrbKind::Lightning);
    c.channel_orb(0, OrbKind::Lightning);
    c.channel_orb(1, OrbKind::Frost);
    c.evoke_front_orb(0);
    let before = format!("{c:?}");
    for (owner, expected) in [(0, 2), (1, 1)] {
        let obs = project(&c, owner); let v = obs.view().unwrap();
        let i = (0..v.state_len).find(|&i| v.rows.kind[i] == RowKind::CardPreview
            && v.rows.detail[i][0] == v2::public_variable("CalculatedChannels").unwrap()).expect("owner-filtered Voltaic preview");
        assert_eq!(v.rows.integers[i][0], expected);
    }
    assert_eq!(format!("{c:?}"), before);
}

#[test]
fn four_thousand_pile_cards_encode_losslessly_without_changing_legal_bindings_or_privacy() {
    let mut c = fixture(1); c.players[0].draw.clear();
    for _ in 1..10 {
        let k = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
        c.players[0].hand.push(k);
    }
    let mut baseline = NativeObsV2::new();
    NativeEncoder::new().encode_combat_snapshot(&c, &[Character::Necrobinder], decision(0), 1, &mut baseline).unwrap();
    let view = baseline.view().unwrap();
    let expected: Vec<_> = (0..view.candidate_len).filter(|&i| view.candidates.sampleable(i, view.candidate_len))
        .map(|i| baseline.binding(i, decision(0), 1).unwrap()).collect();
    for _ in 0..4000 {
        let k = c.new_card(0, CardId::StrikeNecrobinder, false).unwrap();
        assert!(c.cards[usize::from(k)].apply_static_enchantment(EnchantmentId::Sharp, 1));
        c.players[0].draw.push(k);
    }
    let mut obs = NativeObsV2::new();
    NativeEncoder::new().encode_combat_snapshot(&c, &[Character::Necrobinder], decision(0), 1, &mut obs)
        .expect("the original game has no pile-card limit");
    let view = obs.view().unwrap();
    assert!(view.state_len < STATE_CAPACITY);
    let actual: Vec<_> = (0..view.candidate_len).filter(|&i| view.candidates.sampleable(i, view.candidate_len))
        .map(|i| obs.binding(i, decision(0), 1).unwrap()).collect();
    assert_eq!(actual, expected);
    let draw = id(Namespace::PileKind, PileKind::Draw as usize);
    let weighted = |view: &ObservationView<'_>| {
        (0..view.state_len).filter(|&i| view.rows.kind[i] == RowKind::Card && view.rows.detail[i][3] == draw)
            .map(|i| u64::from(view.rows.multiplicity[i])).sum::<u64>()
        + view.streamed_piles.weighted_rows().filter(|(row, _)| row.kind == RowKind::Card && row.detail[3] == draw)
            .map(|(_, n)| u64::from(n)).sum::<u64>()
    };
    assert_eq!(weighted(&view), 4000);
    assert_eq!((0..view.state_len).filter(|&i| view.rows.kind[i] == RowKind::Card
        && view.rows.detail[i][3] == id(Namespace::PileKind, PileKind::Hand as usize)).count(), 10);
    assert!(view.legacy.is_none());
    // Every group is now unique. Overflow must remain a lossless weighted
    // stream; averaging different numeric facts together would fail this check.
    for (i, &k) in c.players[0].draw.iter().enumerate() {
        c.cards[usize::from(k)].enchantments[0].1 = (i + 1) as i32;
    }
    NativeEncoder::new().encode_combat_snapshot(&c, &[Character::Necrobinder], decision(0), 1, &mut obs).unwrap();
    let view = obs.view().unwrap();
    assert_eq!(weighted(&view), 4000);
    assert!(!view.streamed_piles.groups.is_empty());
    assert!(view.state_len <= STATE_CAPACITY && view.edges.len() <= EDGE_CAPACITY);
    let sharp: ContentId = EnchantmentId::Sharp.into();
    let mut amounts = std::collections::BTreeMap::new();
    for i in 0..view.state_len {
        if view.rows.kind[i] == RowKind::Modifier && view.rows.model[i] == sharp {
            *amounts.entry(view.rows.integers[i][0]).or_insert(0u32) += view.rows.multiplicity[i];
        }
    }
    for (row, n) in view.streamed_piles.weighted_rows() {
        if row.kind == RowKind::Modifier && row.model == sharp {
            *amounts.entry(row.numbers.integers[0]).or_insert(0u32) += n;
        }
    }
    assert_eq!(amounts, (1..=4000).map(|n| (n, 1)).collect());
    let actual: Vec<_> = (0..view.candidate_len).filter(|&i| view.candidates.sampleable(i, view.candidate_len))
        .map(|i| obs.binding(i, decision(0), 1).unwrap()).collect();
    assert_eq!(actual, expected);
    let public = format!("{:?}/{:?}/{:?}/{:?}", view.rows, view.candidates, view.edges, view.streamed_piles);
    c.players[0].draw.reverse();
    c.run.rngs.shuffle.next_int(0, 1000);
    for &k in &c.players[0].draw { c.cards[usize::from(k)].instance_id += 1_000_000; }
    NativeEncoder::new().encode_combat_snapshot(&c, &[Character::Necrobinder], decision(0), 1, &mut obs).unwrap();
    let view = obs.view().unwrap();
    assert_eq!(public, format!("{:?}/{:?}/{:?}/{:?}", view.rows, view.candidates, view.edges, view.streamed_piles));
}

#[test]
fn v2_x_preview_uses_energy_and_chemical_x_without_spending_them() {
    // Whirlwind.ResolveEnergyXValue and ChemicalX.ModifyXValue: energy + 2.
    let mut c = fixture(1); c.players[0].hand.clear();
    let k = c.new_card(0, CardId::Whirlwind, false).unwrap(); c.players[0].hand.push(k);
    c.players[0].energy = 4; c.run.relics.push(Relic::new(RelicId::ChemicalX));
    let before = format!("{c:?}"); let obs = project(&c, 0); let v = obs.view().unwrap();
    let i = (0..v.state_len).find(|&i| v.rows.kind[i] == RowKind::CardPreview).unwrap();
    assert_eq!(v.rows.integers[i][3], 6); assert_eq!(format!("{c:?}"), before);
}

#[test]
fn v2_target_preview_uses_calculated_base_strength_and_vulnerable() {
    // Bully.cs: base 4 + 2 * target Vulnerable stacks. StrengthPower adds 3;
    // VulnerablePower multiplies by 1.5: floor((4 + 4 + 3) * 1.5) = 16.
    let mut c = fixture(1); c.players[0].hand.clear();
    let k = c.new_card(0, CardId::Bully, false).unwrap(); c.players[0].hand.push(k);
    c.apply_power_to(0, PowerId::Strength, 3, Some(0));
    c.apply_power_to(c.enemies[0], PowerId::Vulnerable, 2, Some(0));
    let obs = project(&c, 0); let v = obs.view().unwrap();
    let play = v.candidates.action.iter().position(|&a| a == ActionKind::PlayCard).unwrap();
    assert_eq!(v.candidates.integers[play][11], 16);
    assert!(v.edges.iter().any(|e| e.relation == Relation::Target && e.to == v.candidates.target[play]
        && v.rows.kind[e.from.index().unwrap()] == RowKind::CardPreview
        && v.rows.integers[e.from.index().unwrap()][0] == 16));
}

#[test]
fn v2_upgrade_preview_is_an_upgraded_instance_and_preserves_enchantments() {
    // StrikeIronclad.cs: Damage 6 -> 9. TezcatarasEmber adds 3 displayed
    // damage and sets canonical energy cost to zero; preserve both effects.
    let mut ctx = fixture(1).run;
    let mut card = Card::from_factory(CardId::StrikeIronclad, false, 991).unwrap();
    assert!(card.apply_static_enchantment(EnchantmentId::TezcatarasEmber, 1));
    ctx.deck.push(card);
    let before = format!("{ctx:?}");
    assert_eq!(ctx.native_upgrade_preview(&ctx.deck[0]).unwrap().damage, Some(9));
    let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV2::new();
    encoder.begin(&mut obs, decision(0), 1).unwrap();
    let global = encoder.global(&mut obs, &ctx, id(Namespace::RunPhase, RunPhase::RestSite as usize), 1, None, None).unwrap();
    let seat = NativeEncoder::row(&mut obs, Row { kind: RowKind::Seat, parent: global, ..Row::default() }).unwrap();
    encoder.seat_rows[0] = seat;
    encoder.inventory(&mut obs, 0, &ctx.deck, &ctx.relics, &ctx.potions, 2, None).unwrap();
    let source = encoder.card_handle(deck_key(0, 991));
    encoder.upgrade_preview(&mut obs, &ctx, &ctx.deck[0], source, seat, ChoicePurpose::RunDeckChoiceKindSmith).unwrap();
    encoder.simple(&mut obs, Command::Solo(SoloCommand::EndTurn), seat, true).unwrap();
    obs.finish(DecisionStatus::Actionable).unwrap(); let v = obs.view().unwrap();
    let marker = v.rows.kind.iter().position(|&k| k == RowKind::UpgradePreview).unwrap();
    let marker = RowHandle((marker + 1) as u16);
    let after = v.edges.iter().find(|e| e.from == marker && e.relation == Relation::Target).unwrap().to;
    let i = after.index().unwrap();
    assert_eq!(v.rows.integers[i][5], 12); assert_eq!(v.rows.integers[i][0], 0);
    assert!(v.edges.contains(&Edge { from: marker, to: source, relation: Relation::Source }));
    assert!((0..v.state_len).any(|i| v.rows.parent[i] == after
        && v.rows.model[i] == ContentId::from(EnchantmentId::TezcatarasEmber)));
    assert_eq!(format!("{ctx:?}"), before);
}

#[test]
fn v2_deck_summary_counts_only_owned_members_not_live_or_offered_cards() {
    let mut c = fixture(1);
    c.run.deck.push(Card::from_factory(CardId::StrikeIronclad, true, 901).unwrap());
    c.run.deck.push(Card::from_factory(CardId::DefendIronclad, false, 902).unwrap());
    let obs = project(&c, 0); let v = obs.view().unwrap();
    let summary = (0..v.state_len).find(|&i| v.rows.kind[i] == RowKind::PileSummary
        && v.rows.detail[i][0] == id(Namespace::PileKind, PileKind::None as usize)).unwrap();
    assert_eq!(&v.rows.integers[summary][..7], &[2, 1, 1, 0, 0, 0, 1]);
    assert_eq!(v.rows.integers[summary][8], 2);
    assert_eq!(v.rows.integers[summary][15], 1);
    let members: Vec<_> = v.edges.iter().filter(|e| e.from == RowHandle((summary + 1) as u16)
        && e.relation == Relation::Member).collect();
    assert_eq!(members.len(), 2);
    assert!(members.iter().all(|e| v.rows.detail[e.to.index().unwrap()][3] == id(Namespace::PileKind, PileKind::None as usize)));
}

#[test]
fn v2_directed_route_summary_deduplicates_reconverging_visible_paths() {
    let run = RunManager::new_debug_act(7, RunOptions::default(), Character::Ironclad, 2).unwrap();
    let mut map = run.visible_map().unwrap().clone();
    map.points = map.points.iter().filter(|p| map.point(p.coord).is_some_and(|q| std::ptr::eq(*p, q)))
        .take(4).cloned().collect();
    for (i, point) in map.points.iter_mut().enumerate() {
        point.coord = sts2_sim::run::Coord::new(i as i32, i as i32);
        point.children.clear(); point.parents.clear();
        point.point_type = match i { 0 => MapPointType::Unknown, 1 | 2 => MapPointType::RestSite, _ => MapPointType::Boss };
    }
    map.points[0].children.extend([1, 2]); map.points[1].children.push(3); map.points[2].children.push(3);
    let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV2::new();
    encoder.begin(&mut obs, decision(0), 1).unwrap();
    let global = encoder.global(&mut obs, run.run_ctx(), id(Namespace::RunPhase, RunPhase::MapChoice as usize), 1, None, None).unwrap();
    encoder.map(&mut obs, &map, 2, None, |i| i == 0, |_| None).unwrap();
    encoder.simple(&mut obs, Command::Solo(SoloCommand::EndTurn), global, true).unwrap();
    obs.finish(DecisionStatus::Actionable).unwrap(); let v = obs.view().unwrap();
    for (kind, count, distance) in [(MapPointType::RestSite, 2, 1), (MapPointType::Boss, 1, 2),
        (MapPointType::Unknown, 1, 0), (MapPointType::Treasure, 0, 0)] {
        let i = (0..v.state_len).find(|&i| v.rows.kind[i] == RowKind::RouteSummary
            && v.rows.detail[i][0] == id(Namespace::MapPointType, kind as usize)).unwrap();
        assert_eq!(v.rows.integers[i][0], count);
        if count > 0 { assert_eq!(&v.rows.integers[i][1..3], &[distance, distance]); }
        else { assert_eq!(v.rows.integer_present[i] & 6, 0); }
    }
    map.points[0].children = vec![1];
    map.points[1].point_type = MapPointType::Shop;
    encoder.begin(&mut obs, decision(1), 1).unwrap();
    let global = encoder.global(&mut obs, run.run_ctx(), id(Namespace::RunPhase, RunPhase::MapChoice as usize), 1, None, None).unwrap();
    encoder.map(&mut obs, &map, 2, None, |i| i == 0, |_| None).unwrap();
    encoder.simple(&mut obs, Command::Solo(SoloCommand::EndTurn), global, true).unwrap();
    obs.finish(DecisionStatus::Actionable).unwrap();
    let v = obs.view().unwrap();
    for (kind, count) in [(MapPointType::RestSite, 0), (MapPointType::Shop, 1), (MapPointType::Boss, 1)] {
        let i = (0..v.state_len).find(|&i| v.rows.kind[i] == RowKind::RouteSummary
            && v.rows.detail[i][0] == id(Namespace::MapPointType, kind as usize)).unwrap();
        assert_eq!(v.rows.integers[i][0], count);
    }
}

#[test]
fn v2_a10_act_three_exposes_both_visible_bosses_in_map_order() {
    // NBossMapPoint._Ready chooses BossEncounter vs SecondBossEncounter
    // by the corresponding MapPoint, not by the next random encounter.
    let run = RunManager::new_debug_act(7, RunOptions::default(), Character::Ironclad, 2).unwrap();
    let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV2::new();
    encoder.encode_solo(&run, decision(0), &mut obs).unwrap();
    let v = obs.view().unwrap();
    let rows: Vec<_> = (0..v.state_len).filter(|&i| v.rows.kind[i] == RowKind::Boss).collect();
    assert_eq!(rows.len(), 2);
    assert_ne!(v.rows.model[rows[0]], v.rows.model[rows[1]]);
    for (order, &i) in rows.iter().enumerate() {
        assert_eq!(v.rows.integers[i][0], order as i32); assert_eq!(v.rows.integers[i][1], 2);
        let edge = v.edges.iter().find(|e| e.from == RowHandle((i + 1) as u16) && e.relation == Relation::Target).unwrap();
        assert_eq!(v.rows.detail[edge.to.index().unwrap()][1], id(Namespace::MapPointType, MapPointType::Boss as usize));
        assert_eq!(v.rows.flags[i] & Flag::Current.bit() != 0, order == 0);
        assert_eq!(v.rows.flags[i] & Flag::Complete.bit(), 0);
    }
}

#[test]
fn v2_potion_magnitude_and_relic_tooltip_parameters_match_original_canonical_vars() {
    // FirePotion.cs DamageVar(20, Unpowered); RegalPillow.cs HealVar(15).
    let mut c = fixture(1); c.run.relics.push(Relic::new(RelicId::RegalPillow));
    let obs = project(&c, 0); let v = obs.view().unwrap();
    for (kind, model, variable, expected) in [
        (RowKind::Potion, id(Namespace::PotionId, PotionId::Firepotion.index()), "Damage", 20),
        (RowKind::Relic, id(Namespace::RelicId, RelicId::RegalPillow.index()), "Heal", 15),
    ] {
        let parent = (0..v.state_len).find(|&i| v.rows.kind[i] == kind && v.rows.model[i] == model).unwrap();
        assert!((0..v.state_len).any(|i| v.rows.kind[i] == RowKind::EffectParameter
            && v.rows.parent[i] == RowHandle((parent + 1) as u16)
            && v.rows.model[i] == v2::public_variable(variable).unwrap() && v.rows.integers[i][0] == expected));
    }
}

#[test]
fn v2_event_option_hp_and_gold_costs_are_separate_public_parameters() {
    // DenseVegetation.cs HpLossVar(8), displayed Gold roll 61..99.
    // FakeMerchant.cs relicCost = 50, shown on MerchantRelicEntry.
    let run = RunManager::new(7, RunOptions::default());
    let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV2::new();
    encoder.begin(&mut obs, decision(0), 1).unwrap();
    let global = encoder.global(&mut obs, run.run_ctx(), id(Namespace::RunPhase, RunPhase::Event as usize), 1, None, None).unwrap();
    let seat = NativeEncoder::row(&mut obs, Row { kind: RowKind::Seat, parent: global, ..Row::default() }).unwrap();
    encoder.seat_rows[0] = seat;
    for (option, description, expected) in [
        (EventOptionId::DenseVegetationInitialTrudgeOn, EventDescription::DenseVegetationTrudgeGold(77), vec![("HpCost", 8), ("GoldGain", 77)]),
        (EventOptionId::FakeMerchantNFakeMerchantMerchantRelicEntry, EventDescription::FakeMerchantCost(50), vec![("GoldCost", 50)]),
    ] {
        let parameters = run.native_event_parameters(EventOptionIdentity { id: option, parameter: EventParameter::None }, Some(description)).unwrap();
        let parent = encoder.event_option(&mut obs, EventOptionIdentity { id: option, parameter: EventParameter::None },
            option, Some(description), 0, seat, true, None, &parameters).unwrap();
        encoder.simple(&mut obs, Command::Solo(SoloCommand::EndTurn), parent, true).unwrap();
        obs.finish(DecisionStatus::Actionable).unwrap(); let v = obs.view().unwrap();
        for (variable, amount) in expected {
            assert!((0..v.state_len).any(|i| v.rows.kind[i] == RowKind::EffectParameter && v.rows.parent[i] == parent
                && v.rows.model[i] == v2::public_variable(variable).unwrap() && v.rows.integers[i][0] == amount));
        }
    }
}

#[test]
fn v2_regal_pillow_rest_heal_is_capped_and_candidate_targets_its_seat() {
    use sts2_sim::run::{Coord, mp::MpPhase};
    let specs = [PlayerSpec { net_id: 10, character: Character::Ironclad }, PlayerSpec { net_id: 11, character: Character::Silent }];
    let mut run = MpRun::new("V2-REST-CAP", &specs, 10).unwrap();
    let player = run.roster.player_mut(0).unwrap(); player.hp = 77; player.max_hp = 80;
    player.relics.push(Relic::new(RelicId::RegalPillow));
    run.phase = MpPhase::Map; let (x, y) = run.world.run.coord.unwrap();
    let child = run.world.map.point(Coord::new(x, y)).unwrap().children[0] as usize;
    run.world.map.points[child].point_type = MapPointType::RestSite; let coord = run.world.map.points[child].coord;
    for seat in 0..2 { run.vote(10 + seat as u64, seat, coord).unwrap(); }
    assert_eq!(run.native_rest_heal_amount(0), Some(3)); // min(80-77, 24+15)
    let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV2::new();
    encoder.encode_multiplayer(&run, decision(0), 10, &mut obs).unwrap(); let v = obs.view().unwrap();
    let service = (0..v.state_len).find(|&i| v.rows.kind[i] == RowKind::RestService
        && v.rows.detail[i][0] == id(Namespace::RestOptionKind, sts2_sim::run::RestOptionKind::Heal as usize)).unwrap();
    assert_eq!(v.rows.integers[service][0], 3);
    let candidate = (0..v.candidate_len).find(|&i| v.candidates.action[i] == ActionKind::RestOption
        && v.candidates.source[i] == RowHandle((service + 1) as u16)).unwrap();
    assert_eq!(v.rows.kind[v.candidates.target[candidate].index().unwrap()], RowKind::Seat);
    assert_eq!(v.candidates.target[candidate], v.rows.owner[service]);
}

#[test]
fn v2_summary_groups_keep_map_out_of_combat_and_keep_direct_context_out_of_pools() {
    let run = RunManager::new_debug_late_boss(7, RunOptions::default(), Character::Ironclad, 2).unwrap();
    let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV2::new();
    encoder.encode_solo(&run, decision(0), &mut obs).unwrap(); let v = obs.view().unwrap();
    let mut counts = [0; 3];
    for i in 0..v.state_len {
        let group = v.summary_group(i);
        if matches!(v.rows.kind[i], RowKind::Seat | RowKind::Global) { assert_eq!(group, None); }
        if matches!(v.rows.kind[i], RowKind::MapNode | RowKind::Boss) { assert_eq!(group, Some(SummaryGroup::Map)); }
        if matches!(v.rows.kind[i], RowKind::Intent | RowKind::Creature) { assert_eq!(group, Some(SummaryGroup::Combat)); }
        if v.rows.kind[i] == RowKind::CardPreview {
            assert_eq!(group, v.summary_group(v.rows.parent[i].index().unwrap()));
        }
        if let Some(group) = group { counts[group as usize] += 1; }
    }
    assert!(counts.into_iter().all(|n| n > 0));
}

#[test]
fn v2_shop_card_and_scroll_boxes_bundle_retain_offered_instance_values_and_members() {
    // Shop offers preserve the actual upgraded card (Strike: 9 damage).
    // ScrollBoxes.AfterObtained displays bundles containing three actual cards.
    let run = RunManager::new(7, RunOptions::default());
    let card = Card::from_factory(CardId::StrikeIronclad, true, 992).unwrap();
    let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV2::new();
    encoder.begin(&mut obs, decision(0), 1).unwrap();
    let global = encoder.global(&mut obs, run.run_ctx(), id(Namespace::RunPhase, RunPhase::Shop as usize), 1, None, None).unwrap();
    let seat = NativeEncoder::row(&mut obs, Row { kind: RowKind::Seat, parent: global, ..Row::default() }).unwrap();
    encoder.seat_rows[0] = seat;
    encoder.shop_offer(&mut obs, sts2_sim::run::NativeShopOffer { kind: sts2_sim::run::NativeShopKind::Card,
        index: 0, card: Some(&card), relic: None, potion: None, price: 123, on_sale: false, sold: false, enabled: true }, true, false).unwrap();
    obs.finish(DecisionStatus::Actionable).unwrap(); let v = obs.view().unwrap();
    let offer = v.candidates.source[0]; let target = v.candidates.target[0];
    assert_eq!(v.rows.integers[offer.index().unwrap()][0], 123);
    assert_eq!(v.rows.integers[target.index().unwrap()][5], 9);
    assert!(v.edges.contains(&Edge { from: offer, to: target, relation: Relation::Member }));

    let specs = [PlayerSpec { net_id: 10, character: Character::Ironclad }, PlayerSpec { net_id: 11, character: Character::Silent }];
    let mut multi = MpRun::new("V2-SCROLL-BOXES", &specs, 10).unwrap();
    multi.scroll_box_bundles[0] = Some(vec![vec![
        Card::from_factory(CardId::StrikeIronclad, false, 901).unwrap(),
        Card::from_factory(CardId::DefendIronclad, true, 902).unwrap(),
        Card::from_factory(CardId::Bash, false, 903).unwrap(),
    ]]);
    encoder.encode_multiplayer(&multi, decision(0), 10, &mut obs).unwrap(); let v = obs.view().unwrap();
    let bundle = v.candidates.source[v.candidates.action.iter().position(|&a| a == ActionKind::PickCardBundle).unwrap()];
    assert_eq!(v.rows.integers[bundle.index().unwrap()][2], 3);
    let members: Vec<_> = v.edges.iter().filter(|e| e.from == bundle && e.relation == Relation::Member).collect();
    assert_eq!(members.len(), 3);
    let defend = members.iter().map(|e| e.to.index().unwrap()).find(|&i|
        v.rows.model[i] == id(Namespace::CardId, CardId::DefendIronclad.index())).unwrap();
    assert_eq!(v.rows.integers[defend][6], 8);
}

#[test]
fn hidden_daggers_fixed_selection_preserves_explicit_replay_confirmation() {
    // Checkpoint 279 lane 6, seed 20261031001542: HiddenDaggers,
    // owner 2, hand selector min=max=2 was left toggling indefinitely.
    let mut c = fixture(2);
    let card = c.new_card(1, CardId::HiddenDaggers, false).unwrap();
    let a = c.new_card(1, CardId::StrikeIronclad, false).unwrap();
    let b = c.new_card(1, CardId::DefendIronclad, false).unwrap();
    c.players[1].hand.clear(); c.players[1].hand.extend([a,b]);
    c.pending_choice = Some(PendingChoice { player:1,
        resolver:Resolver {src:ChoiceSrc::Card(CardId::HiddenDaggers),k:card,target:None,step:0,arg:0},
        source:PileKind::Hand,allow_skip:false,min:2,max:2,single:false});
    c.choice_options=vec![a,b]; c.choice_selected.clear();
    assert!(c.resolve_pending_choice_for_player(1,Some(0)));
    assert!(c.pending_choice_for_player(1).is_some());
    assert!(c.resolve_pending_choice_for_player(1,Some(1)));
    // The serialized oracle protocol retains its explicit confirmation;
    // the native UI adapter combines it with the final fixed-count click.
    assert!(c.pending_choice_for_player(1).is_some());
    assert!(c.resolve_pending_choice_for_player(1,None));
    assert!(c.pending_choice_for_player(1).is_none());
    assert!(!c.players[1].hand.contains(&a));
    assert!(!c.players[1].hand.contains(&b));
}

#[test]
fn ordered_choice_preserves_selected_instance_order_without_leaking_to_other_seat() {
    let mut c = fixture(2); let first = c.players[0].hand[0];
    let second = c.new_card(0, CardId::StrikeIronclad, false).unwrap(); c.players[0].hand.push(second);
    c.pending_choice = Some(PendingChoice { player: 0,
        resolver: Resolver { src: ChoiceSrc::Card(CardId::Stratagem), k: first, target: None, step: 1, arg: 0 },
        source: PileKind::Hand, allow_skip: true, min: 0, max: 2, single: false });
    c.choice_options = vec![first, second]; c.choice_selected = vec![1, 0];
    let obs = project(&c, 0); let v = obs.view().unwrap();
    let options: Vec<_> = v.rows.kind.iter().enumerate().filter_map(|(i, &kind)|
        (kind == RowKind::ChoiceOption).then_some(i)).collect();
    assert_eq!(options.len(), 2);
    assert_eq!(v.rows.integers[options[0]][1], 1);
    assert_eq!(v.rows.integers[options[1]][1], 0);
    assert!(v.candidates.action.contains(&ActionKind::ConfirmChoice));
    let other = project(&c, 1); let v = other.view().unwrap();
    assert_eq!(v.status, DecisionStatus::Waiting);
    assert!(!v.rows.kind.contains(&RowKind::Choice));
    assert_eq!(v.candidate_len, 0);
}

#[test]
fn fractional_event_amount_reconstructs_exactly_and_locked_options_stay_visible() {
    let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new();
    encoder.begin(&mut obs, decision(0), 1).unwrap();
    let global = NativeEncoder::row(&mut obs, Row { kind: RowKind::Global, ..Row::default() }).unwrap();
    encoder.seat_rows[0] = NativeEncoder::row(&mut obs, Row { kind: RowKind::Seat, parent: global, ..Row::default() }).unwrap();
    let option = EventOptionIdentity::neow(0);
    for value in [150.0 + f64::from(0.1f32), 50.0 + f64::from(-14.987654f32),
        150.12345678901234, 35.99999999999999, 164.99999999999997] {
        let row = encoder.event_option(&mut obs, option, option.id,
            Some(EventDescription::JungleSoloGold(value.to_bits())), 0, global, false, None, &[]).unwrap();
        obs.finish(DecisionStatus::Waiting).unwrap();
        let v = obs.view().unwrap(); let i = row.index().unwrap();
        assert_eq!(f64::from(v.rows.integers[i][4]) + f64::from(v.rows.floats[i][10])
            + f64::from(v.rows.floats[i][11]), value);
        assert_ne!(v.rows.flags_present[i] & Flag::Locked.bit(), 0);
        assert_ne!(v.rows.flags[i] & Flag::Locked.bit(), 0);
    }
    let _ = EventParameter::None;
}

#[test]
fn four_seat_large_inventory_stress_never_truncates_or_publishes_partial_state() {
    let specs: Vec<_> = (0..4).map(|seat| PlayerSpec { net_id: 10 + seat, character: Character::Ironclad }).collect();
    let mut run = MpRun::new("NATIVE-CAPACITY", &specs, 0).unwrap();
    for owner in 0..4 {
        let player = run.roster.player_mut(owner).unwrap(); player.deck.clear();
        for index in 0..128 {
            let mut card = Card::from_factory(CardId::StrikeIronclad, false, 1000 + (owner * 128 + index) as u32).unwrap();
            card.owner = owner as u8; card.enchantments.push((EnchantmentId::TezcatarasEmber, 1));
            player.deck.push(card);
        }
    }
    let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV1::new();
    let result = encoder.encode_multiplayer(&run, decision(0), 10, &mut obs);
    match result {
        Ok(()) => assert!(obs.lengths().0 > 1024),
        Err(EncodeError::Contract(ContractError::Capacity { buffer: BufferKind::Rows, needed, capacity })) => {
            assert_eq!(needed, capacity + 1); assert!(obs.view().is_err());
        }
        other => panic!("unexpected stress result: {other:?}"),
    }
}
