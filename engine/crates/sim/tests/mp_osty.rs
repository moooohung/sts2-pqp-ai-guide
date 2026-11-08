use sts2_sim::{
    card::Card,
    combat::Combat,
    rng::Rng,
    run::{RunManager, RunOptions},
    tables::{CardId, PowerId, RelicId, Hook},
};

fn combat(deck_owner: u8, deck: &[Card]) -> Combat {
    let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    run.deck = if deck_owner == 0 { deck.to_vec() } else { Vec::new() };
    let mut c = Combat::new(run, 1);
    for owner in 1..4 {
        c.add_player(50, 50, 3, 0, if owner == deck_owner { deck } else { &[] });
    }
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    c.start_combat();
    c
}

#[test]
fn legion_of_bone_supports_all_four_owners_alongside_both_relic_pet_types() {
    use sts2_sim::{hooks::Ev, relic::Relic};
    for caster in 0..4 {
        let mut c = combat(caster, &[]);
        for owner in 0..4 {
            for id in [RelicId::Byrdpip, RelicId::PaelsLegion] {
                let mut relic = Relic::new(id); relic.owner = owner;
                c.run.relics.push(relic);
            }
        }
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        assert_eq!(c.allies.len(), 8);
        let k = c.new_card(caster, CardId::LegionOfBone, false).unwrap();
        c.players[caster].hand.push(k);
        c.players[caster].energy = 99;
        let index = c.players[caster].hand.iter().position(|&card| card == k).unwrap();
        assert!(c.play_card(caster, index, None));
        assert_eq!(c.allies.len(), 12);
        for owner in 0..4 {
            let pet = c.osty(owner).unwrap();
            assert_eq!(c.cr(pet).hp, 6);
            assert_eq!(c.cr(pet).pet_owner, Some(owner));
            assert_eq!(c.players[owner].pets.len(), 3);
        }
        assert_eq!(c.all_creatures().len(), 4 + 12 + c.enemies.len());
        let side = c.player_side();
        assert_eq!(side.len(), 16);
        assert_eq!(&side[..4], &[0, 1, 2, 3]);
        assert_eq!(&side[4..], c.allies.as_slice());
        assert_eq!(c.fault, None, "caster {caster}");
    }
}

#[test]
fn starting_deck_flatten_listens_to_its_owners_osty_in_every_seat() {
    for owner in 0..4 {
        let deck = [
            Card::from_factory(CardId::Flatten, false, 1).unwrap(),
            Card::from_factory(CardId::SicEm, false, 2).unwrap(),
        ];
        let mut c = combat(owner, &deck);
        c.summon_osty(owner, 5).unwrap();
        let enemy = c.enemies[0];
        assert_eq!(c.osty(enemy), None);
        let flatten = *c.players[owner].hand.iter()
            .find(|&&k| c.cards[k as usize].id == CardId::Flatten).unwrap();
        assert_eq!(c.modified_card_cost(owner, flatten), 2);
        let sic_em = c.players[owner].hand.iter()
            .position(|&k| c.cards[k as usize].id == CardId::SicEm).unwrap();
        assert!(c.play_card(owner, sic_em, Some(0)));
        assert_eq!(c.modified_card_cost(owner, flatten), 0, "Flatten.AfterAttack seat {owner}");
        c.players[owner].energy = 0;
        let index = c.players[owner].hand.iter().position(|&k| k == flatten).unwrap();
        let hp = c.cr(enemy).hp;
        assert!(c.play_card(owner, index, Some(0)));
        assert!(c.cr(enemy).hp < hp);
        assert_eq!(c.fault, None);
    }
}

#[test]
fn victory_clears_each_pet_roster_without_erasing_living_or_dead_arena_pets() {
    let mut c = combat(0, &[]);
    let pets: Vec<_> = (0..4).map(|owner| c.summon_osty(owner, 5 + i32::from(owner)).unwrap()).collect();
    for owner in [1, 3] { c.kill_creature(pets[owner]); }
    for owner in 0..4u8 {
        assert_eq!(c.players[owner].pets, vec![pets[owner as usize]]);
        assert_eq!(c.osty(owner), Some(pets[owner as usize]));
    }
    let hp: Vec<_> = pets.iter().map(|&pet| c.cr(pet).hp).collect();
    for enemy in c.enemies.clone() { c.kill_creature(enemy); }
    assert!(c.won);
    for owner in 0..4u8 {
        let pet = pets[owner as usize];
        assert!(c.players[owner].pets.is_empty());
        assert_eq!(c.osty(owner), None);
        assert!(c.allies.contains(&pet));
        assert_eq!(c.cr(pet).hp, hp[owner as usize]);
        assert_eq!(c.power_amount(pet, PowerId::DieForYou), 1);
    }
    assert_eq!(c.fault, None);
}

#[test]
fn dead_osty_stays_registered_and_revives_for_each_owner_until_a_defeat() {
    let mut c = combat(0, &[]);
    let pets: Vec<_> = (0..4).map(|owner| c.summon_osty(owner, 5).unwrap()).collect();
    for owner in 0..4u8 {
        let pet = pets[owner as usize];
        c.kill_creature(pet);
        assert_eq!(c.osty(owner), Some(pet));
        assert!(c.cr(pet).is_dead());
        assert_eq!(c.summon_osty(owner, 7), Some(pet));
        assert_eq!(c.players[owner].pets, vec![pet]);
        assert_eq!(c.cr(pet).hp, 7);
        assert_eq!(c.cr(pet).max_hp, 7);
    }
    for owner in 0..4 { c.kill_creature(owner); }
    assert!(c.is_over && !c.won);
    for owner in 0..4u8 {
        assert_eq!(c.players[owner].pets, vec![pets[owner as usize]]);
        assert!(c.cr(pets[owner as usize]).is_dead());
    }
    assert_eq!(c.fault, None);
}
