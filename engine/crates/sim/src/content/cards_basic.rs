//! r7 `cards/ironclad_basic.py`: the Ironclad starter cards.

use crate::card::{CId, KRef};
use crate::combat::Combat;
use crate::tables::*;

/// Returns `false` for cards this group does not own.
pub(crate) fn play(c: &mut Combat, k: KRef, target: Option<CId>) -> bool {
    match c.cards[k as usize].id {
        CardId::StrikeIronclad => {
            let t = target.expect("ANY_ENEMY card without target");
            c.deal_card_damage(k, t);
        }
        CardId::DefendIronclad => c.gain_card_block(k),
        CardId::Bash => {
            let t = target.expect("ANY_ENEMY card without target");
            c.deal_card_damage(k, t);
            let vuln = c.cards[k as usize].var(VarKey::Vulnerable).unwrap_or(2);
            c.apply_power_to(t, PowerId::Vulnerable, vuln, None);
        }
        _ => return false,
    }
    true
}

/// Choice continuations for this group's cards; `false` = not ours. (No starter card chooses.)
pub(crate) fn resolve(_c: &mut Combat, _r: crate::choice::Resolver, _selected: &[KRef]) -> bool {
    false
}
