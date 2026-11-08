use super::*;
use sts2_sim::run::{EventDescription, EventOptionIdentity, EventPageIdentity, EventParameter,
    NativeRewardContent, NativeShopKind, RestOptionKind, RunPhase};

impl NativeEncoder {
    /// Read-only solo decision. The solo driver's authenticated network identity is 1.
    pub fn encode_solo(&mut self, run: &RunManager, decision: DecisionId, obs: &mut NativeObsV1) -> Result<()> {
        self.solo_decision(run, decision, obs)?;
        if let Some(encoder) = &mut self.legacy
            && !run.native_event_page().is_some_and(modal::is_modal_page) {
            let mut old = obs.legacy.take().unwrap_or_default();
            let result = encoder.solo_decision(run, decision, &mut old);
            Self::publish_legacy(obs, old, result)?;
        }
        Ok(())
    }
    pub(super) fn solo_decision(&mut self, run: &RunManager, decision: DecisionId, obs: &mut NativeObsV1) -> Result<()> {
        self.begin(obs, decision, 1)?;
        if decision.actor != 0 { return Err(ContractError::WrongOwner.into()); }
        let ctx = run.run_ctx(); let phase = run.phase(); let character = run.native_character();
        let combat = (phase == RunPhase::Combat).then(|| run.combat()).flatten();
        let global = self.global(obs, ctx, id(Namespace::RunPhase, phase as usize), 1, combat, run.native_public_boss())?;
        if let Some(c) = combat {
            let status = self.combat(obs, c, global, false, |_| character,
                |slot| c.can_discard_potion_for_player(0, slot))?;
            if let Some(map) = run.visible_map() {
                self.map(obs, map, ctx.act_index, ctx.coord, |_| false, |index| run.native_map_visited(index))?;
                if !self.legacy_mode { self.bosses(obs, global, ctx.act_index, run.native_public_bosses())?; }
            }
            self.finish(obs, status)?; return Ok(());
        }
        let mut seat = Row { kind: RowKind::Seat, detail: [id(Namespace::Character, character as usize),
            ContentId::ABSENT, ContentId::ABSENT, ContentId::ABSENT], parent: global, ..Row::default() };
        ints(&mut seat.numbers, &[(0, 0), (1, 0), (2, ctx.hp), (3, ctx.max_hp), (6, ctx.max_energy),
            (8, ctx.gold), (9, ctx.max_potion_slots)])?;
        seat.flags.set(Flag::Alive, ctx.hp > 0); seat.flags.set(Flag::Participating, true);
        self.seat_rows[0] = Self::row(obs, seat)?;
        self.inventory(obs, 0, &ctx.deck, &ctx.relics, &ctx.potions, ctx.max_potion_slots.max(0) as usize, None)?;
        if let Some(map) = run.visible_map() {
            self.map(obs, map, ctx.act_index, ctx.coord, |index| run.native_map_reachable(index), |index| run.native_map_visited(index))?;
            if !self.legacy_mode { self.bosses(obs, global, ctx.act_index, run.native_public_bosses())?; }
        }
        if run.native_fault().is_some() { self.finish(obs, DecisionStatus::GameplayFault)?; return Ok(()); }
        if phase == RunPhase::RunOver { self.finish(obs, DecisionStatus::Terminal)?; return Ok(()); }
        let choice = run.native_choice(); let active = choice.is_none();
        match phase {
            RunPhase::Event => self.solo_event(run, obs, active)?,
            RunPhase::MapChoice if active => {
                if let Some(map) = run.visible_map() {
                    for (index, point) in map.points.iter().enumerate() {
                        if !run.native_map_reachable(index) { continue; }
                        let mut candidate = Candidate { source: self.map_rows[index], target: self.map_rows[index], legal: true, ..Candidate::default() };
                        ints(&mut candidate.numbers, &[(8, point.coord.col), (9, point.coord.row)])?;
                        self.action(obs, candidate, Command::Solo(SoloCommand::Move { x: point.coord.col, y: point.coord.row }), None, None, None)?;
                    }
                }
            }
            RunPhase::CardReward => self.solo_rewards(run, obs, active)?,
            RunPhase::Shop => {
                for offer in run.native_shop_offers() { self.shop_offer(obs, offer, active, false)?; }
                if active { self.simple(obs, Command::Solo(SoloCommand::LeaveShop), self.seat_rows[0], true)?; }
            }
            RunPhase::RestSite => {
                for offer in run.native_rest_offers() {
                    let mut row = Row { kind: RowKind::RestService, detail: [id(Namespace::RestOptionKind, offer.kind as usize),
                        ContentId::ABSENT, ContentId::ABSENT, ContentId::ABSENT], owner: self.seat_rows[0], ..Row::default() };
                    ints(&mut row.numbers, &[(3, as_i32(offer.index)?)])?; row.flags.set(Flag::Locked, !offer.enabled);
                    if offer.kind == RestOptionKind::Heal {
                        let heal = if self.legacy_mode { run.native_rest_base_heal_amount() } else { run.native_rest_heal_amount() };
                        ints(&mut row.numbers, &[(0, heal)])?;
                    }
                    let handle = Self::row(obs, row)?;
                    if !self.legacy_mode {
                        self.rest_parameters(obs, handle, self.seat_rows[0], offer.kind, run.native_rest_heal_amount(), &ctx.deck, &ctx.relics)?;
                    }
                    if offer.kind == RestOptionKind::Smith && !self.legacy_mode {
                        for card in &ctx.deck {
                            self.upgrade_preview(obs, ctx, card, self.card_handle(deck_key(0, card.instance_id)),
                                handle, ChoicePurpose::RunDeckChoiceKindSmith)?;
                        }
                    }
                    if active { self.action(obs, Candidate { source: handle,
                        target: if self.legacy_mode { RowHandle::ABSENT } else { self.seat_rows[0] }, legal: offer.enabled, ..Candidate::default() },
                        Command::Solo(SoloCommand::RestOption { index: offer.index as u32 }), None, None, None)?; }
                }
            }
            RunPhase::Treasure => {
                let relic = run.native_treasure_relic();
                let row = Row { kind: RowKind::Treasure, model: relic.map_or(ContentId::ABSENT, |id_| id(Namespace::RelicId, id_.index())),
                    owner: self.seat_rows[0], ..Row::default() };
                let handle = Self::row(obs, row)?;
                if active { self.simple(obs, Command::Solo(SoloCommand::Collect), handle, true)?; }
            }
            RunPhase::Combat => return Err(EncodeError::InvalidState("combat phase without combat")),
            RunPhase::RunOver | RunPhase::MapChoice => {},
        }
        if let Some(choice) = choice {
            let (min, max, optional, single) = choice.bounds();
            let mut row = Row { kind: RowKind::Choice, detail: [choice.purpose().into(), choice.effect().map_or(ContentId::ABSENT, Into::into),
                ContentId::ABSENT, ContentId::ABSENT], owner: self.seat_rows[0], ..Row::default() };
            ints(&mut row.numbers, &[(1, as_i32(min)?), (2, as_i32(max)?), (3, as_i32(choice.selected_count())?), (4, 0)])?;
            for (flag, value) in [(Flag::Optional, optional), (Flag::SingleSelect, single), (Flag::Ordered, true),
                (Flag::CanConfirm, choice.can_confirm())] { row.flags.set(flag, value); }
            let parent = Self::row(obs, row)?;
            if let Some((enchantment, amount)) = choice.enchantment() {
                self.modifier(obs, self.seat_rows[0], parent, enchantment.into(), ContentId::ABSENT, amount)?;
            }
            for index in 0..choice.len() {
                let option = choice.card(index).ok_or(EncodeError::InvalidState("run choice instance missing"))?;
                let source = if option.offered {
                    self.card(obs, option.card, self.seat_rows[0], parent, PileKind::None, None, None)?
                } else { self.card_handle(deck_key(0, option.card.instance_id)) };
                if source == RowHandle::ABSENT { return Err(ContractError::InvalidHandle.into()); }
                let mut row = Row { kind: RowKind::ChoiceOption, model: id(Namespace::CardId, option.card.id.index()), parent,
                    owner: self.seat_rows[0], ..Row::default() };
                ints(&mut row.numbers, &[(0, as_i32(index)?)])?; row.flags.set(Flag::Selected, option.selected_order.is_some());
                if let Some(order) = option.selected_order { ints(&mut row.numbers, &[(1, as_i32(order)?), (2, as_i32(order)?)])?; }
                let target = Self::row(obs, row)?; obs.push_edge(Edge { from: target, to: source, relation: Relation::Source })?;
                if v2::is_upgrade_purpose(choice.purpose()) {
                    self.upgrade_preview(obs, ctx, option.card, source, target, choice.purpose())?;
                }
                let command = if choice.picks_generated_card() { SoloCommand::PickCard { index: index as u32 } }
                    else { SoloCommand::Choose { index: index as u32 } };
                self.action(obs, Candidate { model: row.model, source, target, flags: row.flags, legal: option.enabled, ..Candidate::default() },
                    Command::Solo(command), Some(option.card.instance_id), None, Some(index as u64))?;
            }
            if choice.can_confirm() { self.simple(obs, Command::Solo(SoloCommand::ConfirmChoice), parent, true)?; }
        }
        self.solo_inventory_actions(run, obs)?;
        self.finish(obs, DecisionStatus::Actionable)?; Ok(())
    }

    fn solo_inventory_actions(&self, run: &RunManager, obs: &mut NativeObsV1) -> Result<()> {
        if run.native_potions_locked() { return Ok(()); }
        for (slot, potion) in run.run_ctx().potions.iter().copied().enumerate() {
            let Some(potion) = potion else { continue; }; let info = &POTION_INFO[potion.index()];
            let candidate = Candidate { model: id(Namespace::PotionId, potion.index()), source: self.seat_rows[0], legal: true, ..Candidate::default() };
            self.action(obs, candidate, Command::Solo(SoloCommand::DiscardPotion { slot: slot as u32 }), None, None, Some(slot as u64))?;
            let foul_available = run.phase() == RunPhase::Shop || run.native_event_id() == Some(EventId::FakeMerchant);
            if info.usage == PotionUsageType::AnyTime && (potion != PotionId::Foulpotion || foul_available) {
                let target = if info.target == PotionTargetType::AnyPlayer { PotionRecipient::Player(0) } else { PotionRecipient::None };
                self.action(obs, candidate, Command::Solo(SoloCommand::UsePotion { slot: slot as u32, target }), None, None, Some(slot as u64))?;
            }
        }
        Ok(())
    }
    fn solo_event(&self, run: &RunManager, obs: &mut NativeObsV1, active: bool) -> Result<()> {
        if let Some(offers) = run.native_neow_offers() {
            let event = offers[0].0.id.event();
            let parent = self.event_page(obs, event, EventPageIdentity { id: event.initial_page(), parameter: EventParameter::None })?;
            for (index, (option, relic)) in offers.into_iter().enumerate() {
                let parameters = if self.legacy_mode { Vec::new() }
                    else { run.native_event_parameters(option, None).map_err(EncodeError::Gameplay)? };
                let source = self.event_option(obs, option, option.id, None, index, parent, true, Some(id(Namespace::RelicId, relic.index())), &parameters)?;
                self.offered_relic(obs, relic, source, self.seat_rows[0])?;
                if active { self.event_action(obs, option, index, source, true)?; }
            }
            return Ok(());
        }
        let event = run.native_event_id().ok_or(EncodeError::InvalidState("event identity missing"))?;
        let page = run.native_event_page().ok_or(EncodeError::InvalidState("event page missing"))?;
        let parent = self.event_page(obs, event, page)?;
        for (index, option) in run.native_event_options().ok_or(EncodeError::InvalidState("event offers missing"))?.iter().enumerate() {
            let model = event_offer_model(run.native_event_offer(option.option_id));
            let display = EventOptionIdentity { id: option.native_id(), ..option.option_id };
            let parameters = if self.legacy_mode { Vec::new() }
                else { run.native_event_parameters(display, option.dynamic_description).map_err(EncodeError::Gameplay)? };
            let source = self.event_option(obs, option.option_id, option.native_id(), option.dynamic_description, index, parent, option.enabled, model, &parameters)?;
            if let Some(sts2_sim::run::NativeEventOffer::Relic(relic)) = run.native_event_offer(option.option_id) {
                self.offered_relic(obs, relic, source, self.seat_rows[0])?;
            }
            if !self.legacy_mode && option.option_id.id == EventOptionId::SlipperyBridgeInitialOvercome {
                if let Some((_, card)) = run.native_bridge_offer() {
                    obs.push_edge(Edge { from: source, to: self.card_handle(deck_key(0, card.instance_id)), relation: Relation::Target })?;
                }
            }
            self.event_offer_detail(obs, source, run.native_event_offer_detail(option.option_id))?;
            if active { self.event_action(obs, option.option_id, index, source, option.enabled)?; }
        }
        Ok(())
    }
    pub(super) fn event_page(&self, obs: &mut NativeObsV1, event: EventId, page: EventPageIdentity) -> Result<RowHandle> {
        if self.legacy_mode && modal::is_modal_page(page) {
            return Err(EncodeError::InvalidState("original popup/reward modal is not an event page"));
        }
        let owner = self.seat_rows[usize::from(self.decision.actor)];
        let parent = Self::row(obs, Row { kind: RowKind::Event, model: event.into(), detail: [event.into(), ContentId::ABSENT,
            ContentId::ABSENT, ContentId::ABSENT], owner, ..Row::default() })?;
        if modal::is_modal_page(page) {
            let kind = if matches!(page.parameter, EventParameter::AbandonRunConfirmation(_)) { 0 } else { 1 };
            return Self::row(obs, Row { kind: RowKind::Modal, model: id(Namespace::PublicModalKind, kind),
                detail: [page.id.into(), ContentId::ABSENT, ContentId::ABSENT, ContentId::ABSENT],
                owner, parent, ..Row::default() });
        }
        let mut row = Row { kind: RowKind::EventPage, model: page.id.into(), detail: [page.id.into(), ContentId::ABSENT,
            ContentId::ABSENT, ContentId::ABSENT], owner, parent, ..Row::default() };
        let (parameter, x, y) = event_parameter(page.parameter);
        if let Some(value) = parameter.or(x) { ints(&mut row.numbers, &[(1, value)])?; }
        if let Some(value) = y { ints(&mut row.numbers, &[(2, value)])?; }
        Self::row(obs, row)
    }
    pub(super) fn event_option(&self, obs: &mut NativeObsV1, option: EventOptionIdentity, visible_id: EventOptionId,
        description: Option<EventDescription>, index: usize, parent: RowHandle, enabled: bool,
        offered: Option<ContentId>, parameters: &[sts2_sim::run::event_preview::PublicEventParameter]) -> Result<RowHandle> {
        if let EventParameter::AbandonRunConfirmation(confirm) = option.parameter {
            return self.abandon_modal_option(obs, parent, index, confirm, enabled);
        }
        let mut row = Row { kind: RowKind::EventOption, model: offered.unwrap_or_else(|| visible_id.into()),
            detail: [visible_id.into(), ContentId::ABSENT, ContentId::ABSENT, ContentId::ABSENT],
            owner: self.seat_rows[usize::from(self.decision.actor)], parent, ..Row::default() };
        ints(&mut row.numbers, &[(0, as_i32(index)?)])?; row.flags.set(Flag::Locked, !enabled);
        let (parameter, x, y) = event_parameter(option.parameter);
        for (lane, value) in [(7, parameter), (8, x), (9, y)] { if let Some(value) = value { ints(&mut row.numbers, &[(lane, value)])?; } }
        if let Some(description) = description {
            use EventDescription as D;
            let effect = match description {
                D::DenseVegetationTrudgeGold(gold) => { ints(&mut row.numbers, &[(1, 8), (4, gold)])?; EffectId::EventEffectGainGold },
                D::DenseVegetationRestHeal(heal) | D::UnrestSiteHeal(heal) | D::SpiralingWhirlpoolHeal(heal) => {
                    ints(&mut row.numbers, &[(4, heal)])?; EffectId::EventEffectHeal },
                D::ColossalFlowerExtractGold(gold) | D::SunkenTreasurySmallGold(gold) | D::SunkenTreasuryLargeGold(gold)
                    | D::LostWispSearchGold(gold) => { ints(&mut row.numbers, &[(4, gold)])?; EffectId::EventEffectGainGold },
                D::ColossalFlowerReachDeeperDamage(damage) | D::ColossalFlowerPollinousCoreDamage(damage) | D::BridgeHoldOn(damage) => {
                    ints(&mut row.numbers, &[(1, damage)])?; EffectId::EventEffectLoseHp },
                D::SunkenStatueDive(gold) => { ints(&mut row.numbers, &[(1, 7), (4, gold)])?; EffectId::EventEffectGainGold },
                D::ThisOrThatPlainGold(gold) => { ints(&mut row.numbers, &[(1, 6), (4, gold)])?; EffectId::EventEffectGainGold },
                D::TabletDecipherCost(cost) => { ints(&mut row.numbers, &[(2, cost)])?; EffectId::EventEffectLoseMaxHp },
                D::FakeMerchantCost(gold) => { ints(&mut row.numbers, &[(3, gold)])?; EffectId::EventEffectLoseGold },
                D::JungleSoloGold(bits) | D::JungleJoinGold(bits) => {
                    // C# JungleMazeAdventure.CalculateVars adds a decimal-cast
                    // variance to 150 or 50. This engine retains the sum in f64.
                    // In its [35, 165] range the fractional part has at most 47
                    // binary digits: two f32 limbs preserve it exactly.
                    let value = f64::from_bits(bits); let whole = value.trunc();
                    let fraction = (value - whole) as f32;
                    let tail = (value - whole - f64::from(fraction)) as f32;
                    if !value.is_finite() || whole < i32::MIN as f64 || whole > i32::MAX as f64
                        || whole + f64::from(fraction) + f64::from(tail) != value {
                        return Err(EncodeError::InvalidState("event fraction needs exact numeric representation"));
                    }
                    row.numbers.set_integer(4, whole as i32)?;
                    row.numbers.set_float(10, fraction)?;
                    row.numbers.set_float(11, tail)?;
                    if matches!(description, D::JungleSoloGold(_)) { ints(&mut row.numbers, &[(1, 18)])?; }
                    EffectId::EventEffectGainGold
                }
            }; row.detail[1] = effect.into();
        }
        let handle = Self::row(obs, row)?;
        self.event_parameters(obs, handle, row.owner, parameters)?;
        Ok(handle)
    }
    pub(super) fn event_action(&self, obs: &mut NativeObsV1, option: EventOptionIdentity, index: usize, source: RowHandle, legal: bool) -> Result<()> {
        if let EventParameter::AbandonRunConfirmation(confirm) = option.parameter {
            return self.action(obs, Candidate { source, target: self.seat_rows[usize::from(self.decision.actor)],
                model: id(Namespace::PublicModalAction, usize::from(!confirm)), legal, ..Candidate::default() },
                Command::Solo(SoloCommand::AbandonConfirmation { confirm }), None, None, Some(index as u64));
        }
        let (parameter, x, y) = event_parameter(option.parameter);
        let mut candidate = Candidate { model: option.id.into(), source, legal, ..Candidate::default() };
        for (lane, value) in [(8, x), (9, y), (10, parameter)] { if let Some(value) = value { ints(&mut candidate.numbers, &[(lane, value)])?; } }
        let command = match (x, y) { (Some(x), Some(y)) => SoloCommand::EventChoiceAt { option: option.id, x, y },
            _ => SoloCommand::EventChoice { option: option.id } };
        self.action(obs, candidate, Command::Solo(command), None, None, Some(index as u64))
    }
    pub(super) fn shop_offer(&self, obs: &mut NativeObsV1, offer: sts2_sim::run::NativeShopOffer<'_>, active: bool, multi: bool) -> Result<()> {
        let owner = self.seat_rows[usize::from(self.decision.actor)];
        let model = offer.card.map(|card| id(Namespace::CardId, card.id.index()))
            .or_else(|| offer.relic.map(|relic| id(Namespace::RelicId, relic.index())))
            .or_else(|| offer.potion.map(|potion| id(Namespace::PotionId, potion.index()))).unwrap_or(ContentId::ABSENT);
        let mut row = Row { kind: RowKind::Offer, model, owner, ..Row::default() };
        ints(&mut row.numbers, &[(1, as_i32(offer.index)?)])?;
        if !offer.sold { ints(&mut row.numbers, &[(0, offer.price)])?; }
        row.flags.set(Flag::Sold, offer.sold); row.flags.set(Flag::OnSale, offer.on_sale);
        let source = Self::row(obs, row)?;
        if let Some(relic) = offer.relic { self.offered_relic(obs, relic, source, owner)?; }
        if !self.legacy_mode && let Some(potion) = offer.potion {
            let handle = self.potion(obs, Some(potion), owner, source, 0, 0)?;
            obs.mark_v2_only(handle);
        }
        let target = if let Some(card) = offer.card {
            let child = self.card(obs, card, owner, source, PileKind::None, None, None)?;
            if !self.legacy_mode { obs.push_edge(Edge { from: source, to: child, relation: Relation::Member })?; }
            if self.legacy_mode { RowHandle::ABSENT } else { child }
        } else { RowHandle::ABSENT };
        if !active { return Ok(()); }
        let command = if multi {
            Command::MpMerchant(match offer.kind {
                NativeShopKind::Card => MerchantCommand::BuyCard { index: offer.index as u32 },
                NativeShopKind::Relic => MerchantCommand::BuyRelic { index: offer.index as u32 },
                NativeShopKind::Potion => MerchantCommand::BuyPotion { index: offer.index as u32 },
                NativeShopKind::Removal => MerchantCommand::RemoveCard { instance: offer.card.map(|card| card.instance_id) },
            })
        } else { Command::Solo(match offer.kind {
            NativeShopKind::Card => SoloCommand::BuyCard { index: offer.index as u32 },
            NativeShopKind::Relic => SoloCommand::BuyRelic { index: offer.index as u32 },
            NativeShopKind::Potion => SoloCommand::BuyPotion { index: offer.index as u32 },
            NativeShopKind::Removal => SoloCommand::RemoveCard,
        }) };
        let mut candidate = Candidate { model, source, target, legal: offer.enabled, ..Candidate::default() };
        if !offer.sold { ints(&mut candidate.numbers, &[(2, offer.price)])?; }
        self.action(obs, candidate, command, offer.card.map(|card| card.instance_id), None, Some(offer.index as u64))
    }
    fn solo_rewards(&self, run: &RunManager, obs: &mut NativeObsV1, active: bool) -> Result<()> {
        if run.native_awaiting_next_act() {
            if active { self.simple(obs, Command::Solo(SoloCommand::Skip), self.seat_rows[0], true)?; }
            return Ok(());
        }
        let modal_card = run.native_reward_indices().any(|i| run.native_reward(i).is_some_and(|r|
            r.active && matches!(r.content, NativeRewardContent::Cards { closed: false, .. } | NativeRewardContent::Bundles(_))));
        let mut can_skip_set = false;
        for (display, index) in run.native_reward_indices().enumerate() {
            let reward = run.native_reward(index).ok_or(EncodeError::InvalidState("reward vanished"))?;
            can_skip_set |= reward.skippable;
            let mut row = Row { kind: RowKind::Reward, detail: [reward.kind.into(), ContentId::ABSENT, ContentId::ABSENT, ContentId::ABSENT],
                owner: self.seat_rows[0], ..Row::default() };
            ints(&mut row.numbers, &[(1, as_i32(display)?)])?;
            for (flag, value) in [(Flag::Selected, reward.active), (Flag::Skippable, reward.skippable),
                (Flag::SkipCompletes, reward.skip_completes)] { row.flags.set(flag, value); }
            let mut reroll_allowed = false; let mut sacrifice_allowed = false;
            if let NativeRewardContent::Cards { reroll, .. } = reward.content {
                reroll_allowed = reroll && run.run_ctx().owned_relics(0).any(|r| r.id == RelicId::Driftwood && r.enabled);
                sacrifice_allowed = reroll && run.run_ctx().owned_relics(0).any(|r| r.id == RelicId::PaelsWing && r.enabled);
                ints(&mut row.numbers, &[(2, i32::from(reroll_allowed)), (3, i32::from(sacrifice_allowed))])?;
            }
            let parent = Self::row(obs, row)?;
            let container_active = active && !modal_card && !reward.active;
            let focused = active && reward.active;
            let index_arg = if focused { None } else { Some(display as u32) };
            match reward.content {
                NativeRewardContent::Cards { cards, closed, .. } => {
                    if container_active { self.reward_action(obs, SoloCommand::OpenReward { reward_index: display as u32 }, parent, ContentId::ABSENT, None, index, true)?; }
                    if reward.active && !closed {
                        for (index, card) in cards.iter().enumerate() {
                            let source = self.card(obs, card, self.seat_rows[0], parent, PileKind::None, None, None)?;
                            if focused { self.reward_action(obs, SoloCommand::PickCard { index: index as u32 }, source,
                                id(Namespace::CardId, card.id.index()), Some(card.instance_id), reward.backing_index, true)?; }
                        }
                        if focused {
                            if reward.skippable { self.simple(obs, Command::Solo(SoloCommand::Skip), parent, true)?; }
                            if reroll_allowed { self.simple(obs, Command::Solo(SoloCommand::RerollCardReward), parent, true)?; }
                            if sacrifice_allowed { self.simple(obs, Command::Solo(SoloCommand::SacrificeCardReward), parent, true)?; }
                        }
                    }
                }
                NativeRewardContent::SpecialCard { card, owner } => {
                    let source = self.card(obs, card, self.seat_rows[usize::from(owner)], parent, PileKind::None, None, None)?;
                    if container_active { self.reward_action(obs, SoloCommand::PickSpecialCard { reward_index: display as u32 }, source,
                        id(Namespace::CardId, card.id.index()), Some(card.instance_id), index, true)?; }
                }
                NativeRewardContent::RemoveCard { .. } => {
                    if container_active { self.reward_action(obs, SoloCommand::OpenReward { reward_index: display as u32 }, parent, ContentId::ABSENT, None, index, true)?; }
                }
                NativeRewardContent::Potion(potion) => {
                    self.potion(obs, Some(potion), self.seat_rows[0], parent, 0, run.run_ctx().max_potion_slots.max(0) as usize)?;
                    if focused || container_active { self.reward_action(obs, SoloCommand::PickPotion { reward_index: index_arg }, parent,
                        id(Namespace::PotionId, potion.index()), None, index, run.native_can_procure_potion())?; }
                    if focused { self.simple(obs, Command::Solo(SoloCommand::SkipPotion), parent, true)?; }
                }
                NativeRewardContent::Relic(relic) => {
                    self.modifier(obs, self.seat_rows[0], parent, id(Namespace::RelicId, relic.index()), ContentId::ABSENT, 1)?;
                    self.offered_relic(obs, relic, parent, self.seat_rows[0])?;
                    if focused || container_active { self.reward_action(obs, SoloCommand::PickRelicReward { reward_index: index_arg }, parent,
                        id(Namespace::RelicId, relic.index()), None, index, true)?; }
                    if focused && reward.skippable { self.simple(obs, Command::Solo(SoloCommand::SkipRelic), parent, true)?; }
                }
                NativeRewardContent::Bundles(bundles) => {
                    if reward.active { for (index, bundle) in bundles.iter().enumerate() {
                        let mut offer = Row { kind: RowKind::Offer, owner: self.seat_rows[0], parent, ..Row::default() };
                        ints(&mut offer.numbers, &[(1, as_i32(index)?), (2, as_i32(bundle.len())?)])?;
                        let source = Self::row(obs, offer)?;
                        for card in bundle {
                            let child = self.card(obs, card, self.seat_rows[0], source, PileKind::None, None, None)?;
                            if !self.legacy_mode { obs.push_edge(Edge { from: source, to: child, relation: Relation::Member })?; }
                        }
                        if focused { self.reward_action(obs, SoloCommand::PickCardBundle { index: index as u32 }, source, ContentId::ABSENT,
                            bundle.first().map(|card| card.instance_id), reward.backing_index, true)?; }
                    } }
                }
            }
        }
        if active && !modal_card && can_skip_set { self.simple(obs, Command::Solo(SoloCommand::SkipRewards), self.seat_rows[0], true)?; }
        Ok(())
    }
    fn reward_action(&self, obs: &mut NativeObsV1, command: SoloCommand, source: RowHandle, model: ContentId,
        instance: Option<u32>, reward: usize, legal: bool) -> Result<()> {
        self.action(obs, Candidate { model, source, legal, ..Candidate::default() }, Command::Solo(command), instance, None, Some(reward as u64))
    }
}

pub(super) fn event_parameter(parameter: EventParameter) -> (Option<i32>, Option<i32>, Option<i32>) {
    match parameter {
        EventParameter::None | EventParameter::Relic(_) => (None, None, None),
        EventParameter::AbandonRunConfirmation(_)|EventParameter::SphereReward{..} => unreachable!("modal must be refused before event encoding"),
        EventParameter::Offer(value) => (Some(i32::from(value)), None, None),
        EventParameter::Counter(value) => (Some(value), None, None),
        EventParameter::Bridge(value) => (value.map(i32::from), None, None),
        EventParameter::ConveyorDish(dish) => (Some(dish as i32), None, None),
        EventParameter::Sphere { big, x, y } => (Some(i32::from(big)), Some(i32::from(x)), Some(i32::from(y))),
    }
}
