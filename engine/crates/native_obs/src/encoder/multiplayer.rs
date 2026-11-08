use super::*;
use sts2_sim::run::{Coord, EventOptionIdentity, EventPageIdentity, EventParameter,
    NativeShopKind, RestOptionKind, mp::MpPhase};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SelectionKind { #[default] None, Event, Cheese, Shears, Merchant }

#[derive(Clone)]
pub(super) struct LocalUi {
    pub episode: u64,
    pub kind: SelectionKind,
    purpose: ContentId,
    offered: [u32; SELECTION_CAPACITY],
    pub offered_len: usize,
    pub selected: [SelectionItem; SELECTION_CAPACITY],
    pub selected_len: usize,
    pub max: usize,
    pub auto_confirm: bool,
    pub removal: bool,
}
impl Default for LocalUi {
    fn default() -> Self { Self { episode: 0, kind: SelectionKind::None, purpose: ContentId::ABSENT,
        offered: [0; SELECTION_CAPACITY], offered_len: 0, selected: [SelectionItem::default(); SELECTION_CAPACITY],
        selected_len: 0, max: 0, auto_confirm: false, removal: false } }
}
impl LocalUi {
    pub(super) fn toggle(&mut self, index: u32, instance: Option<u32>) -> Result<()> {
        let index_usize = index as usize;
        if self.kind == SelectionKind::None || index_usize >= self.offered_len || instance != Some(self.offered[index_usize]) {
            return Err(ContractError::InvalidBinding.into());
        }
        if let Some(at) = self.selected[..self.selected_len].iter().position(|item| item.option_index == index) {
            self.selected.copy_within(at + 1..self.selected_len, at); self.selected_len -= 1;
        } else {
            if self.selected_len >= self.max { return Err(ContractError::InvalidBinding.into()); }
            self.selected[self.selected_len] = SelectionItem { option_index: index, instance_id: instance };
            self.selected_len += 1;
        }
        Ok(())
    }
}

impl NativeEncoder {
    /// Authenticated perspective over one shared multiplayer run. No automatic work runs here.
    pub fn encode_multiplayer(&mut self, run: &MpRun, decision: DecisionId, net_id: u64, obs: &mut NativeObsV1) -> Result<()> {
        self.multiplayer_decision(run, decision, net_id, obs)?;
        if let Some(encoder) = &mut self.legacy
            && !run.native_event_page(net_id, usize::from(decision.actor)).map_err(EncodeError::InvalidState)?
                .is_some_and(modal::is_modal_page) {
            encoder.ui = self.ui.clone();
            let mut old = obs.legacy.take().unwrap_or_default();
            let result = encoder.multiplayer_decision(run, decision, net_id, &mut old);
            Self::publish_legacy(obs, old, result)?;
        }
        Ok(())
    }
    pub(super) fn multiplayer_decision(&mut self, run: &MpRun, decision: DecisionId, net_id: u64, obs: &mut NativeObsV1) -> Result<()> {
        self.begin(obs, decision, net_id)?;
        let actor = usize::from(decision.actor);
        run.roster.actor(net_id, actor).map_err(|_| EncodeError::Contract(ContractError::WrongOwner))?;
        for ui in &mut self.ui {
            if ui.episode != decision.episode { *ui = LocalUi::default(); ui.episode = decision.episode; }
            if run.phase != MpPhase::Shop { ui.removal = false; }
        }
        let combat = (run.phase == MpPhase::Combat).then(|| run.combat()).flatten();
        let ctx = combat.map_or(&run.world.run, |c| &c.run);
        let global = self.global(obs, ctx, id(Namespace::MpPhase, run.phase as usize), run.roster.players().len(), combat, Some(run.native_public_boss()))?;
        if let Some(c) = combat {
            let status = self.combat(obs, c, global, true, |seat| run.roster.players()[seat].character,
                |slot| run.can_discard_potion(net_id, actor, slot))?;
            if !self.team { self.mp_map(run, obs, global)?; }
            self.finish(obs, status)?; return Ok(());
        }
        let count = run.roster.players().len();
        for (owner, player) in run.roster.players().iter().enumerate() {
            let mut row = Row { kind: RowKind::Seat, detail: [id(Namespace::Character, player.character as usize),
                ContentId::ABSENT, ContentId::ABSENT, ContentId::ABSENT], parent: global, ..Row::default() };
            ints(&mut row.numbers, &[(0, as_i32(owner)?), (1, as_i32((owner + count - actor) % count)?),
                (2, player.hp), (3, player.max_hp), (6, player.max_energy), (8, player.gold), (9, as_i32(player.max_potion_slots)?)])?;
            row.flags.set(Flag::Alive, player.hp > 0); row.flags.set(Flag::Participating, true);
            self.seat_rows[owner] = Self::row(obs, row)?;
            self.inventory(obs, owner, &player.deck, &player.relics, &player.potions, player.max_potion_slots, None)?;
        }
        self.mp_map(run, obs, global)?;
        if run.phase == MpPhase::RunOver { self.finish(obs, DecisionStatus::Terminal)?; return Ok(()); }
        if run.phase == MpPhase::Combat { return Err(EncodeError::InvalidState("combat phase without combat")); }
        if run.phase == MpPhase::Rewards && run.rewards.is_none() { self.finish(obs, DecisionStatus::Waiting)?; return Ok(()); }
        let modal = self.mp_pickup(run, obs)?;
        let active = if modal { true } else { match run.phase {
            MpPhase::Neow => self.mp_neow(run, obs)?,
            MpPhase::Map => {
                if run.native_map_blocked() { false } else {
                    let source = run.world.run.coord.and_then(|(x, y)| run.world.map.point(Coord::new(x, y)))
                        .ok_or(EncodeError::InvalidState("map source missing"))?;
                    for &index in &source.children {
                        let point = &run.world.map.points[usize::from(index)];
                        let mut candidate = Candidate { source: self.map_rows[usize::from(index)], target: self.map_rows[usize::from(index)],
                            legal: true, ..Candidate::default() };
                        ints(&mut candidate.numbers, &[(8, point.coord.col), (9, point.coord.row)])?;
                        self.action(obs, candidate, Command::MpMove { x: point.coord.col, y: point.coord.row }, None, None, None)?;
                    }
                    !source.children.is_empty()
                }
            }
            MpPhase::Rewards => self.mp_rewards(run, obs)?,
            MpPhase::Event => self.mp_event(run, obs)?,
            MpPhase::Shop => {
                let leave = run.native_can_leave_shop(net_id, actor);
                // No policy distribution for a seat with no enabled stock
                // while another owner's selector blocks the shared exit.
                if !self.ui[actor].removal && !leave
                    && !run.native_shop_offers(actor).map_err(EncodeError::InvalidState)?.any(|offer| offer.enabled) {
                    self.finish(obs, DecisionStatus::Waiting)?;
                    return Ok(());
                }
                let removing = self.ui[actor].removal; let mut removal_offered = false;
                let mut available = removing;
                for offer in run.native_shop_offers(actor).map_err(EncodeError::InvalidState)? {
                    available |= offer.enabled;
                    if offer.kind == NativeShopKind::Removal {
                        self.shop_offer(obs, offer, removing, true)?;
                        if !removing && !removal_offered { self.simple(obs, Command::MpBeginRemoval, self.seat_rows[actor], offer.enabled)?; removal_offered = true; }
                    } else { self.shop_offer(obs, offer, !removing, true)?; }
                }
                self.simple(obs, if removing { Command::MpCancelRemoval } else { Command::MpLeaveShop },
                    self.seat_rows[actor], removing || leave)?;
                available || leave
            }
            MpPhase::RestSite => self.mp_rest(run, obs)?,
            MpPhase::Treasure => { self.mp_treasure(run, obs)?; true },
            MpPhase::Combat | MpPhase::RunOver => false,
        }};
        let inventory = self.mp_inventory_actions(run, obs)?;
        self.finish(obs, if active || inventory { DecisionStatus::Actionable } else { DecisionStatus::Waiting })?;
        Ok(())
    }

    fn mp_map(&mut self, run: &MpRun, obs: &mut NativeObsV1, global: RowHandle) -> Result<()> {
        let current = run.world.run.coord;
        let source = current.and_then(|(x, y)| run.world.map.point(Coord::new(x, y)));
        self.map(obs, &run.world.map, run.world.run.act_index, current,
            |index| run.phase == MpPhase::Map && !run.native_map_blocked() && source.is_some_and(|point| point.children.contains(&(index as u8))),
            |index| run.world.visited.iter().filter(|(act, _)| *act == run.world.run.act_index)
                .position(|(_, coord)| *coord == run.world.map.points[index].coord))?;
        if !self.legacy_mode { self.bosses(obs, global, run.world.run.act_index, run.native_public_bosses())?; }
        for (seat, vote) in run.voting.votes().iter().enumerate() {
            let Some(vote) = vote else { continue; };
            if let Some(index) = run.world.map.points.iter().position(|point| point.coord == vote.coord) {
                let target = self.map_rows[index]; if target != RowHandle::ABSENT {
                    obs.push_edge(Edge { from: self.seat_rows[seat], to: target, relation: Relation::Target })?;
                }
            }
        }
        Ok(())
    }
    fn mp_inventory_actions(&self, run: &MpRun, obs: &mut NativeObsV1) -> Result<bool> {
        if !matches!(run.phase, MpPhase::Neow | MpPhase::Map | MpPhase::Rewards | MpPhase::Event) || run.combat().is_some() { return Ok(false); }
        let seat = usize::from(self.decision.actor); let player = &run.roster.players()[seat]; let mut any = false;
        for (slot, potion) in player.potions.iter().enumerate() {
            let Some(potion) = potion else { continue; }; any = true;
            self.action(obs, Candidate { model: id(Namespace::PotionId, potion.index()), source: self.seat_rows[seat],
                legal: run.can_discard_potion(self.net_id, seat, slot),
                ..Candidate::default() }, Command::MpDiscardPotion { slot: slot as u32 }, None, None, Some(slot as u64))?;
        }
        Ok(any)
    }
    fn mp_neow(&self, run: &MpRun, obs: &mut NativeObsV1) -> Result<bool> {
        let seat = usize::from(self.decision.actor);
        let page = self.event_page(obs, EventId::Neow, EventPageIdentity { id: EventId::Neow.initial_page(), parameter: EventParameter::None })?;
        if run.neow_finished(seat) != Some(false) { return Ok(false); }
        for (index, &relic) in run.neow[seat].iter().enumerate() {
            let option = EventOptionIdentity::neow(index as u16);
            let parameters = if self.legacy_mode { Vec::new() }
                else { run.native_event_parameters(seat, option, None).map_err(EncodeError::Gameplay)? };
            let source = self.event_option(obs, option, option.id, None, index, page, true, Some(id(Namespace::RelicId, relic.index())), &parameters)?;
            self.offered_relic(obs, relic, source, self.seat_rows[seat])?;
            self.action(obs, Candidate { source, model: id(Namespace::RelicId, relic.index()), legal: true, ..Candidate::default() },
                Command::MpChooseNeow { option: index as u32 }, None, None, Some(index as u64))?;
        }
        Ok(true)
    }

    fn mp_pickup(&mut self, run: &MpRun, obs: &mut NativeObsV1) -> Result<bool> {
        let seat = usize::from(self.decision.actor); let player = &run.roster.players()[seat];
        if run.phase == MpPhase::Shop && self.mp_merchant_pickup(run, obs)? { return Ok(true); }
        if let Some(cards) = run.neow_cards[seat].as_deref() {
            self.mp_cards(obs, cards, ChoicePurpose::NativeModalKindNeowCardReward, run.native_neow_card_source(seat).map(|r| id(Namespace::RelicId, r.index())),
                |index| Command::MpSelectNeowCard { index: Some(index) }, Some(Command::MpSelectNeowCard { index: None }))?; return Ok(true);
        }
        if let Some(potion) = run.neow_potion_rewards[seat] {
            self.mp_potion_offer(run, obs, potion, Command::MpSelectNeowPotion { accept: true }, Some(Command::MpSelectNeowPotion { accept: false }), 0)?; return Ok(true);
        }
        if let Some(bundles) = run.scroll_box_bundles[seat].as_ref() {
            for (index, cards) in bundles.iter().enumerate() {
                let mut row = Row { kind: RowKind::Offer, model: id(Namespace::RelicId, RelicId::ScrollBoxes.index()), owner: self.seat_rows[seat], ..Row::default() };
                ints(&mut row.numbers, &[(1, as_i32(index)?), (2, as_i32(cards.len())?)])?; let parent = Self::row(obs, row)?;
                for (order, card) in cards.iter().enumerate() {
                    let child = self.card(obs, card, self.seat_rows[seat], parent, PileKind::None, Some(order), None)?;
                    if !self.legacy_mode { obs.push_edge(Edge { from: parent, to: child, relation: Relation::Member })?; }
                }
                self.action(obs, Candidate { source: parent, legal: true, ..Candidate::default() }, Command::MpScrollBoxBundle { index: index as u32 }, None, None, Some(index as u64))?;
            }
            return Ok(true);
        }
        for (choices, purpose, source, kind) in [
            (&run.pomander_choices[seat], ChoicePurpose::NativeModalKindPomander, RelicId::Pomander, 0),
            (&run.precise_scissors_choices[seat], ChoicePurpose::NativeModalKindPreciseScissors, RelicId::PreciseScissors, 1),
            (&run.new_leaf_choices[seat], ChoicePurpose::NativeModalKindNewLeaf, RelicId::NewLeaf, 2),
            (&run.paels_growth_choices[seat], ChoicePurpose::NativeModalKindPaelsGrowth, RelicId::PaelsGrowth, 3)] {
            if let Some(choices) = choices {
                self.mp_instances(obs, &run.world.run, player, choices, purpose, Some(id(Namespace::RelicId, source.index())), |instance| match kind {
                    0 => Command::MpPomanderCard { instance }, 1 => Command::MpPreciseScissorsCard { instance },
                    2 => Command::MpNewLeafCard { instance }, _ => Command::MpPaelsGrowthCard { instance } }, None)?;
                return Ok(true);
            }
        }
        if let Some(choices) = run.shears_choices[seat].as_ref() {
            self.mp_multi_cards(obs, &run.world.run, choices.len(), |index| player.deck.iter().find(|c| c.instance_id == choices[index]),
                SelectionKind::Shears, ChoicePurpose::NativeModalKindPrecariousShears, 2, 2, false, None)?; return Ok(true);
        }
        if let Some(relic) = run.relic_rewards[seat] {
            let (take, skip) = if run.phase == MpPhase::Event { (Command::MpEventRelic { accept: true }, Command::MpEventRelic { accept: false }) }
                else { (Command::MpRelicReward { accept: true }, Command::MpRelicReward { accept: false }) };
            self.mp_relic_offer(obs, relic, take, run.native_relic_skippable(seat).then_some(skip), 0)?; return Ok(true);
        }
        Ok(false)
    }

    fn mp_merchant_pickup(&mut self, run: &MpRun, obs: &mut NativeObsV1) -> Result<bool> {
        use sts2_sim::run::mp::MerchantPickupChoice;
        let seat = usize::from(self.decision.actor);
        let Some(choice) = run.merchant_pickup(self.net_id, seat).map_err(EncodeError::Gameplay)? else { return Ok(false); };
        self.ui[seat].removal = false;
        let (cards, relic, min, max) = match choice {
            MerchantPickupChoice::Deck { relic, cards, min, max } => (cards.as_slice(), *relic, *min, *max),
            MerchantPickupChoice::Cards(bundles) => (bundles.first().ok_or(EncodeError::InvalidState("empty merchant bundles"))?.as_slice(),
                RelicId::Orrery, 0, 1),
            MerchantPickupChoice::Potions(potions) => {
                let potion = *potions.first().ok_or(EncodeError::InvalidState("empty merchant potions"))?;
                let parent = self.mp_choice(obs, ChoicePurpose::ChoiceSrcRelic, Some(id(Namespace::RelicId, RelicId::Cauldron.index())),
                    0, 1, 0, true, true, false)?;
                let source = self.potion(obs, Some(potion), self.seat_rows[seat], parent, 0,
                    run.roster.players()[seat].max_potion_slots)?;
                self.action(obs, Candidate { source, model: id(Namespace::PotionId, potion.index()),
                    legal: run.native_can_select_merchant_pickup(self.net_id, seat, &[0]), ..Candidate::default() },
                    Command::MpMerchantPickup { index: Some(0) }, None, None, Some(0))?;
                self.simple(obs, Command::MpMerchantPickup { index: None }, parent,
                    run.native_can_select_merchant_pickup(self.net_id, seat, &[]))?;
                return Ok(true);
            }
        };
        if max > 1 {
            self.mp_multi_cards(obs, &run.world.run, cards.len(), |index| cards.get(index), SelectionKind::Merchant,
                ChoicePurpose::ChoiceSrcRelic, min.min(cards.len()), max, min == 0, Some((run, relic)))?;
            return Ok(true);
        }
        let skip = run.native_can_select_merchant_pickup(self.net_id, seat, &[]);
        let parent = self.mp_choice(obs, ChoicePurpose::ChoiceSrcRelic, Some(id(Namespace::RelicId, relic.index())),
            min.min(cards.len()), max, 0, skip, true, false)?;
        for (index, card) in cards.iter().enumerate() {
            let source = self.card(obs, card, self.seat_rows[seat], parent, PileKind::None, Some(index), None)?;
            self.action(obs, Candidate { source, model: id(Namespace::CardId, card.id.index()),
                legal: run.native_can_select_merchant_pickup(self.net_id, seat, &[index]), ..Candidate::default() },
                Command::MpMerchantPickup { index: Some(index as u32) }, Some(card.instance_id), None, Some(index as u64))?;
        }
        if skip { self.simple(obs, Command::MpMerchantPickup { index: None }, parent, true)?; }
        Ok(true)
    }

    fn mp_choice(&self, obs: &mut NativeObsV1, purpose: ChoicePurpose, model: Option<ContentId>, min: usize, max: usize,
        selected: usize, optional: bool, single: bool, confirm: bool) -> Result<RowHandle> {
        let mut row = Row { kind: RowKind::Choice, model: model.unwrap_or(ContentId::ABSENT), detail: [purpose.into(), ContentId::ABSENT,
            ContentId::ABSENT, ContentId::ABSENT], owner: self.seat_rows[usize::from(self.decision.actor)], ..Row::default() };
        ints(&mut row.numbers, &[(1, as_i32(min)?), (2, as_i32(max)?), (3, as_i32(selected)?), (4, i32::from(self.decision.actor))])?;
        for (flag, value) in [(Flag::Optional, optional), (Flag::SingleSelect, single), (Flag::Ordered, true), (Flag::CanConfirm, confirm)] { row.flags.set(flag, value); }
        Self::row(obs, row)
    }
    fn mp_cards(&self, obs: &mut NativeObsV1, cards: &[Card], purpose: ChoicePurpose, model: Option<ContentId>,
        command: impl Fn(u32) -> Command, skip: Option<Command>) -> Result<()> {
        let parent = self.mp_choice(obs, purpose, model, 1, 1, 0, skip.is_some(), true, false)?;
        for (index, card) in cards.iter().enumerate() {
            let source = self.card(obs, card, self.seat_rows[usize::from(self.decision.actor)], parent, PileKind::None, Some(index), None)?;
            self.action(obs, Candidate { source, model: id(Namespace::CardId, card.id.index()), legal: true, ..Candidate::default() },
                command(index as u32), Some(card.instance_id), None, Some(index as u64))?;
        }
        if let Some(command) = skip { self.simple(obs, command, parent, true)?; } Ok(())
    }
    fn mp_instances(&self, obs: &mut NativeObsV1, ctx: &RunCtx, player: &sts2_sim::run::mp::MpPlayer, instances: &[u32], purpose: ChoicePurpose,
        model: Option<ContentId>, command: impl Fn(u32) -> Command, skip: Option<Command>) -> Result<()> {
        let parent = self.mp_choice(obs, purpose, model, 1, 1, 0, skip.is_some(), true, false)?;
        for (index, &instance) in instances.iter().enumerate() {
            let card = player.deck.iter().find(|card| card.instance_id == instance).ok_or(EncodeError::InvalidState("pickup card missing"))?;
            let source = self.card_handle(deck_key(usize::from(self.decision.actor), instance));
            let mut row = Row { kind: RowKind::ChoiceOption, model: id(Namespace::CardId, card.id.index()), owner: self.seat_rows[usize::from(self.decision.actor)],
                parent, ..Row::default() }; ints(&mut row.numbers, &[(0, as_i32(index)?)])?;
            let option = Self::row(obs, row)?; obs.push_edge(Edge { from: option, to: source, relation: Relation::Source })?;
            if v2::is_upgrade_purpose(purpose) { self.upgrade_preview(obs, ctx, card, source, option, purpose)?; }
            self.action(obs, Candidate { source, target: option, model: row.model, legal: true, ..Candidate::default() }, command(instance), Some(instance), None, Some(index as u64))?;
        }
        if let Some(command) = skip { self.simple(obs, command, parent, true)?; } Ok(())
    }
    fn mp_multi_cards<'a>(&mut self, obs: &mut NativeObsV1, ctx: &RunCtx, count: usize, cards: impl Fn(usize) -> Option<&'a Card>,
        kind: SelectionKind, purpose: ChoicePurpose, min: usize, max: usize, optional: bool,
        merchant: Option<(&MpRun, RelicId)>) -> Result<()> {
        if count > SELECTION_CAPACITY { return Err(capacity(BufferKind::Selection, count, SELECTION_CAPACITY)); }
        let seat = usize::from(self.decision.actor);
        {
            let ui = &mut self.ui[seat];
            let same = ui.kind == kind && ui.purpose == purpose.into() && ui.offered_len == count && ui.max == max
                && (0..count).all(|i| cards(i).is_some_and(|card| ui.offered[i] == card.instance_id));
            if !same { ui.selected_len = 0; }
            ui.kind = kind; ui.purpose = purpose.into(); ui.max = max; ui.offered_len = count;
            ui.auto_confirm = kind == SelectionKind::Event && min == max;
            for index in 0..count { ui.offered[index] = cards(index).ok_or(EncodeError::InvalidState("choice card missing"))?.instance_id; }
        }
        let selected = self.ui[seat].selected_len;
        let confirm = if let Some((run, _)) = merchant {
            let mut indices = [0usize; SELECTION_CAPACITY];
            for (at, item) in self.ui[seat].selected[..selected].iter().enumerate() { indices[at] = item.option_index as usize; }
            run.native_can_select_merchant_pickup(self.net_id, seat, &indices[..selected])
        } else { (selected >= min && selected <= max) || optional && selected == 0 };
        let parent = self.mp_choice(obs, purpose, merchant.map(|(_, relic)| id(Namespace::RelicId, relic.index())),
            min, max, selected, optional, false, confirm)?;
        for index in 0..count {
            let card = cards(index).ok_or(EncodeError::InvalidState("choice card missing"))?;
            let mut source = self.card_handle(deck_key(seat, card.instance_id));
            if source == RowHandle::ABSENT { source = self.card(obs, card, self.seat_rows[seat], parent, PileKind::None, Some(index), None)?; }
            let at = self.ui[seat].selected[..selected].iter().position(|item| item.option_index as usize == index);
            let mut row = Row { kind: RowKind::ChoiceOption, model: id(Namespace::CardId, card.id.index()), owner: self.seat_rows[seat], parent, ..Row::default() };
            ints(&mut row.numbers, &[(0, as_i32(index)?)])?; row.flags.set(Flag::Selected, at.is_some());
            if let Some(at) = at { ints(&mut row.numbers, &[(1, as_i32(at)?), (2, as_i32(at)?)])?; }
            let option = Self::row(obs, row)?; obs.push_edge(Edge { from: option, to: source, relation: Relation::Source })?;
            if v2::is_upgrade_purpose(purpose) { self.upgrade_preview(obs, ctx, card, source, option, purpose)?; }
            self.action(obs, Candidate { source, target: option, model: row.model, flags: row.flags, legal: at.is_some() || selected < max,
                ..Candidate::default() }, Command::ToggleSelection { index: index as u32 }, Some(card.instance_id), None, Some(index as u64))?;
        }
        if confirm {
            let selected = &self.ui[seat].selected[..selected];
            let command = match kind {
                SelectionKind::Event => Command::MpEventCards { selection: obs.push_selection(selected)? },
                SelectionKind::Shears => Command::MpPrecariousShearsCards { selection: obs.push_selection(selected)? },
                SelectionKind::Cheese => Command::MpCheeseCards { indices: [selected[0].option_index, selected[1].option_index] },
                SelectionKind::Merchant => Command::MpMerchantPickupCards { selection: obs.push_selection(selected)? },
                SelectionKind::None => return Err(EncodeError::InvalidState("missing selection kind")),
            };
            self.simple(obs, command, parent, true)?;
        }
        Ok(())
    }

    fn mp_potion_offer(&self, run: &MpRun, obs: &mut NativeObsV1, potion: PotionId, take: Command, skip: Option<Command>, order: usize) -> Result<()> {
        let seat = usize::from(self.decision.actor); let player = &run.roster.players()[seat];
        let source = self.potion(obs, Some(potion), self.seat_rows[seat], self.seat_rows[seat], order, player.max_potion_slots)?;
        let legal = player.potions.iter().any(Option::is_none) || player.relics.iter().any(|r| r.enabled && r.id == RelicId::Sozu);
        self.action(obs, Candidate { source, model: id(Namespace::PotionId, potion.index()), legal, ..Candidate::default() }, take, None, None, Some(order as u64))?;
        if let Some(skip) = skip { self.simple(obs, skip, source, true)?; } Ok(())
    }
    fn mp_relic_offer(&self, obs: &mut NativeObsV1, relic: RelicId, take: Command, skip: Option<Command>, order: usize) -> Result<()> {
        let mut row = Row { kind: RowKind::Reward, model: id(Namespace::RelicId, relic.index()), detail: [RewardKind::Relic.into(),
            ContentId::ABSENT, ContentId::ABSENT, ContentId::ABSENT], owner: self.seat_rows[usize::from(self.decision.actor)], ..Row::default() };
        ints(&mut row.numbers, &[(1, as_i32(order)?)])?; let source = Self::row(obs, row)?;
        self.offered_relic(obs, relic, source, row.owner)?;
        self.action(obs, Candidate { source, model: row.model, legal: true, ..Candidate::default() }, take, None, None, Some(order as u64))?;
        if let Some(skip) = skip { self.simple(obs, skip, source, true)?; } Ok(())
    }
    fn mp_rewards(&self, run: &MpRun, obs: &mut NativeObsV1) -> Result<bool> {
        let seat = usize::from(self.decision.actor);
        let reward = run.rewards.as_ref().and_then(|rewards| rewards.get(seat)).ok_or(EncodeError::InvalidState("missing rewards"))?;
        if reward.completed { return Ok(false); }
        let mut order = 0;
        for &amount in &reward.gold {
            let mut row = Row { kind: RowKind::Reward, detail: [RewardKind::Gold.into(), ContentId::ABSENT, ContentId::ABSENT, ContentId::ABSENT],
                owner: self.seat_rows[seat], ..Row::default() }; ints(&mut row.numbers, &[(0, amount), (1, as_i32(order)?)])?;
            row.flags.set(Flag::Used, reward.gold_taken); Self::row(obs, row)?; order += 1;
        }
        if let Some(potion) = reward.potion {
            if !reward.potion_taken { self.mp_potion_offer(run, obs, potion, Command::MpTakePotion, None, order)?; } order += 1;
        }
        for (index, potion) in reward.extra_potions.iter().enumerate() {
            if let Some(potion) = potion { self.mp_potion_offer(run, obs, *potion, Command::MpTakeExtraPotion { index: index as u32 }, None, order)?; } order += 1;
        }
        for (index, relic) in reward.extra_relics.iter().enumerate() {
            if let Some(relic) = relic { self.mp_relic_offer(obs, *relic, Command::MpTakeExtraRelic { index: index as u32 }, None, order)?; } order += 1;
        }
        for (index, card) in reward.special_cards.iter().enumerate() {
            if let Some(card) = card {
                let reward_index = reward.special_card_reward_index(index).ok_or(EncodeError::InvalidState("special reward index missing"))?;
                let mut row = Row { kind: RowKind::Reward, detail: [RewardKind::SpecialCard.into(), ContentId::ABSENT, ContentId::ABSENT, ContentId::ABSENT],
                    owner: self.seat_rows[seat], ..Row::default() }; ints(&mut row.numbers, &[(1, as_i32(order)?)])?;
                let parent = Self::row(obs, row)?; let source = self.card(obs, card, self.seat_rows[seat], parent, PileKind::None, None, None)?;
                self.action(obs, Candidate { source, model: id(Namespace::CardId, card.id.index()), legal: true, ..Candidate::default() },
                    Command::MpTakeSpecialCard { reward_index: reward_index as u32 }, Some(card.instance_id), None, Some(reward_index as u64))?;
            } order += 1;
        }
        if !reward.card_taken {
            let mut row = Row { kind: RowKind::Reward, detail: [RewardKind::Card.into(), ContentId::ABSENT, ContentId::ABSENT, ContentId::ABSENT],
                owner: self.seat_rows[seat], ..Row::default() }; ints(&mut row.numbers, &[(1, as_i32(order)?)])?; let parent = Self::row(obs, row)?;
            for (index, card) in reward.cards.iter().enumerate() {
                let source = self.card(obs, card, self.seat_rows[seat], parent, PileKind::None, Some(index), None)?;
                self.action(obs, Candidate { source, model: id(Namespace::CardId, card.id.index()), legal: true, ..Candidate::default() },
                    Command::MpTakeCard { index: index as u32 }, Some(card.instance_id), None, Some(index as u64))?;
            }
        }
        self.simple(obs, Command::MpTakeGoldSkipRemaining, self.seat_rows[seat], true)?; Ok(true)
    }
    fn mp_event(&mut self, run: &MpRun, obs: &mut NativeObsV1) -> Result<bool> {
        let seat = usize::from(self.decision.actor); let net = self.net_id;
        let event = run.native_event_id(seat).ok_or(EncodeError::InvalidState("event identity missing"))?;
        let page = run.native_event_page(net, seat).map_err(EncodeError::InvalidState)?
            .unwrap_or(EventPageIdentity { id: event.initial_page(), parameter: EventParameter::None });
        let parent = self.event_page(obs, event, page)?;
        if run.native_event_finished(seat) { return Ok(false); }
        if matches!(page.parameter, EventParameter::SphereReward { .. }) {
            self.sphere_rewards(run, obs, parent)?;
            return Ok(true);
        }
        if let Some(reward)=run.native_event_card_reward(seat) {
            let mut commands=Vec::with_capacity(reward.cards.len());
            for (index,card) in reward.cards.iter().enumerate() {
                let selection=obs.push_selection(&[crate::binding::SelectionItem{
                    option_index:index as u32,instance_id:Some(card.instance_id)}])?;
                commands.push(Command::MpEventCards{selection});
            }
            let skip=Command::MpEventCards{selection:obs.push_selection(&[])?};
            self.mp_cards(obs,reward.cards,reward.purpose,None,
                |index|commands[index as usize],Some(skip))?;
            return Ok(true);
        }
        if let Some(choice) = run.native_event_card_choice(seat) {
            self.mp_multi_cards(obs, &run.world.run, choice.cards.len(), |index| choice.cards.get(index), SelectionKind::Event,
                choice.purpose, choice.min, choice.max, choice.skippable, None)?; return Ok(true);
        }
        if let Some(cards) = run.cheese_cards[seat].as_ref() {
            self.mp_multi_cards(obs, &run.world.run, cards.len(), |index| cards.get(index), SelectionKind::Cheese,
                ChoicePurpose::NativeModalKindCheese, 2, 2, false, None)?; return Ok(true);
        }
        if let Some(potions) = run.native_event_potions(seat) {
            for (index, potion) in potions.iter().enumerate() {
                if let Some(potion) = potion {
                    let command = if event == EventId::WhisperingHollow { Command::MpWhisperingReward { index: index as u32 } }
                        else { Command::MpEventPotion { index: index as u32 } };
                    self.mp_potion_offer(run, obs, *potion, command, None, index)?;
                }
            }
            if potions.iter().all(Option::is_none) { self.simple(obs, if event == EventId::WhisperingHollow { Command::MpFinishWhisperingOption }
                else { Command::MpFinishEventPotions }, parent, true)?; }
            return Ok(true);
        }
        if event == EventId::RoomFullOfCheese {
            for (index, option) in [EventOptionId::RoomFullOfCheeseInitialGorge, EventOptionId::RoomFullOfCheeseInitialSearch].into_iter().enumerate() {
                let identity = EventOptionIdentity { id: option, parameter: EventParameter::None };
                let parameters = if self.legacy_mode { Vec::new() }
                    else { run.native_event_parameters(seat, identity, None).map_err(EncodeError::Gameplay)? };
                let source = self.event_option(obs, identity, option, None, index, parent, true, None, &parameters)?;
                self.action(obs, Candidate { source, model: option.into(), legal: true, ..Candidate::default() }, Command::MpCheeseOption { option: index as u32 }, None, None, Some(index as u64))?;
            }
            return Ok(true);
        }
        let options = run.native_event_options(net, seat).map_err(EncodeError::InvalidState)?.ok_or(EncodeError::InvalidState("event options missing"))?;
        let mut enabled_index = 0;
        for (index, option) in options.iter().enumerate() {
            let model = event_offer_model(run.native_event_offer(seat, option.option_id));
            let executable=option.enabled && !run.native_event_click_blocked(seat,option.native_id());
            let display = EventOptionIdentity { id: option.native_id(), ..option.option_id };
            let parameters = if self.legacy_mode { Vec::new() }
                else { run.native_event_parameters(seat, display, option.dynamic_description).map_err(EncodeError::Gameplay)? };
            let source = self.event_option(obs, option.option_id, option.native_id(), option.dynamic_description, index, parent, executable, model, &parameters)?;
            if let Some(sts2_sim::run::NativeEventOffer::Relic(relic)) = run.native_event_offer(seat, option.option_id) {
                self.offered_relic(obs, relic, source, self.seat_rows[seat])?;
            }
            if !self.legacy_mode && option.option_id.id == EventOptionId::SlipperyBridgeInitialOvercome {
                if let Some((_, card)) = run.native_bridge_offer(seat) {
                    obs.push_edge(Edge { from: source, to: self.card_handle(deck_key(seat, card.instance_id)), relation: Relation::Target })?;
                }
            }
            self.event_offer_detail(obs, source, run.native_event_offer_detail(seat, option.option_id))?;
            let command = if let EventParameter::AbandonRunConfirmation(confirm) = option.option_id.parameter {
                Command::MpAbandonConfirmation { option: enabled_index, confirm }
            } else if event == EventId::SlipperyBridge { Command::MpBridgeOption { option: index as u32 } }
                else { Command::MpEventOption { option: enabled_index } };
            let (model, target) = if let EventParameter::AbandonRunConfirmation(confirm) = option.option_id.parameter {
                (id(Namespace::PublicModalAction, usize::from(!confirm)), self.seat_rows[seat])
            } else { (model.unwrap_or(option.native_id().into()), RowHandle::ABSENT) };
            self.action(obs, Candidate { source, target, model, legal: executable, ..Candidate::default() },
                command, None, None, Some(index as u64))?;
            if option.enabled { enabled_index += 1; }
        }
        Ok(true)
    }
    fn mp_rest(&self, run: &MpRun, obs: &mut NativeObsV1) -> Result<bool> {
        let seat = usize::from(self.decision.actor); let player = &run.roster.players()[seat];
        if run.native_rest_completed(seat) { return Ok(false); }
        let active = run.native_rest_menu_available(seat);
        for (index, &(kind, enabled)) in run.native_rest_options(seat).ok_or(EncodeError::InvalidState("rest state missing"))?.iter().enumerate() {
            let mut row = Row { kind: RowKind::RestService, detail: [id(Namespace::RestOptionKind, kind as usize), ContentId::ABSENT,
                ContentId::ABSENT, ContentId::ABSENT], owner: self.seat_rows[seat], ..Row::default() };
            ints(&mut row.numbers, &[(3, as_i32(index)?)])?; row.flags.set(Flag::Locked, !enabled);
            if kind == RestOptionKind::Heal {
                let heal = if self.legacy_mode { run.native_rest_base_heal_amount(seat) } else { run.native_rest_heal_amount(seat) };
                ints(&mut row.numbers, &[(0, heal.ok_or(EncodeError::InvalidState("rest owner missing"))?)])?;
            }
            let source = Self::row(obs, row)?;
            if !self.legacy_mode {
                let heal = run.native_rest_heal_amount(seat).ok_or(EncodeError::InvalidState("rest owner missing"))?;
                self.rest_parameters(obs, source, self.seat_rows[seat], kind, heal, &player.deck, &player.relics)?;
            }
            if kind == RestOptionKind::Smith && !self.legacy_mode {
                for card in &player.deck {
                    self.upgrade_preview(obs, &run.world.run, card, self.card_handle(deck_key(seat, card.instance_id)),
                        source, ChoicePurpose::NativeModalKindSmith)?;
                }
            }
            if active { self.action(obs, Candidate { source,
                target: if self.legacy_mode { RowHandle::ABSENT } else { self.seat_rows[seat] }, legal: enabled, ..Candidate::default() },
                Command::MpRestOption { index: index as u32 }, None, None, None)?; }
        }
        if let Some(choices) = run.rest_smith_choices(self.net_id, seat).map_err(EncodeError::InvalidState)? {
            self.mp_instances(obs, &run.world.run, player, choices, ChoicePurpose::NativeModalKindSmith, None,
                |instance| Command::MpRestSmith { instance: Some(instance) }, Some(Command::MpRestSmith { instance: None }))?;
        } else if let Some(targets) = run.rest_mend_targets(self.net_id, seat).map_err(EncodeError::InvalidState)? {
            let parent = self.mp_choice(obs, ChoicePurpose::NativeModalKindMend, None, 1, 1, 0, true, true, false)?;
            for &target in targets {
                let mut candidate = Candidate { source: parent, target: self.seat_rows[target], legal: true, ..Candidate::default() };
                let heal = if self.legacy_mode { run.native_rest_base_heal_amount(target) } else { run.native_rest_heal_amount(target) };
                ints(&mut candidate.numbers, &[(3, heal.ok_or(EncodeError::InvalidState("Mend target missing"))?)])?;
                self.action(obs, candidate, Command::MpRestMend { target: Some(target as u8) }, None, Some(target as u32), None)?;
            }
            self.simple(obs, Command::MpRestMend { target: None }, parent, true)?;
        } else if let Some(cards) = run.rest_card_reward(self.net_id, seat).map_err(EncodeError::InvalidState)? {
            self.mp_cards(obs, cards, ChoicePurpose::NativeModalKindRestCardReward, None, |index| Command::MpRestCardReward { index: Some(index) },
                Some(Command::MpRestCardReward { index: None }))?;
        } else if let Some(potion) = run.rest_potion_reward(self.net_id, seat).map_err(EncodeError::InvalidState)? {
            self.mp_potion_offer(run, obs, potion, Command::MpRestPotionReward { accept: true }, Some(Command::MpRestPotionReward { accept: false }), 0)?;
        } else if active { self.simple(obs, Command::MpSkipRest, self.seat_rows[seat], true)?; }
        Ok(true)
    }
    fn mp_treasure(&self, run: &MpRun, obs: &mut NativeObsV1) -> Result<()> {
        let seat = usize::from(self.decision.actor); let opened = run.native_treasure_opened(seat);
        let mut row = Row { kind: RowKind::Treasure, owner: self.seat_rows[seat], ..Row::default() };
        row.flags.set(Flag::Revealed, opened); ints(&mut row.numbers, &[(1, as_i32((0..run.roster.players().len()).filter(|&i| run.native_treasure_opened(i)).count())?),
            (2, as_i32(run.native_treasure_votes().iter().filter(|vote| vote.is_some()).count())?)])?;
        let parent = Self::row(obs, row)?;
        if !opened { self.simple(obs, Command::MpOpenTreasure, parent, true)?; }
        if opened {
            for (index, &relic) in run.treasure_relics.iter().enumerate() {
                let mut row = Row { kind: RowKind::Offer, model: id(Namespace::RelicId, relic.index()), parent, ..Row::default() };
                ints(&mut row.numbers, &[(1, as_i32(index)?)])?; let source = Self::row(obs, row)?;
                for (owner, vote) in run.native_treasure_votes().iter().enumerate() {
                    if *vote == Some(Some(index)) { obs.push_edge(Edge { from: self.seat_rows[owner], to: source, relation: Relation::Target })?; }
                }
                self.action(obs, Candidate { source, model: row.model, legal: true, ..Candidate::default() }, Command::MpVoteTreasure { index: Some(index as u32) }, None, None, Some(index as u64))?;
            }
            self.simple(obs, Command::MpVoteTreasure { index: None }, parent, true)?;
        }
        Ok(())
    }
}
