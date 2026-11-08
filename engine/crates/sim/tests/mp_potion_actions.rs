use sts2_sim::character::Character;
use sts2_sim::{
    combat::Combat,
    obs::{decode_combat_action_for_player, R7_POTION_ACTION_START},
    rng::Rng,
    run::{RunManager, RunOptions},
    tables::{CardId, PotionId, PowerId},
};

#[test]
fn entropic_brew_fills_fixed_capacity_even_when_seat_zero_storage_is_short() {
    for recipient in 0..4 {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        run.all_cards_unlocked = Some(true);
        run.max_potion_slots = 3;
        run.potions = vec![None]; // lazy storage after consuming the sole potion
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        for p in 0..4 {
            c.players[p].character = Some(Character::Ironclad);
            if p != 0 { c.players[p].potions = Some(vec![None; 3]); }
        }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        c.start_combat();
        c.potion_effect_for_player((recipient + 1) % 4, PotionId::Entropicbrew, Some(recipient));
        let slots = c.potion_slots_for_player(recipient).unwrap();
        assert_eq!(slots.len(), 3);
        assert!(slots.iter().all(Option::is_some));
        assert_eq!(c.fault, None);
    }
}

#[test]
fn stable_serum_retains_only_its_recipient_hand_in_every_seat() {
    for recipient in [1, 0, 2, 3] {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        c.start_combat();
        let mut original_hand = Vec::new();
        for p in 0..4 {
            let k = c.new_card(p, CardId::DefendIronclad, false).unwrap();
            c.players[p].hand.push(k);
            original_hand.push(k);
            // Keep the next turn's draw from recycling the flushed card.
            for _ in 0..5 {
                let k = c.new_card(p, CardId::StrikeIronclad, false).unwrap();
                c.players[p].draw.push(k);
            }
        }
        let owner = (recipient + 1) % 4;
        c.potion_effect_for_player(owner, PotionId::Stableserum, Some(recipient));
        c.end_player_turn();
        eprintln!("flush probe potion=StableSerum owner={owner} target={recipient} power=RetainHand fault={:?}", c.fault);
        assert_eq!(c.fault, None);
        for p in 0..4 {
            assert_eq!(c.players[p].hand.contains(&original_hand[usize::from(p)]), p == recipient);
        }
    }
}

#[test]
fn final_five_missing_potions_keep_target_card_and_orb_ownership() {
    // C# CosmicConcoction(3 upgraded), CunningPotion(3 upgraded Shivs),
    // PotOfGhouls(2 Souls), EssenceOfDarkness(capacity Dark), PotionOfCapacity(+2).
    for owner in 0..4 {
        for id in [PotionId::Cosmicconcoction, PotionId::Cunningpotion,
            PotionId::Potofghouls, PotionId::Essenceofdarkness, PotionId::Potionofcapacity]
        {
            let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
            run.deck.clear();
            run.all_cards_unlocked = Some(true);
            let mut c = Combat::new(run, 1);
            for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
            c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
            c.start_combat();
            for p in 0..4 { c.players[p].orbs.capacity = 3; }
            let ally = (owner + 1) % 4;
            c.potion_effect_for_player(owner, id, Some(ally));
            assert_eq!(c.fault, None, "{id:?} owner {owner}");
            match id {
                PotionId::Essenceofdarkness => assert_eq!(c.count_orbs(ally, sts2_sim::orbs::OrbKind::Dark), 3),
                PotionId::Potionofcapacity => assert_eq!(c.players[ally].orbs.capacity, 5),
                _ => {
                    let expected = if id == PotionId::Potofghouls { 2 } else { 3 };
                    assert_eq!(c.players[ally].hand.len(), expected);
                    for &k in &c.players[ally].hand {
                        assert_eq!(c.cards[k as usize].owner, ally);
                        if id != PotionId::Potofghouls { assert!(c.cards[k as usize].upgraded); }
                        if id == PotionId::Cunningpotion { assert_eq!(c.cards[k as usize].id, CardId::Shiv); }
                        if id == PotionId::Potofghouls { assert_eq!(c.cards[k as usize].id, CardId::Soul); }
                    }
                }
            }
            for p in 0..4 {
                if p != ally {
                    assert!(c.players[p].hand.is_empty());
                    assert_eq!(c.players[p].orbs.capacity, 3);
                    assert!(c.players[p].orbs.orbs.is_empty());
                }
            }
        }
    }
}

#[test]
fn second_missing_potion_group_resolves_in_each_seat() {
    // C# GhostInAJar(1), KingsCourage(15), PotionOfDoom(33);
    // DeprecatedPotion has no OnUse override.
    for owner in 0..4 {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        c.start_combat();
        let ally = (owner + 1) % 4;
        let enemy = c.enemies[0];
        let blade = c.new_card(ally, CardId::SovereignBlade, false).unwrap();
        let before = c.cards[blade as usize].damage.unwrap();
        c.players[ally].hand.push(blade);
        c.potion_effect_for_player(owner, PotionId::Ghostinajar, Some(ally));
        assert_eq!(c.power_amount(ally, PowerId::Intangible), 1);
        c.potion_effect_for_player(owner, PotionId::Kingscourage, Some(ally));
        assert_eq!(c.cards[blade as usize].damage, Some(before + 15));
        c.potion_effect_for_player(owner, PotionId::Potionofdoom, Some(enemy));
        assert_eq!(c.power_amount(enemy, PowerId::Doom), 33);
        let hp = c.cr(enemy).hp;
        c.potion_effect_for_player(owner, PotionId::Deprecatedpotion, Some(enemy));
        assert_eq!(c.cr(enemy).hp, hp);
        assert_eq!(c.fault, None);
        for p in 0..4 {
            if p != ally { assert_eq!(c.power_amount(p, PowerId::Intangible), 0); }
        }
    }
}

#[test]
fn first_missing_potion_group_resolves_in_each_seat() {
    for owner in 0..4 {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        c.start_combat();
        let ally = (owner + 1) % 4;
        let enemy = c.enemies[0];
        c.potion_effect_for_player(owner, PotionId::Bonebrew, Some(ally));
        assert!(c.allies.iter().any(|&p| c.cr(p).pet_owner == Some(ally) && c.cr(p).hp == 15));
        c.potion_effect_for_player(owner, PotionId::Focuspotion, Some(ally));
        assert_eq!(c.power_amount(ally, PowerId::Focus), 2);
        c.potion_effect_for_player(owner, PotionId::Poisonpotion, Some(enemy));
        assert_eq!(c.power_amount(enemy, PowerId::Poison), 6);
        c.potion_effect_for_player(owner, PotionId::Starpotion, Some(ally));
        assert_eq!(c.players[ally].stars, 3);
        assert_eq!(c.fault, None);
    }
}

#[test]
fn remaining_potions_follow_owner_and_target_in_every_seat() {
    // C# Models/Potions OnUse: counts and amounts are canonical, not r7 output.
    for owner in 0..4 {
        for id in [
            PotionId::Bottledpotential, PotionId::Distilledchaos, PotionId::Entropicbrew,
            PotionId::Fairyinabottle, PotionId::Fruitjuice, PotionId::Orobicacid,
            PotionId::Shacklingpotion, PotionId::Sneckooil, PotionId::Soldiersstew,
            PotionId::Ambergris, PotionId::Foulpotion, PotionId::Glowwaterpotion,
            PotionId::Potionshapedrock,
        ] {
            let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
            run.deck.clear();
            run.all_cards_unlocked = Some(true);
            run.max_potion_slots = 3;
            run.potions = vec![None; 3];
            let mut c = Combat::new(run, 1);
            for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
            for p in 0..4 {
                c.players[p].character = Some(Character::Ironclad);
                if p != 0 { c.players[p].potions = Some(vec![None; 3]); }
            }
            c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
            c.start_combat();
            let ally = (owner + 1) % 4;
            for p in 0..4 {
                c.cr_mut(p).max_hp = 50;
                c.cr_mut(p).hp = if id == PotionId::Foulpotion { 30 } else { 10 };
                let k = c.new_card(p, CardId::StrikeIronclad, false).unwrap();
                c.players[p].hand.push(k);
                for _ in 0..10 {
                    let k = c.new_card(p, CardId::StrikeIronclad, false).unwrap();
                    c.players[p].draw.push(k);
                }
            }
            let enemy = c.enemies[0];
            let hp = c.cr(enemy).hp;
            let target = match id {
                PotionId::Foulpotion | PotionId::Shacklingpotion => None,
                PotionId::Potionshapedrock => Some(enemy),
                PotionId::Fairyinabottle => Some(owner),
                _ => Some(ally),
            };
            c.potion_effect_for_player(owner, id, target);
            if id == PotionId::Ambergris {
                assert_eq!(c.fault, None);
                assert_eq!(c.cr(ally).hp, 35);
                assert_eq!(c.power_amount(ally, PowerId::Ambergris), 1);
                continue;
            }
            assert_eq!(c.fault, None, "{id:?} owner {owner}");
            match id {
                PotionId::Bottledpotential => assert_eq!(c.players[ally].hand.len(), 5),
                PotionId::Distilledchaos => assert_eq!(c.players[ally].draw.len(), 7),
                PotionId::Entropicbrew => assert!(c.potion_slots_for_player(ally).unwrap().iter().all(Option::is_some)),
                PotionId::Fairyinabottle => assert_eq!(c.cr(owner).hp, 25),
                PotionId::Fruitjuice => assert_eq!((c.cr(ally).hp, c.cr(ally).max_hp), (15, 55)),
                PotionId::Orobicacid => {
                    assert_eq!(c.players[ally].hand.len(), 4);
                    assert_eq!(c.players[owner].hand.len(), 1);
                    assert!(c.players[ally].hand.iter().all(|&k| c.cards[k as usize].owner == ally));
                }
                PotionId::Shacklingpotion => assert_eq!(c.power_amount(enemy, PowerId::ShacklingPotion), 7),
                PotionId::Sneckooil => assert_eq!(c.run.rngs.combat_energy_costs.counter, 8),
                PotionId::Soldiersstew => assert!(c.players[ally].hand.iter()
                    .chain(c.players[ally].draw.iter()).all(|&k| c.cards[k as usize].replay == 1)),
                PotionId::Foulpotion => assert_eq!(c.cr(enemy).hp, hp - 12),
                PotionId::Glowwaterpotion => {
                    assert_eq!(c.players[ally].hand.len(), 10);
                    assert_eq!(c.players[ally].exhaust.len(), 1);
                }
                PotionId::Potionshapedrock => assert_eq!(c.cr(enemy).hp, hp - 15),
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn second_fifteen_potions_use_the_chosen_seat_for_every_owner() {
    // CanonicalVars and OnUse in the named C# Models/Potions files.
    let powers = [
        (PotionId::Duplicator, PowerId::Duplication, 1),
        (PotionId::Heartofiron, PowerId::Plating, 7),
        (PotionId::Liquidbronze, PowerId::Thorns, 3),
        (PotionId::Powdereddemise, PowerId::Demise, 9),
        (PotionId::Radianttincture, PowerId::Radiance, 3),
        (PotionId::Regenpotion, PowerId::Regen, 5),
        (PotionId::Stableserum, PowerId::RetainHand, 2),
        (PotionId::Beetlejuice, PowerId::Shrink, 4),
        (PotionId::Gigantificationpotion, PowerId::Gigantification, 1),
        (PotionId::Luckytonic, PowerId::Buffer, 1),
        (PotionId::Mazalethsgift, PowerId::Ritual, 1),
        (PotionId::Shipinabottle, PowerId::BlockNextTurn, 10),
    ];
    for owner in 0..4 {
        for (id, power, amount) in powers.into_iter().chain([
            (PotionId::Fortifier, PowerId::Dexterity, 0),
            (PotionId::Fyshoil, PowerId::Strength, 1),
            (PotionId::Potionofbinding, PowerId::Weak, 1),
        ]) {
            let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
            run.deck.clear();
            let mut c = Combat::new(run, 1);
            for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
            c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
            c.start_combat();
            let ally = (owner + 1) % 4;
            let enemy = c.enemies[0];
            let target = match id {
                PotionId::Powdereddemise | PotionId::Beetlejuice => Some(enemy),
                PotionId::Potionofbinding => None,
                _ => Some(ally),
            };
            let receiver = target.unwrap_or(enemy);
            c.cr_mut(ally).block = 4;
            let energy = c.players[ally].energy;
            c.potion_effect_for_player(owner, id, target);
            assert_eq!(c.fault, None, "{id:?} owner {owner}");
            assert_eq!(c.power_amount(receiver, power), amount, "{id:?} owner {owner}");
            match id {
                PotionId::Fortifier => assert_eq!(c.cr(ally).block, 12),
                PotionId::Fyshoil => assert_eq!(c.power_amount(ally, PowerId::Dexterity), 1),
                PotionId::Potionofbinding => assert_eq!(c.power_amount(enemy, PowerId::Vulnerable), 1),
                PotionId::Radianttincture => assert_eq!(c.players[ally].energy, energy + 1),
                PotionId::Shipinabottle => assert_eq!(c.cr(ally).block, 14),
                _ => {}
            }
            for p in 0..4 {
                if p != receiver { assert_eq!(c.power_amount(p, power), 0); }
            }
        }
    }
}

#[test]
fn first_fifteen_potions_separate_owner_and_recipient_in_all_four_seats() {
    // Values and targeting: Models/Potions/{Block,Blood,Dexterity,Energy,
    // Fire,Flex,Speed,Strength,Swift,Vulnerable,Weak}Potion.cs,
    // ExplosiveAmpoule.cs, BlessingOfTheForge.cs, Clarity.cs, CureAll.cs.
    let ids = [
        PotionId::Blockpotion, PotionId::Bloodpotion, PotionId::Dexteritypotion,
        PotionId::Energypotion, PotionId::Explosiveampoule, PotionId::Firepotion,
        PotionId::Flexpotion, PotionId::Speedpotion, PotionId::Strengthpotion,
        PotionId::Swiftpotion, PotionId::Vulnerablepotion, PotionId::Weakpotion,
        PotionId::Blessingoftheforge, PotionId::Clarity, PotionId::Cureall,
    ];
    for owner in 0..4 {
        for id in ids {
            let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
            run.deck.clear();
            run.potions = vec![Some(id)];
            let mut c = Combat::new(run, 1);
            for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
            for p in 1..4 { c.players[p].potions = Some(vec![Some(id)]); }
            c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
            c.start_combat();
            let recipient = (owner + 1) % 4;
            for p in 0..4 {
                c.cr_mut(p).max_hp = 50;
                c.cr_mut(p).hp = 10;
                let k = c.new_card(p, CardId::DefendIronclad, false).unwrap();
                c.players[p].hand.push(k);
                for _ in 0..3 {
                    let k = c.new_card(p, CardId::StrikeIronclad, false).unwrap();
                    c.players[p].draw.push(k);
                }
            }
            let enemy = c.enemies[0];
            let hp = c.cr(enemy).hp;
            let target = match id {
                PotionId::Explosiveampoule => None,
                PotionId::Firepotion | PotionId::Vulnerablepotion | PotionId::Weakpotion => Some(0),
                _ => Some(usize::from(recipient)),
            };
            let energy = c.players[recipient].energy;
            assert!(c.use_potion_for_player(owner, 0, target), "{id:?} owner {owner}");
            assert_eq!(c.fault, None, "{id:?} owner {owner}");
            match id {
                PotionId::Blockpotion => assert_eq!(c.cr(recipient).block, 12),
                PotionId::Bloodpotion => assert_eq!(c.cr(recipient).hp, 20),
                PotionId::Energypotion => assert_eq!(c.players[recipient].energy, energy + 2),
                PotionId::Explosiveampoule => assert_eq!(c.cr(enemy).hp, hp - 10),
                PotionId::Firepotion => assert_eq!(c.cr(enemy).hp, hp - 20),
                PotionId::Swiftpotion => assert_eq!(c.players[recipient].hand.len(), 4),
                PotionId::Blessingoftheforge => assert!(c.players[recipient].hand.iter()
                    .all(|&k| c.cards[k as usize].upgraded)),
                PotionId::Clarity => {
                    assert_eq!(c.players[recipient].hand.len(), 2);
                    assert_eq!(c.power_amount(recipient, PowerId::Clarity), 3);
                }
                PotionId::Cureall => {
                    assert_eq!(c.players[recipient].hand.len(), 3);
                    assert_eq!(c.players[recipient].energy, energy + 1);
                }
                _ => {
                    let (who, power, amount) = match id {
                        PotionId::Dexteritypotion => (recipient, PowerId::Dexterity, 2),
                        PotionId::Flexpotion => (recipient, PowerId::FlexPotion, 5),
                        PotionId::Speedpotion => (recipient, PowerId::SpeedPotion, 5),
                        PotionId::Strengthpotion => (recipient, PowerId::Strength, 2),
                        PotionId::Vulnerablepotion => (enemy, PowerId::Vulnerable, 3),
                        PotionId::Weakpotion => (enemy, PowerId::Weak, 3),
                        _ => unreachable!(),
                    };
                    assert_eq!(c.power_amount(who, power), amount, "{id:?}");
                }
            }
            for p in 0..4 {
                assert_eq!(c.potion_slots_for_player(p).unwrap()[0],
                    if p == owner { None } else { Some(id) });
                if p != recipient {
                    assert_eq!(c.cr(p).block, 0);
                    assert_eq!(c.cr(p).hp, 10);
                    assert_eq!(c.players[p].hand.len(), 1);
                }
            }
        }
    }
}

#[test]
fn potion_inventory_actor_and_ally_selector_are_independent_in_every_seat() {
    // PotionModel.IsValidTarget / Ashwater.OnUse: the user owns the slot,
    // while target.Player owns the hand and the choice.
    for owner in 0..4 {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        run.potions = vec![Some(PotionId::Ashwater)];
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        for p in 1..4 { c.players[p].potions = Some(vec![Some(PotionId::Ashwater)]); }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        c.start_combat();
        let recipient = (owner + 1) % 4;
        let card = c.new_card(recipient, CardId::DefendIronclad, false).unwrap();
        c.players[recipient].hand.push(card);
        let slot = (R7_POTION_ACTION_START + 1 + usize::from(recipient)) as u16;
        let action = decode_combat_action_for_player(slot, &c, owner, &c.enemies, false).unwrap();
        assert_eq!((action.actor, action.target), (owner, Some(recipient)));
        assert!(c.use_potion_for_player(owner, 0, Some(usize::from(recipient))));
        assert!(c.pending_choice_for_player(recipient).is_some());
        for p in 0..4 {
            assert_eq!(c.potion_slots_for_player(p).unwrap()[0],
                if p == owner { None } else { Some(PotionId::Ashwater) });
        }
        assert_eq!(c.fault, None);
    }
}
