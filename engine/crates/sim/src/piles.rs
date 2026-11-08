//! Commands/CardPileCmd and CardCmd.Transform lifecycle notifications.
use crate::{card::KRef, combat::{Combat, PileKind, PilePos}, hooks::Ev, tables::*};

/// C# AbstractModel identity supplied as CardPileCmd.Add's clonedBy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloneSource { Card(KRef), Power(crate::creature::PRef), Relic(usize) }

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lifecycle {
    Changed(KRef, PileKind),
    ChangedLate(KRef, PileKind),
    TransformedFrom(KRef),
    TransformedTo(KRef),
    Generated(KRef, Option<crate::card::CId>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::{RunManager, RunOptions};

    fn combat() -> Combat {
        let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run, 1);
        c.players[0].draw.clear();
        c.players[0].hand.clear();
        c
    }

    #[test]
    fn add_draw_reorder_discard_and_exhaust_notify_only_real_transitions() {
        let mut c = combat();
        let k = c.new_card(0, CardId::DefendIronclad, false).unwrap();
        c.add_generated_card(k, PileKind::Draw, PilePos::Bottom, true);
        assert_eq!(c.hist.card_lifecycle, vec![
            Lifecycle::Changed(k, PileKind::None), Lifecycle::ChangedLate(k, PileKind::None),
            Lifecycle::Generated(k, Some(0))]);
        c.hist.card_lifecycle.clear();
        c.move_card_to_pile(k, PileKind::Draw, PilePos::Top);
        assert!(c.hist.card_lifecycle.is_empty());
        c.draw_cards(0, 1, false);
        c.discard_cards(0, &[k], 0);
        c.move_card_to_pile(k, PileKind::Exhaust, PilePos::Bottom);
        c.fire_after_card_exhausted(k);
        assert_eq!(c.hist.card_lifecycle, vec![
            Lifecycle::Changed(k, PileKind::Draw), Lifecycle::ChangedLate(k, PileKind::Draw),
            Lifecycle::Changed(k, PileKind::Hand), Lifecycle::ChangedLate(k, PileKind::Hand),
            Lifecycle::Changed(k, PileKind::Discard), Lifecycle::ChangedLate(k, PileKind::Discard)]);
        assert_eq!(c.card_pile(k), PileKind::Exhaust);
        assert_eq!(c.fault, None);
    }

    #[test]
    fn seance_fires_normal_then_late_then_from_then_to_even_for_same_pile_type() {
        let mut c = combat();
        let old = c.new_card(0, CardId::SovereignBlade, false).unwrap();
        c.add_generated_card(old, PileKind::Draw, PilePos::Bottom, true);
        c.hist.card_lifecycle.clear();
        c.apply_power_to(0, PowerId::PillarOfCreation, 3, Some(0));
        c.apply_power_to(0, PowerId::Arsenal, 2, Some(0));
        let seance = c.new_card(0, CardId::Seance, false).unwrap();
        c.play_card_effect(seance, None);
        let new = c.players[0].draw[0];
        assert_eq!(c.cards[new as usize].id, CardId::Soul);
        assert_eq!(c.card_pile(old), PileKind::None);
        assert_eq!(c.hist.card_lifecycle, vec![
            Lifecycle::Changed(new, PileKind::Draw),
            Lifecycle::ChangedLate(new, PileKind::Draw),
            Lifecycle::TransformedFrom(old), Lifecycle::TransformedTo(new),
            Lifecycle::Generated(new, Some(0))]);
        assert_eq!(c.cr(0).block, 3);
        assert_eq!(c.power_amount(0, PowerId::Strength), 2);
        assert_eq!(c.hist.generated_combat, 2);
        assert_eq!(c.fault, None);
    }

    #[test]
    fn compact_generated_notifications_follow_the_entire_transform_batch() {
        let mut c = combat();
        let first = c.new_card(0, CardId::Wound, false).unwrap();
        let second = c.new_card(0, CardId::Dazed, false).unwrap();
        c.players[0].hand.extend([first, second]);
        let compact = c.new_card(0, CardId::Compact, false).unwrap();
        c.apply_power_to(0, PowerId::PillarOfCreation, 3, Some(0));
        c.apply_power_to(0, PowerId::Arsenal, 2, Some(0));
        c.play_card_effect(compact, None);
        let new_first = first + 3;
        let new_second = second + 3;
        assert_eq!(c.hist.card_lifecycle, vec![
            Lifecycle::Changed(new_first, PileKind::Hand), Lifecycle::ChangedLate(new_first, PileKind::Hand),
            Lifecycle::TransformedFrom(first), Lifecycle::TransformedTo(new_first),
            Lifecycle::Changed(new_second, PileKind::Hand), Lifecycle::ChangedLate(new_second, PileKind::Hand),
            Lifecycle::TransformedFrom(second), Lifecycle::TransformedTo(new_second),
            Lifecycle::Generated(new_first, Some(0)), Lifecycle::Generated(new_second, Some(0))]);
        assert_eq!(c.cr(0).block, 6 + 3 + 3);
        assert_eq!(c.power_amount(0, PowerId::Strength), 4);
        assert_eq!(c.players[0].hand.as_slice(), &[new_second, new_first]);
        assert!(c.players[0].hand.iter().all(|&q| c.cards[q as usize].id == CardId::Fuel));
        assert_eq!(c.fault, None);
    }

    #[test]
    fn batch_add_keeps_old_piles_and_checks_overflow_after_each_insertion() {
        let mut c = combat();
        for _ in 0..crate::combat::MAX_HAND_SIZE - 1 {
            let k = c.new_card(0, CardId::DefendIronclad, false).unwrap();
            c.players[0].hand.push(k);
        }
        let first = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
        let second = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
        c.players[0].draw.extend([first, second]);
        c.move_cards_to_pile(&[first, second], PileKind::Hand, PilePos::Bottom);
        assert_eq!(c.card_pile(first), PileKind::Hand);
        assert_eq!(c.card_pile(second), PileKind::Discard);
        assert_eq!(c.hist.card_lifecycle, vec![
            Lifecycle::Changed(first, PileKind::Draw), Lifecycle::ChangedLate(first, PileKind::Draw),
            Lifecycle::Changed(second, PileKind::Draw), Lifecycle::ChangedLate(second, PileKind::Draw)]);
        // The first callback must already see the second card's completed insertion.
        assert!(c.hist.pile_contents_on_change[0].1.contains(&(second, PileKind::Discard)));
    }

    #[test]
    fn autoplay_already_in_play_notifies_only_the_result_pile_move() {
        let mut c = combat();
        let k = c.new_card(0, CardId::DefendIronclad, false).unwrap();
        c.players[0].play.push(k);
        c.execute_card_play(k, None, false, false);
        assert_eq!(c.hist.card_lifecycle, vec![
            Lifecycle::Changed(k, PileKind::Play), Lifecycle::ChangedLate(k, PileKind::Play)]);
        assert_eq!(c.fault, None);
    }

    #[test]
    fn batch_add_during_combat_ending_leaves_piles_and_callbacks_untouched() {
        let mut c = combat();
        let k = c.new_card(0, CardId::DefendIronclad, false).unwrap();
        c.players[0].draw.push(k);
        c.is_ending = true;
        c.move_cards_to_pile(&[k], PileKind::Hand, PilePos::Bottom);
        assert_eq!(c.players[0].draw.as_slice(), &[k]);
        assert!(c.players[0].hand.is_empty());
        assert!(c.hist.card_lifecycle.is_empty());
    }

    #[test]
    fn manual_play_and_dead_player_removal_keep_the_original_pile() {
        let mut c = combat();
        let k = c.new_card(0, CardId::DefendIronclad, false).unwrap();
        c.add_generated_card(k, PileKind::Hand, PilePos::Bottom, true);
        c.hist.card_lifecycle.clear();
        c.players[0].energy = 3;
        assert!(c.play_card(0, 0, None));
        assert_eq!(c.hist.card_lifecycle, vec![
            Lifecycle::Changed(k, PileKind::Hand), Lifecycle::ChangedLate(k, PileKind::Hand),
            Lifecycle::Changed(k, PileKind::Play), Lifecycle::ChangedLate(k, PileKind::Play)]);
        c.add_player(50, 50, 3, 0, &[]);
        c.hist.card_lifecycle.clear();
        c.creatures[0].hp = 0;
        c.remove_dead_player_cards(0);
        assert_eq!(c.hist.card_lifecycle, vec![
            Lifecycle::Changed(k, PileKind::Discard), Lifecycle::ChangedLate(k, PileKind::Discard)]);
    }
}

impl Combat {
    pub fn card_pile(&self, k: KRef) -> PileKind {
        let p = &self.players[self.cards[k as usize].owner];
        if p.hand.contains(&k) { PileKind::Hand }
        else if p.draw.contains(&k) { PileKind::Draw }
        else if p.discard.contains(&k) { PileKind::Discard }
        else if p.exhaust.contains(&k) { PileKind::Exhaust }
        else if p.play.contains(&k) { PileKind::Play }
        else { PileKind::None }
    }

    /// Hook.AfterCardChangedPiles: ALL normal listeners, then ALL late listeners.
    /// The audited combat card/monster overrides (SovereignBlade and SoulFysh)
    /// only update VFX/music. Relic overrides and Hoarder act only on Deck adds,
    /// which belong to the run layer, not these combat piles.
    pub fn fire_after_card_changed_piles(&mut self, card: KRef, old_pile: PileKind, cloned_by: Option<CloneSource>) {
        self.record_card_move(card,old_pile);
        let ev = Ev::PileChanged { card, old_pile, cloned_by };
        #[cfg(test)]
        {
            let piles = (0..self.cards.len()).map(|k| (k as KRef, self.card_pile(k as KRef))).collect();
            self.hist.pile_contents_on_change.push((card, piles));
        }
        #[cfg(test)]
        self.hist.card_lifecycle.push(Lifecycle::Changed(card, old_pile));
        self.fire(Hook::AfterCardChangedPiles, ev, None);
        #[cfg(test)]
        self.hist.card_lifecycle.push(Lifecycle::ChangedLate(card, old_pile));
        self.fire(Hook::AfterCardChangedPilesLate, ev, None);
    }

    /// CardPileCmd.Add: notifications follow the entire insertion batch.
    pub(crate) fn move_card_to_pile(&mut self, k: KRef, pile: PileKind, pos: PilePos) {
        self.move_cards_to_pile(&[k], pile, pos);
    }

    pub(crate) fn move_cards_to_pile(&mut self, cards: &[KRef], pile: PileKind, pos: PilePos) {
        if self.is_over_or_ending() { return; }
        let old_piles: Vec<_> = cards.iter().map(|&k| self.card_pile(k)).collect();
        for (&k, &old) in cards.iter().zip(&old_piles) {
            let owner = self.cards[k as usize].owner;
            let destination = if pile == PileKind::Hand && self.players[owner].hand.len() >= crate::combat::MAX_HAND_SIZE {
                PileKind::Discard
            } else { pile };
            self.remove_card_from_piles(k);
            self.insert_pile_from(k, old, destination, pos);
        }
        for (&k, old) in cards.iter().zip(old_piles) {
            if old != self.card_pile(k) {
                self.fire_after_card_changed_piles(k, old, None);
            }
        }
    }

    /// CardCmd.Transform: after AfterCardChangedPiles and InvokeCardAddFinished.
    /// CardModel's defaults do nothing; the sole AfterTransformedFrom override
    /// is SovereignBlade.RemoveSovereignBladeNode (visual only). No To override.
    pub(crate) fn fire_after_transformed(&mut self, old: KRef, new: KRef) {
        #[cfg(test)]
        {
            self.hist.card_lifecycle.push(Lifecycle::TransformedFrom(old));
            self.hist.card_lifecycle.push(Lifecycle::TransformedTo(new));
        }
        #[cfg(not(test))]
        let _ = (old, new);
    }
}
