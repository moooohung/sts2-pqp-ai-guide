//! r7 `core/damage.py` (calculate/apply), attack contexts, death and combat end.

use crate::card::{CId, KRef};
use crate::creature::PRef;
use arrayvec::ArrayVec;
use smallvec::SmallVec;
use crate::combat::*;
use crate::creature::Side;
use crate::hooks::Ev;
use crate::tables::*;

#[derive(Clone, Debug)]
pub(crate) struct MonsterAttackWalk {
    attacker: CId, base: i32, hits: i32,
    card: Option<KRef>,
    targets: Vec<CId>, next: usize, started: bool,
    killed: Vec<CId>, suck: Option<PRef>,
    callbacks: Vec<DamageCallback>,
    applied: bool,
    tail: MonsterAttackTail,
}

#[derive(Clone, Debug)]
enum MonsterAttackTail { None, Strength(i32), WaterfallStomp }

#[derive(Clone, Debug)]
pub(crate) struct DamageCallback {
    target: CId, damage: i32, blocked: i32, hp_lost: i32,
    props: Props, dealer: Option<CId>, was_block_broken: bool,
    was_killed: bool, kill: bool, card_source: Option<KRef>,
}

impl Combat {
    // ───────────────────────────── attack contexts ─────────────────────────────

    /// `_ensure_pending_attack_context`: returns the active attack index.
    fn ensure_pending_attack(&mut self, attacker: Option<CId>, target: CId, props: Props) -> usize {
        if !self.attack_stack.is_empty() {
            return self.attack_stack.len() - 1;
        }
        if !self.pending_auto_attack {
            self.pending_auto_attack = true;
            self.attack_stack.push(AttackCtx { attacker, target: Some(target), props, base_damage: None, results: Vec::new() });
            // fire: core/combat.py:712-716
            self.fire(
                Hook::BeforeAttack,
                Ev::Attack {
                    attacker,
                    target: Some(target),
                    props,
                    card: self.active_card,
                },
                None,
            );
        }
        self.attack_stack.len() - 1
    }

    /// `flush_pending_attack_context`.
    pub fn flush_pending_attack(&mut self) {
        if !self.pending_auto_attack || !self.pending_monster_attacks.is_empty() {
            return;
        }
        self.pending_auto_attack = false;
        let attack = self.attack_stack.pop().unwrap();
        self.finish_attack_hooks(attack);
    }

    /// AttackCommand.Results belongs to the command, not the attacker/power.
    /// Preserve an outer command's results while AfterAttack launches a nested one.
    pub(crate) fn finish_attack_hooks(&mut self, attack: AttackCtx) {
        let outer = std::mem::replace(&mut self.completed_attack_results, attack.results);
        // fire: core/combat.py:728
        self.fire(
            Hook::AfterAttack,
            Ev::Attack {
                attacker: attack.attacker,
                target: attack.target,
                props: attack.props,
                card: self.active_card,
            },
            None,
        );
        self.completed_attack_results = outer;
    }

    /// `calculate_damage(base, dealer, target, props, combat)`.
    pub fn calculate_damage(&mut self, base: i32, dealer: Option<CId>, target: CId, props: Props) -> i32 {
        if dealer.is_some() {
            let a = self.ensure_pending_attack(dealer, target, props);
            self.attack_stack[a].base_damage = Some(base);
        }
        let card = self.active_card;
        self.modify_damage(base, dealer, target, props, card)
    }

    /// `calculate_block(base, target, props, combat, card_source)`.
    pub fn calculate_block(&mut self, base: i32, target: CId, props: Props, card: Option<KRef>) -> i32 {
        self.modify_block(base, target, props, card)
    }

    /// Creature-side HP change hook (`fire_after_current_hp_changed`).
    pub(crate) fn after_hp_changed(&mut self, c: CId, delta: i32) {
        self.fire(Hook::AfterCurrentHpChanged, Ev::HpChanged { creature: c, delta }, None);
    }

    pub fn heal(&mut self, c: CId, amount: i32) -> i32 {
        let cr = &self.creatures[c as usize];
        if amount <= 0 || cr.is_dead() {
            return 0;
        }
        let public_before=crate::between::PublicState::of(cr);
        let before = cr.hp;
        let cr = &mut self.creatures[c as usize];
        cr.hp = (cr.hp + amount).min(cr.max_hp);
        let healed = cr.hp - before;
        self.record_public_heal(c,"heal",public_before,amount);
        if healed > 0 && !self.is_over {
            self.after_hp_changed(c, healed);
        }
        healed
    }

    /// `apply_damage(target, damage, props, combat, dealer)`; returns hp lost.
    pub fn apply_damage(&mut self, target: CId, damage: i32, props: Props, dealer: Option<CId>) -> i32 {
        self.apply_damage_result(target, damage, props, dealer).hp_lost
    }

    /// CreatureCmd.Damage returns an immutable result; nested damage hooks may overwrite
    /// last_damage, so cards such as Blight Strike must not read that shared scratch value.
    pub(crate) fn apply_damage_result(&mut self, target: CId, damage: i32, props: Props, dealer: Option<CId>) -> DamageOut {
        let source = if props.is_powered_attack() { self.active_card } else { None };
        self.apply_damage_result_with_source(target, damage, props, dealer, source)
    }

    /// C# CreatureCmd.Damage's cardSource belongs to this hit, not an enclosing play.
    pub fn apply_damage_with_card_source(&mut self, target: CId, damage: i32, props: Props,
        dealer: Option<CId>, card_source: Option<KRef>) -> i32
    {
        self.apply_damage_result_with_source(target, damage, props, dealer, card_source).hp_lost
    }

    fn apply_damage_result_with_source(&mut self, target: CId, damage: i32, props: Props,
        dealer: Option<CId>, card_source: Option<KRef>) -> DamageOut
    {
        let _damage = self.orb_trace("damage", Some(target));
        let defer_kill = std::mem::take(&mut self.defer_kill);
        if !self.can_hit(target) {
            return DamageOut::default();
        }
        // CreatureCmd.cs:147-264: only this command's results join its batch.
        // BeforeDamageReceived (Thorns) awaits a separate nested command,
        // including its AfterDamageReceived/Slippery callbacks.
        let callbacks = self.collect_damage_callbacks.take();
        self.end_defer_depth += 1;
        // fire: core/damage.py:220
        self.fire(Hook::BeforeDamageReceived, Ev::Damage { target, dealer, card_source, amount: damage, total_damage: damage, blocked_damage: 0, props, was_block_broken: false }, None);
        self.end_defer_depth -= 1;
        let unblockable = props.has(Props::UNBLOCKABLE);
        // C# CreatureCmd.Damage: pets consume their owner's block, even when targeted directly.
        let block_owner = self.cr(target).pet_owner.unwrap_or(target);
        let before=crate::between::PublicState::of(self.cr(target));
        let block_before=self.cr(block_owner).block;
        let blocked = {
            let _block = self.orb_trace("block", Some(block_owner));
            self.cr_mut(block_owner).damage_block(damage, unblockable)
        };
        let was_block_broken = self.cr(target).block <= 0 && blocked > 0;
        let mut remaining = damage - blocked;
        if remaining > 0 {
            // fire: core/hooks.py:362 (powers), core/hooks.py:462 (relics)
            remaining = self.modify_hp_lost_phase(remaining, target, dealer, props, true);
        }
        // C# DieForYouPower.ModifyUnblockedDamageTarget: only powered attacks redirect.
        let receiver = if props.is_powered_attack() {
            self.osty(target).filter(|&o| self.cr(o).is_alive() && self.has_power(o, PowerId::DieForYou)).unwrap_or(target)
        } else {
            target
        };
        remaining = self.modify_hp_lost_phase(remaining, receiver, dealer, props, false);
        // CreatureCmd.cs:168 computes this before HP loss and applies the same
        // flag to both the redirected and original receiver's history entries.
        let fully_blocked = !unblockable && (blocked > 0 || self.cr(target).block > 0) && remaining == 0;
        for receiver in [Some(receiver), (receiver != target).then_some(target)].into_iter().flatten() {
            if self.players.is_player(receiver) {
                self.hist.damage_received_turns.push((receiver, self.players[receiver].turn_number, fully_blocked));
            }
        }
        if receiver != target {
            // C# CreatureCmd.Damage builds both DamageResults before dispatching callbacks.
            let pet_before=crate::between::PublicState::of(self.cr(receiver));
            let pet_lost = remaining.min(self.cr(receiver).hp).max(0);
            self.cr_mut(receiver).hp -= pet_lost;
            let overkill = (remaining - pet_lost).max(0);
            let spill = self.modify_hp_lost_phase(overkill, target, dealer, props, false);
            let player_lost = spill.min(self.cr(target).hp).max(0);
            self.cr_mut(target).hp -= player_lost;
            self.record_damage_hit(receiver,target,block_owner,pet_before,block_before,damage,0,pet_lost,overkill,props,dealer);
            self.record_damage_hit(target,target,block_owner,before,block_before,damage,blocked,player_lost,(spill-player_lost).max(0),props,dealer);
            self.collect_damage_callbacks = callbacks;
            self.finish_damage(receiver, damage, 0, pet_lost, overkill, props, dealer, false, false, card_source);
            self.finish_damage(target, damage, blocked, player_lost, (spill - player_lost).max(0), props, dealer, was_block_broken, false, card_source);
            if !defer_kill {
                if self.cr(receiver).is_dead() {
                    self.kill_creature(receiver);
                }
                if self.cr(target).is_dead() {
                    self.kill_creature(target);
                }
            }
            return DamageOut { blocked, hp_lost: player_lost, overkill: (spill - player_lost).max(0) };
        }
        let was_alive = self.cr(target).is_alive();
        let hp_lost = {
            let cr = self.cr_mut(target);
            if remaining <= 0 {
                0
            } else {
                let actual = remaining.min(cr.hp);
                cr.hp = (cr.hp - remaining).max(0);
                actual
            }
        };
        let was_killed = was_alive && self.cr(target).is_dead();
        self.record_damage_hit(target,target,block_owner,before,block_before,damage,blocked,hp_lost,(remaining-hp_lost).max(0),props,dealer);
        self.collect_damage_callbacks = callbacks;
        self.finish_damage(target, damage, blocked, hp_lost, (remaining - hp_lost).max(0), props, dealer, was_block_broken, was_killed && !defer_kill, card_source);
        DamageOut { blocked, hp_lost, overkill: (remaining - hp_lost).max(0) }
    }

    fn finish_damage(&mut self, target: CId, damage: i32, blocked: i32, hp_lost: i32,
                     overkill: i32, props: Props, dealer: Option<CId>, was_block_broken: bool, kill: bool, card_source: Option<KRef>) {
        let was_killed = self.cr(target).is_dead();
        // r7 DamageResult of this hit (total_damage = blocked + unblocked; overkill beyond hp).
        self.last_damage = DamageOut { blocked, hp_lost, overkill };
        // record_damage_event (r7 core/damage.py:253): unblocked_damage = hp_lost.
        self.hist.damage.push(DamageEvent { dealer, target, props, unblocked: hp_lost });
        // fire: core/damage.py:244-246
        self.power_record_attack_damage_result(dealer, target, props, hp_lost);
        let callback = DamageCallback { target, damage, blocked, hp_lost, props,
            dealer, was_block_broken, was_killed, kill, card_source };
        if let Some(callbacks) = self.collect_damage_callbacks.as_mut() {
            callbacks.push(callback);
            return;
        }
        self.finish_damage_callbacks(callback);
    }

    fn finish_damage_callbacks(&mut self, callback: DamageCallback) {
        let DamageCallback { target, damage, blocked, hp_lost, props, dealer,
            was_block_broken, was_killed, kill, card_source } = callback;
        if was_block_broken {
            // fire: core/damage.py:254-258
            self.power_after_block_broken(target);
        }
        if hp_lost > 0 {
            self.after_hp_changed(target, -hp_lost);
        }
        // CreatureCmd.Damage: update the dealer's run metric after HP-change
        // callbacks and before AfterDamageGiven, including all damage props.
        if let Some(owner) = dealer
            && self.players.is_player(owner) && !self.players.is_player(target) {
            self.player_damage_dealt[owner as usize] += hp_lost;
        }
        // relic fire: core/hooks.py:1104
        self.fire(
            Hook::AfterDamageGiven,
            // C# DamageResult.TotalDamage excludes overkill and uses post-HP-loss modifiers.
            Ev::Damage { target, dealer, card_source, amount: hp_lost, total_damage: blocked + hp_lost, blocked_damage: blocked, props, was_block_broken },
            None,
        );
        if !was_killed || !self.cr(target).is_dead() {
            // relic fire: core/hooks.py:1112
            self.fire(
                Hook::AfterDamageReceived,
                Ev::Damage { target, dealer, card_source, amount: hp_lost, total_damage: damage, blocked_damage: blocked, props, was_block_broken },
                None,
            );
        }
        if was_killed && kill {
            self.kill_creature(target);
        }
    }

    /// `CombatState.deal_damage(dealer, target, amount, props)` (powers/relics): an attack
    /// context around calculate + apply when there is a dealer; combat end deferred until done.
    pub fn deal_damage(&mut self, dealer: Option<CId>, target: CId, amount: i32, props: Props) {
        let _attack = self.orb_trace("attack", Some(target));
        // CreatureCmd.Damage has no IsOverOrEnding early return: an awaited
        // command (e.g. Corrupted.OnPlay after a killing hit) still loses HP.
        // A dead dealer/receiver is checked below, independently of combat end.
        self.damage_depth += 1;
        match dealer {
            None => {
                if !self.cr(target).is_dead() {
                    self.apply_damage(target, amount, props, None);
                }
            }
            Some(d) if !self.cr(d).is_dead() => {
                self.attack_stack.push(AttackCtx { attacker: Some(d), target: Some(target), props, base_damage: None, results: Vec::new() });
                // fire: core/combat.py:712-716
                self.fire(
                    Hook::BeforeAttack,
                    Ev::Attack {
                        attacker: Some(d),
                        target: Some(target),
                        props,
                        card: self.active_card,
                    },
                    None,
                );
                if !self.cr(target).is_dead() {
                    let dmg = self.calculate_damage(amount, Some(d), target, props);
                    self.apply_damage(target, dmg, props, Some(d));
                }
                let attack = self.attack_stack.pop().unwrap();
                self.finish_attack_hooks(attack);
            }
            Some(_) => {}
        }
        self.damage_depth -= 1;
        if self.damage_depth == 0 {
            self.check_combat_end();
        }
    }

    /// Card helper `_deal_damage_to_target`.
    pub fn deal_card_damage(&mut self, k: KRef, target: CId) {
        let owner = self.cards[k as usize].owner;
        if self.cr(owner).is_dead() {
            return;
        }
        let base = self.cards[k as usize].damage.unwrap_or(0);
        let dmg = self.calculate_damage(base, Some(owner), target, Props::MOVE);
        self.apply_damage_with_card_source(target, dmg, Props::MOVE, Some(owner), Some(k));
    }

    /// Card helper `_gain_block`.
    pub fn gain_card_block(&mut self, k: KRef) {
        let owner = self.cards[k as usize].owner;
        let _block = self.orb_trace("gain_block", Some(owner));
        if self.is_over_or_ending() || self.cr(owner).is_dead() {
            return;
        }
        let base = self.cards[k as usize].block.unwrap_or(0);
        let block = self.calculate_block(base, owner, Props::MOVE, Some(k));
        let before = self.cr(owner).block;
        self.gain_recorded_block(owner,block);
        let gained = self.cr(owner).block - before;
        if gained > 0 {
            self.record_block_gained_event(owner, Props::MOVE, Some(k));
            // fire: cards/effects.py:115
            self.fire(Hook::AfterBlockGained, Ev::BlockGained { creature: owner, amount: gained }, None);
        }
    }

    /// Monster helper `gain_move_block`.
    pub fn gain_move_block(&mut self, c: CId, amount: i32) {
        let _block = self.orb_trace("gain_block", Some(c));
        if self.is_over_or_ending() {
            return;
        }
        let block = self.calculate_block(amount, c, Props::MOVE, None);
        let before = self.cr(c).block;
        self.gain_recorded_block(c,block);
        let gained = self.cr(c).block - before;
        if gained > 0 {
            self.record_block_gained_event(c, Props::MOVE, None);
            // fire: monsters/block.py:25
            self.fire(Hook::AfterBlockGained, Ev::BlockGained { creature: c, amount: gained }, None);
        }
    }

    /// A monster move's `PowerCmd.Apply<T>(targets, ..)` with `targets = CombatState.PlayerCreatures`
    /// (`MonsterModel.PerformMove`; also `GetOpponentsOf(monster)`): every player creature in
    /// `Players` order, dead ones included (`Creature.CanReceivePowers` does not check life).
    pub fn monster_power_to_players(&mut self, monster: CId, id: PowerId, amount: i32) {
        for p in 0..self.players.len() as CId {
            self.apply_power_to(p, id, amount, Some(monster));
        }
    }

    /// `DamageCmd.Attack(..).FromMonster(m)` (Commands/Builders/AttackCommand.cs `Execute`): each
    /// hit targets every living player creature (`CombatState.PlayerCreatures`, pets excluded),
    /// damaged one by one in `Players` order (`CreatureCmd.Damage`); a dead attacker stops.
    pub fn monster_attack_players(&mut self, attacker: CId, base: i32, hits: i32) {
        self.start_monster_attack(attacker, base, hits, MonsterAttackTail::None);
    }

    /// CubexConstruct.cs:138-149 awaits the attack before gaining Strength.
    pub(crate) fn monster_attack_players_then_strength(&mut self, attacker: CId, base: i32, hits: i32, strength_after: i32) {
        self.start_monster_attack(attacker, base, hits, MonsterAttackTail::Strength(strength_after));
    }

    /// WaterfallGiant.cs:243-250 awaits every received callback before Weak/Steam.
    pub(crate) fn monster_stomp_players(&mut self, attacker: CId, base: i32) {
        self.start_monster_attack(attacker, base, 1, MonsterAttackTail::WaterfallStomp);
    }

    fn start_monster_attack(&mut self, attacker: CId, base: i32, hits: i32, tail: MonsterAttackTail) {
        let mut walk = MonsterAttackWalk { attacker, base, hits, targets: Vec::new(),
            card:None,
            next: 0, started: false, killed: Vec::new(), suck: None,
            callbacks: Vec::new(), applied: false, tail };
        if !self.resume_monster_attack(&mut walk) {
            self.pending_monster_attacks.push((self.pending_play.len(), walk));
        }
    }

    /// AttackCommand.TargetingAllOpponents -> CreatureCmd.Damage(targets).
    /// Reuse the same resumable apply/callback/death batch as monster attacks.
    pub(crate) fn deal_card_damage_all(&mut self,card:KRef) {
        let mut walk=MonsterAttackWalk {
            attacker:self.cards[card as usize].owner,
            base:self.cards[card as usize].damage.unwrap_or(0),hits:1,card:Some(card),
            targets:Vec::new(),next:0,started:false,killed:Vec::new(),suck:None,
            callbacks:Vec::new(),applied:false,tail:MonsterAttackTail::None,
        };
        if !self.resume_monster_attack(&mut walk) {
            self.pending_monster_attacks.push((self.pending_play.len(),walk));
        }
    }

    pub(crate) fn resume_monster_attack(&mut self, walk: &mut MonsterAttackWalk) -> bool {
        while walk.hits > 0 {
            if !walk.started {
                if self.cr(walk.attacker).is_dead() || self.is_over { break; }
                walk.targets = if walk.card.is_some() {self.hittable_enemies().to_vec()}
                    else {self.living_players().to_vec()};
                if walk.targets.is_empty() { break; }
                walk.next = 0;
                walk.killed.clear();
                walk.suck = self.find_power(walk.attacker, PowerId::Suck);
                if let Some(s) = walk.suck { self.powers[s as usize].vars[2] = 1; }
                walk.started = true;
            }
            // CreatureCmd.cs:147-264 applies every target before the callback
            // loop at 267-299. Each hit owns its immutable ordered results.
            if !walk.applied {
              self.collect_damage_callbacks = Some(Vec::new());
              for p in walk.targets.iter().copied() {
                if self.cr(p).is_dead() { continue; }
                let dmg = self.calculate_damage(walk.base, Some(walk.attacker), p, Props::MOVE);
                self.defer_kill = true;
                if let Some(card)=walk.card {
                    self.apply_damage_with_card_source(p,dmg,Props::MOVE,Some(walk.attacker),Some(card));
                } else {self.apply_damage(p, dmg, Props::MOVE, Some(walk.attacker));}
                // Redirected DamageResults precede their player's result in CreatureCmd.Damage.
                if self.players.is_player(p) && let Some(o) = self.osty(p)
                    && self.cr(o).is_dead() && !self.cr(o).death_processed
                {
                    walk.killed.push(o);
                }
                if self.cr(p).is_dead() {
                    walk.killed.push(p);
                }
              }
              walk.callbacks = self.collect_damage_callbacks.take().unwrap();
              walk.applied = true;
            }
            while let Some(callback) = walk.callbacks.get(walk.next).cloned() {
                walk.next += 1;
                self.finish_damage_callbacks(callback);
                if self.pending_choice.is_some() || self.fault.is_some() { return false; }
            }
            if let Some(s) = walk.suck {
                self.powers[s as usize].vars[2] = 0;
            }
            // CreatureCmd.Kill(IReadOnlyCollection), CreatureCmd.cs:312-348:
            // finish every death/prevention in the batch before checking defeat.
            // Otherwise an earlier dead ally ends combat before seat 2's Tail.
            self.damage_depth += 1;
            for k in std::mem::take(&mut walk.killed) {
                self.kill_creature(k);
            }
            self.damage_depth -= 1;
            self.check_combat_end();
            if self.is_over {
                break;
            }
            walk.hits -= 1;
            walk.started = false;
            walk.applied = false;
            walk.callbacks.clear();
        }
        let tail = std::mem::replace(&mut walk.tail, MonsterAttackTail::None);
        if !matches!(tail, MonsterAttackTail::None) {
            self.flush_pending_attack();
            match tail {
                MonsterAttackTail::Strength(amount) =>
                    { self.apply_power_to(walk.attacker, PowerId::Strength, amount, Some(walk.attacker)); }
                MonsterAttackTail::WaterfallStomp => {
                    self.monster_power_to_players(walk.attacker, PowerId::Weak, 1);
                    self.apply_power_to(walk.attacker, PowerId::SteamEruption, 3, Some(walk.attacker));
                }
                MonsterAttackTail::None => {}
            }
        }
        true
    }

    // ───────────────────────────── death ─────────────────────────────

    /// `CombatState.kill_creature` (r7 core/combat.py), lifecycle order kept exactly:
    /// before_death -> death prevention -> removal decisions -> after_death/on_ally_death ->
    /// power removal -> creature removal -> ending state.
    pub fn kill_creature(&mut self, c: CId) -> bool {
        self.kill_creature_with_force(c, false)
    }

    /// C# CreatureCmd.Kill(force: true) skips ShouldDie (Fairy in a Bottle / Lizard Tail),
    /// while still running the normal death hooks and cleanup.
    pub(crate) fn force_kill_creature(&mut self, c: CId) -> bool {
        self.kill_creature_with_force(c, true)
    }

    fn kill_creature_with_force(&mut self, c: CId, force: bool) -> bool {
        let cr = self.cr(c);
        if cr.escaped || (cr.is_dead() && cr.death_processed) {
            return false;
        }
        let current = cr.hp;
        let before = crate::between::PublicState::of(cr);
        {
            let cr = self.cr_mut(c);
            cr.hp = 0;
            // C# CreatureCmd.KillWithoutCheckingWinCondition only loses HP; death does
            // not clear block left by unblockable damage (e.g. Beckon.OnTurnEndInHand).
        }
        if current > 0 {
            self.after_hp_changed(c, -current);
        }
        // C# SwipePower.BeforeDeath runs for every instance, including each player's card.
        for o in self.all_creatures() {
            for p in self.cr(o).power_list() {
                if self.powers[p as usize].meta().death & 1 != 0 {
                    self.power_before_death(p, o, c);
                }
            }
        }
        // LizardTail.cs ShouldDieLate: Hook.ShouldDie checks the dying player's
        // own relics, not only LocalContext/seat zero. Pets remain separate.
        if !force && self.players.is_player(c) && self.prevent_player_death(c) {
            self.fire_death_listeners(c, true);
            self.fire_relic_after_death(c);
            self.fire_card_after_death(true);
            self.cr_mut(c).death_processed = false;
            self.check_combat_end();
            return true;
        }
        self.cr_mut(c).death_processed = true;
        self.record_creature_lifecycle(c, "death", before);
        // `isPrimaryEnemy` is read before the dead creature's powers go.
        let primary_enemy = self.cr(c).side == Side::Enemy && !self.is_secondary_enemy(c);
        // C# Creature.Powers includes every instanced power in application order.
        let original = self.cr(c).power_list();
        let mut remove: SmallVec<[(PRef, bool); 32]> = SmallVec::with_capacity(original.len());
        let mut remove_creature = true;
        for &p in &original {
            let meta = self.powers[p as usize].meta();
            let mut keep = false;
            if meta.listens(Hook::ShouldPowerBeRemovedAfterOwnerDeath) {
                keep = !self.power_removed_after_owner_death(p);
            }
            // Creature.cs:574 + Hook.cs:1994-2008 query ALL combat listeners.
            for q in self.all_creatures().into_iter().flat_map(|o| self.cr(o).power_list()).collect::<Vec<_>>() {
                if self.powers[q as usize].meta().listens(Hook::ShouldOtherPowerBeRemovedOnOwnerDeath) {
                    match self.power_other_removed_after_owner_death(q, p) {
                        Some(false) => {
                            keep = true;
                            break;
                        }
                        Some(true) => {},
                        None => {}
                    }
                }
            }
            remove.push((p, !keep));
            if meta.listens(Hook::ShouldCreatureBeRemovedFromCombatAfterDeath) {
                remove_creature &= self.power_creature_removed_after_owner_death(p);
            }
        }
        // PlayerCombatState.OnPetDied unregisters removable pets, but DieForYou
        // keeps a dead Osty in this roster for the next Summon.
        if remove_creature && let Some(owner) = self.cr(c).pet_owner {
            self.players[owner].pets.retain(|&pet| pet != c);
        }
        self.fire_death_listeners(c, false);
        for (p, rm) in remove {
            if rm {
                self.remove_power(c, p);
            }
        }
        // C# removes only enemies from combat (`creature.Side == Enemy`); a dead player stays
        // seated and can still receive powers (`Creature.CanReceivePowers`).
        if remove_creature && !self.players.is_player(c) {
            let before=crate::between::PublicState::of(self.cr(c));
            let cr = self.cr_mut(c);
            cr.escaped = true;
            cr.in_combat = false;
            if let Some(pos) = self.enemies.iter().position(|&e| e == c) {
                self.enemies.remove(pos);
            }
            self.sort_enemies_by_slot();
            if self.cr(c).side==Side::Enemy {
                self.record_creature_lifecycle(c,"remove",before);
            }
        }
        // relic fire: core/combat.py:3522 (relic after_death precedes the Queen sync in r7)
        self.fire_relic_after_death(c);
        self.fire_card_after_death(false);
        // _sync_monster_death_move_responses: Torch Head Amalgam -> Queen.
        if self.cr(c).monster_id == "TORCH_HEAD_AMALGAM" {
            // C# Queen.AfterDeath: only replace a pending BurnBrightForMe with Enrage.
            // Subsequent conditional branches already inspect the living Amalgam.
            let queens: Vec<CId> = self.enemies.iter().copied()
                .filter(|&e| self.cr(e).monster_id == "QUEEN" && self.cr(e).is_alive()).collect();
            for queen in queens {
                if let Some(ai) = self.cr_mut(queen).ai.as_mut() {
                    if ai.state_id(ai.current) == "BURN_BRIGHT_FOR_ME_MOVE" {
                        ai.current = ai.def().states.iter().position(|s| s.id == "ENRAGE_MOVE").unwrap() as u8;
                        ai.next_unset = false;
                    }
                }
            }
        }
        // CreatureCmd.KillWithoutCheckingWinCondition: a primary enemy whose living teammates are
        // all secondary (Minion) takes them down with it (`Kill(teammates)`).
        if primary_enemy {
            let teammates: ArrayVec<CId, 8> = self.alive_enemies();
            if !teammates.is_empty() && teammates.iter().all(|&t| self.is_secondary_enemy(t)) {
                // CreatureCmd.Kill(teammates) runs KillWithoutCheckingWinCondition
                // for the complete snapshot. Otherwise the first minion's death
                // ends combat and marks later living minions escaped before killing.
                self.damage_depth += 1;
                for t in teammates {
                    self.kill_creature(t);
                }
                self.damage_depth -= 1;
            }
        }
        if self.players.is_player(c) {
            self.player_died(c);
        }
        self.check_combat_end();
        true
    }

    /// The player branch of `CreatureCmd.KillWithoutCheckingWinCondition` plus
    /// `CombatManager.HandlePlayerDeath`: the orb queue empties; while a teammate lives the dead
    /// player's cards leave combat and its energy and stars drop to 0; the team loses once every
    /// player is dead (`CreatureCmd.Kill` -> `LoseCombat`).
    fn player_died(&mut self, p: CId) {
        // C# CreatureCmd.KillWithoutCheckingWinCondition: a player's living Osty dies too.
        if let Some(o) = self.osty(p).filter(|&o| self.cr(o).is_alive()) {
            self.kill_creature(o);
        }
        let orbs = &mut self.players[p].orbs;
        orbs.orbs.clear();
        orbs.capacity = 0;
        // C# KillWithoutCheckingWinCondition: DeactivateHooks follows Osty's Kill.
        self.players[p].hooks_active = false;
        if self.living_players().is_empty() {
            self.is_ending = true;
            return;
        }
        // `HandlePlayerDeath`: while the player's own card play is resolving
        // (`IsExecutingCardOrPotionEffect`), its cards stay until that play ends
        // (`EndCardOrPotionEffect` -> `RemoveDeadPlayerCardsFromCombat`, see resume_pending_play).
        if !self.has_executing_player_effect(p) {
            self.remove_dead_player_cards(p);
        }
        let ps = &mut self.players[p];
        ps.energy = 0;
        ps.stars = 0;
    }

    /// `CombatManager.RemoveDeadPlayerCardsFromCombat`: every pile of the dead player, the play
    /// pile included, leaves combat.
    pub(crate) fn remove_dead_player_cards(&mut self, p: CId) {
        let cards: Vec<_> = self.players[p].hand.iter()
            .chain(self.players[p].draw.iter()).chain(self.players[p].discard.iter())
            .chain(self.players[p].exhaust.iter()).chain(self.players[p].play.iter())
            .map(|&k| (k, self.card_pile(k))).collect();
        let ps = &mut self.players[p];
        ps.hand.clear();
        ps.draw.clear();
        ps.discard.clear();
        ps.exhaust.clear();
        ps.play.clear();
        // CombatManager.RemoveDeadPlayerCardsFromCombat -> CardPileCmd.RemoveFromCombat:
        // remove the complete batch, notify each old pile, then remove from state.
        if !self.living_players().is_empty() {
            for (k, old) in cards {
                self.fire_after_card_changed_piles(k, old, None);
            }
            self.revalidate_removed_player_hand_choice(p);
        }
    }

    /// after_death + on_ally_death for every power instance of every creature (r7 iterates
    /// `iter_power_instances()` per listener and calls both on each power).
    fn fire_death_listeners(&mut self, dead: CId, prevented: bool) {
        for o in self.all_creatures() {
            for p in self.cr(o).power_list() {
                let death = self.powers[p as usize].meta().death;
                if death & 2 != 0 {
                    self.power_after_death(p, o, dead, prevented);
                }
                if death & 4 != 0 {
                    self.power_on_ally_death(p, o, dead, prevented);
                }
            }
        }
    }

    /// r7 `power.before_death(owner, creature, combat)`.
    fn power_before_death(&mut self, p: PRef, owner: CId, dead: CId) {
        match self.powers[p as usize].id {
            PowerId::Heist => {
                if dead == owner {
                    // C# HeistPower.BeforeDeath: reward belongs to this instance's Target,
                    // not the whole party. Escape does not invoke this death hook.
                    // Each instance adds its own GoldReward(amount) (RewardsSet.cs:104).
                    if let Some(target) = self.powers[p as usize].target {
                        let amount = self.powers[p as usize].amount.max(0);
                        if target == PLAYER {
                            self.run.extra_gold_reward += amount;
                        } else {
                            self.players[target].extra_gold_reward += amount;
                        }
                        // C# CombatRoom.AddExtraReward appends in listener order.
                        self.end_rewards.push(crate::combat::EndReward::Gold { owner: target, amount });
                    }
                }
            }
            // C# SwipePower.cs:56-63 queues the original DeckVersion, not the
            // combat copy. SpecialCardReward.cs:77-80 adds it only when picked.
            PowerId::Swipe => {
                if dead != owner {
                    return;
                }
                let stored = self.powers[p as usize].vars[0];
                if stored <= 0 {
                    return;
                }
                let Some(target) = self.powers[p as usize].target else { return };
                if let Some(mut card) = self.players[target].stolen_deck_cards
                    .get_mut((stored - 1) as usize).and_then(Option::take)
                {
                    // SwipePower.BeforeDeath registers the DeckVersion for
                    // Target.Player, which may differ from its original owner.
                    card.owner = target;
                    self.end_rewards.push(crate::combat::EndReward::SpecialCard { owner: target, card });
                }
                self.powers[p as usize].vars[0] = 0;
            }
            // powers/monster.py:1370-1376 DoorRevivalPower.before_death.
            PowerId::DoorRevival => {
                if dead == owner {
                    if self.power_private_int(owner, PowerId::DoorRevival, 1) <= 0 {
                        self.set_power_private_int(
                            owner,
                            PowerId::DoorRevival,
                            1,
                            self.cr(owner).max_hp,
                        );
                    }
                    self.set_power_private_int(owner, PowerId::DoorRevival, 0, 1);
                }
            }
            _ => self.unported("power before_death"),
        }
    }

    /// r7 `power.after_death(owner, creature, combat, was_removal_prevented)`.
    fn power_after_death(&mut self, p: PRef, owner: CId, dead: CId, prevented: bool) {
        match self.powers[p as usize].id {
            // C# GuardedPower/CoveredPower.AfterDeath: only this applier's protection.
            PowerId::Guarded | PowerId::Covered | PowerId::MagicBomb => {
                if !prevented && self.powers[p as usize].applier == Some(dead) {
                    self.remove_power(owner, p);
                }
            }
            // powers/common.py ShrinkPower.after_death: owner.powers.pop(SHRINK).
            PowerId::Shrink => {
                if !prevented && self.powers[p as usize].applier == Some(dead) {
                    self.pop_primary_power(owner, PowerId::Shrink);
                }
            }
            // powers/monster.py SurprisePower.after_death
            PowerId::Surprise => {
                if !prevented && dead == owner && self.powers[p as usize].payload == 0 {
                    self.powers[p as usize].payload = 1;
                    // r7 creates and prepares Fat first, transfers stolen gold, inserts Sneaky,
                    // then inserts the already-prepared Fat.
                    let _ = self.rng.next_int(0, crate::rng::INT_MAX);
                    let (fat, fat_ai) = self.create_monster(MonsterKind::FatGremlin, 0);
                    let fat_id = self.prepare_enemy_in_slot(fat, fat_ai, Some("fat"));
                    // SurprisePower.AfterDeath: one Heist per Thievery instance, same Target.
                    let thefts: ArrayVec<(i32, Option<CId>), 8> = self
                        .cr(owner)
                        .power_list()
                        .iter()
                        .map(|&q| &self.powers[q as usize])
                        .filter(|pw| pw.id == PowerId::Thievery)
                        .map(|pw| (pw.vars[0], pw.target))
                        .collect();
                    for (stolen, target) in thefts {
                        // C# SurprisePower.AfterDeath marks the encounter when
                        // totalStolen > 0, not on an individual Thievery hit.
                        self.gremlin_gold_stolen |= stolen > 0;
                        if stolen > 0
                            && let Some(h) = self.creature_apply_power(fat_id, PowerId::Heist, stolen, None, None, false, true)
                        {
                            self.powers[h as usize].target = target;
                        }
                    }
                    let _ = self.rng.next_int(0, crate::rng::INT_MAX);
                    let (sneaky, sneaky_ai) = self.create_monster(MonsterKind::SneakyGremlin, 0);
                    self.add_enemy_in_slot(sneaky, sneaky_ai, Some("sneaky"));
                    self.insert_prepared_enemy(fat_id);
                }
            }
            PowerId::Dampen => {
                if !prevented && self.dampen_remove_caster(owner, dead) {
                    return;
                }
                if !prevented
                    && self.find_power(owner, PowerId::Dampen) == Some(p)
                    && !self.dampen_remove_caster(owner, dead)
                {
                    self.remove_power(owner, p);
                }
            }
            // powers/turn_effects.py ConstrictPower.after_death.
            PowerId::Constrict => {
                if !prevented && self.powers[p as usize].applier == Some(dead) {
                    self.pop_primary_power(owner, PowerId::Constrict);
                }
            }
            // PossessSpeedPower.cs:57-66 returns each dictionary entry to its
            // actual player, in first-stolen order, without clearing the dictionary.
            PowerId::PossessSpeed => {
                if !prevented && dead == owner {
                    for (_, receiver, stolen) in self.stolen_power_amounts.clone().into_iter().filter(|e| e.0 == p) {
                        self.creature_apply_power(receiver, PowerId::Dexterity, stolen, None, None, false, false);
                    }
                }
            }
            // powers/remaining_c.py ReattachPower.after_death.
            PowerId::Reattach => {
                if prevented || dead != owner {
                    return;
                }
                let has_living_segment = self.enemies.iter().copied().any(|e| {
                    e != owner
                        && self.cr(e).side == self.cr(owner).side
                        && self.cr(e).is_alive()
                        && self.has_power(e, PowerId::Reattach)
                });
                if has_living_segment {
                    self.set_power_private_int(owner, PowerId::Reattach, 0, 1);
                    self.force_enemy_state(owner, "DEAD_MOVE");
                }
            }
            // powers/remaining_a.py AdaptablePower.after_death.
            PowerId::Adaptable => {
                if !prevented && dead == owner {
                    self.set_power_private_int(owner, PowerId::Adaptable, 0, 1);
                    self.force_enemy_state(owner, "RESPAWN_MOVE");
                }
            }
            // powers/monster.py:672-714 IllusionPower.after_death.
            PowerId::Illusion => {
                if prevented || dead != owner {
                    return;
                }
                self.set_power_private_int(owner, PowerId::Illusion, 0, 1);
                let Some(ai) = self.cr(owner).ai.as_ref() else { return };
                // C# IllusionPower.AfterDeath defaults to StateLog.Last().Id;
                // an injected current stun is not part of that log.
                let Some(&follow_up) = ai.log.last() else {
                    self.unported("Illusion requires logged move follow-up");
                    return;
                };
                if !ai.can_transition_away(ai.current) {
                    return; // C# MonsterModel.SetMoveImmediate rejects replacement.
                }
                if let Some(ai) = self.cr_mut(owner).ai.as_mut() {
                    ai.revive_follow_up = follow_up;
                    ai.revive_performed_once = false;
                    ai.current = crate::monster::ILLUSION_REVIVE;
                    ai.next_unset = false;
                }
            }
            // powers/monster.py:822-875 SteamEruptionPower.after_death.
            PowerId::SteamEruption => {
                if prevented || dead != owner || self.cr(owner).monster_id != "WATERFALL_GIANT" {
                    return;
                }
                let cr = self.cr_mut(owner);
                cr.max_hp = 999_999_999;
                cr.hp = 999_999_999;
                cr.hp_display_infinite = true;
                cr.escaped = false;
                cr.death_processed = false;
                self.force_enemy_state(owner, "ABOUT_TO_BLOW_MOVE");
            }
            // powers/remaining_c.py:1048-1069 StockPower.after_death.
            PowerId::Stock => {
                if !prevented && dead == owner {
                    let amount = self.powers[p as usize].amount;
                    if amount > 0 {
                        self.spawn_stock_axebot(amount - 1);
                    }
                }
            }
            // powers/monster.py:1378-1390 DoorRevivalPower.after_death.
            PowerId::DoorRevival => {
                if dead != owner {
                    return;
                }
                if prevented {
                    self.set_power_private_int(owner, PowerId::DoorRevival, 0, 0);
                    return;
                }
                self.spawn_doormaker();
                self.force_enemy_state(owner, "DEAD_MOVE");
            }
            // powers/remaining_b.py:448-458 HexPower.after_death.
            PowerId::Hex => {
                if !prevented && self.powers[p as usize].applier == Some(dead) {
                    self.remove_power(owner, p);
                }
            }
            // powers/monster.py InfestedPower.after_death.
            PowerId::Infested => {
                if !prevented
                    && dead == owner
                    && self.power_private_int(owner, PowerId::Infested, 0) == 0
                {
                    self.set_power_private_int(owner, PowerId::Infested, 0, 1);
                    self.spawn_infested_wrigglers();
                }
            }
            // powers/remaining_b.py PossessStrengthPower.after_death.
            PowerId::PossessStrength => {
                if !prevented && dead == owner {
                    for (_, receiver, stolen) in self.stolen_power_amounts.clone().into_iter().filter(|e| e.0 == p) {
                        self.creature_apply_power(receiver, PowerId::Strength, stolen, None, None, false, false);
                    }
                }
            }
            _ => self.unported("power after_death"),
        }
    }

    /// r7 `power.on_ally_death(owner, dead_creature, combat, was_removal_prevented)`.
    fn power_on_ally_death(&mut self, p: PRef, owner: CId, dead: CId, prevented: bool) {
        let same_side = dead != owner && self.cr(dead).side == self.cr(owner).side;
        match self.powers[p as usize].id {
            PowerId::Dampen => {
                if prevented || !self.dampen_remove_caster(owner, dead) {
                    if !prevented && self.find_power(owner, PowerId::Dampen) == Some(p) {
                        self.remove_power(owner, p);
                    }
                }
            }
            // powers/monster.py RavenousPower: Strength + one-turn stun.
            PowerId::Ravenous => {
                if !prevented && same_side && self.cr(owner).is_alive() {
                    let amount = self.powers[p as usize].amount;
                    self.creature_apply_power(owner, PowerId::Strength, amount, None, None, false, false);
                    self.stun_enemy(owner);
                }
            }
            // powers/monster.py CrabRagePower: 6 Strength, 99 unpowered block, then pop itself.
            PowerId::CrabRage => {
                if same_side {
                    self.creature_apply_power(owner, PowerId::Strength, 6, None, None, false, false);
                    let before = self.cr(owner).block;
                    self.gain_recorded_block(owner,99);
                    if self.cr(owner).block > before {
                        self.fire(
                            Hook::AfterBlockGained,
                            Ev::BlockGained {
                                creature: owner,
                                amount: self.cr(owner).block - before,
                            },
                            None,
                        );
                    }
                    self.pop_primary_power(owner, PowerId::CrabRage);
                }
            }
            // powers/monster.py:1222-1238 SurroundedPower.on_ally_death.
            PowerId::Surrounded => {
                if prevented || self.cr(dead).side == self.cr(owner).side {
                    return;
                }
                let hittable = self.hittable_enemies();
                let Some(&first) = hittable.first() else { return };
                let all_left = hittable.iter().all(|&e| self.has_power(e, PowerId::BackAttackLeft));
                let all_right = hittable.iter().all(|&e| self.has_power(e, PowerId::BackAttackRight));
                if all_left || all_right {
                    if all_left {
                        self.set_power_private_int(owner, PowerId::Surrounded, 0, 1);
                    } else {
                        self.set_power_private_int(owner, PowerId::Surrounded, 0, 0);
                    }
                    let _ = first;
                }
            }
            _ => self.unported("power on_ally_death"),
        }
    }

    /// r7 `should_power_be_removed_after_owner_death` overrides.
    fn power_removed_after_owner_death(&mut self, p: PRef) -> bool {
        match self.powers[p as usize].id {
            PowerId::DieForYou
            | PowerId::Minion
            | PowerId::Reattach
            | PowerId::Adaptable
            | PowerId::Illusion
            | PowerId::SteamEruption
            | PowerId::PainfulStabs
            | PowerId::DoorRevival => false,
            _ => {
                self.unported("should_power_be_removed_after_owner_death");
                true
            }
        }
    }

    /// r7 `should_other_power_be_removed_on_owner_death`; `None` leaves the target power's
    /// own decision unchanged.
    fn power_other_removed_after_owner_death(&mut self, listener: PRef, target: PRef) -> Option<bool> {
        match self.powers[listener as usize].id {
            // powers/monster.py:742-749. Illusion removes debuffs and preserves non-debuffs.
            PowerId::Illusion => {
                let pw = &self.powers[target as usize];
                // IllusionPower.cs:54-61 preserves ITemporaryPower debuffs too.
                // Type is fixed for Strength even at negative Amount;
                // PowerModel.GetTypeForAmount is NOT used here.
                Some(pw.meta().power_type == PowerType::Debuff && !pw.meta().temporary)
            }
            _ => {
                self.unported("should_other_power_be_removed_on_owner_death");
                None
            }
        }
    }

    fn power_creature_removed_after_owner_death(&mut self, p: PRef) -> bool {
        match self.powers[p as usize].id {
            PowerId::DieForYou
            | PowerId::Reattach
            | PowerId::Adaptable
            | PowerId::Illusion
            | PowerId::SteamEruption
            | PowerId::PainfulStabs
            | PowerId::DoorRevival => false,
            _ => {
                self.unported("should_creature_be_removed_from_combat_after_death");
                true
            }
        }
    }

    fn force_enemy_state(&mut self, owner: CId, state: &str) {
        let Some(ai) = self.cr(owner).ai.as_ref() else { return };
        let index = crate::content::state_index(ai.def(), state);
        if let Some(ai) = self.cr_mut(owner).ai.as_mut() {
            ai.current = index;
            ai.next_unset = false;
        }
    }

    /// powers/monster.py:716-722 IllusionPower.revive.
    pub(crate) fn power_illusion_revive(&mut self, owner: CId) {
        if self.power_private_int(owner, PowerId::Illusion, 0) == 0 {
            return;
        }
        self.set_power_private_int(owner, PowerId::Illusion, 0, 0);
        let max_hp = self.cr(owner).max_hp;
        let cr = self.cr_mut(owner);
        cr.hp = max_hp;
        cr.escaped = false;
        cr.death_processed = false;
    }

    /// `CombatState.should_owner_death_trigger_fatal` for currently ported overrides.
    pub(crate) fn should_owner_death_trigger_fatal(&mut self, target: CId) -> bool {
        let powers = self.cr(target).powers.clone();
        for p in powers {
            if !self.powers[p as usize].meta().listens(Hook::ShouldOwnerDeathTriggerFatal) {
                continue;
            }
            match self.powers[p as usize].id {
                PowerId::Minion => return false,
                PowerId::Reattach => {
                    let other_living_segment = self.enemies.iter().copied().any(|e| {
                        e != target
                            && self.cr(e).side == self.cr(target).side
                            && self.cr(e).is_alive()
                            && self.has_power(e, PowerId::Reattach)
                    });
                    if other_living_segment {
                        return false;
                    }
                }
                _ => {
                    self.unported("should_owner_death_trigger_fatal");
                    return false;
                }
            }
        }
        true
    }

    /// `owner.powers.pop(power_id, None)`: drops the primary dict entry without on_removed
    /// (extra instances stay where they are).
    fn pop_primary_power(&mut self, owner: CId, id: PowerId) {
        let cr = &mut self.creatures[owner as usize];
        if let Some(pos) = cr.powers.iter().position(|&q| self.powers[q as usize].id == id) {
            cr.powers.remove(pos);
        }
    }

    /// `CombatState.stun_enemy(creature)`: the next move becomes a one-turn stun that then
    /// continues with the move that was selected.
    pub fn stun_enemy(&mut self, e: CId) -> bool {
        self.stun_enemy_with_move(e, crate::monster::StunMove::Nothing, None)
    }

    /// C# Creature.StunInternal -> MonsterModel.SetMoveImmediate: an unperformed
    /// must-perform move rejects replacement, preserving both its body and follow-up.
    pub(crate) fn stun_enemy_with_move(
        &mut self, e: CId, body: crate::monster::StunMove, follow_up: Option<u8>,
    ) -> bool {
        let cr = &mut self.creatures[e as usize];
        if cr.is_player || cr.is_dead() {
            return false;
        }
        let Some(ai) = cr.ai.as_mut() else { return false };
        if !ai.can_transition_away(ai.current) {
            return true;
        }
        // C# Creature.StunInternal defaults to StateLog.Last(). The interrupted
        // rolled move is already there; an injected/performed stun is never there.
        if let Some(next) = follow_up {
            ai.stun_follow_up = next;
        } else if let Some(&last) = ai.log.last() {
            ai.stun_follow_up = last;
        }
        ai.performed_once &= !(1u64 << 63);
        ai.stun_move = body;
        ai.current = crate::monster::STUNNED;
        // C# SetMoveImmediate/ForceCurrentState do not perform a move.
        true
    }

    /// `_prevent_death_if_needed` for the player: Fairy in a Bottle first, then relic
    /// `should_die_late` (Lizard Tail), in r7 order.
    fn prevent_player_death(&mut self, owner: CId) -> bool {
        // FairyInABottle.cs:27 ShouldDie checks Owner.Creature. Its
        // AfterPreventingDeath calls OnUseWrapper, consuming that owner's slot
        // before OnUse heals that same creature; Fairy precedes ShouldDieLate.
        let Some(slots) = self.potion_slots_for_player(owner) else {
            self.unported("death prevention requires owner potion inventory");
            return false;
        };
        if let Some(slot) = slots.iter().position(|p| *p == Some(PotionId::Fairyinabottle)) {
            if owner == PLAYER { self.run.potions[slot] = None; }
            else { self.players[owner].potions.as_mut().unwrap()[slot] = None; }
            self.fire(Hook::BeforePotionUsed, Ev::None, None);
            self.potion_effect_for_player(owner, PotionId::Fairyinabottle, Some(owner));
            self.fire_relic_after_potion_used(owner, PotionId::Fairyinabottle, Some(owner));
            return true;
        }
        for r in 0..self.run.relics.len() {
            if self.relic_should_prevent_player_death(r, owner) {
                return true;
            }
        }
        false
    }

    pub fn sort_enemies_by_slot(&mut self) {
        if self.encounter_slots.is_empty() {
            return;
        }
        let slots = self.encounter_slots;
        let creatures = &self.creatures;
        let mut v: Vec<CId> = self.enemies.iter().copied().collect();
        v.sort_by_key(|&e| creatures[e as usize].slot.and_then(|s| slots.iter().position(|x| *x == s)).unwrap_or(slots.len()));
        self.enemies.clear();
        self.enemies.extend(v);
    }

    // ───────────────────────────── combat end ─────────────────────────────

    /// `_check_combat_end`.
    pub fn check_combat_end(&mut self) {
        if self.damage_depth > 0 {
            return;
        }
        // r7 _check_combat_end uses the same live victory predicate as
        // CombatManager.IsEnding, including powers that keep a dead owner's
        // combat alive while a revive is pending.
        let victory_ready = self.is_combat_ending_live();
        let loss_ready = self.living_players().is_empty();
        self.is_ending = victory_ready || loss_ready;
        if self.is_ending && (self.in_play_phase || self.end_defer_depth > 0
            || self.pending_turn_end_hooks.is_some() || self.turn_end_join.is_some()
            || self.has_parked_turn_end_tasks()) {
            return;
        }
        if victory_ready {
            for e in self.alive_enemies() {
                self.cr_mut(e).escaped = true;
            }
            self.end_combat(true);
        } else if loss_ready {
            self.end_combat(false);
        }
    }

    fn end_combat(&mut self, won: bool) {
        if self.is_over {
            return;
        }
        self.is_over = true;
        self.won = won;
        crate::between::stage("combat_end",None);
        self.pending_exhaust_walks.clear();
        for (_, walk) in std::mem::take(&mut self.pending_damage_walks) {
            self.cancel_damage_received_walk(walk);
        }
        self.pending_puzzle_draws.clear();
        self.pending_monster_attacks.clear();
        if self.run.choice_scheduling.is_live() {
            // CombatManager.CheckWinCondition cancels queued/paused hook actions.
            self.cancel_live_orb_walks();
            self.pending_choice = None;
            self.pending_choice_orb_context = None;
            let spans = std::mem::take(&mut self.pending_choice_orb_spans);
            self.finish_orb_context(&spans, "cancel");
            self.choice_options.clear();
            self.choice_selected.clear();
            self.pending_player_turn_hooks = None;
            self.pending_live_setup = None;
            self.pending_turn_setup = None;
            self.deferred_setup_choices.clear();
            self.signalled_setup_choice = None;
        }
        if !won {
            self.record_combat_result();
            return;
        }
        // `Player.ReviveBeforeCombatEnd`: a dead teammate comes back with 1 hp (`CreatureCmd.Heal`).
        for p in 0..self.players.len() as CId {
            let before=crate::between::PublicState::of(self.cr(p));
            let cr = self.cr_mut(p);
            if cr.is_dead() {
                cr.hp = 1;
                cr.death_processed = false;
                // Creature.HealInternal -> Player.ActivateHooks, before AfterCombatEnd.
                self.players[p].hooks_active = true;
                self.record_public_heal(p,"revive",before,1);
            }
        }
        self.fire(Hook::AfterCombatEnd, Ev::None, None);
        // fire: run/run_manager.py:1943-1949
        self.fire_card_after_combat_end();
        // _clear_combat_end_state: every player's block/powers and piles.
        for owner in 0..self.players.len() as CId {
            let player = self.cr_mut(owner);
            player.block = 0;
            player.powers.clear();
            player.extra.clear();
            let p = &mut self.players[owner];
            p.hand.clear();
            p.draw.clear();
            p.discard.clear();
            p.exhaust.clear();
            p.play.clear();
            // PlayerCombatState.AfterCombatEnd clears _pets, not the allies' powers.
            p.pets.clear();
        }
        self.fire_after_combat_victory();
        self.record_combat_result();
        let _ = Side::Player;
    }
}
