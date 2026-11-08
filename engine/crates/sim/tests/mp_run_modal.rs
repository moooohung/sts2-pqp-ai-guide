use sts2_sim::identity::EnchantmentId;
use sts2_sim::run::mp::{Character, MpPhase, MpRun, PlayerSpec};

fn run(seats: usize) -> MpRun {
    let specs: Vec<_> = (0..seats).map(|seat| PlayerSpec {
        net_id: 100 + seat as u64, character: Character::Ironclad,
    }).collect();
    let mut run = MpRun::new("73001", &specs, 0).unwrap();
    run.phase = MpPhase::Map;
    run
}


#[test]
fn paels_growth_automatically_selects_only_eligible_card_and_handles_empty_deck() {
    let mut run = run(2);
    run.roster.player_mut(0).unwrap().deck.truncate(1);
    run.obtain_paels_growth(100, 0).unwrap();
    assert!(run.paels_growth_choices[0].is_none());
    assert!(run.roster.player(0).unwrap().deck[0].enchantments.iter().any(|&(name, n)| name == EnchantmentId::Clone && n == 4));
    run.roster.player_mut(1).unwrap().deck.clear();
    run.obtain_paels_growth(101, 1).unwrap();
    assert!(run.paels_growth_choices[1].is_none());
}

#[test]
fn relic_discovery_uses_local_network_identity_and_first_profile_sighting() {
    use sts2_sim::tables::RelicId;
    let mut first = run(2);
    first.local_net_id = 101;
    first.obtain_paels_growth(100, 0).unwrap();
    assert!(first.roster.player(0).unwrap().discovered_relics.is_empty());
    first.obtain_paels_growth(101, 1).unwrap();
    assert_eq!(first.roster.player(1).unwrap().discovered_relics,
        vec![("RELIC".to_owned(), "PAELS_GROWTH".to_owned())]);

    let mut known = run(2);
    known.seen_relics.insert(RelicId::PaelsGrowth);
    known.obtain_paels_growth(100, 0).unwrap();
    assert!(known.roster.player(0).unwrap().discovered_relics.is_empty());
}

#[test]
fn remote_combat_card_selection_requires_matching_network_actor_and_pending_owner() {
    use sts2_sim::{card::Card, run::Coord, tables::{CardId, MapPointType}};
    for owner in 1..4 {
        let mut run = run(4);
        // An all-Survivor deck guarantees an actual one-card discard modal
        // for each remote owner, without injecting a synthetic pending choice.
        for card in &mut run.roster.player_mut(owner).unwrap().deck {
            *card = Card::from_factory(CardId::Survivor, false, card.instance_id).unwrap();
            card.owner = owner as u8;
        }
        let (col, row) = run.world.run.coord.unwrap();
        let source = run.world.map.point(Coord::new(col, row)).unwrap();
        let dest = source.children.iter().map(|&i| &run.world.map.points[i as usize])
            .find(|point| point.point_type == MapPointType::Monster).unwrap().coord;
        for seat in 0..4 { run.vote(100 + seat as u64, seat, dest).unwrap(); }
        run.play_card(100 + owner as u64, owner, 0, None).unwrap();
        let c = run.combat().unwrap();
        assert_eq!(c.pending_choice.as_ref().unwrap().player as usize, owner);
        let choices = c.choice_options.clone();
        assert!(run.resolve_combat_choice(100, owner, Some(0)).is_err());
        assert!(run.resolve_combat_choice(100, 0, Some(0)).is_err());
        assert_eq!(run.combat().unwrap().choice_options, choices);
        assert_eq!(run.combat().unwrap().pending_choice.as_ref().unwrap().player as usize, owner);
        run.resolve_combat_choice(100 + owner as u64, owner, Some(0)).unwrap();
        assert!(run.combat().unwrap().pending_choice.is_none());
    }
}

#[test]
fn shears_removal_screen_is_mandatory_atomic_and_owned_by_each_remote_seat() {
    for owner in 1..4 {
        let mut run = run(4);
        run.obtain_precarious_shears(100 + owner as u64, owner).unwrap();
        let choices = run.shears_choices[owner].clone().unwrap();
        let other = run.roster.player(0).unwrap().deck[0].instance_id;
        let before = format!("{run:?}");
        for (actor, selected) in [
            (100, vec![choices[0], choices[1]]),
            (100 + owner as u64, vec![choices[0]]),
            (100 + owner as u64, vec![choices[0], choices[0]]),
            (100 + owner as u64, vec![choices[0], other]),
        ] {
            assert!(run.select_precarious_shears_cards(actor, owner, &selected).is_err());
            assert_eq!(format!("{run:?}"), before);
        }
        assert!(run.obtain_paels_growth(100 + owner as u64, owner).is_err());
        assert_eq!(run.roster.player(owner).unwrap().hp, 80);
        run.select_precarious_shears_cards(100 + owner as u64, owner, &choices[..2]).unwrap();
        for (seat, p) in run.roster.players().iter().enumerate() {
            assert_eq!(p.hp, if seat == owner { 64 } else { 80 });
            assert_eq!(p.deck.len(), if seat == owner { 8 } else { 10 });
        }
        assert!(run.shears_choices[owner].is_none());
        assert_eq!(run.phase, MpPhase::Map);
    }
}

#[test]
fn shears_auto_selection_handles_small_decks_and_owner_death_without_party_loss() {
    let mut run = run(4);
    run.roster.player_mut(2).unwrap().deck.truncate(1);
    run.roster.player_mut(2).unwrap().hp = 9;
    run.obtain_precarious_shears(102, 2).unwrap();
    assert!(run.shears_choices[2].is_none());
    assert!(run.roster.player(2).unwrap().deck.is_empty());
    assert_eq!(run.roster.player(2).unwrap().hp, 0);
    assert_eq!(run.roster.player(0).unwrap().hp, 80);
    assert_eq!(run.phase, MpPhase::Map);
}

#[test]
fn shears_uses_curse_first_deck_order_excludes_eternal_and_blocks_map_until_resolved() {
    use sts2_sim::{card::Card, run::Coord, tables::CardId};
    let mut run = run(4);
    let mut curse = Card::from_factory(CardId::Decay, false, 1001).unwrap();
    curse.owner = 3;
    let mut eternal = Card::from_factory(CardId::AscendersBane, false, 1002).unwrap();
    eternal.owner = 3;
    run.roster.player_mut(3).unwrap().deck.extend([curse, eternal]);
    run.obtain_precarious_shears(103, 3).unwrap();
    let choices = run.shears_choices[3].as_ref().unwrap();
    assert_eq!(choices[0], 1001);
    assert!(!choices.contains(&1002));
    let (col, row) = run.world.run.coord.unwrap();
    let source = run.world.map.point(Coord::new(col, row)).unwrap();
    let dest = run.world.map.points[source.children[0] as usize].coord;
    assert_eq!(run.vote(100, 0, dest), Err("owner pickup choice pending"));
}


#[test]
fn pandoras_box_preserves_eternal_basics_and_bing_bong_clones_each_replacement_once() {
    use sts2_sim::{relic::Relic, tables::{RelicId, KW_ETERNAL}};
    let mut run = run(4);
    let preserved = run.roster.player(2).unwrap().deck[0].instance_id;
    run.roster.player_mut(2).unwrap().deck[0].keywords |= KW_ETERNAL;
    let niche = run.world.run.rngs.niche.counter;
    assert_eq!(run.obtain_pandoras_box(102, 2).unwrap().len(), 8);
    assert_eq!(run.world.run.rngs.niche.counter, niche + 8);
    assert_eq!(run.roster.player(2).unwrap().deck[0].instance_id, preserved);

    // CardCmd.Transform fires AfterCardChangedPiles for every Deck replacement;
    // BingBong.cs clones it once (clonedBy = BingBong does not re-clone). The
    // preview is CardPileAddResult.cardAdded, i.e. the replacement, not a clone.
    let mut relic = Relic::new(RelicId::BingBong);
    relic.owner = 3;
    run.roster.player_mut(3).unwrap().relics.push(relic);
    let before_len = run.roster.player(3).unwrap().deck.len();
    let niche = run.world.run.rngs.niche.counter;
    let preview = run.obtain_pandoras_box(103, 3).unwrap();
    assert!(!preview.is_empty());
    assert_eq!(run.world.run.rngs.niche.counter, niche + preview.len() as u64);
    let deck = &run.roster.player(3).unwrap().deck;
    assert_eq!(deck.len(), before_len + preview.len());
    for shown in &preview {
        let at = deck.iter().position(|c| c.instance_id == shown.instance_id).unwrap();
        let clone = &deck[at + 1];
        assert_eq!(clone.id, shown.id);
        assert_ne!(clone.instance_id, shown.instance_id);
        assert!(preview.iter().all(|p| p.instance_id != clone.instance_id));
    }
}

#[test]
fn retained_relic_reward_acceptance_and_skip_are_actor_bound_for_seats_one_to_three() {
    use sts2_sim::tables::RelicId;
    for owner in 1..4 {
        let mut run = run(4);
        run.phase = MpPhase::Neow;
        run.relic_rewards[owner] = Some(RelicId::PotionBelt);
        let before = format!("{run:?}");
        assert!(run.select_relic_reward(100, owner, true).is_err());
        assert_eq!(format!("{run:?}"), before);
        run.select_relic_reward(100 + owner as u64, owner, true).unwrap();
        for (seat, player) in run.roster.players().iter().enumerate() {
            assert_eq!(player.max_potion_slots, if seat == owner { 5 } else { 3 });
            assert_eq!(player.relics.len(), if seat == owner { 2 } else { 1 });
        }
        assert!(run.relic_rewards[owner].is_none());
        assert!(run.select_relic_reward(100 + owner as u64, owner, true).is_err());
    }
}

#[test]
fn skipping_relic_screen_does_not_take_another_owners_offer_or_advance_rng() {
    use sts2_sim::tables::RelicId;
    let mut run = run(4);
    run.phase = MpPhase::Neow;
    run.relic_rewards[1] = Some(RelicId::Whetstone);
    run.relic_rewards[2] = Some(RelicId::VenerableTeaSet);
    let rngs: Vec<_> = run.roster.players().iter().map(|p| p.rngs.clone()).collect();
    run.select_relic_reward(101, 1, false).unwrap();
    assert!(run.relic_rewards[1].is_none());
    assert_eq!(run.relic_rewards[2], Some(RelicId::VenerableTeaSet));
    for (seat, player) in run.roster.players().iter().enumerate() {
        assert_eq!(player.relics.len(), 1);
        assert_eq!(player.rngs, rngs[seat]);
    }
}

#[test]
fn small_capsule_pauses_neow_at_the_real_retained_offer_instead_of_auto_obtaining_it() {
    use sts2_sim::tables::RelicId;
    let specs = [Character::Ironclad, Character::Necrobinder, Character::Silent, Character::Defect]
        .into_iter().enumerate().map(|(seat, character)| PlayerSpec {
            net_id: seat as u64 + 1, character,
        }).collect::<Vec<_>>();
    // Original-DLL 024's fixture inputs produce SmallCapsule -> Tea Set.
    let mut run = MpRun::new("73003", &specs, 0).unwrap();
    run.choose_neow(1, 0, 1).unwrap();
    assert_eq!(run.relic_rewards[0], Some(RelicId::VenerableTeaSet));
    assert_eq!(run.neow_finished(0), Some(false));
    assert_eq!(run.roster.player(0).unwrap().relics.len(), 2);
    assert_eq!(run.roster.player(0).unwrap().rngs.rewards.counter, 1);
    assert!(run.world.shared_bags.iter().all(|bag| !bag.contains(&RelicId::VenerableTeaSet)));
    run.select_relic_reward(1, 0, false).unwrap();
    assert_eq!(run.neow_finished(0), Some(true));
    assert_eq!(run.roster.player(0).unwrap().relics.len(), 2);
    assert_eq!(run.roster.player(0).unwrap().rngs.rewards.counter, 1);
}

#[test]
fn remote_neow_card_screens_apply_owner_hooks_and_retain_choices_on_refusal() {
    use sts2_sim::{card::Card, relic::Relic, tables::{CardId, RelicId}};
    for owner in 1..4 {
        let mut run = run(4);
        run.phase = MpPhase::Neow;
        let mut egg = Relic::new(RelicId::MoltenEgg);
        egg.owner = owner as u8;
        run.roster.player_mut(owner).unwrap().relics.push(egg);
        run.roster.next_card_id += 1;
        let mut offered = Card::from_factory(CardId::PommelStrike, false, run.roster.next_card_id).unwrap();
        offered.owner = owner as u8;
        run.neow_cards[owner] = Some(vec![offered.clone()]);
        let rngs: Vec<_> = run.roster.players().iter().map(|p| p.rngs.clone()).collect();
        let before = format!("{run:?}");
        assert!(run.select_neow_card(100, owner, Some(0)).is_err());
        assert!(run.select_neow_card(100 + owner as u64, owner, Some(1)).is_err());
        assert_eq!(format!("{run:?}"), before);
        run.select_neow_card(100 + owner as u64, owner, Some(0)).unwrap();
        let accepted = run.roster.player(owner).unwrap().deck.last().unwrap();
        assert_eq!(accepted.instance_id, offered.instance_id);
        assert!(accepted.upgraded);
        assert_eq!(accepted.floor_added_to_deck, Some(1));
        assert!(run.neow_cards[owner].is_none());
        for (seat, player) in run.roster.players().iter().enumerate() {
            assert_eq!(player.rngs, rngs[seat]);
            assert_eq!(player.deck.len(), if seat == owner { 11 } else { 10 });
        }
        // BingBong.AfterCardChangedPiles clones the accepted Deck card once,
        // after MoltenEgg's TryModifyCardBeingAddedToDeck upgraded it.
        let mut bing_bong = Relic::new(RelicId::BingBong);
        bing_bong.owner = owner as u8;
        run.roster.player_mut(owner).unwrap().relics.push(bing_bong);
        run.roster.next_card_id += 1;
        offered.instance_id = run.roster.next_card_id;
        run.neow_cards[owner] = Some(vec![offered.clone()]);
        run.select_neow_card(100 + owner as u64, owner, Some(0)).unwrap();
        assert!(run.neow_cards[owner].is_none());
        for (seat, player) in run.roster.players().iter().enumerate() {
            assert_eq!(player.deck.len(), if seat == owner { 13 } else { 10 });
        }
        let deck = &run.roster.player(owner).unwrap().deck;
        let (accepted, clone) = (&deck[deck.len() - 2], &deck[deck.len() - 1]);
        assert_eq!(accepted.instance_id, offered.instance_id);
        assert!(accepted.upgraded && clone.upgraded);
        assert_eq!(clone.id, accepted.id);
        assert_ne!(clone.instance_id, accepted.instance_id);
    }
}
