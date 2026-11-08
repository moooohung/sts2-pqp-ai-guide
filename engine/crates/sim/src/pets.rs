//! Event-pet allies used by Byrdpip and Pael's Legion.

use crate::card::{CId, KRef};
use crate::combat::{AttackCtx, Combat, Props};
use crate::creature::Creature;
use crate::hooks::Ev;
use crate::tables::{Hook, PowerId, RelicId};

const EVENT_PET_HP: i32 = 9_999;

impl Combat {
    /// C# Player.Osty / PlayerCombatState.GetPet<Osty>: each summoner owns one Osty,
    /// retained in Allies even after death.
    pub fn osty(&self, owner: CId) -> Option<CId> {
        // Non-player creature callers have no PlayerCombatState/GetPet.
        self.players.0.get(owner as usize)?.pets.iter().copied()
            .find(|&c| self.cr(c).monster_id == "OSTY")
    }

    /// C# Commands/OstyCmd.Summon and CreatureCmd.GainMaxHp.
    pub fn summon_osty(&mut self, owner: CId, amount: i32) -> Option<CId> {
        let amount = self.modify_summon_amount(owner, amount, self.active_card);
        if amount == 0 {
            return self.osty(owner).filter(|&c| self.cr(c).is_alive());
        }
        if amount < 0 {
            self.unported("negative Osty summon");
            return None;
        }
        // OstyCmd.Summon searches Allies for the reusable creature, while
        // Player.Osty/GetPet reads the owner's separate registered-pet list.
        let existing = self.allies.iter().copied().find(|&c|
            self.cr(c).monster_id == "OSTY" && self.cr(c).pet_owner == Some(owner));
        let reviving = existing.is_some_and(|c| self.cr(c).is_dead());
        let before = existing.map(|c| crate::between::PublicState::of(self.cr(c)));
        let pet = match existing {
            Some(c) => {
                if !self.players[owner].pets.contains(&c) { self.players[owner].pets.push(c); }
                c
            }
            None => {
                if self.allies.is_full() {
                    self.unported("Osty ally capacity");
                    return None;
                }
                let mut pet = Creature::player(1, 1);
                pet.is_player = false;
                pet.is_pet = true;
                pet.pet_owner = Some(owner);
                pet.monster_id = "OSTY";
                let c = self.creatures.len() as CId;
                self.creatures.push(pet);
                self.allies.push(c);
                self.players[owner].pets.push(c); // PlayerCmd.AddPet before CreatureCmd.Add.
                self.fire(Hook::AfterCreatureAddedToCombat, Ev::Creature(c), None);
                self.creature_apply_power(c, PowerId::DieForYou, 1, None, None, false, false);
                // New pets are set to the summon amount, not grown from their initial 1 HP.
                self.cr_mut(c).hp = 0;
                c
            }
        };
        if self.cr(pet).is_alive() {
            self.cr_mut(pet).max_hp += amount;
        } else {
            let cr = self.cr_mut(pet);
            cr.max_hp = amount;
            cr.death_processed = false;
            cr.escaped = false;
            cr.in_combat = true;
        }
        // CreatureCmd.Heal permits reviving pets; Combat::heal rejects dead creatures.
        if self.cr(pet).hp == 0 {
            self.cr_mut(pet).hp = amount;
            self.fire(Hook::AfterCurrentHpChanged, Ev::HpChanged { creature: pet, delta: amount }, None);
        } else {
            self.heal(pet, amount);
        }
        if reviving {
            self.record_creature_lifecycle(pet, "revive", before.unwrap());
            self.fire(Hook::AfterOstyRevived, Ev::Creature(pet), None);
        }
        self.hist.summons.push((owner, amount));
        self.fire(Hook::AfterSummon, Ev::Summon { owner, amount }, None);
        Some(pet)
    }

    /// C# AttackCommand.FromOsty: the pet is the attacker, the active card remains
    /// the model source. Osty.CheckMissingWithAnim makes attacks without a live pet no-ops.
    pub fn osty_card_hit(&mut self, k: KRef, target: CId, base: i32) -> i32 {
        self.osty_card_attack(k, &[target], Some(target), base, 1, false)
    }

    /// Rattle.OnPlay uses one AttackCommand with multiple hits.
    pub fn osty_card_hit_n(&mut self, k: KRef, target: CId, base: i32, hits: i32) -> i32 {
        self.osty_card_attack(k, &[target], Some(target), base, hits, false)
    }

    /// HighFive.OnPlay uses one AttackCommand for all opponents, not one attack per enemy.
    pub fn osty_card_hit_all(&mut self, k: KRef, base: i32) -> i32 {
        let targets = self.hittable_enemies();
        self.osty_card_attack(k, &targets, None, base, 1, false)
    }

    /// AttackCommand.TargetingRandomOpponents chooses AFTER BeforeAttack from
    /// living opponents, using the pet owner's run-level CombatTargets stream.
    pub fn osty_card_hit_random(&mut self, k: KRef, base: i32) -> i32 {
        self.osty_card_attack(k, &[], None, base, 1, true)
    }

    fn osty_card_attack(&mut self, k: KRef, targets: &[CId], target: Option<CId>, base: i32, hits: i32, random: bool) -> i32 {
        let owner = self.cards[k as usize].owner;
        let Some(pet) = self.osty(owner).filter(|&c| self.cr(c).is_alive()) else {
            return 0;
        };
        // AttackCommand.Execute: one complete attack scope, not a pending player attack.
        self.flush_pending_attack();
        let previous_card = self.active_card.replace(k);
        self.damage_depth += 1;
        self.attack_stack.push(AttackCtx {
            attacker: Some(pet), target, props: Props::MOVE, base_damage: None, results: Vec::new(),
        });
        self.fire(Hook::BeforeAttack, Ev::Attack {
            attacker: Some(pet), target, props: Props::MOVE, card: Some(k),
        }, None);
        let mut lost = 0;
        for _ in 0..hits {
            if self.cr(pet).is_dead() { break; }
            let random_target;
            let targets = if random {
                let alive = self.alive_enemies();
                if alive.is_empty() { break; }
                random_target = [alive[self.run.rngs.combat_targets.choice_index(alive.len())]];
                &random_target[..]
            } else { targets };
            let mut killed = arrayvec::ArrayVec::<CId, 8>::new();
            // CreatureCmd.Damage: every receiver takes damage before the killed list is processed.
            for &t in targets {
                if !self.cr(t).is_alive() {
                    continue;
                }
                let damage = self.calculate_damage(base, Some(pet), t, Props::MOVE);
                self.defer_kill = true;
                lost += self.apply_damage(t, damage, Props::MOVE, Some(pet));
                if self.cr(t).is_dead() {
                    killed.push(t);
                }
            }
            for t in killed {
                self.kill_creature(t);
            }
        }
        let attack = self.attack_stack.pop().unwrap();
        self.finish_attack_hooks(attack);
        self.damage_depth -= 1;
        self.active_card = previous_card;
        if self.damage_depth == 0 {
            self.check_combat_end();
        }
        lost
    }

    /// r7 `CombatState.summon_event_pet`.
    ///
    /// Event pets are player-side allies but are deliberately not player creatures; this
    /// preserves r7's `get_player_allies_of` targeting rule.
    pub(crate) fn summon_event_pet(&mut self, owner: CId, id: RelicId) -> Option<u8> {
        // fire: core/combat.py:3319-3358
        // Models/Relics/PaelsLegion.cs:221: PlayerCmd.AddPet(Owner).
        if !matches!(id, RelicId::Byrdpip | RelicId::PaelsLegion) || !self.cr(owner).is_player {
            return None;
        }
        let name = id.name();
        if let Some(&existing) = self.players[owner].pets.iter().find(|&&c| {
            let ally = self.cr(c);
            ally.is_pet && ally.pet_owner == Some(owner) && ally.monster_id == name
        }) {
            return Some(existing);
        }
        if self.allies.is_full() {
            self.unported("event pet ally capacity");
            return None;
        }

        let mut pet = Creature::player(EVENT_PET_HP, EVENT_PET_HP);
        pet.is_player = false;
        pet.is_pet = true;
        pet.pet_owner = Some(owner);
        pet.monster_id = name;
        let c = self.creatures.len() as u8;
        self.creatures.push(pet);
        self.allies.push(c);
        self.players[owner].pets.push(c);
        self.fire(Hook::AfterCreatureAddedToCombat, Ev::Creature(c), None);
        Some(c)
    }
}
