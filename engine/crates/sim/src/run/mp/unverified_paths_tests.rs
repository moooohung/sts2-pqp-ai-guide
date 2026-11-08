use super::*;
use crate::{
    relic::Relic,
    run::mp::{Character, PlayerSpec},
    tables::{CardRarity, CardTag, PotionId, RelicId},
};

fn party() -> MpRun {
    let specs = [
        PlayerSpec { net_id: 7, character: Character::Ironclad },
        PlayerSpec { net_id: 42, character: Character::Ironclad },
    ];
    MpRun::new("73003", &specs, 0).unwrap()
}

fn force_neow(mut run: MpRun, seat: usize, relic: RelicId) -> MpRun {
    run.neow[seat][0] = relic;
    let net = run.roster.player(seat).unwrap().net_id;
    run.choose_neow(net, seat, 0).unwrap();
    run
}

#[test]
fn multiplayer_neow_filters_solo_only_silver_crucible_and_winged_boots() {
    // SilverCrucible.cs:83-86 and WingedBoots.cs:50-53 both require one player;
    // Neow.GenerateInitialOptions removes relics failing IsAllowedAtNeow.
    for seed in 0..4096u64 {
        let offers = crate::run::manager::multiplayer_neow_offers(seed);
        assert!(!offers.contains(&RelicId::SilverCrucible), "seed {seed}");
        assert!(!offers.contains(&RelicId::WingedBoots), "seed {seed}");
    }

    // A corrupted/injected offer must fail closed instead of enabling a path
    // that the original multiplayer game can never generate.
    for relic in [RelicId::SilverCrucible, RelicId::WingedBoots] {
        let mut run = party();
        run.neow[0][0] = relic;
        assert_eq!(run.choose_neow(7, 0, 0), Err("Neow pickup continuation not yet supported"));
        assert!(!run.roster.player(0).unwrap().relics.iter().any(|r| r.id == relic));
    }
}

#[test]
fn immediate_ancient_pickups_mutate_only_the_owner() {
    // NeowsTalisman.cs:18-28 upgrades the owner's last Basic Strike/Defend.
    let run = force_neow(party(), 1, RelicId::NeowsTalisman);
    let owner = run.roster.player(1).unwrap();
    let strike = owner.deck.iter().rposition(|card|
        card.rarity == CardRarity::Basic && card.has_tag(CardTag::Strike)).unwrap();
    let defend = owner.deck.iter().rposition(|card|
        card.rarity == CardRarity::Basic && card.has_tag(CardTag::Defend)).unwrap();
    assert!(owner.deck[strike].upgraded);
    assert!(owner.deck[defend].upgraded);
    assert!(run.roster.player(0).unwrap().deck.iter().all(|card| !card.upgraded));

    // SilkenTress.cs:39-42 loses all of only the owner's gold on pickup.
    let mut run = party();
    run.roster.player_mut(0).unwrap().gold = 111;
    run.roster.player_mut(1).unwrap().gold = 321;
    let run = force_neow(run, 1, RelicId::SilkenTress);
    assert_eq!(run.roster.player(1).unwrap().gold, 0);
    assert_eq!(run.roster.player(0).unwrap().gold, 111);

    // NutritiousOyster.cs:18-20 delegates to GainMaxHp; CreatureCmd.GainMaxHp
    // heals by exactly the actual max-HP increase.
    let mut run = party();
    run.roster.player_mut(1).unwrap().hp = 20;
    let before_max = run.roster.player(1).unwrap().max_hp;
    let run = force_neow(run, 1, RelicId::NutritiousOyster);
    assert_eq!(run.roster.player(1).unwrap().max_hp, before_max + 11);
    assert_eq!(run.roster.player(1).unwrap().hp, 31);
    assert_eq!(run.roster.player(0).unwrap().max_hp, before_max);
}

#[test]
fn lava_rock_and_booming_conch_have_no_pickup_side_effects() {
    // LavaRock and BoomingConch do not override AfterObtained in the original;
    // their effects are boss-reward and elite-combat hooks respectively.
    for relic in [RelicId::LavaRock, RelicId::BoomingConch] {
        let mut run = party();
        let before = {
            let owner = run.roster.player(1).unwrap();
            (owner.gold, owner.hp, owner.max_hp, owner.deck.len())
        };
        run = force_neow(run, 1, relic);
        let owner = run.roster.player(1).unwrap();
        assert_eq!((owner.gold, owner.hp, owner.max_hp, owner.deck.len()), before);
        assert!(owner.relics.iter().any(|owned| owned.id == relic));
    }
}

#[test]
fn elite_entry_uses_shared_elite_cursor_and_booming_conch_owner_hooks() {
    // RoomSet.NextEliteEncounter / MarkVisited share one elite cursor for the run.
    // BoomingConch.cs:27-55 grants only its owner +2 opening draw and +1 energy
    // on that owner's first turn in an Elite room.
    let mut run = party();
    let expected = run.world.encounters.elite[0];
    let second = run.world.encounters.elite[1];
    let mut conch = Relic::new(RelicId::BoomingConch);
    conch.owner = 1;
    run.roster.player_mut(1).unwrap().relics.push(conch);

    run.enter_elite().unwrap();

    assert_eq!(run.world.current_room, Some(expected.name()));
    let combat = run.combat().unwrap();
    assert_eq!(combat.run.room_type, crate::tables::RoomType::Elite);
    assert_eq!(combat.players[1].hand.len(), combat.players[0].hand.len() + 2);
    assert_eq!(combat.players[1].energy, combat.players[0].energy + 1);

    run.enter_elite().unwrap();
    assert_eq!(run.world.current_room, Some(second.name()));
}

#[test]
fn combat_potion_action_is_authenticated_and_consumes_only_owner_slot() {
    // NetUsePotionAction.ToGameAction binds the action to the source Player;
    // UsePotionAction.ExecuteAction resolves PotionIndex on that same Player.
    let mut run = party();
    run.roster.player_mut(1).unwrap().potions[0] = Some(PotionId::Strengthpotion);
    run.enter_monster().unwrap();

    assert!(run.use_potion(7, 1, 0, None).is_err());
    assert_eq!(run.combat().unwrap().potion_slots_for_player(1).unwrap()[0],
        Some(PotionId::Strengthpotion));

    run.use_potion(42, 1, 0, None).unwrap();
    assert_eq!(run.combat().unwrap().potion_slots_for_player(1).unwrap()[0], None);
    assert_eq!(run.combat().unwrap().potion_slots_for_player(0).unwrap()[0], None);
}

#[test]
fn combat_discard_is_owner_only_fires_inventory_hook_and_survives_carryback(){
    for seat in 0..2{
        let mut run=party();
        for owner in 0..2{run.roster.player_mut(owner).unwrap().potions[0]=Some(PotionId::Strengthpotion);}
        let mut buckle=Relic::new(RelicId::BeltBuckle);buckle.owner=seat as u8;
        run.roster.player_mut(seat).unwrap().relics.push(buckle);
        run.enter_monster().unwrap();
        let net=run.roster.player(seat).unwrap().net_id;
        assert!(run.discard_potion(999,seat,0).is_err());
        assert!(run.discard_potion(net,seat,999).is_err());
        run.discard_potion(net,seat,0).unwrap();
        let c=run.combat().unwrap();
        assert_eq!(c.potion_slots_for_player(seat as u8).unwrap()[0],None);
        assert_eq!(c.potion_slots_for_player((1-seat) as u8).unwrap()[0],Some(PotionId::Strengthpotion));
        assert!(c.run.relics.iter().find(|r|r.id==RelicId::BeltBuckle).unwrap().flag1);
        assert!(run.discard_potion(net,seat,0).is_err());
        run.debug_win_combat().unwrap();
        run.offer_rewards().unwrap();
        assert_eq!(run.roster.player(seat).unwrap().potions[0],None);
        assert_eq!(run.roster.player(1-seat).unwrap().potions[0],Some(PotionId::Strengthpotion));
    }
}

#[test]
fn combat_discard_menu_and_execution_close_at_owner_end_turn(){
    let mut run=party();
    for seat in 0..2{run.roster.player_mut(seat).unwrap().potions[0]=Some(PotionId::Strengthpotion);}
    run.enter_monster().unwrap();
    assert!(run.census_menu().unwrap().iter().any(|d|d.decision.kind=="discard"&&d.decision.seat==0));
    run.end_turn_for(7,0).unwrap();
    assert!(run.discard_potion(7,0,0).is_err());
    run.end_turn_for(42,1).unwrap();
    let before=format!("{:?}",run.combat().unwrap().run);
    for (seat,net) in [(0,7),(1,42)]{
        assert!(!run.combat().unwrap().can_discard_potion_for_player(seat as u8,0));
        assert!(run.discard_potion(net,seat,0).is_err());
    }
    assert_eq!(format!("{:?}",run.combat().unwrap().run),before);
    assert!(run.census_menu().unwrap().iter().all(|d|d.decision.kind!="discard"));
}

#[test]
fn lava_rock_boss_reward_path_retains_two_owner_relic_rewards_and_disables_once() {
    // LavaRock.cs:45-64 appends two owner-bound RelicRewards at the act-1 boss,
    // then sets HasTriggered and disables the relic before reward selection.
    let mut run = party();
    let mut relic = Relic::new(RelicId::LavaRock);
    relic.owner = 1;
    run.roster.player_mut(1).unwrap().relics.push(relic);
    run.enter_boss().unwrap();
    {
        let combat = run.combat.as_mut().unwrap();
        for enemy in combat.alive_enemies() { combat.kill_creature(enemy); }
        combat.check_combat_end();
        assert!(combat.is_over && combat.won);
    }
    run.phase = MpPhase::Rewards;
    run.offer_rewards().unwrap();

    let reward = &run.rewards.as_ref().unwrap()[1];
    assert_eq!(reward.extra_relics.len(), 2);
    assert!(reward.extra_relics.iter().all(Option::is_some));
    assert!(run.rewards.as_ref().unwrap()[0].extra_relics.is_empty());
    let lava = run.roster.player(1).unwrap().relics.iter()
        .find(|relic| relic.id == RelicId::LavaRock).unwrap();
    assert!(lava.flag1);
    assert!(!lava.enabled);
}
