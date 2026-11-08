//! Frozen-r7 rest-site options and run-side room-entry relic hooks.
use crate::identity::EnchantmentId;

use arrayvec::ArrayVec;

use crate::combat::RunCtx;
use crate::tables::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestOptionKind {
    Heal,
    Smith,
    Dig,
    Lift,
    Cook,
    Clone,
    Kindle,
    Hatch,
    /// Original multiplayer RestSiteOption.Generate inserts Mend after Smith.
    Mend,
}

impl RestOptionKind {
    /// Compatibility/UI boundary only; native consumers use the enum ordinal.
    pub const fn legacy_id(self) -> &'static str {
        match self { Self::Heal => "HEAL", Self::Smith => "SMITH", Self::Dig => "DIG",
            Self::Lift => "LIFT", Self::Cook => "COOK", Self::Clone => "CLONE",
            Self::Kindle => "KINDLE", Self::Hatch => "HATCH", Self::Mend => "MEND" }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct RestOption {
    pub kind: RestOptionKind,
    pub label: &'static str,
    pub description: &'static str,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct RestState {
    pub options: ArrayVec<RestOption, 12>,
}

fn has_relic(run: &RunCtx, id: RelicId) -> bool {
    run.owned_relics(crate::combat::PLAYER).any(|relic| relic.id == id
        && (id != RelicId::DreamCatcher || relic.enabled))
}

pub(super) fn removable_indices(run: &RunCtx) -> Vec<usize> {
    // PlayerState.removable_deck_cards: curses first, then every other removable card.
    let mut out = Vec::with_capacity(run.deck.len());
    for want_curse in [true, false] {
        for (index, card) in run.deck.iter().enumerate() {
            if (card.card_type == CardType::Curse) != want_curse || card.has_kw(KW_ETERNAL) {
                continue;
            }
            out.push(index);
        }
    }
    out
}

pub(super) fn upgradable_indices(run: &mut RunCtx) -> Vec<usize> {
    let mut out = Vec::with_capacity(run.deck.len());
    for index in 0..run.deck.len() {
        if run.is_deck_card_upgradable(index) {
            out.push(index);
        }
    }
    out
}

pub(super) fn generate(state: &mut RestState, run: &mut RunCtx) {
    state.options.clear();
    let has_upgradable = !upgradable_indices(run).is_empty();
    state.options.push(RestOption {
        kind: RestOptionKind::Heal,
        label: "Rest",
        description: "Heal 30% of max HP",
        enabled: true,
    });
    state.options.push(RestOption {
        kind: RestOptionKind::Smith,
        label: "Smith",
        description: "Upgrade a card",
        enabled: has_upgradable,
    });

    // Card hooks run before relic hooks and preserve deck order.
    for card in &run.deck {
        if card.id == CardId::ByrdonisEgg {
            state.options.push(RestOption {
                kind: RestOptionKind::Hatch,
                label: "Hatch",
                description: "Hatch the egg",
                enabled: true,
            });
        }
    }

    // Relic hooks preserve relic acquisition order.
    for relic in run.owned_relics(crate::combat::PLAYER).filter(|r|
        r.enabled || (r.id == RelicId::PumpkinCandle && !r.is_melted)) {
        let option = match relic.id {
            RelicId::Girya if relic.counter < 3 => Some(RestOption {
                kind: RestOptionKind::Lift,
                label: "Lift",
                description: "Exercise with Girya",
                enabled: true,
            }),
            RelicId::Shovel if !state.options.iter().any(|o| o.kind == RestOptionKind::Dig) => {
                Some(RestOption {
                    kind: RestOptionKind::Dig,
                    label: "Dig",
                    description: "Obtain a random relic",
                    enabled: true,
                })
            }
            RelicId::MeatCleaver
                if !state.options.iter().any(|o| o.kind == RestOptionKind::Cook) =>
            {
                Some(RestOption {
                    kind: RestOptionKind::Cook,
                    label: "Cook",
                    description: "Remove 2 cards, gain 9 Max HP",
                    enabled: removable_indices(run).len() >= 2,
                })
            }
            RelicId::PaelsGrowth
                if !state
                    .options
                    .iter()
                    .any(|o| o.kind == RestOptionKind::Clone) =>
            {
                Some(RestOption {
                    kind: RestOptionKind::Clone,
                    label: "Clone",
                    description: "Duplicate all Clone-enchanted cards",
                    enabled: true,
                })
            }
            RelicId::PumpkinCandle => Some(RestOption {
                kind: RestOptionKind::Kindle,
                label: "Kindle",
                description: "Kindle the Pumpkin Candle for 5 combats",
                enabled: true,
            }),
            _ => None,
        };
        if let Some(option) = option {
            state.options.push(option);
        }
    }
}

pub(super) fn option_index(state: &RestState, kind: RestOptionKind) -> Option<usize> {
    state
        .options
        .iter()
        .position(|option| option.enabled && option.kind == kind)
}

pub(super) fn disable_after_choice(state: &mut RestState, index: usize, keep_others: bool) {
    if let Some(chosen) = state.options.get_mut(index) {
        chosen.enabled = false;
    }
    if !keep_others {
        for option in &mut state.options {
            option.enabled = false;
        }
    }
}

pub(super) fn miniature_tent(run: &RunCtx) -> bool {
    // MiniatureTent.ShouldDisableRemainingRestSiteOptions checks player == Owner.
    // This legacy rest-site dispatcher acts only for the local seat-zero run.
    run.relics.iter().any(|relic| relic.id == RelicId::MiniatureTent
        && relic.owner == crate::combat::PLAYER && relic.enabled)
}

pub(super) fn execute_heal(run: &mut RunCtx) -> i32 {
    let amount = heal_amount(run.max_hp, run.has_owned_relic(crate::combat::PLAYER, RelicId::RegalPillow));
    let before = run.hp;
    run.hp = (run.hp + amount).min(run.max_hp);
    let healed = run.hp - before;
    if has_relic(run, RelicId::StoneHumidifier) {
        run.max_hp += 5;
        run.hp += 5;
    }
    healed
}

pub(crate) fn heal_amount(max_hp: i32, regal_pillow: bool) -> i32 {
    (max_hp as f64 * 0.30) as i32 + if regal_pillow { 15 } else { 0 }
}

pub(super) fn execute_lift(run: &mut RunCtx) -> bool {
    if let Some(relic) = run.owned_relics_mut(crate::combat::PLAYER).find(|r| r.id == RelicId::Girya && r.enabled) {
        if relic.counter < 3 {
            relic.counter += 1;
            return true;
        }
    }
    false
}

pub(super) fn execute_kindle(run: &mut RunCtx) -> bool {
    if let Some(relic) = run
        .owned_relics_mut(crate::combat::PLAYER)
        .find(|r| r.id == RelicId::PumpkinCandle)
    {
        relic.counter += 5;
        relic.enabled = true;
        return true;
    }
    false
}

pub(super) fn execute_clone(run: &mut RunCtx) -> usize {
    let originals: Vec<_> = run
        .deck
        .iter()
        .filter(|card| card.enchantments.iter().any(|(name, _)| *name == EnchantmentId::Clone))
        .cloned()
        .collect();
    let count = originals.len();
    for mut card in originals {
        card.instance_id = run.new_card_id();
        run.deck.push(card);
        // CloneRestSiteOption.cs:47-50 adds each clone through CardPileCmd.
        run.after_run_deck_entry(run.deck.len() - 1);
    }
    count
}

pub(super) fn has_heal_followups(run: &RunCtx) -> bool {
    has_relic(run, RelicId::TinyMailbox) || has_relic(run, RelicId::DreamCatcher)
}

/// Run-side `after_room_entered` hooks used by the ported phases. This is called
/// before phase entry except for SHOP, matching frozen-r7 ordering.
pub(super) fn after_room_entered(run: &mut RunCtx, room: RoomType, map_point: MapPointType) {
    let deck_len = run.deck.len();
    for index in 0..run.relics.len() {
        if run.relics[index].owner != crate::combat::PLAYER { continue; }
        if run.relics[index].id == RelicId::MawBank && !run.relics[index].flag1 {
            // MawBank.cs:43-48 calls PlayerCmd.GainGold, not raw addition.
            // Legacy run room callbacks act only for seat zero.
            run.gain_run_gold(12);
        }
        let relic = &mut run.relics[index];
        if matches!(relic.id, RelicId::FakeVenerableTeaSet | RelicId::EternalFeather)
            && (relic.owner != crate::combat::PLAYER || !relic.enabled) { continue; }
        match relic.id {
            RelicId::MealTicket if room == RoomType::Shop && run.hp > 0 => {
                run.hp = (run.hp + 15).min(run.max_hp)
            }
            RelicId::VenerableTeaSet | RelicId::FakeVenerableTeaSet
                if room == RoomType::RestSite =>
            {
                relic.flag1 = true
            }
            RelicId::EternalFeather if room == RoomType::RestSite && run.hp > 0 => {
                let heal = 3 * (deck_len as i32 / 5);
                run.hp = (run.hp + heal).min(run.max_hp);
            }
            RelicId::Permafrost | RelicId::LavaLamp
                if matches!(room, RoomType::Monster | RoomType::Elite | RoomType::Boss) =>
            {
                relic.flag1 = false
            }
            RelicId::SilverCrucible if room == RoomType::Treasure => {
                relic.counter2 += 1;
                if relic.counter >= 3 && relic.counter2 > 0 {
                    relic.enabled = false;
                }
            }
            _ => {}
        }
    }
    if map_point == MapPointType::Unknown && run.hp > 0 && has_relic(run, RelicId::Planisphere) {
        run.hp = (run.hp + 5).min(run.max_hp);
    }
}
