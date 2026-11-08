use arrayvec::ArrayVec;
use core::cmp::Ordering;

use super::*;
use crate::card::{Card, KRef, MAX_CARD_VARS};
use crate::choice::PendingChoice;
use crate::combat::{Combat, PLAYER, Props};
use crate::creature::PRef;
use crate::tables::{
    CardId, CardType, IntentType, KEYWORD_NAMES, POTION_INFO, PotionTargetType, PowerId,
    PowerStackType, RelicId, TargetType, VarKey, card_info, power_meta,
};

const MAX_OBS_CARDS: usize = STATE_CAPACITY;

#[derive(Clone, Copy)]
struct LiveCardFacts {
    playable: bool,
    energy_cost: i32,
    star_cost: i32,
}

#[derive(Clone, Copy, Default)]
struct RowSpan {
    start: usize,
    end: usize,
    valid: bool,
}

fn optional_token(namespace: &'static str, value: &str) -> u16 {
    tokens::token_id(namespace, value).unwrap_or(0)
}

fn mutation_var_name(key: VarKey) -> Option<&'static str> {
    match key {
        VarKey::ExtraDamage | VarKey::CalcBase | VarKey::Increase | VarKey::Repeat | VarKey::Gold => {
            Some(key.name())
        }
        _ => None,
    }
}

fn compare_keyword_sets(a: u16, b: u16) -> Ordering {
    let mut ai = KEYWORD_NAMES
        .iter()
        .enumerate()
        .filter_map(|(i, name)| ((a & (1u16 << i)) != 0).then_some(*name));
    let mut bi = KEYWORD_NAMES
        .iter()
        .enumerate()
        .filter_map(|(i, name)| ((b & (1u16 << i)) != 0).then_some(*name));
    loop {
        match (ai.next(), bi.next()) {
            (Some(x), Some(y)) => match x.cmp(y) {
                Ordering::Equal => {}
                other => return other,
            },
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => return Ordering::Equal,
        }
    }
}

fn decimal_text(value: i32) -> ([u8; 12], usize) {
    let mut out = [0u8; 12];
    let negative = value < 0;
    let mut n = value.unsigned_abs();
    let mut pos = out.len();
    if n == 0 {
        pos -= 1;
        out[pos] = b'0';
    } else {
        while n > 0 {
            pos -= 1;
            out[pos] = b'0' + (n % 10) as u8;
            n /= 10;
        }
    }
    if negative {
        pos -= 1;
        out[pos] = b'-';
    }
    (out, pos)
}

fn compare_decimal_text(a: i32, b: i32) -> Ordering {
    let (ab, ai) = decimal_text(a);
    let (bb, bi) = decimal_text(b);
    ab[ai..].cmp(&bb[bi..])
}

fn public_mutation_vars(card: &Card) -> ArrayVec<(&'static str,i32), {MAX_CARD_VARS+2}> {
    let mut vars: ArrayVec<(&str,i32), {MAX_CARD_VARS+2}> = card
        .vars
        .iter()
        .filter_map(|&(key,value)|mutation_var_name(key).map(|name|(name,value)))
        .collect();
    // Frozen status.make_shiv supplies calc_base=0/calc_extra=1 for both
    // upgrades. Only calc_base is in the wire mutation-variable vocabulary.
    // 41347e8 correctly removed these from C# gameplay; restore the frozen
    // observation representation here, without changing cards or the oracle.
    if card.id==CardId::Shiv && !vars.iter().any(|(name,_)|*name=="calc_base") {
        vars.push(("calc_base",0));
    }
    if card.cv.frantic_escape_cost_increase != 0 {
        vars.push(("cost_increase",card.cv.frantic_escape_cost_increase));
    }
    vars.sort_unstable_by_key(|(name,_)|*name);
    vars
}

#[cfg(test)]
mod shiv_wire_projection_tests {
    use super::*;

    #[test]
    fn frozen_shiv_projection_is_upgrade_and_modifier_independent() {
        for upgraded in [false,true] {
            for modifier in [None,Some(("Sharp",2)),Some(("Momentum",5))] {
                let mut card=Card::from_factory(CardId::Shiv,upgraded,1).unwrap();
                if let Some((name,amount))=modifier {
                    assert!(crate::card::enchant::apply_static_enchantment(&mut card,name,amount));
                    card.cost=1;
                }
                let gameplay_vars=card.vars.clone();
                let projected=public_mutation_vars(&card);
                assert_eq!(projected.iter().filter(|(name,_)|*name=="calc_base").count(),1);
                assert!(projected.contains(&("calc_base",0)));
                assert!(!projected.iter().any(|(name,_)|*name=="calc_extra"));
                assert_eq!(card.vars,gameplay_vars);
                assert!(card.vars.iter().all(|(key,_)|!matches!(*key,VarKey::CalcBase|VarKey::CalcExtra)));
            }
        }
        let other=Card::from_factory(CardId::StrikeIronclad,false,2).unwrap();
        assert!(!public_mutation_vars(&other).iter().any(|(name,_)|*name=="calc_base"));
    }
}

fn compare_dynamic_vars(a: &Card, b: &Card) -> Ordering {
    let av=public_mutation_vars(a);
    let bv=public_mutation_vars(b);
    for (x, y) in av.iter().zip(&bv) {
        match x.0.cmp(y.0) {
            Ordering::Equal => {}
            other => return other,
        }
        match compare_decimal_text(x.1, y.1) {
            Ordering::Equal => {}
            other => return other,
        }
        match compare_decimal_text(x.1, y.1) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    av.len().cmp(&bv.len())
}

fn single_enchantment(card: &Card) -> (&'static str, i32) {
    if card.enchantments.len() == 1 {
        (card.enchantments[0].0.name(), card.enchantments[0].1)
    } else {
        ("", 0)
    }
}

pub fn compare_card_semantics(a: &Card, b: &Card) -> Ordering {
    let ae = single_enchantment(a);
    let be = single_enchantment(b);
    a.id.name()
        .cmp(b.id.name())
        .then_with(|| a.card_type.name().cmp(b.card_type.name()))
        .then_with(|| (a.upgraded as u8).cmp(&(b.upgraded as u8)))
        .then_with(|| ae.0.cmp(be.0))
        .then_with(|| ae.1.cmp(&be.1))
        .then_with(|| (if a.bound { "bound" } else { "" }).cmp(if b.bound { "bound" } else { "" }))
        .then_with(|| i32::from(a.bound).cmp(&i32::from(b.bound)))
        .then_with(|| compare_keyword_sets(a.keywords, b.keywords))
        .then_with(|| compare_dynamic_vars(a, b))
}

/// Source: structured_encoder.py:711-850 for semantic state rows,
/// structured_env.py:1471-1498 for executable event candidates,
/// structured_simulator_teacher.py:1038-1178 (visible "Gain a ... relic").
pub fn write_neow_obs(
    run: &crate::run::RunManager,
    actions: &[crate::run::RunAction],
    context: &CombatObservationContext<'_>,
    buffers: &mut ObsBuffers<'_>,
    slots: &mut ActionSlots,
) -> Result<(), ObsError> {
    write_run_menu_obs(run, actions, context, buffers, slots, false, false).map(|_|())
}

/// Semantic run rows only; the caller must add verified phase candidates.
pub fn write_run_state(
    run: &crate::run::RunManager, context: &CombatObservationContext<'_>,
    buffers: &mut ObsBuffers<'_>,
) -> Result<(), ObsError> {
    write_run_menu_obs(run, &[], context, buffers, &mut ActionSlots::new(), true, false).map(|_|())
}

/// Offline differential recorder only; bypasses admission, not semantic guards.
#[cfg(feature="selector-proof")]
pub fn write_selector_proof_obs(
    run:&crate::run::RunManager, actions:&[crate::run::RunAction],
    context:&CombatObservationContext<'_>, buffers:&mut ObsBuffers<'_>,
    slots:&mut ActionSlots,
) -> Result<Vec<String>,ObsError> {
    write_run_menu_obs(run,actions,context,buffers,slots,false,true)
}

/// Shared noncombat potion contract, independent of the enclosing menu.
pub fn write_run_potion_candidate(
    run: &crate::run::RunManager, action: &crate::run::RunAction,
    slot: usize, buffers: &mut ObsBuffers<'_>,
) -> Result<(), ObsError> {
    use crate::run::RunActionKind as K;
    let kind=match action.kind {
        K::UsePotion => "use_potion", K::DiscardPotion => "discard_potion",
        _ => return Err(ObsError::InvalidActionSlot),
    };
    let potion_slot=action.slot_index.ok_or(ObsError::InvalidActionSlot)?;
    let id=action.potion_id.ok_or(ObsError::InvalidActionSlot)?;
    if run.run_ctx().potions.get(potion_slot)!=Some(&Some(id)) {
        return Err(ObsError::InvalidActionSlot);
    }
    let source=(0..buffers.state_rows()).find(|&row|
        buffers.state_kind_ids[row]==tokens::kind_id("potion").unwrap()
        && buffers.state_numeric_values[row*SCALAR_SLOTS]==potion_slot as i32
        && buffers.state_token_ids[row*TOKEN_SLOTS]==optional_token("potion_id",id.name()))
        .ok_or(ObsError::InvalidActionSlot)?;
    buffers.candidate_token_named(slot,0,"action_kind",kind)?;
    buffers.candidate_token_named(slot,1,"potion_id",id.name())?;
    buffers.candidate_token(slot,2,optional_token("source_kind","potion"));
    buffers.candidate_scalar(slot,5,potion_slot as i32,9.0,true);
    buffers.set_source_applicable(slot,true,true);
    buffers.source_link(slot,source);
    buffers.set_model_eligible(slot,action.enabled);
    Ok(())
}

fn validate_reward_screen(
    run: &crate::run::RunManager,
    has_extra: bool,
    actions: &[crate::run::RunAction],
) -> Result<(), ObsError> {
    if has_extra {
        let sources = run.observation_extra_reward_sources();
        if sources.is_empty() || sources.iter().any(|source|
            !super::rule_admission::admitted("extra_reward", source))
        {
            return Err(ObsError::UnsupportedState("non-card extra reward wire is unported"));
        }
    }
    for action in actions.iter().filter(|a| a.kind == crate::run::RunActionKind::OpenReward
        && a.reward_type != Some("CARD"))
    {
        let source = match action.reward_type {
            Some("SPECIAL_CARD") => "reward:SPECIAL_CARD",
            Some("REMOVE_CARD") => "reward:REMOVE_CARD",
            _ => return Err(ObsError::UnsupportedState("non-card extra reward wire is unported")),
        };
        if !super::rule_admission::admitted("extra_reward", source) {
            return Err(ObsError::UnsupportedState("non-card extra reward wire is unported"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod reward_screen_tests {
    #[test]
    fn non_card_open_and_hidden_extra_reject_the_entire_screen() {
        use crate::run::{RunActionKind as K, RunManager, RunOptions};
        let mut run=RunManager::new(0,RunOptions::default());
        let mut action=run.available_actions()[0];
        action.kind=K::OpenReward;
        action.reward_type=Some("REMOVE_CARD");
        assert!(super::validate_reward_screen(&run,false,&[action]).is_err());
        action.reward_type=Some("SPECIAL_CARD");
        assert!(super::validate_reward_screen(&run,false,&[action]).is_err());
        action.reward_type=None;
        assert!(super::validate_reward_screen(&run,false,&[action]).is_err());
        action.reward_type=Some("CARD");
        assert!(super::validate_reward_screen(&run,false,&[action]).is_ok());
        action.kind=K::SkipRewards;
        assert!(super::validate_reward_screen(&run,true,&[action]).is_err());
    }
}

fn write_run_menu_obs(
    run: &crate::run::RunManager, actions: &[crate::run::RunAction],
    context: &CombatObservationContext<'_>, buffers: &mut ObsBuffers<'_>,
    slots: &mut ActionSlots, state_only: bool, proof: bool,
) -> Result<Vec<String>, ObsError> {
    let is_map = run.phase() == crate::run::RunPhase::MapChoice;
    let is_reward = run.phase() == crate::run::RunPhase::CardReward;
    if is_reward && !proof {
        validate_reward_screen(run,run.observation_has_unencoded_extra_reward(),actions)?;
    }
    let is_rest = run.phase() == crate::run::RunPhase::RestSite;
    let is_shop = run.phase() == crate::run::RunPhase::Shop;
    let is_treasure = run.phase() == crate::run::RunPhase::Treasure;
    let is_selector = run.observation_run_choice().is_some();
    if !state_only && !is_selector && !is_map && !is_reward && !is_rest && !is_shop && !is_treasure && (run.current_event_id().is_some() || run.current_act_index() != 0
        || run.run_ctx().total_floor != 1
    ) {
        return Err(ObsError::UnsupportedState("event public semantics are unported"));
    }
    if !state_only && !is_selector && !is_map && !is_reward && !is_rest && !is_shop && !is_treasure && actions.iter().any(|a| a.kind != crate::run::RunActionKind::EventChoice
        || !matches!(a.description, Some("Gain a positive relic" | "Gain a cursed relic")))
    {
        return Err(ObsError::UnsupportedState("Neow choice payload is unported"));
    }
    buffers.clear(context)?;
    slots.clear();
    let ctx = run.run_ctx();
    let global = buffers.push_state("global")?;
    buffers.state_token_named(global, 0, "phase", run.phase().name())?;
    // Uppercase snapshot type EVENT is absent from lowercase context vocab.
    for (slot, (value, scale)) in [
        (ctx.hp, 100.0), (ctx.max_hp, 100.0), (0, 50.0), (0, 5.0),
        (0, 5.0), (0, 10.0), (ctx.gold, 500.0), (ctx.total_floor, 60.0),
    ].into_iter().enumerate() {
        buffers.state_scalar(global, slot, value, scale, true);
    }
    buffers.state_bool(global, 0, true);
    let player = buffers.push_state("player")?;
    buffers.state_scalar(player, 0, ctx.ascension, 20.0, true);
    buffers.state_scalar(player, 1, ctx.act_index + 1, 4.0, true);
    buffers.state_scalar(player, 2, 1, 30.0, true);
    let mut deck: Vec<_> = ctx.deck.iter().collect();
    deck.sort_by(|a, b| compare_card_semantics(a, b));
    let mut deck_spans = Vec::with_capacity(deck.len());
    for card in deck {
        let span = write_card_row(buffers, card, "persistent_deck", None, None)?;
        deck_spans.push((card.instance_id, span));
    }
    for (slot, potion) in ctx.potions.iter().enumerate() {
        let row = buffers.push_state("potion")?;
        buffers.state_scalar(row, 0, slot as i32, 9.0, true);
        // Preserve allocated-but-vacated slots; only unallocated capacity is
        // absent (structured_encoder.py's potionSlots loop).
        if let Some(id) = potion {
            buffers.state_token_named(row, 0, "potion_id", id.name())?;
            buffers.state_bool(row, 0, true);
        }
    }
    // StructuredV2Encoder emits potion inventory before relics.
    for relic in &ctx.relics {
        let row = buffers.push_state("relic")?;
        buffers.state_token_named(row, 0, "relic_id", relic.id.name())?;
        let counter = relic_noncombat_counter(relic.id, relic.counter);
        buffers.state_scalar(row, 0, counter.unwrap_or(0), 100.0, counter.is_some());
        buffers.state_bool(row, 0, counter.is_some());
    }
    // relational_observation.py:509-534: generated choice cards are appended
    // after the ordinary state, sorted semantically, with no fake pile token.
    if let Some(choice)=run.observation_run_choice().filter(|c| !c.single_select) {
        let mut cards:Vec<_>=choice.reward_cards.iter()
            .filter(|card| !ctx.deck.iter().any(|owned| owned.instance_id==card.instance_id)).collect();
        cards.sort_by(|a,b| compare_card_semantics(a,b));
        for card in cards {
            let span=write_card_row(buffers,card,"persistent_deck",None,None)?;
            for row in span.start..span.end {
                let kind=buffers.state_kind_ids[row];
                buffers.state_token(row,if kind==tokens::kind_id("card").unwrap() {3}else{1},0);
            }
            deck_spans.push((card.instance_id,span));
        }
    }
    if !proof && !state_only && run.observation_run_choice().is_some()
        && actions.iter().any(|a| selector_inventory_unproven(a.kind))
        && !selector_rule_admitted(run, actions)
    {
        return Err(ObsError::UnsupportedState("selector inventory requires whole-screen proof"));
    }
    for action in actions {
        let slot = buffers.push_candidate()?;
        if (is_rest || run.observation_run_choice().is_some())
            && matches!(action.kind,crate::run::RunActionKind::UsePotion|crate::run::RunActionKind::DiscardPotion)
        {
            write_run_potion_candidate(run,action,slot,buffers)?;
            continue;
        }
        if let Some(choice) = run.observation_run_choice().filter(|c| !(c.single_select && !c.reward_cards.is_empty())) {
            // Whole-shape or compositional admission is checked atomically
            // after all candidates and exact instance routes are encoded.
            use crate::run::RunActionKind as K;
            let kind = match action.kind {
                K::Choose => "choose", K::ConfirmChoice => "confirm_choice",
                _ => return Err(ObsError::UnsupportedState("run selector auxiliary action is unported")),
            };
            buffers.candidate_token_named(slot, 0, "action_kind", kind)?;
            let selected = action.index.is_some_and(|index| choice.selected.contains(&index));
            // Morphic Grove's resident event-selector publication retains legal
            // toggle bindings (seed77/d334). Ordinary run menus publish selected
            // rows as present=2 / column22=1 (observation212.py:510).
            // C# NSimpleCardSelectScreen permits deselection while open,
            // including SeaGlass.AfterObtained's optional fifteen-card grid.
            // Keep that legal toggle (never model-eligible); the offline Cheese
            // certificate handles the legacy producer's selected-row legal bit.
            let resident_toggle = run.current_event_id()==Some("MORPHIC_GROVE")
                || matches!(choice.wire_source,Some("event:ROOM_FULL_OF_CHEESE"|"event.orobas.add"));
            buffers.legal_mask[slot] = action.enabled && (!selected || resident_toggle);
            if action.kind == K::Choose {
                let option = action.index.and_then(|i| choice.options.get(i))
                    .ok_or(ObsError::InvalidActionSlot)?;
                buffers.candidate_token_named(slot, 1, "card_id", option.card_id.name())?;
                if let Some((_,span)) = deck_spans.iter().find(|(id, _)| *id == option.instance_id) {
                    buffers.set_source_applicable(slot, true, true);
                    route_span(buffers, slot, *span, true);
                } else {
                    buffers.set_source_applicable(slot, true, false);
                }
            } else {
                buffers.candidate_token_named(slot, 1, "action_kind", "confirm_choice")?;
            }
            for (scalar, value) in [(5, choice.selected.len()), (6, choice.min_choices), (7, choice.max_choices)] {
                buffers.candidate_scalar(slot, scalar, value as i32, 5.0, true);
            }
            buffers.set_choice_purpose(slot, choice_purpose(choice.prompt,
                if action.kind == K::Choose { ActionKind::Choose } else { ActionKind::ConfirmChoice }))?;
            buffers.candidate_bool(slot, 8, selected);
            buffers.candidate_bool(slot, 9, choice.can_confirm());
            buffers.candidate_bool(slot, 10, choice.allow_skip);
            buffers.candidate_bool(slot, 11, choice.is_multi());
            buffers.set_model_eligible(slot, action.enabled && !selected);
            continue;
        }
        if is_rest {
            if action.kind != crate::run::RunActionKind::RestOption {
                return Err(ObsError::UnsupportedState("rest auxiliary action is unported"));
            }
            buffers.candidate_token_named(slot, 0, "action_kind", "rest_option")?;
            // structured_env.py:1428; run_manager.py:1597 publishes no
            // heal_amount, so the parent encoder's exact default is zero.
            match action.option_id.ok_or(ObsError::InvalidActionSlot)? {
                "HEAL" => {
                    buffers.candidate_token_named(slot, 3, "target_type", "SELF")?;
                    buffers.candidate_scalar(slot, 4, 0, 50.0, true);
                }
                "SMITH" => {
                    buffers.candidate_token_named(slot, 1, "action_kind", "choose")?;
                    buffers.candidate_bool(slot, 2, true);
                }
                "HATCH" => buffers.candidate_token_named(slot, 1, "card_id", "BYRDONIS_EGG")?,
                "LIFT" => buffers.candidate_token_named(slot, 1, "relic_id", "GIRYA")?,
                "DIG" => buffers.candidate_token_named(slot, 1, "relic_id", "SHOVEL")?,
                "COOK" | "CLONE" | "KINDLE" => {}
                _ => return Err(ObsError::UnsupportedState("rest option identity is unported")),
            }
            buffers.set_model_eligible(slot, action.enabled);
            continue;
        }
        if is_shop && !matches!(action.kind, crate::run::RunActionKind::UsePotion | crate::run::RunActionKind::DiscardPotion) {
            use crate::run::RunActionKind as K;
            let kind = match action.kind {
                K::LeaveShop => "leave_shop", K::BuyCard => "buy_card",
                K::BuyRelic => "buy_relic", K::BuyPotion => "buy_potion",
                K::RemoveCard => "remove_card",
                _ => return Err(ObsError::UnsupportedState("shop auxiliary action is unported")),
            };
            buffers.candidate_token_named(slot, 0, "action_kind", kind)?;
            if let Some(id) = action.card_id {
                buffers.candidate_token_named(slot, 1, "card_id", id.name())?;
            } else if let Some(id) = action.relic_id {
                buffers.candidate_token_named(slot, 1, "relic_id", id.name())?;
            } else if let Some(id) = action.potion_id {
                buffers.candidate_token_named(slot, 1, "potion_id", id.name())?;
            }
            // structured_env.py:1356-1359; relational routing removes only
            // slot/index scalars 0/1, preserving the exact shop price.
            if let Some(price) = action.price {
                buffers.candidate_scalar(slot, 4, price, 500.0, true);
            }
            buffers.set_model_eligible(slot, action.enabled);
            continue;
        }
        if is_treasure && !matches!(action.kind, crate::run::RunActionKind::UsePotion | crate::run::RunActionKind::DiscardPotion) {
            // run_manager.py:1658-1662 and the parent encoder's relic identity.
            if action.kind != crate::run::RunActionKind::Collect {
                return Err(ObsError::UnsupportedState("treasure action is unported"));
            }
            buffers.candidate_token_named(slot, 0, "action_kind", "collect")?;
            if let Some(id) = action.relic_id {
                buffers.candidate_token_named(slot, 1, "relic_id", id.name())?;
            }
            buffers.set_model_eligible(slot, action.enabled);
            continue;
        }
        if is_reward && !matches!(action.kind, crate::run::RunActionKind::UsePotion | crate::run::RunActionKind::DiscardPotion) {
            use crate::run::RunActionKind as K;
            let kind = match action.kind {
                K::Skip => "skip", K::PickCard => "pick_card",
                K::SacrificeCardReward => "sacrifice_card_reward",
                K::PickCardBundle => "pick_card_bundle",
                K::SkipRewards => "skip_rewards", K::SkipPotion => "skip_potion",
                K::PickPotion => "pick_potion", K::SkipRelic => "skip_relic",
                K::PickRelicReward => "pick_relic_reward", K::OpenReward => "open_reward",
                // C# Rewards/SpecialCardReward.cs:77-84 selects the retained
                // mutable card, rather than opening a normal card offer.
                // The action is r8-v1; runtime requires original-game evidence.
                K::PickSpecialCard => "pick_special_card",
                _ => return Err(ObsError::UnsupportedState("reward auxiliary action is unported")),
            };
            buffers.candidate_token_named(slot, 0, "action_kind", kind)?;
            if action.kind == K::PickCardBundle {
                for (i, id) in action.bundle_card_ids.iter().take(action.bundle_len as usize).enumerate() {
                    buffers.candidate_token_named(slot, i + 1, "card_id",
                        id.ok_or(ObsError::InvalidActionSlot)?.name())?;
                }
            } else if let Some(id) = action.card_id {
                buffers.candidate_token_named(slot, 1, "card_id", id.name())?;
            } else if let Some(id) = action.relic_id {
                buffers.candidate_token_named(slot, 1, "relic_id", id.name())?;
            } else if let Some(id) = action.potion_id {
                buffers.candidate_token_named(slot, 1, "potion_id", id.name())?;
            }
            // relational_observation.py:763-794 removes parent slot/index
            // scalars 0 and 1. Rewards have no other routing scalars.
            buffers.set_model_eligible(slot, action.enabled);
            continue;
        }
        if is_map || is_shop || is_treasure || is_reward {
            if matches!(action.kind, crate::run::RunActionKind::UsePotion | crate::run::RunActionKind::DiscardPotion) {
                // structured_env.py:1360-1400, relational_observation.py:905.
                write_run_potion_candidate(run,action,slot,buffers)?;
                continue;
            }
            if action.kind != crate::run::RunActionKind::Move {
                return Err(ObsError::UnsupportedState("map potion candidate is unported"));
            }
            write_map_candidate(run, action, slot, buffers)?;
            continue;
        }
        buffers.candidate_token_named(slot, 0, "action_kind", "event_choice")?;
        buffers.candidate_token_named(slot, 1, "event_option_id",
            action.option_id.ok_or(ObsError::InvalidActionSlot)?)?;
        buffers.candidate_token_named(slot, 2, "event_id", "Neow")?;
        for bit in [10, 11, 12, 15] { buffers.candidate_bool(slot, bit, true); }
        buffers.set_model_eligible(slot, action.enabled);
    }
    // Row consistency is diagnostic only: it cannot establish that int1
    // publishes this candidate set. Runtime requires a whole-screen witness.
    let certificates=if proof {selector_row_contract(run,actions,buffers,&deck_spans)} else {None};
    if !proof && !state_only && run.observation_run_choice().is_some()
        && !selector_rule_admitted(run, actions)
    {
        return Err(ObsError::UnsupportedState("unverified run selector source"));
    }
    Ok(certificates.unwrap_or_default())
}

// Card identity and routing are typed parameters, not ignored differences:
// selector_row_contract verifies both against the real persistent instance.
// All other candidate bytes, including float bits and both masks, are literal.
fn selector_candidate_layout(b:&ObsBuffers<'_>, slot:usize) -> String {
    let tokens=&b.candidate_token_ids[slot*TOKEN_SLOTS..(slot+1)*TOKEN_SLOTS];
    let values=&b.candidate_numeric_values[slot*SCALAR_SLOTS..(slot+1)*SCALAR_SLOTS];
    let scales:Vec<_>=b.candidate_numeric_scales[slot*SCALAR_SLOTS..(slot+1)*SCALAR_SLOTS]
        .iter().map(|v|v.to_bits()).collect();
    let tail_width=b.candidate_numeric_tail.len()/CANDIDATE_CAPACITY;
    let tail:Vec<_>=b.candidate_numeric_tail[slot*tail_width..(slot+1)*tail_width]
        .iter().map(|v|v.to_bits()).collect();
    format!("R|persistent-card|kind={}|tokens={:?},{:?}|values={values:?}|scales={scales:?}|tail={tail:?}|choice={}|model={}|legal={}|routes={},{},{},{}",
        b.candidate_kind_ids[slot],tokens[0],&tokens[2..],b.choice_purpose_ids[slot],
        b.model_mask[slot],b.legal_mask[slot],b.source_applicable[slot],b.source_known[slot],
        b.target_applicable[slot],b.target_known[slot])
}

fn selector_inventory_unproven(kind:crate::run::RunActionKind) -> bool {
    matches!(kind,crate::run::RunActionKind::UsePotion|crate::run::RunActionKind::DiscardPotion)
}

fn selector_rule_admitted(
    run:&crate::run::RunManager,
    actions:&[crate::run::RunAction],
) -> bool {
    if run.observation_verified_selector() {
        return true;
    }
    if !run.observation_selector_integrity() {
        return false;
    }
    let Some(source)=run.observation_selector_source() else {
        return false;
    };
    let kind=if actions.iter().any(|a|selector_inventory_unproven(a.kind)) {
        "run_selector_inventory"
    } else {
        "run_selector"
    };
    super::rule_admission::admitted(kind,source)
}

#[cfg(test)]
mod selector_inventory_tests {
    use super::*;
    #[test]
    fn inventory_rows_cannot_borrow_choose_only_witnesses() {
        use crate::run::RunActionKind as K;
        assert!(selector_inventory_unproven(K::UsePotion));
        assert!(selector_inventory_unproven(K::DiscardPotion));
        assert!(!selector_inventory_unproven(K::Choose));
        assert!(!selector_inventory_unproven(K::ConfirmChoice));
    }
}

fn selector_row_contract(
    run:&crate::run::RunManager, actions:&[crate::run::RunAction],
    b:&ObsBuffers<'_>, spans:&[(u32,RowSpan)],
) -> Option<Vec<String>> {
    use crate::run::RunActionKind as K;
    let c=run.observation_run_choice()?;
    // Small, proven family only. Generated cards, multi/confirm, auxiliary
    // candidates and non-deck routes retain whole-shape admission.
    if !c.single_select || !c.reward_cards.is_empty() || c.allow_skip
        || !c.selected.is_empty() || c.min_choices!=1 || c.max_choices!=1
        || c.options.is_empty() || actions.len()!=c.options.len() {return None;}
    let mut certificates=vec![
        format!("H|{}|single|count={}|min=1|max=1|skip=false|selected=0",run.phase().name(),c.options.len()),
        "O|persistent-deck-insertion|single".to_owned(),
    ];
    let mut previous=None;
    for (slot,(a,o)) in actions.iter().zip(&c.options).enumerate() {
        if a.kind!=K::Choose || !a.enabled || a.index!=Some(slot) || a.card_id!=Some(o.card_id)
            || o.source_pile!="deck" {return None;}
        let position=run.run_ctx().deck.iter().position(|card|
            card.instance_id==o.instance_id && card.id==o.card_id)?;
        if previous.is_some_and(|p|p>=position) {return None;}
        previous=Some(position);
        let span=spans.iter().find(|(id,_)|*id==o.instance_id)?.1;
        if b.candidate_token_ids[slot*TOKEN_SLOTS]!=optional_token("action_kind","choose")
            || b.candidate_token_ids[slot*TOKEN_SLOTS+1]!=optional_token("card_id",o.card_id.name())
        {return None;}
        for row in 0..STATE_CAPACITY {
            let offset=slot*ROUTE_BYTES_PER_CANDIDATE+row/8;
            let mask=1u8<<(row%8);
            if (b.source_links_bits[offset]&mask!=0)!=(span.start<=row && row<span.end)
                || b.target_links_bits[offset]&mask!=0 {return None;}
        }
        certificates.push(selector_candidate_layout(b,slot));
    }
    Some(certificates)
}

/// Frozen relational_observation.py:118-217. Visible DAG only, no RNG calls.
fn write_map_candidate(
    run: &crate::run::RunManager, action: &crate::run::RunAction,
    slot: usize, buffers: &mut ObsBuffers<'_>,
) -> Result<(), ObsError> {
    let map = run.visible_map().ok_or(ObsError::UnsupportedState("map unavailable"))?;
    let coord = action.coord.ok_or(ObsError::InvalidActionSlot)?;
    let index = map.point_index(coord).ok_or(ObsError::InvalidActionSlot)? as usize;
    fn analyze(map: &crate::run::ActMap, i: usize, active: &mut [bool],
        memo: &mut [Option<[i32; 8]>]) -> Result<[i32; 8], ObsError>
    {
        if let Some(row) = memo[i] { return Ok(row); }
        if active[i] { return Err(ObsError::UnsupportedState("visible map cycle")); }
        active[i] = true;
        let point = &map.points[i];
        let children: Vec<_> = point.children.iter()
            .map(|&child| analyze(map, child as usize, active, memo))
            .collect::<Result<_, _>>()?;
        let mut row = [0; 8];
        for (offset, kind) in [(0, "ELITE"), (2, "REST_SITE"), (4, "SHOP")] {
            let own = i32::from(point.point_type.name() == kind);
            row[offset] = own + children.iter().map(|c| c[offset]).min().unwrap_or(0);
            row[offset + 1] = own + children.iter().map(|c| c[offset + 1]).max().unwrap_or(0);
        }
        for (offset, kind) in [(6, "REST_SITE"), (7, "SHOP")] {
            row[offset] = if point.point_type.name() == kind { 1 } else {
                children.iter().map(|c| c[offset]).filter(|&v| v >= 0)
                    .min().map_or(-1, |v| v + 1)
            };
        }
        active[i] = false;
        memo[i] = Some(row);
        Ok(row)
    }
    let row = analyze(map, index, &mut vec![false; map.points.len()],
        &mut vec![None; map.points.len()])?;
    buffers.candidate_token_named(slot, 0, "action_kind", "move")?;
    // The parent encoder owns token 1; relational code replaces numeric routes
    // and adds only forced-prefix tokens 2 and 3 (relational_observation.py:1130).
    buffers.candidate_token_named(slot, 1, "map_point_type", map.points[index].point_type.name())?;
    let mut cursor = index;
    for token_slot in 2..=3 {
        if map.points[cursor].children.len() != 1 { break; }
        cursor = map.points[cursor].children[0] as usize;
        buffers.candidate_token_named(slot, token_slot, "map_point_type", map.points[cursor].point_type.name())?;
    }
    for (scalar, value) in row.into_iter().enumerate() {
        buffers.candidate_scalar(slot, scalar, value, if scalar < 6 { 8.0 } else { 20.0 }, true);
    }
    buffers.candidate_bool(slot, 0, true);
    buffers.set_model_eligible(slot, action.enabled);
    Ok(())
}

fn write_card_row(
    buffers: &mut ObsBuffers<'_>,
    card: &Card,
    pile: &'static str,
    ordered_slot: Option<usize>,
    live: Option<LiveCardFacts>,
) -> Result<RowSpan, ObsError> {
    let start = buffers.state_rows();
    let row = buffers.push_state("card")?;
    buffers.state_token_named(row, 0, "card_id", card.id.name())?;
    // The frozen DTO publishes PascalCase card/target strings against an uppercase vocabulary.
    buffers.state_token(row, 1, 0);
    buffers.state_token(row, 2, 0);
    buffers.state_token_named(row, 3, "pile", pile)?;
    buffers.state_scalar(
        row,
        0,
        ordered_slot.unwrap_or(0) as i32,
        10.0,
        ordered_slot.is_some(),
    );
    buffers.state_scalar(row, 1, i32::from(card.upgraded), 3.0, true);
    if let Some(facts) = live {
        buffers.state_scalar(row, 2, facts.energy_cost, 5.0, true);
        buffers.state_scalar(row, 3, facts.star_cost, 5.0, true);
    }
    let enchantment = single_enchantment(card);
    buffers.state_scalar(row, 4, enchantment.1, 5.0, true);
    buffers.state_scalar(row, 5, card.observation_affliction_amount(), 5.0, true);
    buffers.state_bool(row, 0, card.upgraded);
    buffers.state_bool(row, 1, live.is_some_and(|facts| facts.playable));
    buffers.state_bool(row, 2, live.is_some_and(|_| card.x_cost));
    buffers.state_bool(row, 3, live.is_some_and(|_| card.star_x));
    buffers.state_bool(row, 4, true);
    buffers.state_bool(row, 5, true);
    buffers.state_bool(row, 6, true);

    let mut enchantments = card.enchantments.clone();
    enchantments.sort_unstable_by_key(|(name, _)| *name);
    for &(name, amount) in &enchantments {
        let child = buffers.push_state("enchantment")?;
        buffers.state_token_named(child, 0, "enchantment", name.name())?;
        buffers.state_token_named(child, 1, "pile", pile)?;
        buffers.state_scalar(child, 0, amount, 5.0, true);
    }

    let vars=public_mutation_vars(card);
    for &(name, value) in &vars {
        let child = buffers.push_state("dynamic_var")?;
        buffers.state_token_named(child, 0, "dynamic", name)?;
        buffers.state_token_named(child, 1, "pile", pile)?;
        buffers.state_token_named(child, 2, "card_id", card.id.name())?;
        // Captured int1 kernels_4.rs:1173-1178 / funcs_27.rs:1843:
        // division, sqrt and log are f64; publish f32 only at the end.
        let x = value as f64 / 10.0;
        let ax = x.abs();
        let magnitude = (ax + (ax * ax + 1.0).sqrt()).ln();
        let smooth = (if x < 0.0 { -magnitude } else { magnitude }) as f32;
        buffers.state_tail_value(child, 0, smooth);
        buffers.state_tail_value(child, 1, smooth);
    }
    Ok(RowSpan {
        start,
        end: buffers.state_rows(),
        valid: true,
    })
}

fn public_power_amount(combat: &Combat, p: PRef, proof: bool) -> Result<(bool, i32), ObsError> {
    let power = &combat.powers[p as usize];
    if power_meta(power.id).stack_type != PowerStackType::Counter {
        return Ok((false, 0));
    }
    if matches!(power.id,
        PowerId::Automation | PowerId::Cacophony | PowerId::Feral |
        PowerId::HardenedShell | PowerId::Monologue | PowerId::Orbit |
        PowerId::PaleBlueDot | PowerId::Panache | PowerId::Sloth |
        PowerId::Slow | PowerId::TagTeam | PowerId::Tender)
        && !proof
        && !super::rule_admission::admitted("public_power_counter", &format!("power:{}", power.id.name()))
    {
        return Err(ObsError::UnsupportedState("public power display counter"));
    }
    let amount = match power.id {
        // C# PowerModel.DisplayAmount overrides mirrored by frozen-r7
        // structured_simulator_teacher._public_power_display_amount. These are
        // presentation counters, not the backing stack amount.
        // The frozen wire reads AutomationPower.cards_left, not the newest
        // _instances entry. Reapplication adds a countdown without changing
        // the displayed counter until the next AfterCardDrawn callback.
        PowerId::Automation => power.vars[0],
        PowerId::Cacophony => (33 - power.vars[0]).max(0),
        PowerId::Feral => (power.amount - power.vars[0]).max(0),
        PowerId::HardenedShell => (power.amount - power.vars[0]).max(0),
        PowerId::Monologue => power.vars[0],
        PowerId::Orbit => 4 - power.vars[0].rem_euclid(4),
        PowerId::PaleBlueDot => {
            let attacks = combat.hist.play_start_cards.iter()
                .filter(|&&k| {
                    let card = &combat.cards[k as usize];
                    card.owner == power.owner && card.card_type == CardType::Attack
                })
                .count() as i32;
            (5 - attacks).max(0)
        }
        PowerId::Panache => (5 - power.vars[0]).max(0),
        PowerId::Sloth => power.vars[0],
        PowerId::Slow => power.vars[0] * 10,
        PowerId::TagTeam => 1,
        PowerId::Tender => power.vars[0],
        // C# WitheringPresencePower.DisplayAmount is per-target CardsLeft.
        PowerId::WitheringPresence => power.vars[0],
        _ => power.amount,
    };
    Ok((true, amount))
}

fn write_power_rows(
    combat: &Combat,
    actor: CId,
    owner: CId,
    owner_slot: Option<usize>,
    proof: bool,
    buffers: &mut ObsBuffers<'_>,
) -> Result<(), ObsError> {
    let mut powers = combat.cr(owner).power_list();
    powers.sort_unstable_by(|&a, &b| {
        let pa = &combat.powers[a as usize];
        let pb = &combat.powers[b as usize];
        let aa = public_power_amount(combat, a, proof).unwrap_or((false, 0)).1;
        let ba = public_power_amount(combat, b, proof).unwrap_or((false, 0)).1;
        pa.id.name().cmp(pb.id.name()).then_with(|| aa.cmp(&ba))
    });
    for p in powers {
        let power = &combat.powers[p as usize];
        let (known, amount) = public_power_amount(combat, p, proof)?;
        let row = buffers.push_state("power")?;
        buffers.state_token_named(row, 0, "power_id", power.id.name())?;
        buffers.state_scalar(row, 0, amount, 10.0, known);
        if let Some(slot) = owner_slot {
            buffers.state_scalar(row, 1, slot as i32, 8.0, true);
        }
        let targeted = power.id == PowerId::WitheringPresence
            && power.target.is_some_and(|target|combat.players.is_player(target));
        if targeted {
            buffers.state_scalar(row,2,power.target.unwrap() as i32,8.0,true);
        }
        buffers.state_bool(row, 0, owner == actor);
        buffers.state_bool(row, 1, targeted);
        // Frozen r7 structured_simulator_teacher._public_power_rows emits no
        // `instanced` flag for ordinary power rows; only its synthetic
        // per-target Withering Presence rows set that bit. Publishing the
        // simulator's internal stack metadata here changes the policy wire for
        // ordinary instanced powers such as Panache, Orbit and Monologue.
        buffers.state_bool(row, 2, targeted);
    }
    Ok(())
}

#[cfg(test)]
mod public_counter_tests {
    use super::*;

    #[test]
    fn sloth_count_survives_enemy_turn_history_clear_and_starts_at_application() {
        use crate::{hooks::Ev,creature::Side,tables::Hook};
        let mut c=combat();
        c.cards.push(Card::from_factory(CardId::StrikeIronclad,false,999).unwrap());
        c.hist.play_start_cards.push(0);
        let p=c.apply_power_to(PLAYER,PowerId::Sloth,3,Some(PLAYER)).unwrap();
        assert_eq!(public_power_amount(&c,p,true).unwrap(),(true,0));
        for _ in 0..3 { c.fire(Hook::BeforeCardPlayed,Ev::Card(0),None); }
        assert_eq!(public_power_amount(&c,p,true).unwrap(),(true,3));
        c.hist.play_start_cards.clear();
        c.fire(Hook::BeforeSideTurnStart,Ev::Side(Side::Enemy),None);
        assert_eq!(public_power_amount(&c,p,true).unwrap(),(true,3));
        c.fire(Hook::BeforeSideTurnStart,Ev::Side(Side::Player),None);
        assert_eq!(public_power_amount(&c,p,true).unwrap(),(true,0));
    }

    #[test]
    fn mad_science_block_is_projected_from_its_canonical_dynamic_var() {
        let mut card=Card::from_factory(CardId::MadScience,false,999).unwrap();
        card.block=None;
        card.vars.retain(|(key,_)|*key!=VarKey::Block);
        card.vars.push((VarKey::Block,8));
        for kind in [CardType::Attack,CardType::Skill,CardType::Power] {
            card.card_type=kind;
            assert_eq!(candidate_card_block(&card),Some(8));
        }
        card.vars.iter_mut().find(|(key,_)|*key==VarKey::Block).unwrap().1=11;
        assert_eq!(candidate_card_block(&card),Some(11));
    }

    fn combat() -> Combat {
        let run = crate::run::RunManager::new(1, crate::run::RunOptions::default());
        Combat::new(run.run_ctx().clone(), 1)
    }

    #[test]
    fn automation_reapplication_preserves_display_until_a_draw() {
        let mut c = combat();
        let p = c.apply_power_to(PLAYER, PowerId::Automation, 1, Some(PLAYER)).unwrap();
        assert_eq!(public_power_amount(&c, p, true).unwrap(), (true, 10));
        assert_eq!(public_power_amount(&c, p, false).unwrap(), (true, 10));
        for _ in 0..7 {
            assert_eq!(c.automation_on_draw(PLAYER), 0);
        }
        assert_eq!(public_power_amount(&c, p, true).unwrap(), (true, 3));
        c.apply_power_to(PLAYER, PowerId::Automation, 1, Some(PLAYER));
        // Frozen r7 _add_instance does not assign cards_left (seed
        // 20261011000000 d48). Both independent countdowns still advance.
        assert_eq!(public_power_amount(&c, p, true).unwrap(), (true, 3));
        assert_eq!(c.automation_on_draw(PLAYER), 0);
        assert_eq!(public_power_amount(&c, p, true).unwrap(), (true, 9));
        assert_eq!(c.automation_on_draw(PLAYER), 0);
        assert_eq!(c.automation_on_draw(PLAYER), 1);
        assert_eq!(public_power_amount(&c, p, true).unwrap(), (true, 7));
        c.remove_power(PLAYER, p);
        let p = c.apply_power_to(PLAYER, PowerId::Automation, 1, Some(PLAYER)).unwrap();
        assert_eq!(public_power_amount(&c, p, true).unwrap(), (true, 10));
    }

    #[test]
    fn tender_displays_play_count_not_stack_and_resets_on_owner_turn_end() {
        let mut c = combat();
        let p = c.apply_power_to(PLAYER, PowerId::Tender, 1, Some(PLAYER)).unwrap();
        assert_eq!(public_power_amount(&c, p, true).unwrap(), (true, 0));
        assert_eq!(public_power_amount(&c, p, false).unwrap(), (true, 0));
        c.set_power_private_int(PLAYER, PowerId::Tender, 0, 3);
        assert_eq!(public_power_amount(&c, p, true).unwrap(), (true, 3));
        c.fire_after_turn_end(crate::creature::Side::Enemy);
        assert_eq!(public_power_amount(&c, p, true).unwrap(), (true, 3));
        c.fire_after_turn_end(crate::creature::Side::Player);
        assert_eq!(public_power_amount(&c, p, true).unwrap(), (true, 0));
    }

    #[test]
    fn playing_toxic_does_not_advertise_its_end_of_turn_damage() {
        let card=Card::from_factory(CardId::Toxic,false,1).unwrap();
        assert_eq!(card.damage,Some(5));
        assert_eq!(candidate_card_damage(&card),Some(0));
        assert_eq!(candidate_base_damage(&card),None); // choose/exhaust is not OnPlay
        let infection=Card::from_factory(CardId::Infection,false,2).unwrap();
        assert_eq!(infection.damage,Some(3)); // C# OnTurnEndInHand penalty
        assert_eq!(candidate_base_damage(&infection),None); // selector doesn't deal it
        let burn=Card::from_factory(CardId::Burn,false,3).unwrap();
        assert_eq!(burn.damage,Some(2));
        assert_eq!(candidate_base_damage(&burn),None);
        let mut science=Card::from_factory(CardId::MadScience,false,4).unwrap();
        science.card_type=CardType::Power;
        science.block=None;
        science.vars.retain(|(key,_)|*key!=VarKey::Block);
        science.vars.push((VarKey::Block,8));
        assert_eq!(candidate_card_block(&science),Some(8));
    }

    #[test]
    fn forgotten_dread_preview_uses_live_dexterity_before_damage_hooks() {
        use crate::tables::{MonsterKind,monster_def};
        let mut c=combat();
        c.run.ascension=10;
        c.run.relics.clear();
        let (cr,mut ai)=c.create_monster(MonsterKind::TheForgotten,0);
        ai.current=monster_def(MonsterKind::TheForgotten,1).states.iter()
            .position(|state|state.id=="DREAD").unwrap() as u8;
        let enemy=c.add_enemy(cr,ai);
        c.apply_power_to(enemy,PowerId::Dexterity,4,Some(enemy));
        // Recorded seed 20261011000000 d1061: base15 + Dex4; the enemy's
        // own Vulnerable is target-side and must not affect its outgoing hit.
        c.apply_power_to(enemy,PowerId::Vulnerable,4,Some(PLAYER));
        assert_eq!(incoming_damage_for_enemy(&mut c,PLAYER,enemy).0,19);
        c.apply_power_to(enemy,PowerId::Strength,1,Some(enemy));
        c.apply_power_to(enemy,PowerId::Weak,1,Some(PLAYER));
        c.apply_power_to(PLAYER,PowerId::Vulnerable,1,Some(enemy));
        // C# additive stage: 15+4+1=20; multiplicative: .75*1.5; truncate.
        assert_eq!(incoming_damage_for_enemy(&mut c,PLAYER,enemy).0,22);
        c.run.ascension=0;
        assert_eq!(c.forgotten_dread_damage(enemy),17); // base13 + Dex4
    }

    #[test]
    fn test_subject_claw_preview_tracks_post_attack_repeat_growth() {
        use crate::tables::{MonsterKind,monster_def};
        let mut c=combat();
        c.run.ascension=10;
        c.run.relics.clear();
        let (cr,mut ai)=c.create_monster(MonsterKind::TestSubject,0);
        let mv=monster_def(MonsterKind::TestSubject,1).states.iter()
            .position(|state|state.id=="MULTI_CLAW_MOVE").unwrap() as u8;
        ai.current=mv;
        let enemy=c.add_enemy(cr,ai);
        assert_eq!(incoming_damage_for_enemy(&mut c,PLAYER,enemy),(33,3,0,1));
        c.monster_move_effect(enemy,mv);
        assert_eq!(incoming_damage_for_enemy(&mut c,PLAYER,enemy),(44,4,0,1));
    }

    #[test]
    fn terror_eel_intent_includes_vigor_before_an_attack_command_exists() {
        use crate::tables::{MonsterKind,monster_def};
        let mut c=combat();
        c.run.ascension=10;
        c.run.relics.clear();
        let (cr,mut ai)=c.create_monster(MonsterKind::TerrorEel,0);
        ai.current=monster_def(MonsterKind::TerrorEel,1).states.iter()
            .position(|state|state.id=="CRASH_MOVE").unwrap() as u8;
        let enemy=c.add_enemy(cr,ai);
        c.apply_power_to(enemy,PowerId::Vigor,6,Some(enemy));
        assert_eq!(incoming_damage_for_enemy(&mut c,PLAYER,enemy),(24,1,0,1));
        assert_eq!(c.power_amount(enemy,PowerId::Vigor),6); // preview never consumes
    }

    #[test]
    fn waterfall_pressure_gun_preview_grows_after_execution() {
        use crate::tables::{MonsterKind,monster_def};
        let mut c=combat();
        c.run.ascension=10;
        c.run.relics.clear();
        let (cr,mut ai)=c.create_monster(MonsterKind::WaterfallGiant,0);
        let mv=monster_def(MonsterKind::WaterfallGiant,1).states.iter()
            .position(|state|state.id=="PRESSURE_GUN_MOVE").unwrap() as u8;
        ai.current=mv;
        let enemy=c.add_enemy(cr,ai);
        assert_eq!(incoming_damage_for_enemy(&mut c,PLAYER,enemy).0,23);
        c.monster_move_effect(enemy,mv);
        assert_eq!(incoming_damage_for_enemy(&mut c,PLAYER,enemy).0,28);
        c.apply_power_to(enemy,PowerId::SteamEruption,125,Some(enemy)); // existing3 +125
        let about=monster_def(MonsterKind::WaterfallGiant,1).states.iter()
            .position(|state|state.id=="ABOUT_TO_BLOW_MOVE").unwrap() as u8;
        c.monster_move_effect(enemy,about);
        let explode=monster_def(MonsterKind::WaterfallGiant,1).states.iter()
            .position(|state|state.id=="EXPLODE_MOVE").unwrap() as u8;
        c.cr_mut(enemy).ai.as_mut().unwrap().current=explode;
        assert_eq!(incoming_damage_for_enemy(&mut c,PLAYER,enemy).0,128);
    }
}

fn relic_noncombat_counter(id: RelicId, counter: i32) -> Option<i32> {
    match id {
        // C# ShowCounter is CombatManager.IsInProgress for these four relics.
        // Keep the known-counter schema, but never expose their private
        // last-turn count in run screens. Do not mutate the persisted count.
        RelicId::Kusarigama | RelicId::Kunai | RelicId::OrnamentalFan | RelicId::Shuriken => Some(0),
        _ => relic_public_counter(id, counter),
    }
}

#[cfg(test)]
mod relic_counter_visibility_tests {
    use super::*;

    #[test]
    fn combat_only_relic_counters_are_zero_in_run_screens() {
        for id in [RelicId::Kusarigama,RelicId::Kunai,RelicId::OrnamentalFan,RelicId::Shuriken] {
            for count in 0..5 {
                assert_eq!(relic_noncombat_counter(id,count),Some(0));
                assert_eq!(relic_public_counter(id,count),Some(count%3));
            }
        }
        assert_eq!(relic_noncombat_counter(RelicId::HappyFlower,2),Some(2));
    }
}

pub(super) fn relic_public_counter(id: RelicId, counter: i32) -> Option<i32> {
    match id {
        RelicId::HappyFlower
        | RelicId::Pendulum
        | RelicId::FakeHappyFlower
        | RelicId::PollinousCore
        | RelicId::Girya => Some(counter.max(0)),
        RelicId::EmberTea => Some((5 - counter).max(0)),
        RelicId::Kusarigama | RelicId::Kunai | RelicId::OrnamentalFan | RelicId::Shuriken => {
            Some(counter.max(0) % 3)
        }
        RelicId::IronClub => Some(counter.max(0) % 4),
        _ => None,
    }
}

fn contains_ascii_ignore_case(haystack: &str, needle: &str) -> bool {
    let needle = needle.as_bytes();
    haystack.as_bytes().windows(needle.len()).any(|window| {
        window
            .iter()
            .zip(needle)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
    })
}

fn choice_purpose(prompt: &str, action: ActionKind) -> &'static str {
    if !matches!(action, ActionKind::Choose | ActionKind::ConfirmChoice) {
        return "none";
    }
    let text = prompt.trim();
    if text.is_empty() {
        return "unknown";
    }
    for (purpose, words) in [
        ("scry", &["scry"][..]),
        ("discard", &["discard"][..]),
        ("exhaust", &["exhaust"][..]),
        ("retain", &["retain"][..]),
        ("upgrade", &["upgrade", "smith"][..]),
        ("transform", &["transform"][..]),
        ("remove", &["remove"][..]),
        ("duplicate", &["duplicate", "copy", "clone"][..]),
        ("select_enemy", &["enemy", "target"][..]),
        ("select_card", &["card", "cards", "hand", "deck"][..]),
    ] {
        if words
            .iter()
            .any(|word| contains_ascii_ignore_case(text, word))
        {
            return purpose;
        }
    }
    "select_generic"
}

fn route_span(buffers: &mut ObsBuffers<'_>, slot: usize, span: RowSpan, source: bool) {
    if !span.valid {
        return;
    }
    for row in span.start..span.end {
        if source {
            buffers.source_link(slot, row);
        } else {
            buffers.target_link(slot, row);
        }
    }
}

fn write_candidate_base(
    buffers: &mut ObsBuffers<'_>,
    slot: usize,
    action_kind: &'static str,
    model_id: Option<&str>,
    card: Option<&Card>,
    cost: Option<i32>,
    damage: Option<i32>,
    block: Option<i32>,
    hp_cost: Option<i32>,
    selected: bool,
    can_confirm: bool,
) -> Result<(), ObsError> {
    buffers.candidate_token_named(slot, 0, "action_kind", action_kind)?;
    if let Some(model_id) = model_id {
        buffers.candidate_token(slot, 1, optional_token("card_id", model_id));
    }
    // Frozen r7 candidate CardType/TargetType strings have the same PascalCase mismatch.
    buffers.candidate_token(slot, 2, 0);
    buffers.candidate_token(slot, 3, 0);
    buffers.candidate_scalar(slot, 0, cost.unwrap_or(0), 10.0, cost.is_some());
    buffers.candidate_scalar(slot, 1, damage.unwrap_or(0), 50.0, damage.is_some());
    buffers.candidate_scalar(slot, 2, block.unwrap_or(0), 50.0, block.is_some());
    buffers.candidate_scalar(slot, 3, hp_cost.unwrap_or(0), 30.0, hp_cost.is_some());
    buffers.candidate_bool(slot, 0, true);
    buffers.candidate_bool(slot, 1, card.is_some_and(|value| value.x_cost));
    buffers.candidate_bool(slot, 2, card.is_some_and(|value| value.upgraded));
    buffers.candidate_bool(slot, 8, selected);
    buffers.candidate_bool(slot, 9, can_confirm);
    Ok(())
}

fn incoming_damage_for_enemy(combat: &mut Combat, actor: CId, enemy: CId) -> (i32, i32, i32, usize) {
    let Some(ai) = combat.cr(enemy).ai.clone() else {
        return (0, 0, 0, 0);
    };
    let Some(move_index) = ai.peek_move() else {
        return (0, 0, 0, 0);
    };
    let intents = ai.intents(move_index);
    let saved_fault = (combat.fault, combat.fault_card, combat.fault_power);
    let mut incoming = 0;
    let mut hits = 0;
    let mut status_cards = 0;
    for intent in intents {
        if matches!(
            intent.kind,
            IntentType::Attack | IntentType::MultiAttack | IntentType::DeathBlow
        ) {
            let hit_count = if ai.kind == crate::tables::MonsterKind::TestSubject
                && ai.state_id(move_index) == "MULTI_CLAW_MOVE"
            {
                combat.test_subject_claw_hits(enemy)
            } else { intent.hits };
            hits += hit_count;
            let base = if ai.kind == crate::tables::MonsterKind::TheForgotten
                && ai.state_id(move_index) == "DREAD"
            {
                combat.forgotten_dread_damage(enemy)
            } else if ai.kind == crate::tables::MonsterKind::WaterfallGiant
                && ai.state_id(move_index) == "PRESSURE_GUN_MOVE"
            {
                combat.waterfall_pressure_gun_damage(enemy)
            } else if ai.kind == crate::tables::MonsterKind::WaterfallGiant
                && ai.state_id(move_index) == "EXPLODE_MOVE"
            {
                combat.waterfall_eruption_damage(enemy)
            } else { intent.damage };
            incoming += combat.modify_damage(base, Some(enemy), actor, Props::MOVE, None)
                * hit_count;
        }
        if intent.kind == IntentType::StatusCard {
            status_cards += intent.cards;
        }
    }
    (combat.fault, combat.fault_card, combat.fault_power) = saved_fault;
    (incoming, hits, status_cards, intents.len())
}

fn smart_target_flags(
    combat: &Combat,
    card: &Card,
    hittable: &[CId],
    target: CId,
    incoming: &[i32],
) -> (bool, bool) {
    let mut best = 0usize;
    let mut best_score = f64::NEG_INFINITY;
    let mut target_kill = false;
    for (index, &enemy) in hittable.iter().enumerate() {
        let creature = combat.cr(enemy);
        let mut damage = card.damage.unwrap_or(0);
        if combat.power_amount(enemy, PowerId::Vulnerable) > 0 && damage > 0 {
            damage = damage * 3 / 2;
        }
        let can_kill = creature.hp > 0 && damage >= creature.hp + creature.block;
        let intent = incoming.get(index).copied().unwrap_or(0).max(0);
        let score = if can_kill {
            1000.0 + intent as f64 * 10.0
        } else {
            let attacking = if intent > 0 {
                100.0 + intent as f64 * 2.0
            } else {
                0.0
            };
            attacking + (50.0 - creature.hp as f64 * 0.5).max(0.0)
        };
        if score > best_score {
            best_score = score;
            best = index;
        }
        if enemy == target {
            target_kill = can_kill;
        }
    }
    (
        hittable.iter().position(|&enemy| enemy == target) == Some(best),
        target_kill,
    )
}

fn candidate_base_damage(card: &Card) -> Option<i32> {
    // C# Toxic/Infection/Burn DamageVars are consumed only by OnTurnEndInHand, not OnPlay.
    // Frozen r7 keeps that penalty in effect_vars, outside base_damage.
    if matches!(card.id, CardId::Toxic | CardId::Infection | CardId::Burn) {
        return None;
    }
    card.damage
}

fn candidate_card_damage(card: &Card) -> Option<i32> {
    candidate_base_damage(card)
        .or_else(|| (card.card_type != CardType::Attack).then_some(0))
}

fn candidate_card_block(card: &Card) -> Option<i32> {
    // C# MadScience has a canonical BlockVar(8) even when GainsBlock is false.
    // The wire publishes the base dynamic fact, not a prediction of OnPlay.
    card.block.or_else(|| card.var(VarKey::Block)).or_else(|| {
        card_info(card.id, card.upgraded)
            .is_some_and(|info| !info.gains_block)
            .then_some(0)
    })
}

fn fill_choice_numeric(
    buffers: &mut ObsBuffers<'_>,
    slot: usize,
    selected_count: i32,
    min: u16,
    max: u16,
    allow_skip: bool,
    is_multi: bool,
) {
    buffers.candidate_scalar(slot, 5, selected_count, 5.0, true);
    buffers.candidate_scalar(slot, 6, min as i32, 5.0, true);
    buffers.candidate_scalar(slot, 7, max as i32, 5.0, true);
    buffers.candidate_bool(slot, 10, allow_skip);
    buffers.candidate_bool(slot, 11, is_multi);
}

pub fn write_combat_obs_and_mask(
    combat: &mut Combat,
    context: &CombatObservationContext<'_>,
    buffers: &mut ObsBuffers<'_>,
    action_slots: &mut ActionSlots,
) -> Result<(), ObsError> {
    write_combat_obs_and_mask_for_player_inner(
        combat, PLAYER, context, buffers, action_slots, false,
    )
}

/// Encode one acting player's hand/resources and bind its candidate targets.
/// The original entry point remains the solo/r7 player-zero wrapper.
pub fn write_combat_obs_and_mask_for_player(
    combat: &mut Combat,
    actor: CId,
    context: &CombatObservationContext<'_>,
    buffers: &mut ObsBuffers<'_>,
    action_slots: &mut ActionSlots,
) -> Result<(), ObsError> {
    write_combat_obs_and_mask_for_player_inner(
        combat, actor, context, buffers, action_slots, false,
    )
}

/// Offline differential census only. Admission-only public counter checks are
/// bypassed so the candidate wire can be compared with frozen r7.
#[cfg(feature = "selector-proof")]
pub fn write_combat_proof_obs_and_mask(
    combat: &mut Combat,
    context: &CombatObservationContext<'_>,
    buffers: &mut ObsBuffers<'_>,
    action_slots: &mut ActionSlots,
) -> Result<(), ObsError> {
    write_combat_obs_and_mask_for_player_inner(
        combat, PLAYER, context, buffers, action_slots, true,
    )
}

pub(super) fn write_combat_obs_and_mask_for_player_inner(
    combat: &mut Combat,
    actor: CId,
    context: &CombatObservationContext<'_>,
    buffers: &mut ObsBuffers<'_>,
    action_slots: &mut ActionSlots,
    proof: bool,
) -> Result<(), ObsError> {
    if !combat.players.is_player(actor) {
        return Err(ObsError::InvalidActionSlot);
    }
    buffers.clear(context)?;
    action_slots.clear();
    combat.activate_choice_for_player(actor);
    if combat.pending_choice.as_ref().is_some_and(|choice| choice.player != actor) {
        return Err(ObsError::UnsupportedState("choice belongs to another player"));
    }
    if !proof && combat.pending_choice.as_ref().is_some_and(is_fury_choice)
        && !rule_admitted("combat_choice_prompt", "card:NEOWS_FURY")
    {
        return Err(ObsError::UnsupportedState("combat card choice prompt source is unported"));
    }

    let global = buffers.push_state("global")?;
    buffers.state_token_named(global, 0, "phase", "COMBAT")?;
    buffers.state_token_named(global, 1, "context", "combat")?;
    buffers.state_scalar(global, 0, combat.cr(actor).hp, 100.0, true);
    buffers.state_scalar(global, 1, combat.cr(actor).max_hp, 100.0, true);
    buffers.state_scalar(global, 2, combat.cr(actor).block, 50.0, true);
    buffers.state_scalar(global, 3, combat.players[actor].energy, 5.0, true);
    buffers.state_scalar(global, 4, combat.players[actor].base_max_energy, 5.0, true);
    buffers.state_scalar(global, 5, combat.players[actor].stars, 10.0, true);
    let gold = if actor == PLAYER { Some(combat.run.gold) } else { combat.players[actor].gold };
    buffers.state_scalar(global, 6, gold.unwrap_or(0), 500.0, gold.is_some());
    buffers.state_scalar(global, 7, combat.run.total_floor.max(1), 60.0, true);
    buffers.state_bool(global, 0, true);
    buffers.state_bool(global, 1, true);

    let player = buffers.push_state("player")?;
    buffers.state_scalar(player, 0, combat.run.ascension, 20.0, true);
    buffers.state_scalar(player, 1, combat.run.act_index + 1, 4.0, true);
    buffers.state_scalar(player, 2, combat.turn_count, 30.0, true);
    if combat.players.len() > 1 {
        buffers.state_scalar(player, 3, actor as i32, 4.0, true);
    }

    let mut hand_spans = [RowSpan::default(); R7_MAX_HAND_SIZE];
    let hand = combat.players[actor].hand.clone();
    for (index, k) in hand.iter().copied().enumerate() {
        // Frozen core/combat.py:1470: a pending selector blocks card plays.
        let playable = combat.pending_choice_for_player(actor).is_none() && combat.can_play_card(k);
        let energy_cost = combat.modified_card_cost(actor, k);
        let star_cost = combat.modified_star_cost(k);
        hand_spans[index] = write_card_row(
            buffers,
            &combat.cards[k as usize],
            "hand",
            Some(index),
            Some(LiveCardFacts {
                playable,
                energy_cost,
                star_cost,
            }),
        )?;
    }

    let mut persistent: ArrayVec<usize, MAX_OBS_CARDS> = ArrayVec::new();
    let deck = if actor == PLAYER { &combat.run.deck } else { &combat.players[actor].persistent_deck };
    for index in 0..deck.len() {
        persistent
            .try_push(index)
            .map_err(|_| ObsError::TooManyStateRows)?;
    }
    persistent.sort_unstable_by(|&a, &b| {
        compare_card_semantics(&deck[a], &deck[b])
    });
    for index in persistent {
        write_card_row(
            buffers,
            &deck[index],
            "persistent_deck",
            None,
            None,
        )?;
    }

    let draw = combat.players[actor].draw.clone();
    let discard = combat.players[actor].discard.clone();
    let exhaust = combat.players[actor].exhaust.clone();
    for (pile, refs) in [("draw", draw), ("discard", discard), ("exhaust", exhaust)] {
        let mut sorted: ArrayVec<KRef, MAX_OBS_CARDS> = ArrayVec::new();
        for k in refs {
            sorted.try_push(k).map_err(|_| ObsError::TooManyStateRows)?;
        }
        sorted.sort_unstable_by(|&a, &b| {
            compare_card_semantics(&combat.cards[a as usize], &combat.cards[b as usize])
        });
        for k in sorted {
            write_card_row(buffers, &combat.cards[k as usize], pile, None, None)?;
        }
    }

    let pending = combat.pending_choice.clone();
    let mut choice_spans: ArrayVec<RowSpan, R7_ACTION_SPACE_SIZE> = ArrayVec::new();
    if pending.is_some() {
        let options = combat.choice_options.clone();
        for (index, k) in options.iter().copied().enumerate() {
            let playable = false; // This row is an option in a pending selector.
            let energy_cost = combat.modified_card_cost(actor, k);
            let star_cost = combat.modified_star_cost(k);
            let span = write_card_row(
                buffers,
                &combat.cards[k as usize],
                "scry",
                Some(index),
                Some(LiveCardFacts {
                    playable,
                    energy_cost,
                    star_cost,
                }),
            )?;
            choice_spans
                .try_push(span)
                .map_err(|_| ObsError::TooManyStateRows)?;
        }
    }

    write_power_rows(combat, actor, actor, None, proof, buffers)?;

    // Multiplayer extension: retain every solo row unchanged. Each additional player
    // has a player row (HP, max HP, block, energy, max energy, stars, seat, hand size)
    // followed by its public powers; target routes cover that span.
    let mut player_spans: ArrayVec<(CId, RowSpan), 4> = ArrayVec::new();
    if combat.players.len() > 1 {
        player_spans.push((actor, RowSpan { start: global, end: player + 1, valid: true }));
        for teammate in (0..combat.players.len() as CId).filter(|&p| p != actor) {
            let start = buffers.state_rows();
            let row = buffers.push_state("player")?;
            let creature = combat.cr(teammate);
            buffers.state_scalar(row, 0, creature.hp, 100.0, true);
            buffers.state_scalar(row, 1, creature.max_hp, 100.0, true);
            buffers.state_scalar(row, 2, creature.block, 50.0, true);
            buffers.state_scalar(row, 3, combat.players[teammate].energy, 5.0, true);
            buffers.state_scalar(row, 4, combat.players[teammate].base_max_energy, 5.0, true);
            buffers.state_scalar(row, 5, combat.players[teammate].stars, 10.0, true);
            buffers.state_scalar(row, 6, teammate as i32, 4.0, true);
            buffers.state_scalar(row, 7, combat.players[teammate].hand.len() as i32, 10.0, true);
            buffers.state_bool(row, 0, creature.is_alive());
            buffers.state_bool(row, 1, creature.is_dead());
            write_power_rows(combat, actor, teammate, Some(teammate as usize), proof, buffers)?;
            player_spans.try_push((teammate, RowSpan {
                start, end: buffers.state_rows(), valid: true,
            })).map_err(|_| ObsError::TooManyStateRows)?;
        }
    }

    let enemies = combat.enemies.clone();
    // Multiplayer pet rows: explicit owner seat, life and Osty identity, followed by
    // that pet's public powers. Keep the frozen solo wire unchanged.
    if combat.players.len() > 1 {
        for owner in 0..combat.players.len() as CId {
            if let Some(osty) = combat.osty(owner) {
                let row = buffers.push_state("player")?;
                let pet = combat.cr(osty);
                buffers.state_scalar(row, 0, pet.hp, 100.0, true);
                buffers.state_scalar(row, 1, pet.max_hp, 100.0, true);
                buffers.state_scalar(row, 2, pet.block, 50.0, true);
                buffers.state_scalar(row, 6, owner as i32, 4.0, true);
                buffers.state_bool(row, 0, pet.is_alive());
                buffers.state_bool(row, 1, pet.is_dead());
                buffers.state_bool(row, 2, true); // Osty row, not a player seat.
                write_power_rows(combat, actor, osty, Some(owner as usize), proof, buffers)?;
            }
        }
    }
    let mut enemy_spans: ArrayVec<(CId, RowSpan), 8> = ArrayVec::new();
    let mut incoming_by_enemy: ArrayVec<(CId, i32), 8> = ArrayVec::new();
    for (enemy_index, enemy) in enemies.iter().copied().enumerate() {
        let start = buffers.state_rows();
        let creature = combat.cr(enemy).clone();
        let (incoming, attack_hits, status_cards, intent_count) =
            incoming_damage_for_enemy(combat, actor, enemy);
        incoming_by_enemy
            .try_push((enemy, incoming))
            .map_err(|_| ObsError::TooManyStateRows)?;
        let row = buffers.push_state("enemy")?;
        buffers.state_token_named(row, 0, "monster_id", creature.monster_id)?;
        for slot in 1..TOKEN_SLOTS.min(intent_count + 1) {
            buffers.state_token(row, slot, 0);
        }
        buffers.state_scalar(row, 0, creature.hp, 100.0, true);
        buffers.state_scalar(row, 1, creature.max_hp, 100.0, true);
        buffers.state_scalar(row, 2, creature.block, 50.0, true);
        buffers.state_scalar(row, 3, incoming, 50.0, true);
        buffers.state_scalar(row, 4, enemy_index as i32, 8.0, true);
        buffers.state_scalar(row, 5, attack_hits, 10.0, true);
        buffers.state_scalar(row, 6, status_cards, 10.0, true);
        buffers.state_bool(row, 0, creature.is_alive());
        buffers.state_bool(row, 1, creature.is_dead());
        buffers.state_bool(row, 2, creature.ai.is_some());
        buffers.state_bool(row, 3, creature.ai.is_some());
        write_power_rows(combat, actor, enemy, Some(enemy_index), proof, buffers)?;
        enemy_spans
            .try_push((
                enemy,
                RowSpan {
                    start,
                    end: buffers.state_rows(),
                    valid: true,
                },
            ))
            .map_err(|_| ObsError::TooManyStateRows)?;
    }

    let mut potion_rows = [None; R7_MAX_POTION_SLOTS];
    let potion_slots = combat.potion_slots_for_player(actor)
        .ok_or(ObsError::UnsupportedState("player potion slots missing"))?.to_vec();
    let potion_capacity = if actor == crate::combat::PLAYER {
        combat.run.max_potion_slots.max(0) as usize
    } else { potion_slots.len() };
    for slot in 0..potion_capacity {
        if slot >= R7_MAX_POTION_SLOTS {
            return Err(ObsError::UnsupportedState("more than r7 potion slots"));
        }
        let row = buffers.push_state("potion")?;
        potion_rows[slot] = Some(row);
        buffers.state_scalar(row, 0, slot as i32, 9.0, true);
        if let Some(Some(id)) = potion_slots.get(slot) {
            buffers.state_bool(row, 0, true);
            buffers.state_token_named(row, 0, "potion_id", id.name())?;
        }
    }

    // Frozen structured_encoder emits queued orbs after potions and before
    // relics. C# DarkOrb starts its independently accumulated EvokeVal at 6.
    for (index, &orb) in combat.players[actor].orbs.orbs.iter().enumerate() {
        let row = buffers.push_state("orb")?;
        buffers.state_token_named(row, 0, "orb_type", orb.kind.name())?;
        buffers.state_scalar(row, 0, index as i32, 10.0, true);
        buffers.state_tail_value(row, 2,
            (combat.orb_passive_value(actor, orb) as f64 / 10.0).asinh() as f32);
        buffers.state_tail_value(row, 3,
            (combat.orb_evoke_value(actor, orb) as f64 / 10.0).asinh() as f32);
    }
    for relic in &combat.run.relics {
        let row = buffers.push_state("relic")?;
        buffers.state_token_named(row, 0, "relic_id", relic.id.name())?;
        let counter = relic_public_counter(relic.id, relic.counter);
        buffers.state_scalar(row, 0, counter.unwrap_or(0), 100.0, counter.is_some());
        buffers.state_bool(row, 0, counter.is_some());
    }

    // Frozen r7 structured_simulator_teacher._build_combat_request gathers
    // both PlayerState.relic_objects and the legacy PlayerState.relics IDs.
    // Its (model_id,counter) de-dup key therefore retains a second unknown-
    // counter row only for relics whose object row exposes a public counter.
    // Noncombat uses get_relic_objects() once and does not have this duplicate.
    for relic in &combat.run.relics {
        if relic_public_counter(relic.id, relic.counter).is_none() { continue; }
        let row = buffers.push_state("relic")?;
        buffers.state_token_named(row, 0, "relic_id", relic.id.name())?;
        buffers.state_scalar(row, 0, 0, 100.0, false);
        buffers.state_bool(row, 0, false);
    }

    let hittable: ArrayVec<CId, 8> = combat
        .enemies
        .iter()
        .copied()
        .filter(|&enemy| combat.can_hit(enemy))
        .collect();
    let mut incoming_hittable: ArrayVec<i32, 8> = ArrayVec::new();
    for &enemy in &hittable {
        incoming_hittable
            .try_push(
                incoming_by_enemy
                    .iter()
                    .find(|(candidate, _)| *candidate == enemy)
                    .map_or(0, |(_, damage)| *damage),
            )
            .map_err(|_| ObsError::TooManyCandidates)?;
    }

    let mut legal_actions: ArrayVec<u16, R7_ACTION_SPACE_SIZE> = ArrayVec::new();
    if combat.is_over {
        legal_actions.push(0);
    } else if let Some(ref choice) = pending {
        let selected = combat.choice_selected.len() as u16;
        for index in 0..combat.choice_options.len().min(R7_ACTION_SPACE_SIZE - 1) {
            let legal = if choice.is_multi() {
                combat
                    .choice_selected
                    .iter()
                    .any(|&value| value as usize == index)
                    || selected < choice.max
            } else {
                true
            };
            if legal {
                legal_actions.push((index + 1) as u16);
            }
        }
        if selected >= choice.min || (choice.allow_skip && selected == 0) {
            legal_actions.push(0);
        }
    } else {
        for index in 0..combat.players[actor].hand.len().min(R7_MAX_HAND_SIZE) {
            let k = combat.players[actor].hand[index];
            if !combat.can_play_card(k) {
                continue;
            }
            match combat.card_target_type(k) {
                TargetType::Self_
                | TargetType::None
                | TargetType::AllEnemies
                | TargetType::AllAllies => {
                    legal_actions.push((1 + index) as u16);
                }
                TargetType::AnyEnemy | TargetType::RandomEnemy => {
                    for target_slot in 0..hittable.len().min(R7_MAX_ENEMIES) {
                        legal_actions.push(
                            (1 + R7_MAX_HAND_SIZE + index * R7_MAX_ENEMIES + target_slot) as u16,
                        );
                    }
                }
                TargetType::AnyAlly => {
                    // C# CardModel.IsValidTarget: living players on the owner's side.
                    for target in 0..combat.players.len().min(R7_MAX_ENEMIES) {
                        if combat.cr(target as CId).is_alive() {
                            legal_actions.push(
                                (1 + R7_MAX_HAND_SIZE + index * R7_MAX_ENEMIES + target) as u16,
                            );
                        }
                    }
                }
            }
        }
        for potion_slot in 0..potion_slots.len().min(R7_MAX_POTION_SLOTS) {
            let Some(id) = potion_slots[potion_slot] else {
                continue;
            };
            let action_base = R7_POTION_ACTION_START + potion_slot * R7_POTION_TARGET_OPTIONS;
            match POTION_INFO[id.index()].target {
                PotionTargetType::Self_ | PotionTargetType::AllEnemies => {
                    if combat.can_use_potion_for_player(actor, potion_slot, None) {
                        legal_actions.push(action_base as u16);
                    }
                }
                PotionTargetType::AnyPlayer => {
                    if combat.can_use_potion_for_player(actor, potion_slot, None) {
                        legal_actions.push(action_base as u16);
                    }
                    for seat in 0..combat.players.len() {
                        // The base slot already targets the actor. Preserve r7's
                        // solo mask and encode each other recipient exactly once.
                        if seat == usize::from(actor) { continue; }
                        if combat.can_use_potion_for_player(actor, potion_slot, Some(seat)) {
                            legal_actions.push((action_base + 1 + seat) as u16);
                        }
                    }
                }
                PotionTargetType::AnyEnemy => {
                    for (compact, &enemy) in hittable.iter().take(R7_MAX_ENEMIES).enumerate() {
                        let raw = combat
                            .enemies
                            .iter()
                            .position(|&candidate| candidate == enemy)
                            .ok_or(ObsError::InvalidActionSlot)?;
                        if combat.can_use_potion_for_player(actor, potion_slot, Some(raw)) {
                            legal_actions.push((action_base + 1 + compact) as u16);
                        }
                    }
                }
                _ => {}
            }
        }
        legal_actions.sort_unstable();
        legal_actions.push(0);
    }

    let mut random_target_cards=[false;R7_MAX_HAND_SIZE];
    for simulator_action in legal_actions {
        let binding =
            decode_combat_action_for_player(simulator_action, combat, actor, hittable.as_slice(), pending.is_some())?;
        let slot = buffers.push_candidate()?;
        action_slots.push(binding)?;
        buffers.set_choice_purpose(slot, choice_purpose(context.choice_prompt, binding.kind))?;
        write_candidate(
            combat,
            actor,
            context,
            buffers,
            slot,
            binding,
            pending.as_ref(),
            &hand_spans,
            &choice_spans,
            &enemy_spans,
            &player_spans,
            &potion_rows,
            hittable.as_slice(),
            incoming_hittable.as_slice(),
        )?;
        // Frozen structured_env.py:1619-1627 keeps the per-enemy rows but
        // masks duplicate RandomEnemy actions by card instance, not card ID.
        // The resolver still chooses the enemy; the first binding stays intact.
        if combat.players.len()==1 && binding.kind==ActionKind::PlayCard {
            let index=binding.hand_index as usize;
            let k=combat.players[actor].hand[index];
            if combat.card_target_type(k)==TargetType::RandomEnemy && buffers.model_mask[slot]!=0 {
                if random_target_cards[index] {
                    buffers.set_model_eligible(slot,false);
                    // Both rows are valid gameplay bindings; only the model
                    // eligibility mask de-duplicates random-target plays.
                }
                random_target_cards[index]=true;
            }
        }
    }
    Ok(())
}

fn is_fury_choice(choice: &PendingChoice) -> bool {
    choice.resolver.src == crate::choice::ChoiceSrc::Card(CardId::NeowsFury)
}

fn write_fury_descriptor(
    buffers: &mut ObsBuffers<'_>, slot: usize, choice: &PendingChoice,
) -> Result<(), ObsError> {
    if is_fury_choice(choice) {
        buffers.candidate_token(slot, 4, sts2_wire::vocab::OPTIONAL_DISCARD_TO_HAND);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_candidate(
    combat: &mut Combat,
    actor: CId,
    _context: &CombatObservationContext<'_>,
    buffers: &mut ObsBuffers<'_>,
    slot: usize,
    binding: ActionSlot,
    pending: Option<&PendingChoice>,
    hand_spans: &[RowSpan; R7_MAX_HAND_SIZE],
    choice_spans: &[RowSpan],
    enemy_spans: &[(CId, RowSpan)],
    player_spans: &[(CId, RowSpan)],
    potion_rows: &[Option<usize>; R7_MAX_POTION_SLOTS],
    hittable: &[CId],
    incoming_hittable: &[i32],
) -> Result<(), ObsError> {
    match binding.kind {
        ActionKind::EndTurn => {
            write_candidate_base(
                buffers,
                slot,
                "end_turn",
                None,
                None,
                Some(0),
                Some(0),
                Some(0),
                Some(0),
                false,
                false,
            )?;
            buffers.set_model_eligible(slot, true);
        }
        ActionKind::ConfirmChoice => {
            let choice = pending.ok_or(ObsError::InvalidActionSlot)?;
            write_fury_descriptor(buffers, slot, choice)?;
            let selected_count = combat.choice_selected.len() as i32;
            let can_confirm =
                selected_count >= choice.min as i32 || (choice.allow_skip && selected_count == 0);
            write_candidate_base(
                buffers,
                slot,
                "confirm_choice",
                None,
                None,
                Some(0),
                Some(0),
                Some(0),
                Some(0),
                false,
                can_confirm,
            )?;
            buffers.candidate_token(slot, 1, optional_token("action_kind", "confirm_choice"));
            fill_choice_numeric(
                buffers,
                slot,
                selected_count,
                choice.min,
                choice.max,
                choice.allow_skip,
                choice.is_multi(),
            );
            buffers.set_model_eligible(slot, true);
        }
        ActionKind::Choose => {
            let choice = pending.ok_or(ObsError::InvalidActionSlot)?;
            write_fury_descriptor(buffers, slot, choice)?;
            let option_index = binding.choice_index as usize;
            let k = *combat
                .choice_options
                .get(option_index)
                .ok_or(ObsError::InvalidActionSlot)?;
            let selected = combat
                .choice_selected
                .iter()
                .any(|&index| index as usize == option_index);
            let selected_count = combat.choice_selected.len() as i32;
            let can_confirm =
                selected_count >= choice.min as i32 || (choice.allow_skip && selected_count == 0);
            let cost = combat.modified_card_cost(actor, k);
            let card = combat.cards[k as usize].clone();
            write_candidate_base(
                buffers,
                slot,
                "choose",
                Some(card.id.name()),
                Some(&card),
                Some(cost),
                candidate_base_damage(&card),
                card.block,
                Some(0),
                selected,
                can_confirm,
            )?;
            fill_choice_numeric(
                buffers,
                slot,
                selected_count,
                choice.min,
                choice.max,
                choice.allow_skip,
                choice.is_multi(),
            );
            buffers.set_model_eligible(slot, !selected);
            if let Some(span) = choice_spans.get(option_index).copied() {
                buffers.set_source_applicable(slot, true, true);
                route_span(buffers, slot, span, true);
            } else {
                buffers.set_source_applicable(slot, true, false);
            }
        }
        ActionKind::PlayCard => {
            let hand_index = binding.hand_index as usize;
            let k = *combat.players[actor]
                .hand
                .get(hand_index)
                .ok_or(ObsError::InvalidActionSlot)?;
            let cost = combat.modified_card_cost(actor, k);
            let card = combat.cards[k as usize].clone();
            let damage = candidate_card_damage(&card);
            let block = candidate_card_block(&card);
            let hp_cost = matches!(
                card.id,
                CardId::StrikeIronclad | CardId::DefendIronclad | CardId::Bash
            )
            .then_some(0);
            write_candidate_base(
                buffers,
                slot,
                "play_card",
                Some(card.id.name()),
                Some(&card),
                Some(cost),
                damage,
                block,
                hp_cost,
                false,
                false,
            )?;
            buffers.set_model_eligible(slot, optional_token("card_id", card.id.name()) != 0);
            buffers.set_source_applicable(slot, true, true);
            route_span(buffers, slot, hand_spans[hand_index], true);

            let target_type = combat.card_target_type(k);
            if target_type != TargetType::RandomEnemy {
                if let Some(target) = binding.target {
                    if let Some((_, span)) = enemy_spans.iter().chain(player_spans.iter())
                        .find(|(creature, _)| *creature == target)
                    {
                        buffers.set_target_applicable(slot, true, true);
                        route_span(buffers, slot, *span, false);
                    } else {
                        buffers.set_target_applicable(slot, true, false);
                    }
                    if target_type != TargetType::AnyAlly {
                        let (recommended, can_kill) =
                            smart_target_flags(combat, &card, hittable, target, incoming_hittable);
                        buffers.candidate_bool(slot, 12, recommended);
                        buffers.candidate_bool(slot, 13, can_kill);
                    }
                } else if matches!(target_type, TargetType::AnyEnemy) {
                    buffers.set_target_applicable(slot, true, false);
                }
            }
        }
        ActionKind::UsePotion => {
            let potion_slot = binding.potion_slot as usize;
            let id = combat
                .potion_slots_for_player(binding.actor)
                .and_then(|slots| slots.get(potion_slot))
                .copied()
                .flatten()
                .ok_or(ObsError::InvalidActionSlot)?;
            write_candidate_base(
                buffers,
                slot,
                "use_potion",
                Some(id.name()),
                None,
                None,
                None,
                None,
                None,
                false,
                false,
            )?;
            let potion_token = optional_token("potion_id", id.name());
            buffers.candidate_token(slot, 1, potion_token);
            buffers.set_model_eligible(slot, potion_token != 0);
            if let Some(row) = potion_rows.get(potion_slot).copied().flatten() {
                buffers.set_source_applicable(slot, true, true);
                buffers.source_link(slot, row);
            } else {
                buffers.set_source_applicable(slot, true, false);
            }
            // The frozen solo r7 potion DTO has implicit self targeting.
            // Keep C# actor-bound execution, but not a new policy target link.
            let implicit_solo_self = combat.players.len() == 1
                && matches!(POTION_INFO[id.index()].target, PotionTargetType::Self_ | PotionTargetType::AnyPlayer);
            if let Some(target) = binding.target.filter(|_| !implicit_solo_self) {
                if let Some((_, span)) = enemy_spans.iter().chain(player_spans.iter())
                    .find(|(creature, _)| *creature == target)
                {
                    buffers.set_target_applicable(slot, true, true);
                    route_span(buffers, slot, *span, false);
                } else {
                    buffers.set_target_applicable(slot, true, false);
                }
            } else if matches!(POTION_INFO[id.index()].target, PotionTargetType::AnyEnemy) {
                buffers.set_target_applicable(slot, true, false);
            }
        }
        ActionKind::Empty => return Err(ObsError::InvalidActionSlot),
    }
    Ok(())
}
