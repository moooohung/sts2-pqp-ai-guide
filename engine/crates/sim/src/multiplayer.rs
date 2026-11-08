//! Player-count scaling (C# `Models/Singleton/MultiplayerScalingModel.cs`,
//! `Creature.ScaleMonsterHpForMultiplayer`, `PowerCmd.Apply`'s `GetScaledAmountForMultiplayer`).
//! Every rule is the identity with one player, so solo results never change. Which powers scale
//! comes from the game C# (`data/csharp_meta.json`, `tables::POWER_MP`).

use crate::card::CId;
use crate::combat::{Combat, Props};
use crate::creature::Side;
use crate::tables::*;

const POWER_MP_SCALES: u8 = 1;
const POWER_MP_CUSTOM: u8 = 2;
const POWER_MP_SECONDARY: u8 = 4;

impl Combat {
    fn player_count(&self) -> i64 {
        self.players.len() as i64
    }

    /// `MultiplayerScalingModel.GetMultiplayerScaling(encounter, actIndex)` in tenths.
    fn multiplayer_scaling_tenths(&mut self) -> i64 {
        let (value, fault) = self.multiplayer_scaling_tenths_readonly();
        if let Some(reason) = fault { self.unported(reason); }
        value
    }

    fn multiplayer_scaling_tenths_readonly(&self) -> (i64, Option<&'static str>) {
        match self.run.act_index {
            0 => (11, None),
            1 => (12, None),
            2 if self.run.room_type == RoomType::Boss => (13, None),
            2 => (12, None),
            _ => (10, Some("multiplayer scaling act index")),
        }
    }

    /// `Creature.IsSecondaryEnemy`: an enemy with a power whose `OwnerIsSecondaryEnemy` (Minion).
    pub fn is_secondary_enemy(&self, c: CId) -> bool {
        let cr = self.cr(c);
        cr.side == Side::Enemy
            && cr.power_list().iter().any(|&p| POWER_MP[self.powers[p as usize].id.index()] & POWER_MP_SECONDARY != 0)
    }

    /// `Creature.ScaleHpForMultiplayer`: `hp * players * scaling`, truncated like `(int)decimal`.
    pub(crate) fn scale_monster_hp(&mut self, hp: i32) -> i32 {
        let n = self.player_count();
        if n <= 1 {
            return hp;
        }
        let tenths = self.multiplayer_scaling_tenths();
        (hp as i64 * n * tenths / 10).min(999_999_999) as i32
    }

    /// `MultiplayerScalingModel.ModifyBlockMultiplicative` for `target`'s powered card or monster
    /// move block: x players (two players), x players x scaling (three or more).
    pub(crate) fn scale_monster_block(&mut self, block: f64, target: CId, props: Props) -> f64 {
        let (value, fault) = self.scale_monster_block_readonly(block, target, props);
        if let Some(reason) = fault { self.unported(reason); }
        value
    }

    pub(crate) fn scale_monster_block_readonly(&self, block: f64, target: CId, props: Props) -> (f64, Option<&'static str>) {
        let n = self.player_count();
        if n <= 1 || self.cr(target).side != Side::Enemy || !props.is_powered_block() {
            return (block, None);
        }
        if n <= 2 {
            return (block * n as f64, None);
        }
        // An integer product divided once by 10 stays exact when the true value is integral.
        let (tenths, fault) = self.multiplayer_scaling_tenths_readonly();
        (block * (n * tenths) as f64 / 10.0, fault)
    }

    /// `PowerCmd.Apply`: a new instance of a scaling power on an enemy gets
    /// `GetScaledAmountForMultiplayer` (stacking through `ModifyAmount` does not scale).
    pub(crate) fn scale_power_for_multiplayer(&mut self, id: PowerId, amount: i32, target: CId) -> i32 {
        let n = self.player_count();
        let bits = POWER_MP[id.index()];
        if n <= 1 || self.cr(target).side != Side::Enemy || bits & POWER_MP_SCALES == 0 {
            return amount;
        }
        let a = amount as i64;
        let scaled = if bits & POWER_MP_CUSTOM == 0 {
            a * n * self.multiplayer_scaling_tenths() / 10
        } else {
            match id {
                // ArtifactPower: amount + players - 1.
                PowerId::Artifact => a + n - 1,
                // PlatingPower / BufferPower: ((players - 1) * 2 + 1) * amount.
                PowerId::Plating | PowerId::Buffer => ((n - 1) * 2 + 1) * a,
                // SkittishPower: amount * (1 + (players - 1) * 0.5).
                PowerId::Skittish => a * (n + 1) / 2,
                // SlipperyPower: amount * players.
                PowerId::Slippery => a * n,
                _ => {
                    self.unported("custom multiplayer power scaling");
                    a
                }
            }
        };
        scaled as i32
    }

    /// C# `PlatingPower` `Decrement` var: an enemy's Plating loses one per player each turn.
    pub(crate) fn plating_decrement(&self, owner: CId) -> i32 {
        if self.cr(owner).side == Side::Enemy { self.players.len() as i32 } else { 1 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::Creature;
    use crate::run::{RunManager, RunOptions};

    /// `players` seated Ironclads, one bare enemy (creature id `players`).
    fn combat(players: usize, act: i32, room: RoomType) -> (Combat, CId) {
        let mut run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        run.act_index = act;
        run.room_type = room;
        let deck = run.deck.clone();
        let mut c = Combat::new(run, 1);
        for _ in 1..players {
            c.add_player(80, 80, 3, 0, &deck);
        }
        let mut enemy = Creature::player(50, 50);
        enemy.side = Side::Enemy;
        enemy.is_player = false;
        let e = c.creatures.len() as CId;
        c.creatures.push(enemy);
        c.enemies.push(e);
        (c, e)
    }

    #[test]
    fn solo_is_identity() {
        let (mut c, e) = combat(1, 2, RoomType::Boss);
        assert_eq!(c.scale_monster_hp(137), 137);
        assert_eq!(c.scale_monster_block(9.0, e, Props::MOVE), 9.0);
        assert_eq!(c.scale_power_for_multiplayer(PowerId::Plating, 8, e), 8);
        assert_eq!(c.plating_decrement(e), 1);
    }

    #[test]
    fn monster_hp_scales_by_players_and_act() {
        // C# Creature.ScaleHpForMultiplayer: hp * players * {1.1, 1.2, 1.2 | boss 1.3}, (int).
        assert_eq!(combat(2, 0, RoomType::Monster).0.scale_monster_hp(45), 99);
        assert_eq!(combat(3, 1, RoomType::Elite).0.scale_monster_hp(45), 162);
        assert_eq!(combat(4, 2, RoomType::Monster).0.scale_monster_hp(45), 216);
        assert_eq!(combat(4, 2, RoomType::Boss).0.scale_monster_hp(45), 234);
    }

    #[test]
    fn monster_block_scales_only_powered_enemy_block() {
        let (mut c, e) = combat(2, 0, RoomType::Monster);
        assert_eq!(c.scale_monster_block(7.0, e, Props::MOVE), 14.0);
        assert_eq!(c.scale_monster_block(7.0, e, Props::UNPOWERED), 7.0);
        assert_eq!(c.scale_monster_block(7.0, PLAYER_ID, Props::MOVE), 7.0);
        let (mut c, e) = combat(3, 0, RoomType::Monster);
        // 3 players, act 1: x 3 x 1.1 = 3.3 (exact for integral results).
        assert_eq!(c.scale_monster_block(10.0, e, Props::MOVE), 33.0);
    }

    #[test]
    fn enemy_powers_scale_with_the_csharp_formulas() {
        let (mut c, e) = combat(3, 1, RoomType::Monster);
        assert_eq!(c.scale_power_for_multiplayer(PowerId::Artifact, 1, e), 3);
        assert_eq!(c.scale_power_for_multiplayer(PowerId::Plating, 4, e), 20);
        assert_eq!(c.scale_power_for_multiplayer(PowerId::Skittish, 3, e), 6);
        assert_eq!(c.scale_power_for_multiplayer(PowerId::Slippery, 2, e), 6);
        assert_eq!(c.scale_power_for_multiplayer(PowerId::Regen, 5, e), 18);
        // BufferPower has the override but not ShouldScaleInMultiplayer: unscaled.
        assert_eq!(c.scale_power_for_multiplayer(PowerId::Buffer, 2, e), 2);
        // Strength does not scale; player targets never do.
        assert_eq!(c.scale_power_for_multiplayer(PowerId::Strength, 2, e), 2);
        assert_eq!(c.scale_power_for_multiplayer(PowerId::Plating, 4, PLAYER_ID), 4);
        assert_eq!(c.plating_decrement(e), 3);
        assert_eq!(c.plating_decrement(PLAYER_ID), 1);
    }

    #[test]
    fn seated_players_get_their_own_piles() {
        let (mut c, _) = combat(2, 0, RoomType::Monster);
        assert_eq!(c.living_players().as_slice(), &[0, 1]);
        assert!(c.players[1].starting_deck.iter().all(|&k| c.cards[k as usize].owner == 1));
        let k = c.new_card(1, CardId::Shiv, false).unwrap();
        c.add_generated_card(k, crate::combat::PileKind::Hand, crate::combat::PilePos::Bottom, true);
        assert!(c.players[1].hand.contains(&k) && !c.players[0].hand.contains(&k));
    }

    #[test]
    fn manual_ally_targets_require_a_living_player_and_allow_self() {
        // C# CardModel.IsValidTarget and BelieveInYou.OnPlay.
        for target in [0, 1] {
            let (mut c, _) = combat(2, 0, RoomType::Monster);
            let k = c.new_card(0, CardId::BelieveInYou, false).unwrap();
            c.players[0].hand.push(k);
            let before = c.players[target].energy;
            assert!(!c.play_card(0, 0, None));
            assert!(!c.play_card(0, 0, Some(2)));
            c.creatures[target as usize].hp = 0;
            assert!(!c.play_card(0, 0, Some(target as usize)));
            c.creatures[target as usize].hp = 80;
            assert!(c.play_card(0, 0, Some(target as usize)));
            assert_eq!(c.players[target].energy, before + 2);
        }
    }

    #[test]
    fn autoplay_ally_targets_exclude_owner_and_dead_players() {
        let (mut c, _) = combat(3, 0, RoomType::Monster);
        c.creatures[1].hp = 0;
        assert_eq!(c.random_ally_target(0), Some(2));
        c.creatures[2].hp = 0;
        let counter = c.run.rngs.combat_targets.counter;
        assert_eq!(c.random_ally_target(0), None);
        assert_eq!(c.run.rngs.combat_targets.counter, counter);
    }

    #[test]
    fn observation_ally_actions_decode_to_players_not_enemies() {
        use crate::obs::{decode_combat_action, R7_MAX_HAND_SIZE};
        let (mut c, enemy) = combat(3, 0, RoomType::Monster);
        let k = c.new_card(0, CardId::BelieveInYou, false).unwrap();
        c.players[0].hand.push(k);
        let base = (1 + R7_MAX_HAND_SIZE) as u16;
        for target in 0..3 {
            let binding = decode_combat_action(base + target, &c, &[enemy], false).unwrap();
            assert_eq!(binding.hand_index, 0);
            assert_eq!(binding.target, Some(target as CId));
        }
        c.creatures[1].hp = 0;
        assert!(decode_combat_action(base + 1, &c, &[enemy], false).is_err());
        assert!(decode_combat_action(base + 3, &c, &[enemy], false).is_err());
    }

    #[test]
    fn ally_hand_cards_require_another_living_player_even_when_targeting_self() {
        // C# CardModel.CanPlay sets NoLivingAllies with <= 1 living PlayerCreatures.
        let (mut c, _) = combat(2, 0, RoomType::Monster);
        let k = c.new_card(0, CardId::BelieveInYou, false).unwrap();
        c.players[0].hand.push(k);
        assert!(c.can_play_card(k));
        c.creatures[1].hp = 0;
        assert!(!c.can_play_card(k));
        assert!(!c.play_card(0, 0, Some(0)));
    }

    const PLAYER_ID: CId = crate::combat::PLAYER;

    #[test]
    fn imitation_tracks_separate_players_and_only_the_first_play_in_a_series() {
        // C# ImitationLearning.OnPlay / ImitationLearningPower's before/after hooks.
        let (mut c, _) = combat(3, 0, RoomType::Monster);
        let imitate = c.new_card(0, CardId::ImitationLearning, false).unwrap();
        for target in [1, 1, 2] {
            c.play_card_effect(imitate, Some(target));
        }
        let instances: Vec<_> = c.cr(0).power_list().into_iter()
            .filter(|&p| c.powers[p as usize].id == PowerId::ImitationLearning).collect();
        assert_eq!(instances.len(), 2);
        assert_eq!(c.powers[instances[0] as usize].amount, 4);
        let power = c.new_card(1, CardId::DemonFormCard, false).unwrap();
        // External duplication belongs to the source owner, not the copied card.
        c.apply_power_to(1, PowerId::Duplication, 1, Some(1));
        c.players[1].hand.push(power);
        c.players[1].energy = 10;
        assert!(c.play_card(1, 0, None));
        assert!(c.power_amount(0, PowerId::DemonForm) > 0);
        assert_eq!(c.power_amount(1, PowerId::DemonForm), 2 * c.power_amount(0, PowerId::DemonForm));
        assert_eq!(c.powers[instances[0] as usize].amount, 3);
        assert_eq!(c.powers[instances[1] as usize].amount, 2);
        assert!(c.imitation_clones.is_empty());
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }

    #[test]
    fn imitation_self_target_decrements_before_recursive_copy_and_then_removes_itself() {
        let (mut c, _) = combat(2, 0, RoomType::Monster);
        let imitate = c.new_card(0, CardId::ImitationLearning, false).unwrap();
        c.play_card_effect(imitate, Some(0));
        let power = c.new_card(0, CardId::DemonFormCard, false).unwrap();
        c.players[0].hand.push(power);
        c.players[0].energy = 10;
        assert!(c.play_card(0, 0, None));
        // One original and two generated plays consume the two charges exactly.
        assert_eq!(c.hist.played_combat.len(), 3);
        assert!(!c.has_power(0, PowerId::ImitationLearning));
        assert!(c.imitation_clones.is_empty());
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }

    #[test]
    fn tag_team_consumes_only_instances_matching_attack_target_and_other_applier() {
        // C# TagTeamPower.ModifyCardPlayCount / AfterModifyingCardPlayCount.
        let (mut c, enemy) = combat(3, 0, RoomType::Monster);
        let own = c.apply_power_to(enemy, PowerId::TagTeam, 1, Some(0)).unwrap();
        let other = c.apply_power_to(enemy, PowerId::TagTeam, 2, Some(1)).unwrap();
        let k = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
        c.active_target = None;
        assert_eq!(c.modify_card_play_count(1, k).0, 1);
        c.active_target = Some(enemy);
        let (count, modifiers) = c.modify_card_play_count(1, k);
        assert_eq!(count, 3);
        c.fire_after_modifying_card_play_count(k, modifiers);
        assert!(c.cr(enemy).power_list().contains(&own));
        assert!(!c.cr(enemy).power_list().contains(&other));
        let cleave = c.new_card(2, CardId::Thunderclap, false).unwrap();
        c.active_target = None;
        let (count, modifiers) = c.modify_card_play_count(1, cleave);
        assert_eq!(count, 2);
        c.fire_after_modifying_card_play_count(cleave, modifiers);
        assert!(c.find_power(enemy, PowerId::TagTeam).is_none());
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }

    #[test]
    fn tank_guards_other_living_seats_and_only_loses_dead_appliers_protection() {
        // C# TankPower.AfterApplied and GuardedPower damage/death hooks.
        for seats in 2..=4 {
            let (mut c, enemy) = combat(seats, 0, RoomType::Monster);
            c.apply_power_to(0, PowerId::Tank, 1, Some(0));
            assert!(c.find_power(0, PowerId::Guarded).is_none());
            assert_eq!(c.calculate_damage(8, Some(enemy), 0, Props::MOVE), 12);
            for ally in 1..seats {
                assert_eq!(c.calculate_damage(8, Some(enemy), ally as CId, Props::MOVE), 4);
                assert_eq!(c.calculate_damage(8, Some(enemy), ally as CId, Props::UNPOWERED), 8);
            }
            c.kill_creature(0);
            for ally in 1..seats {
                assert!(c.find_power(ally as CId, PowerId::Guarded).is_none());
                assert_eq!(c.calculate_damage(8, Some(enemy), ally as CId, Props::MOVE), 8);
            }
            assert!(c.fault.is_none(), "{:?}", c.fault);
        }
    }

    #[test]
    fn coordinate_temporary_strength_stacks_and_expires_for_each_seat() {
        // C# Coordinate.OnPlay and TemporaryStrengthPower's application/expiration hooks.
        for seats in 2..=4 {
            for target in 0..seats {
                let (mut c, _) = combat(seats, 0, RoomType::Monster);
                for upgraded in [false, true] {
                    let k = c.new_card(0, CardId::CoordinateCard, upgraded).unwrap();
                    c.play_card_effect(k, Some(target as CId));
                }
                assert_eq!(c.power_amount(target as CId, PowerId::Strength), 13);
                let p = c.find_power(target as CId, PowerId::Coordinate).unwrap();
                c.chars_after_turn_end(p, target as CId, Side::Enemy);
                assert_eq!(c.power_amount(target as CId, PowerId::Strength), 13);
                c.chars_after_turn_end(p, target as CId, Side::Player);
                assert_eq!(c.power_amount(target as CId, PowerId::Strength), 0);
                assert!(c.fault.is_none(), "{:?}", c.fault);
            }
        }
    }

    #[test]
    fn soulbound_tracks_applier_and_generates_base_souls_without_recursion() {
        use crate::combat::{PileKind, PilePos};
        for id in [CardId::Soulbound] {
            let (mut c, _) = combat(2, 0, RoomType::Monster);
            let k = c.new_card(0, id, false).unwrap();
            c.players[0].hand.push(k);
            c.players[0].energy = 10;
            c.players[0].stars = 10;
            assert!(c.play_card(0, 0, Some(1)), "{id:?}");
            assert_eq!(c.fault, None, "{id:?}");
            assert_eq!(c.power_amount(1, PowerId::Soulbound), 1);
            let p = c.find_power(1, PowerId::Soulbound).unwrap();
            assert_eq!(c.powers[p as usize].applier, Some(0));
            let soul = c.new_card(1, CardId::Soul, true).unwrap();
            c.add_generated_card_by(soul, PileKind::Draw, PilePos::Random, Some(0));
            assert_eq!(c.players[1].draw.len(), 2);
            assert_eq!(c.players[1].draw.iter().filter(|&&q| !c.cards[q as usize].upgraded).count(), 1);
            let unrelated = c.new_card(1, CardId::Soul, false).unwrap();
            c.add_generated_card_by(unrelated, PileKind::Draw, PilePos::Random, Some(1));
            assert_eq!(c.players[1].draw.len(), 3);
            c.apply_power_to(1, PowerId::Soulbound, 1, Some(0));
            let soul = c.new_card(0, CardId::Soul, false).unwrap();
            c.add_generated_card(soul, PileKind::Draw, PilePos::Random, true);
            assert_eq!(c.players[1].draw.len(), 5);
            assert_eq!(c.fault, None);
        }
    }

    #[test]
    fn instanced_team_damage_multipliers_exempt_only_their_applier() {
        let (mut c, enemy) = combat(3, 0, RoomType::Monster);
        let flank = c.apply_power_to(enemy, PowerId::Flanking, 2, Some(0)).unwrap();
        let knock = c.apply_power_to(enemy, PowerId::Knockdown, 3, Some(1)).unwrap();
        assert_eq!(c.calculate_damage(3, Some(0), enemy, Props::MOVE), 9);
        assert_eq!(c.calculate_damage(3, Some(1), enemy, Props::MOVE), 6);
        assert_eq!(c.calculate_damage(3, Some(2), enemy, Props::MOVE), 18);
        c.chars_after_turn_end(flank, enemy, Side::Player);
        assert!(c.find_power(enemy, PowerId::Flanking).is_some());
        c.chars_after_turn_end(flank, enemy, Side::Enemy);
        c.chars_after_turn_end(knock, enemy, Side::Enemy);
        assert!(c.find_power(enemy, PowerId::Flanking).is_none());
        assert!(c.find_power(enemy, PowerId::Knockdown).is_none());
    }

    #[test]
    fn beacons_share_block_with_a_per_instance_recursion_guard() {
        let (mut c, _) = combat(2, 0, RoomType::Monster);
        c.side = Side::Player;
        c.apply_power_to(0, PowerId::BeaconOfHope, 1, Some(0));
        c.apply_power_to(1, PowerId::BeaconOfHope, 1, Some(1));
        c.power_gain_unpowered_block(0, 8);
        assert_eq!(c.cr(0).block, 10);
        assert_eq!(c.cr(1).block, 4);
        c.side = Side::Enemy;
        c.power_gain_unpowered_block(0, 8);
        assert_eq!(c.cr(0).block, 18);
        assert_eq!(c.cr(1).block, 4);
    }
}
