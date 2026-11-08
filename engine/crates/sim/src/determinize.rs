//! Decision-time search determinization: replace every piece of combat state the
//! player cannot observe with a fresh sample derived only from `seed`.
//!
//! Visible in the original game (and therefore kept): the viewer's hand, every seat's
//! discard, exhaust and play piles (face up: cards reach them in view), every creature's
//! HP/block/powers, enemy intents (the move already selected by
//! `MonsterMoveStateMachine`), orbs, potions, relics, energy, the viewer's draw pile
//! *contents* (`NDeckViewScreen` lists the draw pile sorted, never in draw order) and
//! the act map.
//!
//! A teammate's hand is not visible: `NMultiplayerPlayerState.RefreshCombatValues` shows
//! only its size (`Hand.Cards.Count`), and the expanded teammate view
//! (`NMultiplayerPlayerExpandedState`) lists the master deck only. Their hand and the
//! unknown part of their draw pile are therefore one hidden pool, dealt back at random
//! with the hand's size (kept).
//!
//! A teammate's card choice is not visible either: `CardSelectCmd` opens the selection
//! screen only for the choosing player (`ShouldSelectLocalCard` is `LocalContext.IsMe`)
//! and every other client just awaits `WaitForRemoteChoice`. Its options, and the hand
//! and draw cards it references, cannot be re-sampled consistently, so no sample is
//! drawn while a teammate resolves (or has queued) one: search refuses and the plain
//! policy decides.
//!
//! Re-sampled: the unknown part of every seat's draw pile *order*, and every RNG
//! stream, because each one decides a future the player has not seen yet (C#
//! `RunRngSet` and `PlayerRngSet` streams: shuffles, generated cards/potions, random
//! targets and costs, monster move rolls, reward generation, ...), plus each
//! monster's own move-roll RNG and the combat RNG.
//!
//! Known draw positions stay (`PlayerCombatState::draw_known`): cards put on the top
//! or bottom by an effect the viewer watched (C# `CardPileCmd.Add(.., PileType.Draw,
//! CardPilePosition.Top)` in Headbutt.cs, ThinkingAhead.cs, Glimmer.cs, and the
//! Nostalgia/Rebound play-result location), minus those drawn since. A teammate's
//! card put there from their hand is known only to them (its identity never showed).
//! The unknown middle is first put into arena order (`KRef`, assigned when a card
//! enters the combat, independent of any shuffle) so the sample is a function of the
//! public state and `seed` only.
use crate::combat::{Combat, RunRngs};
use crate::player::DrawKnowledge;
use crate::rng::Rng;

/// A fresh, named stream of the per-sample seed, independent of the stream it replaces.
pub fn stream(seed: u64, name: &str) -> Rng {
    Rng::named(seed, name)
}

pub fn run_rngs(rngs: &mut RunRngs, seed: u64) {
    *rngs = RunRngs {
        seed,
        up_front: stream(seed, "up_front"),
        shuffle: stream(seed, "shuffle"),
        unknown_map_point: stream(seed, "unknown_map_point"),
        combat_card_generation: stream(seed, "combat_card_generation"),
        combat_potion_generation: stream(seed, "combat_potion_generation"),
        combat_card_selection: stream(seed, "combat_card_selection"),
        combat_energy_costs: stream(seed, "combat_energy_costs"),
        combat_targets: stream(seed, "combat_targets"),
        monster_ai: stream(seed, "monster_ai"),
        niche: stream(seed, "niche"),
        combat_orbs: stream(seed, "combat_orbs"),
        treasure_room: stream(seed, "treasure_room_relics"),
        rewards: stream(seed, "rewards"),
        shops: stream(seed, "shops"),
        transformations: stream(seed, "transformations"),
    };
}

/// Shuffle a pile whose order is hidden, starting from its seed-independent arena order.
pub fn hidden_order<T: Ord>(items: &mut [T], rng: &mut Rng) {
    items.sort_unstable();
    rng.shuffle(items);
}

impl Combat {
    /// See the module documentation for exactly what is replaced; `viewer` is the seat
    /// whose knowledge the sample keeps. Refused, with nothing changed, while another
    /// seat resolves or has queued a card choice.
    pub fn determinize_hidden(&mut self, seed: u64, viewer: usize) -> Result<(), &'static str> {
        let choosing = |seat: usize| self.pending_choice_for_player(seat as u8).is_some()
            || self.deferred_setup_choices.iter().any(|(owner, _, _)| usize::from(*owner) == seat);
        if (0..self.players.len()).any(|seat| seat != viewer && choosing(seat)) {
            return Err("a teammate is resolving a card choice; its options and hand are hidden from this seat");
        }
        self.rng = stream(seed, "combat");
        run_rngs(&mut self.run.rngs, seed);
        for (index, creature) in self.creatures.iter_mut().enumerate() {
            if let Some(ai) = &mut creature.ai {
                ai.rng = stream(seed.wrapping_add(index as u64), "monster_move");
            }
        }
        for (seat, player) in self.players.0.iter_mut().enumerate() {
            let seat_seed = seed.wrapping_add(seat as u64);
            if player.rewards_rng.is_some() {
                player.rewards_rng = Some(stream(seat_seed, "rewards"));
            }
            let (top, bottom) = player.draw_known.current(&player.draw);
            let seen = |flags: &[bool]| if seat == viewer { flags.len() } else { flags.iter().take_while(|&&p| p).count() };
            let (top, bottom) = (top[..seen(top)].to_vec(), bottom[..seen(bottom)].to_vec());
            let unknown = top.len()..player.draw.len() - bottom.len();
            // A teammate's hand is hidden: its cards and the unknown draw cards are one pool,
            // dealt back with the hand's size.
            if seat != viewer {
                let mut pool: Vec<_> = player.hand.iter().chain(&player.draw[unknown.clone()]).copied().collect();
                hidden_order(&mut pool, &mut stream(seat_seed, "teammate_hand"));
                let (hand, rest) = pool.split_at(player.hand.len());
                player.hand = hand.iter().copied().collect();
                player.draw[unknown].copy_from_slice(rest);
            } else if !unknown.is_empty() {
                hidden_order(&mut player.draw[unknown], &mut stream(seat_seed, "draw_order"));
            }
            player.draw_known = DrawKnowledge::known(&top, &bottom, &player.draw);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::card::KRef;
    use crate::combat::{Combat, PileKind, PilePos};
    use crate::run::{RunManager, RunOptions};
    use crate::tables::CardId;

    /// Two seats, each with a 12-card draw pile in a fixed order.
    fn combat() -> Combat {
        let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run, 1);
        c.add_player(50, 50, 3, 0, &[]);
        c.setup_encounter("TUNNELER_WEAK", &mut crate::rng::Rng::new(1));
        for seat in 0..2u8 {
            c.players[seat].draw.clear();
            c.players[seat].hand.clear();
            c.players[seat].discard.clear();
            for _ in 0..12 { let k = c.new_card(seat, CardId::StrikeIronclad, false).unwrap(); c.players[seat].draw.push(k); }
        }
        c
    }
    fn discard(c: &mut Combat, seat: u8) -> KRef {
        let k = c.new_card(seat, CardId::Bash, false).unwrap();
        c.players[seat].discard.push(k);
        k
    }
    fn sample(c: &Combat, seed: u64, viewer: usize) -> Vec<Vec<KRef>> {
        let mut c = c.clone();
        c.determinize_hidden(seed, viewer).unwrap();
        (0..2u8).map(|s| c.players[s].draw.clone()).collect()
    }

    #[test]
    fn no_sample_is_drawn_while_a_teammate_resolves_a_card_choice() {
        use crate::choice::{ChoiceSrc, Resolver};
        for parked in [false, true] {
            let mut c = combat();
            for seat in 0..2u8 { c.draw_cards(seat, 5, false); }
            let (a, b) = (c.players[1].hand[0], c.players[1].hand[1]);
            let brand = c.new_card(1, CardId::Brand, false).unwrap();
            c.request_card_choice_for_player(1, &[a, b], PileKind::Hand,
                Resolver { src: ChoiceSrc::Card(CardId::Brand), k: brand, target: None, step: 0, arg: 0 }, false);
            assert_eq!(c.choice_options_for_player(1), Some(&[a, b][..]));
            // Suspended: the other seat acts while the choice waits.
            if parked { c.park_live_choice(); }
            let mut refused = c.clone();
            assert!(refused.determinize_hidden(3, 0).is_err(), "seat 0 must not see seat 1's choice or hand");
            assert_eq!(format!("{refused:?}"), format!("{c:?}"), "a refused sample changes nothing");
            let mut chooser = c.clone();
            chooser.determinize_hidden(3, 1).unwrap();
            assert_eq!(chooser.players[1].hand.to_vec(), c.players[1].hand.to_vec(), "the chooser sees its own hand");
            assert_eq!(chooser.choice_options_for_player(1), Some(&[a, b][..]));
        }
    }

    #[test]
    fn put_on_top_positions_survive_and_only_the_rest_is_reshuffled() {
        let mut c = combat();
        // Headbutt: a card from the discard pile onto the top (Headbutt.cs:32).
        let headbutt = discard(&mut c, 0);
        c.players[0].discard.retain(|&q| q != headbutt);
        c.insert_pile_from(headbutt, PileKind::Discard, PileKind::Draw, PilePos::Top);
        let first = sample(&c, 1, 0);
        assert!((1..20).all(|seed| sample(&c, seed, 0)[0][0] == headbutt), "the known top card stays on top");
        assert!((2..20).any(|seed| sample(&c, seed, 0)[0][1..] != first[0][1..]), "the unknown rest is re-sampled");
        // A second card on top, then one draw: the Headbutt card is on top again and still known.
        // Thinking Ahead: a card from the hand onto the top (ThinkingAhead.cs:29).
        let thinking = c.new_card(0, CardId::Bash, false).unwrap();
        c.insert_pile(thinking, PileKind::Draw, PilePos::Top);
        assert!((1..20).all(|seed| sample(&c, seed, 0)[0][..2] == [thinking, headbutt]));
        c.draw_cards(0, 1, false);
        assert!((1..20).all(|seed| sample(&c, seed, 0)[0][0] == headbutt));
        // Generated onto the bottom: that position is known too.
        let wound = c.new_card(0, CardId::Wound, false).unwrap();
        c.add_generated_card(wound, PileKind::Draw, PilePos::Bottom, true);
        assert!((1..20).all(|seed| { let s = sample(&c, seed, 0); s[0][0] == headbutt && *s[0].last().unwrap() == wound }));
    }

    #[test]
    fn hidden_order_changes_alone_never_change_the_sample() {
        let mut c = combat();
        let headbutt = discard(&mut c, 0);
        c.players[0].discard.retain(|&q| q != headbutt);
        c.insert_pile_from(headbutt, PileKind::Discard, PileKind::Draw, PilePos::Top);
        // Another history: the same public events after a different hidden shuffle.
        let mut other = combat();
        let k = discard(&mut other, 0);
        assert_eq!(k, headbutt);
        other.players[0].draw.reverse();
        other.players[1].draw.reverse();
        other.players[0].discard.retain(|&q| q != k);
        other.insert_pile_from(k, PileKind::Discard, PileKind::Draw, PilePos::Top);
        assert_ne!(c.players[0].draw, other.players[0].draw);
        for seed in 0..10 { assert_eq!(sample(&c, seed, 0), sample(&other, seed, 0)); }
    }

    #[test]
    fn a_teammates_hand_is_hidden_and_only_its_size_is_kept() {
        let mut c = combat();
        for seat in 0..2u8 { c.draw_cards(seat, 5, false); }
        let sampled = |c: &Combat, seed: u64, viewer: usize| { let mut c = c.clone(); c.determinize_hidden(seed, viewer).unwrap(); c };
        let pool = |c: &Combat, seat: u8| { let mut v: Vec<KRef> = c.players[seat].hand.iter().chain(&c.players[seat].draw).copied().collect(); v.sort(); v };
        let (mine, theirs) = (c.players[0].hand.to_vec(), c.players[1].hand.to_vec());
        for seed in 1..20 {
            let s = sampled(&c, seed, 0);
            assert_eq!(s.players[0].hand.to_vec(), mine, "the viewer's own hand is kept");
            assert_eq!(s.players[1].hand.len(), theirs.len(), "a teammate's hand size is visible");
            assert_eq!(pool(&s, 1), pool(&c, 1), "dealt from the teammate's hand and draw pile only");
            assert_eq!(sampled(&c, seed, 1).players[1].hand.to_vec(), theirs, "the teammate keeps their own hand");
        }
        assert!((1..20).any(|seed| sampled(&c, seed, 0).players[1].hand.to_vec() != theirs), "re-dealt");
        // Which of the teammate's cards are in hand is hidden: a swap with the draw pile changes nothing.
        let mut other = c.clone();
        let (h, d) = (other.players[1].hand[0], other.players[1].draw[0]);
        (other.players[1].hand[0], other.players[1].draw[0]) = (d, h);
        for seed in 0..10 {
            assert_eq!(format!("{:?}", sampled(&c, seed, 0).players), format!("{:?}", sampled(&other, seed, 0).players));
        }
    }

    #[test]
    fn unwatched_or_voided_positions_are_reshuffled() {
        // A teammate's card from their hand: known to them, not to another seat.
        let mut c = combat();
        let private = c.players[1].draw[3];
        c.players[1].draw.remove(3);
        c.insert_pile(private, PileKind::Draw, PilePos::Top);
        assert!((1..20).all(|seed| sample(&c, seed, 1)[1][0] == private));
        assert!((1..20).any(|seed| sample(&c, seed, 0)[1][0] != private));
        // A public one (from the discard pile) is known to every seat.
        let public = discard(&mut c, 1);
        c.players[1].discard.retain(|&q| q != public);
        c.insert_pile_from(public, PileKind::Discard, PileKind::Draw, PilePos::Top);
        assert!((1..20).all(|seed| sample(&c, seed, 0)[1][0] == public));
        // A random insert or a shuffle voids what was known.
        let mut random = c.clone();
        let dazed = random.new_card(1, CardId::Dazed, false).unwrap();
        random.add_generated_card(dazed, PileKind::Draw, PilePos::Random, false);
        assert!((1..20).any(|seed| sample(&random, seed, 1)[1][0] != public));
        let mut shuffled = c.clone();
        shuffled.shuffle_piles(1);
        assert!((1..20).any(|seed| sample(&shuffled, seed, 1)[1][0] != public));
        // A removal from the middle the tracking does not follow also voids it.
        let mut removed = c.clone();
        removed.players[1].draw.remove(6);
        assert!((1..20).any(|seed| sample(&removed, seed, 1)[1][0] != public));
    }
}
