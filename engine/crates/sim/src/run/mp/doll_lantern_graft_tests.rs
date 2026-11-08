use crate::identity::EventId;
use super::*;

fn party(id: crate::identity::EventId) -> MpRun {
    let mut run = MpRun::new("73003", &[
        PlayerSpec { net_id: 7, character: Character::Ironclad },
        PlayerSpec { net_id: 42, character: Character::Silent },
    ], 0).unwrap();
    run.phase = MpPhase::Event;
    run.world.current_room=Some(id.name());
    run.begin_generic_event(id).unwrap();
    run
}

fn pick(run: &mut MpRun, seat: usize, id: &str) {
    let net = run.roster.player(seat).unwrap().net_id;
    let i = run.event_options(net, seat).unwrap().iter().position(|o| *o == id).unwrap();
    run.choose_event_option(net, seat, i).unwrap();
}

#[test]
fn doll_room_owner_choices_damage_and_rng() {
    for (option, damage, count, draws) in [("take_time", 5, 2, 2), ("examine", 15, 3, 2), ("random", 0, 0, 1)] {
        let mut run = party(EventId::DollRoom);
        let hp = run.roster.player(1).unwrap().hp;
        let other = run.roster.player(0).unwrap().hp;
        assert_eq!(run.events[1].as_ref().unwrap().rng.base_seed,
            run.world.run.rngs.seed.wrapping_add(1).wrapping_add(crate::rng::deterministic_hash64("DOLL_ROOM")));
        assert!(run.choose_event_option(7, 1, 0).is_err());
        pick(&mut run, 1, option);
        assert_eq!(run.roster.player(1).unwrap().hp, hp - damage);
        if count > 0 {
            assert_eq!(run.events[1].as_ref().unwrap().rng.counter, draws);
            assert_eq!(run.event_options(42, 1).unwrap().len(), count);
            pick(&mut run, 1, "doll_1");
        }
        let dolls = run.roster.player(1).unwrap().relics.iter().filter(|r|
            matches!(r.id, crate::tables::RelicId::BingBong | crate::tables::RelicId::DaughterOfTheWind | crate::tables::RelicId::MrStruggles)).count();
        assert_eq!(dolls, 1);
        assert!(run.event_done[1]);
        assert!(!run.event_done[0]);
        assert_eq!(run.roster.player(0).unwrap().hp, other);
        assert_eq!(run.events[0].as_ref().unwrap().rng.counter, 0);
    }
}

#[test]
fn spirit_grafter_heals_adds_owned_card_and_upgrades_before_damage() {
    let mut run = party(EventId::SpiritGrafter);
    run.roster.player_mut(1).unwrap().hp = 30;
    pick(&mut run, 1, "let_it_in");
    assert_eq!(run.roster.player(1).unwrap().hp, 55);
    assert!(run.roster.player(1).unwrap().deck.iter().any(|c| c.id == crate::tables::CardId::Metamorphosis && c.owner == 1));
    let mut run = party(EventId::SpiritGrafter);
    let hp = run.roster.player(1).unwrap().hp;
    pick(&mut run, 1, "rejection");
    assert_eq!(run.roster.player(1).unwrap().hp, hp);
    let id = run.event_card_modal(42, 1).unwrap().0[0].instance_id;
    run.select_event_cards(42, 1, &[0]).unwrap();
    assert_eq!(run.roster.player(1).unwrap().hp, hp - 10);
    assert!(run.roster.player(1).unwrap().deck.iter().any(|c| c.instance_id == id && c.upgraded));
    assert!(!run.event_done[0]);
}

#[test]
fn lantern_key_shared_pages_and_every_owner_special_reward() {
    let mut run = party(EventId::TheLanternKey);
    assert_eq!(run.events[0].as_ref().unwrap().rng.base_seed, run.events[1].as_ref().unwrap().rng.base_seed);
    let gold: Vec<_> = run.roster.players().iter().map(|p| p.gold).collect();
    pick(&mut run, 1, "return_key");
    assert_eq!(run.phase,MpPhase::Event);
    pick(&mut run, 0, "return_key");
    for (seat, before) in gold.into_iter().enumerate() {
        assert_eq!(run.roster.player(seat).unwrap().gold, before + 100);
    }
    let mut run = party(EventId::TheLanternKey);
    pick(&mut run, 1, "keep_key");
    pick(&mut run, 0, "keep_key");
    assert_eq!(run.event_options(7, 0).unwrap(), vec!["fight"]);
    pick(&mut run, 0, "fight");
    pick(&mut run, 1, "fight");
    assert_eq!(run.phase, MpPhase::Combat);
    run.debug_win_combat().unwrap();
    run.offer_rewards().unwrap();
    for seat in 0..2 {
        let card = run.rewards.as_ref().unwrap()[seat].special_cards[0].as_ref().unwrap();
        assert_eq!(card.id, crate::tables::CardId::LanternKey);
        assert_eq!(card.owner, seat as u8);
        assert!(run.roster.next_card_id >= card.instance_id);
    }
}
