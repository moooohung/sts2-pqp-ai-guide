use sts2_sim::{
    combat::Combat, creature::Side, rng::Rng,
    run::{RunManager, RunOptions}, tables::PowerId,
};

fn combat(seats: u8) -> Combat {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    for _ in 1..seats { c.add_player(1000, 1000, 3, 0, &[]); }
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    c
}

// Full C# power override census (Core/Models/Powers, including Mocks):
// AfterDeath: Adaptable, Constrict, Covered, CrabRage, Dampen, Guarded,
// Hex, Illusion, Infested, MagicBomb, MockInvincibleOnDeath, PossessSpeed,
// PossessStrength, Ravenous, Reattach, Shrink, SteamEruption, Stock,
// Surprise, Surrounded.
// ShouldPowerBeRemovedAfterOwnerDeath: Adaptable, DieForYou, Minion,
// MockInvincibleOnDeath, PainfulStabs, Reattach, SteamEruption.
// ShouldPowerBeRemovedOnDeath: Illusion (54-61); applies globally via Hook:1994.
// ShouldOwnerDeathTriggerFatal: Minion (20-23), Reattach (97-100).
// MockInvincibleOnDeath.IsMock=true is excluded from real game content and
// has no simulator identity. DoorRevival is r7's compatibility implementation.
// The generic fallback arms remain integrity guards; no unmatched real
// metadata death bit/override is silently treated as a no-op.

#[test]
fn possessed_stats_return_to_each_original_receiver_on_caster_death() {
    for seats in 2..=4 {
        for (possess, stat) in [(PowerId::PossessStrength, PowerId::Strength),
            (PowerId::PossessSpeed, PowerId::Dexterity)] {
            let mut c = combat(seats);
            let caster = c.enemies[0];
            c.apply_power_to(caster, possess, 1, Some(caster));
            for owner in (0..seats).rev() {
                c.apply_power_to(owner, stat, 10, None);
                c.apply_power_to(owner, stat, -(owner as i32 + 1), Some(caster));
                c.apply_power_to(owner, stat, -1, Some(caster));
            }
            c.kill_creature(caster);
            for owner in 0..seats { assert_eq!(c.power_amount(owner, stat), 10); }
            assert_eq!(c.fault, None);
        }
    }
}

#[test]
fn magic_bombs_are_per_caster_instances_removed_by_only_their_casters_death() {
    for seats in 2..=4 {
        let mut c = combat(seats);
        for owner in 0..seats {
            let first = c.apply_power_to(owner, PowerId::MagicBomb, 3, Some(0)).unwrap();
            let second = c.apply_power_to(owner, PowerId::MagicBomb, 5, Some(1)).unwrap();
            assert_ne!(first, second);
        }
        c.kill_creature(0);
        for owner in 1..seats {
            let instances: Vec<_> = c.cr(owner).power_list().into_iter()
                .filter(|&p| c.powers[p as usize].id == PowerId::MagicBomb).collect();
            assert_eq!(instances.len(), 1);
            assert_eq!(c.powers[instances[0] as usize].applier, Some(1));
            let before = c.cr(owner).hp;
            c.fire_after_turn_end(Side::Player);
            assert_eq!(c.cr(owner).hp, before - 5);
            break; // side hook fires for all recipients exactly once
        }
        assert_eq!(c.fault, None);
    }
}

#[test]
fn illusion_retains_temporary_debuffs_and_other_creatures_buffs_at_death() {
    for seats in 2..=4 {
        let mut c = combat(seats);
        let illusion_owner = c.enemies[0];
        c.apply_power_to(illusion_owner, PowerId::Illusion, 1, None);
        c.apply_power_to(0, PowerId::Strength, -3, None);
        c.apply_power_to(0, PowerId::Weak, 2, None);
        // DarkShackles inherits TemporaryStrengthPower / ITemporaryPower.
        c.apply_power_to(0, PowerId::DarkShackles, 2, None);
        c.kill_creature(0);
        assert!(c.has_power(0, PowerId::Strength));
        assert!(!c.has_power(0, PowerId::Weak));
        assert!(c.has_power(0, PowerId::DarkShackles));
        assert_eq!(c.fault, None);
    }
}
