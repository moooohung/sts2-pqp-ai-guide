//! Seat-bound menus exposed by run::mp. No fabricated rest/shop/event state.
use super::*;
use crate::run::mp::{MpPhase, MpRun};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MpMenuAction {
    ChooseNeow(usize),
    ChooseNeowCard(Option<usize>),
    PickCard(usize),
    PickSpecialCard(usize),
    PickPotion,
    TakeGoldSkipRemaining,
    CheeseGorge,
    ChooseCheeseCard(usize),
    ConfirmCheese([usize; 2]),
    LeaveShop,
    BeginMerchantRemoval,
    CancelMerchantRemoval,
    MerchantPurchase(crate::run::mp::MerchantAction),
}

/// Candidate slot -> original driver's authenticated actor and method argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MpMenuBinding {
    pub actor: CId,
    pub net_id: u64,
    pub action: MpMenuAction,
}

/// Policy menu candidates are NOT combat ActionSlots/discrete action IDs.
pub fn mp_menu_decision_owner(run: &MpRun, after: Option<CId>) -> Option<CId> {
    let n = run.roster.players().len();
    let start = after.map_or(0, |seat| (seat as usize + 1) % n);
    (0..n).map(|i| (start + i) % n).find(|&seat| match run.phase {
        MpPhase::Neow => run.neow_finished(seat) == Some(false),
        MpPhase::Rewards => run.rewards.as_ref().and_then(|r| r.get(seat)).is_some_and(|r| !r.completed),
        MpPhase::Event => run.world.current_room == Some("ROOM_FULL_OF_CHEESE")
            && run.cheese_finished(seat) == Some(false),
        MpPhase::Shop => true,
        _ => false,
    }).map(|seat| seat as CId)
}

/// Frozen-r7 menu wire for one roster seat. Pure snapshot rendering: never
/// generate rewards, execute choices or advance shared/personal RNGs.
/// On error discard buffers and bindings. Only driver-exposed event/shop
/// actions are admitted; rest and merchant purchases remain unsupported.
pub fn write_mp_menu_perspective(
    run: &MpRun, actor: CId, context: &CombatObservationContext<'_>,
    buffers: &mut ObsBuffers<'_>, bindings: &mut Vec<MpMenuBinding>,
) -> Result<(), ObsError> {
    write_mp_menu_with_selection(run, actor, &[], context, buffers, bindings)
}

/// `selected` is consumer-local state for this seat's current Cheese offer.
/// ChooseCheeseCard updates that state; only ConfirmCheese calls the driver.
pub fn write_mp_menu_with_selection(
    run: &MpRun, actor: CId, selected: &[usize], context: &CombatObservationContext<'_>,
    buffers: &mut ObsBuffers<'_>, bindings: &mut Vec<MpMenuBinding>,
) -> Result<(), ObsError> {
    write_mp_menu_with_selection_mode(run,actor,selected,context,buffers,bindings,false)
}

/// Offline original-DLL evidence only; normal MP menus remain fail-closed.
#[cfg(feature="selector-proof")]
pub fn write_mp_special_reward_proof(
    run:&MpRun, actor:CId, context:&CombatObservationContext<'_>,
    buffers:&mut ObsBuffers<'_>, bindings:&mut Vec<MpMenuBinding>,
) -> Result<(),ObsError> {
    if run.phase!=MpPhase::Rewards {return Err(ObsError::UnsupportedState("not at rewards"));}
    write_mp_menu_with_selection_mode(run,actor,&[],context,buffers,bindings,true)
}

fn write_mp_menu_with_selection_mode(
    run:&MpRun, actor:CId, selected:&[usize], context:&CombatObservationContext<'_>,
    buffers:&mut ObsBuffers<'_>, bindings:&mut Vec<MpMenuBinding>, special_proof:bool,
) -> Result<(),ObsError> {
    bindings.clear();
    let seat = actor as usize;
    let player = run.roster.player(seat).ok_or(ObsError::InvalidActionSlot)?;
    if run.paels_growth_choices[seat].is_some() || run.shears_choices[seat].is_some()
        || run.relic_rewards[seat].is_some() {
        return Err(ObsError::UnsupportedState("pickup modal not encoded"));
    }
    let neow_cards = run.neow_cards[seat].as_ref();
    let cheese = if run.phase == MpPhase::Event && run.world.current_room == Some("ROOM_FULL_OF_CHEESE") {
        if run.cheese_finished(seat) != Some(false) {
            return Err(ObsError::UnsupportedState("seat event completed"));
        }
        run.cheese_cards[seat].as_ref()
    } else { None };
    if !selected.is_empty() && (cheese.is_none() || selected.len() > 2
        || selected.iter().enumerate().any(|(i, &v)|
            v >= cheese.unwrap().len() || selected[..i].contains(&v))) {
        return Err(ObsError::InvalidActionSlot);
    }
    let mut actions = Vec::new();
    match run.phase {
        MpPhase::Neow if run.neow_finished(seat) == Some(false) => {
            if let Some(cards) = neow_cards {
                for index in 0..cards.len() { actions.push(MpMenuAction::ChooseNeowCard(Some(index))); }
                actions.push(MpMenuAction::ChooseNeowCard(None));
            } else {
                for index in 0..run.neow[seat].len() { actions.push(MpMenuAction::ChooseNeow(index)); }
            }
        }
        MpPhase::Rewards => {
            let reward = run.rewards.as_ref().and_then(|r| r.get(seat))
                .ok_or(ObsError::UnsupportedState("rewards not offered"))?;
            if reward.completed { return Err(ObsError::UnsupportedState("seat rewards completed")); }
            if reward.special_cards.iter().any(Option::is_some) {
                if !special_proof && !super::rule_admission::admitted(
                    "extra_reward_csharp_r8_v1","reward:SPECIAL_CARD") {
                    return Err(ObsError::UnsupportedState("unproven SPECIAL_CARD whole-screen inventory"));
                }
                for index in 0..reward.special_cards.len() {
                    if reward.special_cards[index].is_some() {actions.push(MpMenuAction::PickSpecialCard(index));}
                }
            }
            if !reward.card_taken {
                for index in 0..reward.cards.len() { actions.push(MpMenuAction::PickCard(index)); }
            }
            if !reward.potion_taken && reward.potion.is_some()
                && (player.potions.iter().any(Option::is_none)
                    || player.relics.iter().any(|r| r.id == crate::tables::RelicId::Sozu && r.enabled)) {
                actions.push(MpMenuAction::PickPotion);
            }
            actions.push(MpMenuAction::TakeGoldSkipRemaining);
        }
        MpPhase::Event if run.world.current_room == Some("ROOM_FULL_OF_CHEESE") => {
            if let Some(cards) = cheese {
                for index in 0..cards.len() { actions.push(MpMenuAction::ChooseCheeseCard(index)); }
                if selected.len() == 2 { actions.push(MpMenuAction::ConfirmCheese([selected[0], selected[1]])); }
            } else { actions.push(MpMenuAction::CheeseGorge); }
        }
        MpPhase::Shop => {
            let offers = run.merchant_offers(player.net_id, seat).map_err(ObsError::UnsupportedState)?;
            let mut removal = false;
            for offer in offers {
                if matches!(offer.action, crate::run::mp::MerchantAction::RemoveCard(_)) {
                    if !removal { actions.push(MpMenuAction::BeginMerchantRemoval); removal = true; }
                } else { actions.push(MpMenuAction::MerchantPurchase(offer.action)); }
            }
            actions.push(MpMenuAction::LeaveShop);
        }
        _ => return Err(ObsError::UnsupportedState("no exposed multiplayer menu")),
    }
    // Reuse the existing encoder's sorted persistent-card/inventory rows, not
    // a second game manager. This temporary Combat is only a rendering carrier.
    let mut ctx = run.world.run.clone();
    ctx.hp = player.hp; ctx.max_hp = player.max_hp; ctx.gold = player.gold;
    ctx.max_energy = player.max_energy; ctx.max_potion_slots = player.potions.len() as i32;
    ctx.deck = player.deck.clone(); ctx.relics = player.relics.clone(); ctx.potions = player.potions.clone();
    // Encode each generated option using the same card-row implementation.
    // r7 relational_observation.py:509-534 appends generated choices after the
    // owned inventory, with no persistent-deck pile token.
    let mut option_rows = Vec::new();
    if let Some(cards) = cheese {
        for card in cards {
            let mut option_ctx = ctx.clone();
            option_ctx.deck = vec![card.clone()];
            option_ctx.relics.clear(); option_ctx.potions.clear(); option_ctx.max_potion_slots = 0;
            let mut option = crate::combat::Combat::new(option_ctx, 0);
            option.is_over = true;
            write_combat_obs_and_mask(&mut option, context, buffers, &mut ActionSlots::new())?;
            option_rows.push((2..buffers.state_rows()).map(|row| SavedRow::capture(buffers, row)).collect::<Vec<_>>());
        }
    }
    let mut ordered_options = Vec::new();
    if let Some(cards) = cheese {
        let mut option_ctx = ctx.clone();
        option_ctx.deck = cards.clone();
        option_ctx.relics.clear(); option_ctx.potions.clear(); option_ctx.max_potion_slots = 0;
        let mut option = crate::combat::Combat::new(option_ctx, 0);
        option.is_over = true;
        write_combat_obs_and_mask(&mut option, context, buffers, &mut ActionSlots::new())?;
        let mut groups: Vec<Vec<SavedRow>> = Vec::new();
        for row in 2..buffers.state_rows() {
            if buffers.state_kind_ids[row] == tokens::kind_id("card").unwrap() { groups.push(Vec::new()); }
            groups.last_mut().ok_or(ObsError::UnsupportedState("option card row missing"))?
                .push(SavedRow::capture(buffers, row));
        }
        let mut used = vec![false; option_rows.len()];
        for rows in groups {
            let index = option_rows.iter().enumerate().position(|(i, original)| !used[i] && *original == rows)
                .ok_or(ObsError::UnsupportedState("option row identity missing"))?;
            used[index] = true;
            ordered_options.push((index, rows));
        }
    }
    let mut carrier = crate::combat::Combat::new(ctx, 0);
    carrier.is_over = true;
    write_combat_obs_and_mask(&mut carrier, context, buffers, &mut ActionSlots::new())?;
    let phase = match run.phase {
        MpPhase::Shop => "SHOP",
        MpPhase::Event => "EVENT",
        MpPhase::Neow if neow_cards.is_none() => "EVENT",
        _ => "CARD_REWARD",
    };
    buffers.state_token_named(0, 0, "phase", phase)?;
    buffers.state_token(0, 1, 0);
    buffers.state_bool(0, 1, false);
    for scalar in [2, 3, 4, 5] { buffers.state_numeric_values[scalar] = 0; }
    buffers.state_scalar(1, 2, 1, 30.0, true);
    // Reset the carrier's sole EndTurn candidate, preserving all state rows.
    clear_menu_candidates(buffers);
    let mut option_spans = vec![0..0; option_rows.len()];
    for (index, rows) in ordered_options {
        let start = buffers.state_rows();
        for row in rows { row.append(buffers)?; }
        option_spans[index] = start..buffers.state_rows();
    }
    for action in actions {
        let slot = buffers.push_candidate()?;
        match action {
            MpMenuAction::ChooseNeow(index) => {
                buffers.candidate_token_named(slot, 0, "action_kind", "event_choice")?;
                buffers.candidate_token_named(slot, 1, "event_option_id", ["positive_1", "positive_2", "cursed"][index])?;
                buffers.candidate_token_named(slot, 2, "event_id", "Neow")?;
                for bit in [10, 11, 12, 15] { buffers.candidate_bool(slot, bit, true); }
            }
            MpMenuAction::PickCard(index) | MpMenuAction::ChooseNeowCard(Some(index)) => {
                let card = if let Some(cards) = neow_cards { &cards[index] } else { &run.rewards.as_ref().unwrap()[seat].cards[index] };
                buffers.candidate_token_named(slot, 0, "action_kind", "pick_card")?;
                buffers.candidate_token_named(slot, 1, "card_id", card.id.name())?;
            }
            MpMenuAction::PickSpecialCard(index) => {
                let card=run.rewards.as_ref().unwrap()[seat].special_cards[index].as_ref().unwrap();
                buffers.candidate_token_named(slot,0,"action_kind","pick_special_card")?;
                buffers.candidate_token_named(slot,1,"card_id",card.id.name())?;
            }
            MpMenuAction::ChooseNeowCard(None) => buffers.candidate_token_named(slot, 0, "action_kind", "skip")?,
            MpMenuAction::PickPotion => {
                buffers.candidate_token_named(slot, 0, "action_kind", "pick_potion")?;
                buffers.candidate_token_named(slot, 1, "potion_id", run.rewards.as_ref().unwrap()[seat].potion.unwrap().name())?;
            }
            MpMenuAction::TakeGoldSkipRemaining => buffers.candidate_token_named(slot, 0, "action_kind", "skip_rewards")?,
            MpMenuAction::CheeseGorge => {
                buffers.candidate_token_named(slot, 0, "action_kind", "event_choice")?;
                buffers.candidate_token_named(slot, 1, "event_option_id", "gorge")?;
                buffers.candidate_token_named(slot, 2, "event_id", "RoomFullOfCheese")?;
            }
            MpMenuAction::LeaveShop => buffers.candidate_token_named(slot, 0, "action_kind", "leave_shop")?,
            MpMenuAction::CancelMerchantRemoval => unreachable!("only removal view emits cancel"),
            MpMenuAction::BeginMerchantRemoval | MpMenuAction::MerchantPurchase(_) => {
                use crate::run::mp::MerchantAction as M;
                let offers = run.merchant_offers(player.net_id, seat).map_err(ObsError::UnsupportedState)?;
                let offer = offers.iter().find(|o| match action {
                    MpMenuAction::BeginMerchantRemoval => matches!(o.action, M::RemoveCard(_)),
                    MpMenuAction::MerchantPurchase(a) => o.action == a,
                    _ => false,
                }).ok_or(ObsError::InvalidActionSlot)?;
                let kind = match offer.action { M::BuyCard(_) => "buy_card", M::BuyRelic(_) => "buy_relic",
                    M::BuyPotion(_) => "buy_potion", M::RemoveCard(_) => "remove_card" };
                buffers.candidate_token_named(slot, 0, "action_kind", kind)?;
                if let Some(card) = &offer.card {
                    if !matches!(offer.action, M::RemoveCard(_)) { buffers.candidate_token_named(slot, 1, "card_id", card.id.name())?; }
                }
                if let Some(id) = offer.relic { buffers.candidate_token_named(slot, 1, "relic_id", id.name())?; }
                if let Some(id) = offer.potion { buffers.candidate_token_named(slot, 1, "potion_id", id.name())?; }
                buffers.candidate_scalar(slot, 4, offer.price, 500.0, true);
            }
            MpMenuAction::ChooseCheeseCard(index) => {
                buffers.candidate_token_named(slot, 0, "action_kind", "choose")?;
                buffers.candidate_token_named(slot, 1, "card_id", cheese.unwrap()[index].id.name())?;
                buffers.set_source_applicable(slot, true, true);
                for row in option_spans[index].clone() { buffers.source_link(slot, row); }
            }
            MpMenuAction::ConfirmCheese(_) => {
                buffers.candidate_token_named(slot, 0, "action_kind", "confirm_choice")?;
                buffers.candidate_token_named(slot, 1, "action_kind", "confirm_choice")?;
            }
        }
        let is_selected = matches!(action, MpMenuAction::ChooseCheeseCard(i) if selected.contains(&i));
        let purchase_enabled = match action {
            MpMenuAction::BeginMerchantRemoval => run.merchant_offers(player.net_id, seat).map_err(ObsError::UnsupportedState)?
                .iter().any(|o| matches!(o.action, crate::run::mp::MerchantAction::RemoveCard(_)) && o.enabled),
            MpMenuAction::MerchantPurchase(a) => run.merchant_offers(player.net_id, seat).map_err(ObsError::UnsupportedState)?
                .iter().any(|o| o.action == a && o.enabled),
            _ => true,
        };
        let eligible = purchase_enabled && !matches!(action, MpMenuAction::ChooseCheeseCard(_) if selected.len() == 2) && !is_selected;
        if matches!(action, MpMenuAction::ChooseCheeseCard(_) | MpMenuAction::ConfirmCheese(_)) {
            for (field, value) in [(5, selected.len()), (6, 2), (7, 2)] {
                buffers.candidate_scalar(slot, field, value as i32, 5.0, true);
            }
            buffers.candidate_bool(slot, 8, is_selected);
            buffers.candidate_bool(slot, 9, selected.len() == 2);
            buffers.candidate_bool(slot, 11, true);
        }
        buffers.legal_mask[slot] = eligible;
        buffers.set_model_eligible(slot, eligible);
        bindings.push(MpMenuBinding { actor, net_id: player.net_id, action });
    }
    Ok(())
}

fn clear_menu_candidates(b: &mut ObsBuffers<'_>) {
    b.candidate_kind_ids.fill(tokens::kind_id("candidate").unwrap());
    b.candidate_token_ids.fill(0); b.candidate_numeric_values.fill(0);
    b.candidate_numeric_scales.fill(0.0); b.candidate_numeric_tail.fill(0.0);
    b.choice_purpose_ids.fill(0); b.model_mask.fill(0); b.legal_mask.fill(false);
    b.source_applicable.fill(0); b.target_applicable.fill(0);
    b.source_known.fill(1); b.target_known.fill(1);
    b.source_links_bits.fill(0); b.target_links_bits.fill(0); b.candidate_rows = 0;
}

/// Consumer-local removal screen. BeginMerchantRemoval opens this view without
/// spending gold. Choose dispatches the retained card instance ID atomically;
/// cancellation closes the local view, leaving driver and gold unchanged.
pub fn write_mp_merchant_removal(
    run: &MpRun, actor: CId, context: &CombatObservationContext<'_>,
    buffers: &mut ObsBuffers<'_>, bindings: &mut Vec<MpMenuBinding>,
) -> Result<(), ObsError> {
    let player = run.roster.player(actor as usize).ok_or(ObsError::InvalidActionSlot)?;
    let offers: Vec<_> = run.merchant_offers(player.net_id, actor as usize).map_err(ObsError::UnsupportedState)?
        .into_iter().filter(|o| matches!(o.action, crate::run::mp::MerchantAction::RemoveCard(_))).collect();
    if offers.is_empty() { return Err(ObsError::UnsupportedState("merchant removal unavailable")); }
    write_mp_menu_perspective(run, actor, context, buffers, bindings)?;
    clear_menu_candidates(buffers);
    bindings.clear();
    for offer in offers {
        let card = offer.card.as_ref().unwrap();
        let slot = buffers.push_candidate()?;
        buffers.candidate_token_named(slot, 0, "action_kind", "choose")?;
        buffers.candidate_token_named(slot, 1, "card_id", card.id.name())?;
        // This adapter does not export persistent instance identities as state
        // rows. Keep that source route explicitly unknown, never guess a row
        // from a card ID shared by several distinct deck instances.
        buffers.set_source_applicable(slot, true, false);
        for (field, value) in [(5, 0), (6, 1), (7, 1)] { buffers.candidate_scalar(slot, field, value, 5.0, true); }
        buffers.candidate_bool(slot, 10, true);
        buffers.set_choice_purpose(slot, "remove")?;
        buffers.legal_mask[slot] = offer.enabled;
        buffers.set_model_eligible(slot, offer.enabled);
        bindings.push(MpMenuBinding { actor, net_id: player.net_id, action: MpMenuAction::MerchantPurchase(offer.action) });
    }
    let slot = buffers.push_candidate()?;
    buffers.candidate_token_named(slot, 0, "action_kind", "confirm_choice")?;
    buffers.candidate_token_named(slot, 1, "action_kind", "confirm_choice")?;
    for (field, value) in [(5, 0), (6, 1), (7, 1)] { buffers.candidate_scalar(slot, field, value, 5.0, true); }
    buffers.candidate_bool(slot, 10, true);
    buffers.legal_mask[slot] = true; buffers.set_model_eligible(slot, true);
    bindings.push(MpMenuBinding { actor, net_id: player.net_id, action: MpMenuAction::CancelMerchantRemoval });
    Ok(())
}

#[derive(PartialEq)]
struct SavedRow {
    kind: u16, tokens: Vec<u16>, values: Vec<i32>, scales: Vec<f32>, tail: Vec<f32>,
}

impl SavedRow {
    fn capture(b: &ObsBuffers<'_>, row: usize) -> Self {
        Self { kind: b.state_kind_ids[row],
            tokens: b.state_token_ids[row * TOKEN_SLOTS..(row + 1) * TOKEN_SLOTS].to_vec(),
            values: b.state_numeric_values[row * SCALAR_SLOTS..(row + 1) * SCALAR_SLOTS].to_vec(),
            scales: b.state_numeric_scales[row * SCALAR_SLOTS..(row + 1) * SCALAR_SLOTS].to_vec(),
            tail: b.state_numeric_tail[row * BOOLEAN_FEATURES..(row + 1) * BOOLEAN_FEATURES].to_vec() }
    }
    fn append(mut self, b: &mut ObsBuffers<'_>) -> Result<(), ObsError> {
        if b.state_rows == STATE_CAPACITY { return Err(ObsError::TooManyStateRows); }
        let row = b.state_rows;
        b.state_rows += 1;
        b.state_mask[row] = 1; b.state_kind_ids[row] = self.kind;
        self.tokens[if self.kind == tokens::kind_id("card").unwrap() { 3 } else { 1 }] = 0;
        b.state_token_ids[row * TOKEN_SLOTS..(row + 1) * TOKEN_SLOTS].copy_from_slice(&self.tokens);
        b.state_numeric_values[row * SCALAR_SLOTS..(row + 1) * SCALAR_SLOTS].copy_from_slice(&self.values);
        b.state_numeric_scales[row * SCALAR_SLOTS..(row + 1) * SCALAR_SLOTS].copy_from_slice(&self.scales);
        b.state_numeric_tail[row * BOOLEAN_FEATURES..(row + 1) * BOOLEAN_FEATURES].copy_from_slice(&self.tail);
        Ok(())
    }
}
