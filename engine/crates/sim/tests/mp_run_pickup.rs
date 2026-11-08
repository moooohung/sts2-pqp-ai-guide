use sts2_sim::{run::{RunManager, RunOptions, mp::{Character, MpRoster, PlayerSpec}}, tables::{CardId, RelicId}};

#[test]
fn whetstone_sorts_mixed_attacks_before_shared_niche_shuffle() {
    use sts2_sim::card::Card;
    let specs = [PlayerSpec { net_id: 1, character: Character::Ironclad },
        PlayerSpec { net_id: 2, character: Character::Ironclad }];
    let mut roster = MpRoster::new("73001", &specs).unwrap();
    let player = roster.player_mut(1).unwrap();
    player.deck = [CardId::StrikeIronclad, CardId::PommelStrike, CardId::Clash, CardId::Bash]
        .into_iter().enumerate().map(|(i, id)| {
            let mut card = Card::from_factory(id, false, 100 + i as u32).unwrap();
            card.owner = 1;
            card
        }).collect();
    let mut shared = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    // ListExtensions.cs sorts ModelId entries before StableShuffle. Deliberately
    // reverse entry order in the deck so a deck-order shuffle cannot pass.
    let mut expected = vec![CardId::Bash, CardId::Clash, CardId::PommelStrike, CardId::StrikeIronclad];
    let mut rng = shared.rngs.niche.clone();
    rng.shuffle(&mut expected);
    roster.obtain_inventory_relic(&mut shared, 1, RelicId::Whetstone).unwrap();
    for card in &roster.player(1).unwrap().deck {
        assert_eq!(card.upgraded, expected[..2].contains(&card.id));
    }
    assert_eq!(shared.rngs.niche, rng);
    assert!(roster.player(0).unwrap().deck.iter().all(|c| !c.upgraded));
}

