//! r7 `CombatState.auto_play_card` / `auto_play_from_draw`.

use crate::card::{CId, KRef};
use crate::combat::{Combat, PileKind, PilePos};
use crate::tables::TargetType;

#[derive(Clone, Debug)]
pub(crate) enum AutoplayWork {
    Snapshot(Vec<KRef>, bool),
    RandomDraw(CId, usize),
    CollectDraw(CId, usize, Vec<KRef>, bool),
}

impl Combat {
    /// Eidolon.OnPlay awaits each AutoPlay from an immutable exhaust snapshot.
    pub(crate) fn autoplay_snapshot(&mut self, cards: Vec<KRef>) {
        self.autoplay_card_snapshot(cards, false);
    }

    /// BeatDown.cs:28-44 rolls an explicit random enemy immediately before EACH
    /// awaited AutoPlay, not when taking the snapshot or before a prior selector finishes.
    pub(crate) fn autoplay_card_snapshot(&mut self, cards: Vec<KRef>, random_enemy_targets: bool) {
        self.with_live_queue_action(|c| c.autoplay_card_snapshot_action(cards, random_enemy_targets));
    }

    fn autoplay_card_snapshot_action(&mut self, cards: Vec<KRef>, random_enemy_targets: bool) {
        self.autoplay_snapshots.push((self.pending_play.len(), AutoplayWork::Snapshot(cards, random_enemy_targets)));
        while self.resume_autoplay_snapshot() {}
    }

    pub(crate) fn autoplay_random_draw(&mut self, owner: CId, count: usize) {
        self.with_live_queue_action(|c| c.autoplay_random_draw_action(owner, count));
    }

    fn autoplay_random_draw_action(&mut self, owner: CId, count: usize) {
        self.autoplay_snapshots.push((self.pending_play.len(), AutoplayWork::RandomDraw(owner, count)));
        while self.resume_autoplay_snapshot() {}
    }

    pub(crate) fn resume_autoplay_snapshot(&mut self) -> bool {
        if self.pending_choice.is_some() { return false; }
        let Some((_,work))=crate::flow::take_suspended_at_depth(
            &mut self.autoplay_snapshots,self.pending_play.len(),|w|w.0) else {return false;};
        // Catastrophe.OnPlay keeps sampling after a lethal child (AutoPlay itself
        // then refuses). Preserve those Shuffle draws; snapshot autoplay has no RNG
        // until its child command starts, so it can stop once combat is ending.
        if self.is_over_or_ending() && matches!(work, AutoplayWork::Snapshot(..)) { return true; }
        let random_enemy_targets = matches!(&work, AutoplayWork::Snapshot(_, true));
        let card = match work {
            AutoplayWork::CollectDraw(owner, count, cards, force_exhaust) => {
                self.collect_autoplay_draw(owner, count, cards, force_exhaust);
                return true;
            }
            AutoplayWork::Snapshot(mut cards, random_targets) => {
                if cards.is_empty() { return true; }
                let card = cards.remove(0);
                if !cards.is_empty() { self.autoplay_snapshots.push((self.pending_play.len(), AutoplayWork::Snapshot(cards, random_targets))); }
                card
            }
            AutoplayWork::RandomDraw(owner, count) => {
                if count == 0 { return true; }
                let mut cards: Vec<_> = self.players[owner].draw.iter().copied()
                    .filter(|&q| !self.cards[q as usize].is_unplayable()).collect();
                if cards.is_empty() { cards.extend(self.players[owner].draw.iter().copied()); }
                if cards.is_empty() { return true; }
                cards.sort_by_key(|&q| (crate::content::CARD_NAME_RANK[self.cards[q as usize].id.index()], self.cards[q as usize].upgraded));
                self.run.rngs.shuffle.shuffle(&mut cards);
                if count > 1 { self.autoplay_snapshots.push((self.pending_play.len(), AutoplayWork::RandomDraw(owner, count - 1))); }
                cards[0]
            }
        };
        let target = if random_enemy_targets && self.card_target_type(card) == TargetType::AnyEnemy {
            let enemies = self.hittable_enemies();
            if enemies.is_empty() { None }
            else { Some(enemies[self.run.rngs.combat_targets.choice_index(enemies.len())]) }
        } else { None };
        self.auto_play_card(card, target, false);
        true
    }
    /// C# CardCmd.AutoPlay: random living other player, on CombatTargets.
    pub(crate) fn random_ally_target(&mut self, owner: CId) -> Option<CId> {
        let allies: Vec<CId> = (0..self.players.len() as CId)
            .filter(|&p| p != owner && self.cr(p).is_alive())
            .collect();
        if allies.is_empty() {
            None
        } else {
            Some(allies[self.run.rngs.combat_targets.choice_index(allies.len())])
        }
    }

    /// `CombatState.can_auto_play_card` + `auto_play_card`.
    pub fn auto_play_card(
        &mut self,
        k: KRef,
        target: Option<CId>,
        force_exhaust: bool,
    ) -> bool {
        self.with_live_queue_action(|c| c.auto_play_card_action(k, target, force_exhaust))
    }

    fn auto_play_card_action(&mut self, k: KRef, target: Option<CId>, force_exhaust: bool) -> bool {
        // fire: core/combat.py:3976-4013
        if self.is_over_or_ending() {
            return false;
        }
        let owner = self.cards[k as usize].owner;
        // Commands/CardCmd.AutoPlay returns before touching piles or RNG for a dead owner.
        if self.cr(owner).is_dead() {
            return false;
        }
        if self.cards[k as usize].is_unplayable() || !self.powers_allow_card_play(k) {
            self.rejected_autoplay_result(k,force_exhaust);
            return false;
        }
        for r in 0..self.run.relics.len() {
            if self.relic_should_play(r, self.run.relics[r].owner, k) == Some(false) {
                self.rejected_autoplay_result(k,force_exhaust);
                return false;
            }
        }
        if !self.cards_allow_hand_card_play(k, true) {
            self.rejected_autoplay_result(k,force_exhaust);
            return false;
        }

        let target_type = self.card_target_type(k);
        let resolved = target.or_else(|| match target_type {
            TargetType::Self_ | TargetType::None => Some(owner),
            TargetType::AnyEnemy => {
                let enemies = self.hittable_enemies();
                if enemies.is_empty() {
                    None
                } else {
                    let i = self.run.rngs.combat_targets.choice_index(enemies.len());
                    Some(enemies[i])
                }
            }
            TargetType::AnyAlly => self.random_ally_target(owner),
            TargetType::AllEnemies | TargetType::AllAllies | TargetType::RandomEnemy => None,
        });
        if matches!(target_type, TargetType::AnyEnemy | TargetType::AnyAlly) && resolved.is_none() {
            self.rejected_autoplay_result(k,force_exhaust);
            return false;
        }
        self.execute_card_play(k, resolved, false, force_exhaust);
        self.check_combat_end();
        true
    }

    fn rejected_autoplay_result(&mut self,k:KRef,force_exhaust:bool) {
        // CardModel.MoveToResultPileWithoutPlaying checks ExhaustOnNextPlay
        // and the canonical Exhaust keyword even when OnPlay never executes.
        let exhaust=force_exhaust||self.cards[k as usize].exhausts();
        self.move_card_to_pile(k,if exhaust {PileKind::Exhaust}else{PileKind::Discard},PilePos::Bottom);
        if exhaust {self.fire_after_card_exhausted(k);}
    }

    /// Pre-collect the requested draw cards before any of them is played, exactly like r7.
    pub fn auto_play_from_draw(&mut self, owner: CId, count: usize, force_exhaust: bool) {
        self.with_live_queue_action(|c| c.auto_play_from_draw_action(owner, count, force_exhaust));
    }

    fn auto_play_from_draw_action(&mut self, owner: CId, count: usize, force_exhaust: bool) {
        // fire: core/combat.py:4019-4039
        if self.is_over_or_ending() || count == 0 {
            return;
        }
        self.collect_autoplay_draw(owner, count, Vec::new(), force_exhaust);
    }

    fn collect_autoplay_draw(&mut self, owner: CId, count: usize, mut cards: Vec<KRef>, force_exhaust: bool) {
        for i in 0..count {
            self.shuffle_if_needed(owner);
            if self.pending_choice.is_some() {
                // CardPileCmd.AutoPlayFromDrawPile awaits shuffle selection before
                // collecting Play cards; never run ahead of an unresolved selector.
                self.autoplay_snapshots.push((self.pending_play.len(),
                    AutoplayWork::CollectDraw(owner, count - i, cards, force_exhaust)));
                return;
            }
            if self.players[owner].draw.is_empty() {
                break;
            }
            if cards.len() == 128 {
                self.unported("auto-play batch > 128");
                break;
            }
            let k = self.players[owner].draw.remove(0);
            self.cards[k as usize].owner = owner;
            self.players[owner].play.push(k);
            self.fire_after_card_changed_piles(k, PileKind::Draw, None);
            cards.push(k);
        }
        if !force_exhaust {
            self.autoplay_snapshot(cards.into_iter().collect());
            return;
        }
        for k in cards {
            if self.is_over {
                break;
            }
            self.auto_play_card(k, None, force_exhaust);
            if self.pending_choice.is_some() {
                break;
            }
        }
    }

    pub(crate) fn remove_card_from_piles(&mut self, k: KRef) {
        // fire: core/combat.py:4007-4008
        let p = &mut self.players[self.cards[k as usize].owner];
        if let Some(i) = p.hand.iter().position(|&q| q == k) {
            p.hand.remove(i);
        }
        for pile in [&mut p.draw, &mut p.discard, &mut p.exhaust, &mut p.play] {
            if let Some(i) = pile.iter().position(|&q| q == k) {
                pile.remove(i);
            }
        }
    }
}
