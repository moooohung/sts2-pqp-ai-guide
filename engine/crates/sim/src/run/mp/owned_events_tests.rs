use crate::identity::EventId;
use super::*;
use crate::tables::{CardId, CardType, VarKey};

fn party(id: crate::identity::EventId) -> MpRun {
    let specs = [
        PlayerSpec { net_id: 7, character: Character::Ironclad },
        PlayerSpec { net_id: 42, character: Character::Silent },
    ];
    let mut run = MpRun::new("73003", &specs, 0).unwrap();
    run.phase = MpPhase::Event;
    run.world.current_room=Some(id.name());
    run.begin_generic_event(id).expect("owned multiplayer event must be enterable");
    run
}

fn choose(run: &mut MpRun, seat: usize, id: &str) {
    let net = run.roster.player(seat).unwrap().net_id;
    let options = run.event_options(net, seat).unwrap();
    run.choose_event_option(net, seat, options.iter().position(|option| *option == id).unwrap()).unwrap();
}

#[test]
fn owned_events_trial_abandonment_forfeits_every_player() {
    let mut run = party(EventId::Trial);
    choose(&mut run, 1, "reject");
    choose(&mut run, 1, "double_down");
    choose(&mut run, 1, "confirm_abandon");
    assert_eq!(run.phase, MpPhase::RunOver);
    assert!(run.roster.players().iter().all(|p| p.hp == 0));
    assert_eq!(run.world.run.hp, 0);
}

#[test]
fn owned_events_tinker_time_mp_keeps_owner_streams_and_card_state_isolated() {
    let mut run = party(EventId::TinkerTime);
    let other = format!("{:?}", run.roster.player(0).unwrap());
    let other_event = format!("{:?}", run.events[0]);
    let niche = run.world.run.rngs.niche.counter;
    choose(&mut run, 1, "choose_card_type");
    let kind = run.event_options(42, 1).unwrap()[0];
    choose(&mut run, 1, kind);
    let rider = run.event_options(42, 1).unwrap()[0];
    choose(&mut run, 1, rider);
    let card = run.roster.player(1).unwrap().deck.last().unwrap();
    assert_eq!(card.id, CardId::MadScience);
    assert_eq!(card.owner, 1);
    assert_eq!(card.card_type, match kind {
        "attack" => CardType::Attack, "skill" => CardType::Skill, "power" => CardType::Power,
        _ => panic!("card type"),
    });
    assert_eq!(card.var(VarKey::Rider), Some(match rider {
        "sapping" => 1, "violence" => 2, "choking" => 3,
        "energized" => 4, "wisdom" => 5, "chaos" => 6,
        "expertise" => 7, "curious" => 8, "improvement" => 9,
        _ => panic!("rider"),
    }));
    assert_eq!(format!("{:?}", run.roster.player(0).unwrap()), other);
    assert_eq!(format!("{:?}", run.events[0]), other_event);
    assert_eq!(run.world.run.rngs.niche.counter, niche);
    assert!(run.event_done[1] && !run.event_done[0]);
    assert_eq!(run.phase, MpPhase::Event);
}

#[test]
fn owned_events_trial_mp_judgments_wait_for_owner_selectors_and_rewards() {
    let base = party(EventId::Trial);
    for page in 2..=4 {
        for verdict in ["guilty", "innocent"] {
            let mut run = base.clone();
            choose(&mut run, 1, "accept");
            let event = run.events[1].as_mut().unwrap();
            event.ints[0] = page;
            event.options.set_page(match page {
                2 => crate::identity::EventPageId::TrialMerchant,
                3 => crate::identity::EventPageId::TrialNoble,
                4 => crate::identity::EventPageId::TrialNondescript,
                _ => unreachable!(),
            }, crate::run::events::EventParameter::None);
            event.options.replace([
                crate::run::events::EventOption::new("guilty", "Guilty", ""),
                crate::run::events::EventOption::new("innocent", "Innocent", ""),
            ]);
            let other = format!("{:?}", run.roster.player(0).unwrap());
            let deck = run.roster.player(1).unwrap().deck.len();
            let gold = run.roster.player(1).unwrap().gold;
            run.roster.player_mut(1).unwrap().hp = 30;
            let rng = run.roster.player(1).unwrap().rngs.rewards.counter;
            choose(&mut run, 1, verdict);
            match (page, verdict) {
                (2, "guilty") => {
                    assert_eq!(run.roster.player(1).unwrap().deck[deck].id, CardId::Regret);
                    assert_eq!(run.roster.player(1).unwrap().relics.len(), base.roster.player(1).unwrap().relics.len() + 2);
                }
                (2, "innocent") | (4, "innocent") => {
                    assert!(!run.event_done[1]);
                    let (_, min, max, skip) = run.event_card_modal(42, 1).unwrap();
                    assert_eq!((min, max, skip), (2, 2, false));
                    run.select_event_cards(42, 1, &[1, 0]).unwrap();
                }
                (3, "guilty") => assert_eq!(run.roster.player(1).unwrap().hp, 40),
                (3, "innocent") => assert_eq!(run.roster.player(1).unwrap().gold, gold + 300),
                (4, "guilty") => {
                    assert_eq!(run.roster.player(1).unwrap().deck[deck].id, CardId::Doubt);
                    assert_eq!(run.roster.player(1).unwrap().rngs.rewards.counter, rng + 12);
                    assert_eq!(run.event_card_reward_queues[1].len(), 2);
                    run.select_event_cards(42, 1, &[]).unwrap();
                    assert!(!run.event_done[1]);
                    run.select_event_cards(42, 1, &[]).unwrap();
                }
                _ => unreachable!(),
            }
            assert!(run.event_done[1] && !run.event_done[0]);
            assert_eq!(format!("{:?}", run.roster.player(0).unwrap()), other);
        }
    }
}
