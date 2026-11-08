use super::*;
use sts2_sim::run::{Coord, RunAction, RunActionKind, mp::{MerchantAction, MpPhase}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchError {
    Encode(EncodeError),
    Contract(ContractError),
    Gameplay(&'static str),
    GenerationOverflow,
    NoMutation,
}
impl From<EncodeError> for DispatchError { fn from(value: EncodeError) -> Self { Self::Encode(value) } }
impl From<ContractError> for DispatchError { fn from(value: ContractError) -> Self { Self::Contract(value) } }
type DispatchResult<T> = core::result::Result<T, DispatchError>;

impl NativeEncoder {
    /// Read-only typed command resolution shared with checked solo dispatch.
    pub fn solo_run_action(run: &RunManager, obs: &NativeObsV1, slot: usize)
        -> DispatchResult<RunAction> {
        let binding = obs.binding(slot, obs.view()?.decision, 1)?;
        let Command::Solo(command) = binding.command else {
            return Err(ContractError::InvalidBinding.into());
        };
        solo_action(run, command, binding)
    }

    /// Validate/count current rows, edges and native commands without writing
    /// observation columns, before any engine mutation or generation advance.
    /// A dispatch attempt expires all old bindings, including on a gameplay refusal.
    pub fn dispatch_solo(&mut self, run: &mut RunManager, decision: &mut DecisionId,
        obs: &mut NativeObsV1, candidate: usize) -> DispatchResult<()> {
        let binding = obs.binding(candidate, *decision, 1)?;
        obs.begin_binding_check(candidate, binding);
        let visited = self.solo_decision(run, *decision, obs);
        let checked = obs.end_binding_check();
        visited?; checked?;
        let Command::Solo(command) = binding.command else { return Err(ContractError::InvalidBinding.into()); };
        let action = solo_action(run, command, binding)?;
        decision.generation = decision.generation.checked_add(1).ok_or(DispatchError::GenerationOverflow)?;
        obs.clear(*decision);
        run.take_action(action).map_err(DispatchError::Gameplay)
    }

    pub fn dispatch_multiplayer(&mut self, run: &mut MpRun, decision: &mut DecisionId, net_id: u64,
        obs: &mut NativeObsV1, candidate: usize) -> DispatchResult<()> {
        self.dispatch_multiplayer_mode(run,decision,net_id,obs,candidate,true)
    }
    /// Restore journals written before native fixed-count UI auto-confirmation.
    pub fn dispatch_multiplayer_journal(&mut self, run: &mut MpRun, decision: &mut DecisionId, net_id: u64,
        obs: &mut NativeObsV1, candidate: usize) -> DispatchResult<()> {
        self.dispatch_multiplayer_mode(run,decision,net_id,obs,candidate,false)
    }
    fn dispatch_multiplayer_mode(&mut self, run: &mut MpRun, decision: &mut DecisionId, net_id: u64,
        obs: &mut NativeObsV1, candidate: usize, auto_confirm: bool) -> DispatchResult<()> {
        let binding = obs.binding(candidate, *decision, net_id)?;
        obs.begin_binding_check(candidate, binding);
        let visited = self.multiplayer_decision(run, *decision, net_id, obs);
        let checked = obs.end_binding_check();
        visited?; checked?;
        let seat = usize::from(decision.actor);
        let mut indices = [0usize; SELECTION_CAPACITY];
        let mut instances = [0u32; SELECTION_CAPACITY];
        let mut count = 0;
        if let Some(span) = binding.command.selection() {
            let selected = obs.selection(span)?; count = selected.len();
            for (index, item) in selected.iter().enumerate() {
                indices[index] = item.option_index as usize;
                instances[index] = item.instance_id.ok_or(ContractError::InvalidBinding)?;
            }
        }
        let next = decision.generation.checked_add(1).ok_or(DispatchError::GenerationOverflow)?;
        decision.generation = next; obs.clear(*decision);
        use Command::*;
        let result = match binding.command {
            Invalid | Solo(_) => return Err(ContractError::InvalidBinding.into()),
            ToggleSelection { index } => {
                self.ui[seat].toggle(index, binding.source_instance)?;
                let ui = &self.ui[seat];
                if auto_confirm && ui.auto_confirm && ui.selected_len == ui.max {
                    let indices: Vec<usize> = ui.selected[..ui.selected_len].iter()
                        .map(|item| item.option_index as usize).collect();
                    run.select_event_cards(net_id, seat, &indices)
                } else { return Ok(()); }
            },
            MpBeginRemoval => { self.ui[seat].removal = true; return Ok(()); },
            MpCancelRemoval => { self.ui[seat].removal = false; return Ok(()); },
            MpChooseNeow { option } => run.choose_neow(net_id, seat, option as usize),
            MpSelectNeowCard { index } => run.select_neow_card(net_id, seat, index.map(|i| i as usize)),
            MpSelectNeowPotion { accept } => run.select_neow_potion_reward(net_id, seat, accept),
            MpMove { x, y } => run.vote(net_id, seat, Coord::new(x, y)),
            MpPlayCard { hand, target } => run.play_card(net_id, seat, hand as usize, target.map(usize::from)),
            MpUsePotion { slot, target } => run.use_potion(net_id, seat, slot as usize, target.map(usize::from)),
            MpDiscardPotion { slot } => run.discard_potion(net_id, seat, slot as usize),
            MpEndTurn => run.end_turn_for(net_id, seat),
            MpReadyForEnemyTurn => run.ready_to_begin_enemy_turn_for(net_id, seat),
            MpResolveCombatChoice { index } => {
                run.resolve_combat_choice(net_id, seat, index.map(|i| i as usize)).map_err(DispatchError::Gameplay)?;
                let confirm = index.is_some() && run.combat().is_some_and(|c| {
                    c.pending_choice_for_player(seat as u8).is_some_and(|choice|
                        !choice.single && choice.min == choice.max &&
                        c.choice_selected_for_player(seat as u8).is_some_and(|selected| selected.len() == choice.max as usize))
                });
                if auto_confirm && confirm { run.resolve_combat_choice(net_id, seat, None) } else { Ok(()) }
            },
            MpTakeCard { index } => run.take_card_reward(net_id, seat, index as usize),
            MpTakeSpecialCard { reward_index } => run.take_special_card_reward(net_id, seat, reward_index as usize),
            MpTakePotion => changed(run.take_potion_reward(net_id, seat)),
            MpTakeExtraPotion { index } => changed(run.take_extra_potion_reward(net_id, seat, index as usize)),
            MpTakeExtraRelic { index } => run.take_extra_relic_reward(net_id, seat, index as usize),
            MpTakeGoldSkipRemaining => run.take_gold_skip_remaining_rewards(net_id, seat),
            MpMerchant(command) => {
                let action = match command {
                    MerchantCommand::BuyCard { index } => MerchantAction::BuyCard(index as usize),
                    MerchantCommand::BuyRelic { index } => MerchantAction::BuyRelic(index as usize),
                    MerchantCommand::BuyPotion { index } => MerchantAction::BuyPotion(index as usize),
                    MerchantCommand::RemoveCard { instance } => MerchantAction::RemoveCard(instance),
                };
                let result = changed(run.merchant_purchase(net_id, seat, action));
                if result.is_ok() { self.ui[seat].removal = false; } result
            }
            MpLeaveShop => run.leave_shop(net_id, seat),
            MpMerchantPickup { index } => match index {
                Some(index) => run.select_merchant_pickup(net_id, seat, &[index as usize]),
                None => run.select_merchant_pickup(net_id, seat, &[]),
            },
            MpMerchantPickupCards { .. } => run.select_merchant_pickup(net_id, seat, &indices[..count]),
            MpEventOption { option } | MpAbandonConfirmation { option, .. }
                | MpSphereReward { option, .. } => run.choose_event_option(net_id, seat, option as usize),
            MpEventCards { .. } => run.select_event_cards(net_id, seat, &indices[..count]),
            MpEventPotion { index } => changed(run.take_event_potion_reward(net_id, seat, index as usize)),
            MpFinishEventPotions => run.finish_event_potion_rewards(net_id, seat),
            MpEventRelic { accept } => run.select_event_relic_reward(net_id, seat, accept),
            MpWhisperingReward { index } => changed(run.take_whispering_hollow_reward(net_id, seat, index as usize)),
            MpFinishWhisperingOption => run.finish_whispering_hollow_option(net_id, seat),
            MpBridgeOption { option } => run.choose_bridge_option(net_id, seat, option as usize),
            MpCheeseOption { option } => run.choose_cheese(net_id, seat, option as usize),
            MpCheeseCards { indices } => run.select_cheese_cards(net_id, seat, indices.map(|i| i as usize)),
            MpScrollBoxBundle { index } => run.select_scroll_box_bundle(net_id, seat, index as usize),
            MpPomanderCard { instance } => run.select_pomander_card(net_id, seat, instance),
            MpPreciseScissorsCard { instance } => run.select_precise_scissors_card(net_id, seat, instance),
            MpNewLeafCard { instance } => run.select_new_leaf_card(net_id, seat, instance),
            MpPaelsGrowthCard { instance } => run.select_paels_growth_card(net_id, seat, instance),
            MpPrecariousShearsCards { .. } => run.select_precarious_shears_cards(net_id, seat, &instances[..count]),
            MpRelicReward { accept } => run.select_relic_reward(net_id, seat, accept),
            MpRestOption { index } => run.choose_rest_option(net_id, seat, index as usize),
            MpRestSmith { instance } => run.select_rest_smith(net_id, seat, instance),
            MpRestMend { target } => run.select_rest_mend(net_id, seat, target.map(usize::from)),
            MpRestCardReward { index } => run.select_rest_card_reward(net_id, seat, index.map(|i| i as usize)),
            MpRestPotionReward { accept } => run.select_rest_potion_reward(net_id, seat, accept),
            MpSkipRest => run.skip_rest_remaining(net_id, seat),
            MpOpenTreasure => run.open_treasure(net_id, seat),
            MpVoteTreasure { index } => run.vote_treasure(net_id, seat, index.map(|i| i as usize)),
        };
        if result.is_ok() && matches!(binding.command, MpEventCards { .. } | MpCheeseCards { .. }
            | MpPrecariousShearsCards { .. } | MpMerchantPickupCards { .. }) {
            self.ui[seat].selected_len = 0; self.ui[seat].kind = multiplayer::SelectionKind::None;
        }
        result.map_err(DispatchError::Gameplay)
    }

    /// Automatic death readiness and retained reward generation belong to the
    /// caller's driver, never encoding or the policy's action distribution.
    pub fn advance_multiplayer(run: &mut MpRun, decision: &mut DecisionId) -> DispatchResult<bool> {
        let mut advanced = false;
        for seat in 0..run.roster.players().len() {
            // Original Commands/CreatureCmd.cs:353-355 automatically calls
            // PlayerCmd.EndTurn(..., canBackOut: false) when a player dies.
            let needs_death_ready = run.phase == MpPhase::Combat && run.combat().is_some_and(|c| {
                let owner = seat as u8;
                c.side == Side::Player && !c.is_over && !c.is_ending && c.fault.is_none()
                    && c.pending_choice.is_none() && c.cr(owner).is_dead()
                    && !c.is_player_ready_to_end_turn(owner)
            });
            if !needs_death_ready { continue; }
            let net_id = run.roster.players()[seat].net_id;
            decision.generation = decision.generation.checked_add(1).ok_or(DispatchError::GenerationOverflow)?;
            run.end_turn_for(net_id, seat).map_err(DispatchError::Gameplay)?;
            advanced = true;
        }
        if run.phase == MpPhase::Rewards && run.rewards.is_none() {
            decision.generation = decision.generation.checked_add(1).ok_or(DispatchError::GenerationOverflow)?;
            run.offer_rewards().map_err(DispatchError::Gameplay)?;
            advanced = true;
        }
        // The original RunManager.EnterNextAct runs after the boss reward
        // barrier. A terminal boss has no map children to expose as decisions.
        if run.phase == MpPhase::Map && run.rewards.is_none() && !run.native_map_blocked()
            && run.world.run.coord.and_then(|(x,y)|run.world.map.point(Coord::new(x,y)))
                .is_some_and(|point|point.point_type==sts2_sim::tables::MapPointType::Boss
                    && point.children.is_empty()) {
            decision.generation = decision.generation.checked_add(1).ok_or(DispatchError::GenerationOverflow)?;
            run.advance_act().map_err(DispatchError::Gameplay)?;
            advanced = true;
        }
        Ok(advanced)
    }
}

fn changed(result: core::result::Result<bool, &'static str>) -> core::result::Result<(), &'static str> {
    match result { Ok(true) => Ok(()), Ok(false) => Err("legal native command made no transition"), Err(error) => Err(error) }
}
fn solo_action(run: &RunManager, command: SoloCommand, binding: Binding) -> DispatchResult<RunAction> {
    use SoloCommand as C;
    use RunActionKind as K;
    let kind = match command {
        C::EventChoice { .. } | C::EventChoiceAt { .. } | C::AbandonConfirmation { .. } => return run.native_event_action(binding.offer_instance.ok_or(ContractError::InvalidBinding)? as usize)
            .ok_or(ContractError::InvalidBinding.into()),
        C::RestOption { index } => return run.native_rest_action(index as usize).ok_or(ContractError::InvalidBinding.into()),
        C::Move { .. } => K::Move, C::LeaveShop => K::LeaveShop,
        C::BuyCard { .. } => K::BuyCard, C::BuyRelic { .. } => K::BuyRelic, C::BuyPotion { .. } => K::BuyPotion,
        C::RemoveCard => K::RemoveCard, C::EndTurn => K::EndTurn, C::PlayCard { .. } => K::PlayCard,
        C::UsePotion { .. } => K::UsePotion, C::DiscardPotion { .. } => K::DiscardPotion,
        C::Choose { .. } => K::Choose, C::ConfirmChoice => K::ConfirmChoice, C::Skip => K::Skip,
        C::PickCard { .. } => K::PickCard, C::PickSpecialCard { .. } => K::PickSpecialCard,
        C::RerollCardReward => K::RerollCardReward, C::SacrificeCardReward => K::SacrificeCardReward,
        C::PickCardBundle { .. } => K::PickCardBundle, C::SkipRewards => K::SkipRewards,
        C::SkipPotion => K::SkipPotion, C::PickPotion { .. } => K::PickPotion, C::SkipRelic => K::SkipRelic,
        C::PickRelicReward { .. } => K::PickRelicReward, C::OpenReward { .. } => K::OpenReward, C::Collect => K::Collect,
    };
    let mut action = RunAction::new(kind);
    match command {
        C::Move { x, y } => action.coord = Some(Coord::new(x, y)),
        C::BuyCard { index } | C::BuyRelic { index } | C::BuyPotion { index } | C::Choose { index }
            | C::PickCard { index } | C::PickCardBundle { index } => action.index = Some(index as usize),
        C::PlayCard { hand, target } => { action.hand_index = Some(hand as usize); action.target_index = target.map(usize::from); }
        C::UsePotion { slot, target } => {
            action.slot_index = Some(slot as usize); action.potion_id = run.run_ctx().potions.get(slot as usize).copied().flatten();
            match target {
                PotionRecipient::None => {},
                PotionRecipient::Creature(target) => action.target_index = Some(usize::from(target)),
                PotionRecipient::Player(target) => { action.target_index = Some(usize::from(target)); action.target_player_id = Some(i32::from(target) + 1); },
            }
        }
        C::DiscardPotion { slot } => { action.slot_index = Some(slot as usize); action.potion_id = run.run_ctx().potions.get(slot as usize).copied().flatten(); }
        C::PickSpecialCard { reward_index } | C::OpenReward { reward_index } => action.reward_index = Some(reward_index as usize),
        C::PickPotion { reward_index } | C::PickRelicReward { reward_index } => action.reward_index = reward_index.map(|i| i as usize),
        _ => {},
    }
    Ok(action)
}
