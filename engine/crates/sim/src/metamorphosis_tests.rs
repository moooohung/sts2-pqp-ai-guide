use super::*;
use crate::run::{RunManager, RunOptions};

#[test]
fn new_mp_metamorphosis_samples_owners_attack_pool_with_replacement() {
    for character in crate::character::Character::ALL {
        for upgraded in [false, true] {
            let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
            let mut c = Combat::new(run, 1);
            c.run.relics.clear();
            c.add_player(50, 50, 3, 0, &[]);
            c.set_character_pool(1, character).unwrap();
            c.players[1].character = Some(character);
            c.setup_encounter("TUNNELER_WEAK", &mut crate::rng::Rng::new(1));
            c.start_combat();
            c.players[1].hand.clear();
            c.players[1].draw.clear();
            c.players[1].energy = 3;
            let strike = format!("STRIKE_{}", character.name());
            let (_, _, names) = CSHARP_TRANSFORMS.iter()
                .find(|(mp, name, _)| *mp && *name == strike).unwrap();
            let pool: Vec<_> = names.iter().filter_map(|name| CardId::ALL.into_iter()
                .find(|id| id.name().strip_suffix("_CARD").unwrap_or(id.name()) == *name))
                .filter(|id| card_info(*id, false).unwrap().card_type == CardType::Attack)
                .collect();
            let count = if upgraded { 5 } else { 3 };
            let mut rng = c.run.rngs.combat_card_generation.clone();
            let expected: Vec<_> = (0..count).map(|_| pool[rng.choice_index(pool.len())]).collect();
            let other = c.players[0].draw.clone();
            let k = c.new_card(1, CardId::Metamorphosis, upgraded).unwrap();
            let first = c.cards.len();
            c.players[1].hand.push(k);
            assert!(c.play_card(1, 0, None));
            assert!(c.fault.is_none(), "{:?}", c.fault);
            assert_eq!(c.cards[first..].iter().map(|c| c.id).collect::<Vec<_>>(), expected);
            assert!(c.cards[first..].iter().all(|c| c.owner == 1
                && c.cv.combat_star_cost_override == Some(0)));
            assert_eq!(c.players[1].draw.len(), count);
            assert_eq!(c.players[0].draw, other);
            assert_eq!(c.run.rngs.combat_card_generation.counter, rng.counter);
        }
    }
}
