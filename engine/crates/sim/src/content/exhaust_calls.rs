//! Awaited exhaust callers: SecondWind.cs:26-34 and FiendFire.cs:30-49.
use super::*;
use super::cards_ironclad_skills::{exhaust_from_hand, gain_resolved_block};

#[derive(Clone, Debug)]
pub(crate) struct ExhaustCaller {
    card: KRef, cards: Vec<KRef>, next: usize, after_exhaust: bool,
    block: Option<i32>, target: Option<CId>, hits_left: usize,
}

impl Combat {
    pub(crate) fn start_exhaust_caller(&mut self, card: KRef, target: Option<CId>) {
        let owner = self.cards[card as usize].owner;
        let block = (self.cards[card as usize].id == CardId::SecondWind)
            .then(|| self.cards[card as usize].var(VarKey::Block).unwrap_or(5));
        let cards: Vec<_> = self.players[owner].hand.iter().copied()
            .filter(|&k| block.is_none() || self.cards[k as usize].card_type != CardType::Attack).collect();
        let mut caller = ExhaustCaller { card, hits_left: if block.is_none() { cards.len() } else { 0 },
            cards, next: 0, after_exhaust: false, block, target };
        if !self.resume_exhaust_caller(&mut caller) {
            if let Some(frame) = self.pending_play.iter_mut().rev().find(|f| f.card == card) {
                frame.exhaust_caller = Some(caller);
            } else { self.unported("exhaust caller continuation requires a play frame"); }
        }
    }

    pub(crate) fn resume_exhaust_caller(&mut self, caller: &mut ExhaustCaller) -> bool {
        let previous = self.active_card.replace(caller.card);
        let done = self.run_exhaust_caller(caller);
        self.flush_pending_attack();
        self.active_card = previous;
        done
    }

    fn run_exhaust_caller(&mut self, caller: &mut ExhaustCaller) -> bool {
        let owner = self.cards[caller.card as usize].owner;
        loop {
            if self.is_over_or_ending() || self.cr(owner).is_dead() { return true; }
            if caller.after_exhaust {
                caller.after_exhaust = false;
                if let Some(raw) = caller.block {
                    let block = self.calculate_block(raw, owner, Props::MOVE, Some(caller.card));
                    gain_resolved_block(self, owner, block);
                }
            }
            if let Some(&card) = caller.cards.get(caller.next) {
                caller.next += 1;
                caller.after_exhaust = true;
                exhaust_from_hand(self, card);
                if self.pending_choice.is_some() || self.fault.is_some() { return false; }
                continue;
            }
            if caller.hits_left > 0 {
                let target = caller.target.expect("FiendFire target");
                if self.cr(target).is_dead() { return true; }
                caller.hits_left -= 1;
                self.deal_card_damage(caller.card, target);
                if self.pending_choice.is_some() || self.fault.is_some() { return false; }
                continue;
            }
            return true;
        }
    }
}
