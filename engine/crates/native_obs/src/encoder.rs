//! Native state projection. All entry points borrow the simulator immutably.
//! Scratch storage is lane-owned; names, wire admissions and RNG never participate.
use crate::*;
use sts2_sim::{card::{Card, KRef}, choice::ChoiceSrc, combat::{Combat, PileKind, Props, RunCtx},
    creature::Side, relic::Relic, run::{ActMap, RunManager, mp::{Character, MpRun}}, tables::*};

mod solo;
mod multiplayer;
mod dispatch;
mod team;
mod v2;
mod modal;
mod piles;
mod map_cache;
use piles::RowWriter;
pub use team::{TeamObsV2, TeamAction, TeamBinding, TEAM_CANDIDATE_CAPACITY};
#[cfg(test)]
mod tests;
#[cfg(test)]
mod counter_tests;
#[cfg(test)]
mod event_parameter_tests;
pub use dispatch::DispatchError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeError { Contract(ContractError), Gameplay(&'static str), InvalidState(&'static str) }
impl From<ContractError> for EncodeError { fn from(value: ContractError) -> Self { Self::Contract(value) } }
type Result<T> = core::result::Result<T, EncodeError>;

#[derive(Clone, Copy, Default)]
struct CardHandle { key: u64, row: RowHandle }

/// Reusable encoder scratch, allocated once per environment lane.
pub struct NativeEncoder {
    legacy_mode: bool,
    legacy: Option<Box<NativeEncoder>>,
    deferred_piles: crate::pile_stream::PileStream,
    team: bool,
    combat_state_len: usize,
    decision: DecisionId,
    net_id: u64,
    seat_rows: [RowHandle; MAX_SEATS],
    creature_rows: [RowHandle; u8::MAX as usize + 1],
    map_rows: [RowHandle; 256],
    map_cache: map_cache::MapCache,
    card_rows: Box<[CardHandle]>,
    card_len: usize,
    order: Vec<usize>,
    card_keys: Vec<Row>,
    preview_keys: Vec<Vec<sts2_sim::card_preview::CardPreviewVar>>,
    ui: [multiplayer::LocalUi; MAX_SEATS],
}
impl Default for NativeEncoder { fn default() -> Self { Self::new() } }
impl NativeEncoder {
    /// Read-only combat-fixture projection using the same unfiltered candidate
    /// emitter as live multiplayer. This diagnostic entry point cannot dispatch.
    pub fn encode_combat_snapshot(&mut self, combat: &Combat, characters: &[Character],
        decision: DecisionId, net_id: u64, obs: &mut NativeObsV1) -> Result<()> {
        self.combat_snapshot(combat, characters, decision, net_id, obs)?;
        if let Some(encoder) = &mut self.legacy {
            let mut old = obs.legacy.take().unwrap_or_default();
            let result = encoder.combat_snapshot(combat, characters, decision, net_id, &mut old);
            Self::publish_legacy(obs, old, result)?;
        }
        Ok(())
    }
    fn combat_snapshot(&mut self, combat: &Combat, characters: &[Character],
        decision: DecisionId, net_id: u64, obs: &mut NativeObsV1) -> Result<()> {
        self.begin(obs, decision, net_id)?;
        if characters.len() != combat.players.len() || characters.is_empty()
            || characters.len() > MAX_SEATS || usize::from(decision.actor) >= characters.len() {
            return Err(EncodeError::InvalidState("combat snapshot roster mismatch"));
        }
        let global = self.global(obs, &combat.run,
            id(Namespace::MpPhase, sts2_sim::run::mp::MpPhase::Combat as usize),
            characters.len(), Some(combat), None)?;
        let status = self.combat(obs, combat, global, true, |seat| characters[seat],
            |slot| combat.can_discard_potion_for_player(decision.actor, slot))?;
        self.finish(obs, status)?;
        Ok(())
    }
    pub fn new() -> Self { Self::with_legacy_mode(false) }
    /// Direct original V1 payload for the F−R ablation in the new lineage.
    /// Executes only the V1 encoder; no V2 rows, streams, or legacy sidecar.
    pub fn v1_compatibility() -> Self { Self::with_legacy_mode(true) }
    /// Transfer-validation only. Normal training and AI play leave this off.
    pub fn with_legacy_projection(mut self) -> Self {
        self.legacy = Some(Box::new(Self::with_legacy_mode(true)));
        self
    }
    fn with_legacy_mode(legacy_mode: bool) -> Self { Self { legacy_mode, deferred_piles: Default::default(),
        legacy: None,
        team: false, combat_state_len: 0, decision: DecisionId::default(), net_id: 0,
        seat_rows: [RowHandle::ABSENT; MAX_SEATS], creature_rows: [RowHandle::ABSENT; u8::MAX as usize + 1],
        map_rows: [RowHandle::ABSENT; 256], map_cache: Default::default(),
        card_rows: vec![CardHandle::default(); STATE_CAPACITY].into_boxed_slice(),
        card_len: 0, order: vec![0; STATE_CAPACITY], card_keys: vec![Row::default(); STATE_CAPACITY],
        preview_keys: (0..STATE_CAPACITY).map(|_| Vec::new()).collect(),
        ui: core::array::from_fn(|_| multiplayer::LocalUi::default()) } }

    fn begin(&mut self, obs: &mut NativeObsV1, decision: DecisionId, net_id: u64) -> Result<()> {
        obs.clear(decision);
        if usize::from(decision.actor) >= MAX_SEATS { return Err(ContractError::InvalidSeat.into()); }
        self.decision = decision; self.net_id = net_id; self.card_len = 0;
        self.seat_rows.fill(RowHandle::ABSENT); self.creature_rows.fill(RowHandle::ABSENT);
        self.map_rows.fill(RowHandle::ABSENT);
        self.deferred_piles.clear();
        Ok(())
    }
    fn row(obs: &mut impl RowWriter, row: Row) -> Result<RowHandle> {
        let handle = obs.push_row(row)?;
        if row.parent != RowHandle::ABSENT { obs.push_edge(Edge { from: handle, to: row.parent, relation: Relation::Parent })?; }
        if row.owner != RowHandle::ABSENT { obs.push_edge(Edge { from: handle, to: row.owner, relation: Relation::Owner })?; }
        Ok(handle)
    }
    fn action(&self, obs: &mut NativeObsV1, mut candidate: Candidate, command: Command,
        source_instance: Option<u32>, target_instance: Option<u32>, offer_instance: Option<u64>) -> Result<()> {
        candidate.action = command.action(); candidate.actor = self.decision.actor; candidate.present = true;
        obs.push_candidate(candidate, Binding { decision: self.decision, net_id: self.net_id, command,
            source_instance, target_instance, offer_instance })?;
        Ok(())
    }
    fn simple(&self, obs: &mut NativeObsV1, command: Command, source: RowHandle, legal: bool) -> Result<()> {
        self.action(obs, Candidate { source, legal, ..Candidate::default() }, command, None, None, None)
    }
    fn remember(&mut self, key: u64, row: RowHandle) -> Result<()> {
        if self.card_len == self.card_rows.len() { return Err(capacity(BufferKind::Rows, self.card_len + 1, STATE_CAPACITY)); }
        self.card_rows[self.card_len] = CardHandle { key, row }; self.card_len += 1; Ok(())
    }
    fn card_handle(&self, key: u64) -> RowHandle {
        self.card_rows[..self.card_len].iter().find(|entry| entry.key == key).map_or(RowHandle::ABSENT, |entry| entry.row)
    }
    fn global(&self, obs: &mut NativeObsV1, run: &RunCtx, phase: ContentId, seats: usize,
        combat: Option<&Combat>, boss: Option<PublicBossId>) -> Result<RowHandle> {
        let mut row = Row { kind: RowKind::Global, detail: [phase, id(Namespace::RoomType, run.room_type as usize),
            boss.map_or(ContentId::ABSENT, Into::into), ContentId::ABSENT], ..Row::default() };
        ints(&mut row.numbers, &[(0, run.act_index), (1, run.total_floor), (2, run.ascension),
            (5, as_i32(seats)?), (6, if self.team { 0 } else { i32::from(self.decision.actor) })])?;
        if let Some((x, y)) = run.coord { ints(&mut row.numbers, &[(7, x), (8, y)])?; }
        if let Some(c) = combat { ints(&mut row.numbers, &[(3, c.round), (4, c.side as i32)])?; }
        Self::row(obs, row)
    }
    fn inventory(&mut self, obs: &mut NativeObsV1, owner: usize, deck: &[Card], relics: &[Relic],
        potions: &[Option<PotionId>], capacity: usize, combat: Option<&Combat>) -> Result<()> {
        let seat = self.seat_rows[owner];
        let summary = self.pile_summary(obs, seat, PileKind::None, deck.iter().map(|card| Ok((card, card.cost))),
            relics.iter().filter(|r| usize::from(r.owner) == owner).count(), potions.iter().flatten().count())?;
        for card in deck {
            let handle = self.card(obs, card, seat, seat, PileKind::None, None, None)?;
            if !self.legacy_mode { obs.push_edge(Edge { from: summary, to: handle, relation: Relation::Member })?; }
            self.remember(deck_key(owner, card.instance_id), handle)?;
        }
        for (order, relic) in relics.iter().filter(|r| usize::from(r.owner) == owner).enumerate() {
            let meta = &RELIC_INFO[relic.id.index()];
            let mut row = Row { kind: RowKind::Relic, model: id(Namespace::RelicId, relic.id.index()),
                detail: [id(Namespace::RelicRarity, meta.rarity as usize), id(Namespace::RelicPool, meta.pool as usize),
                    ContentId::ABSENT, ContentId::ABSENT], owner: seat, parent: seat, ..Row::default() };
            ints(&mut row.numbers, &[(1, as_i32(order)?)])?;
            if let Some(counter) = relic_counter(relic, combat) { ints(&mut row.numbers, &[(0, counter)])?; }
            row.flags.set(Flag::Used, !relic.enabled);
            let handle = Self::row(obs, row)?; self.relic_parameters(obs, relic, handle, seat)?;
        }
        for slot in 0..capacity.max(potions.len()) {
            self.potion(obs, potions.get(slot).copied().flatten(), seat, seat, slot, capacity)?;
        }
        Ok(())
    }
    fn potion(&self, obs: &mut NativeObsV1, potion: Option<PotionId>, owner: RowHandle, parent: RowHandle,
        slot: usize, capacity: usize) -> Result<RowHandle> {
        let mut row = Row { kind: RowKind::Potion, owner, parent, ..Row::default() };
        ints(&mut row.numbers, &[(0, as_i32(slot)?), (1, as_i32(capacity)?)])?;
        if let Some(potion) = potion {
            let meta = &POTION_INFO[potion.index()]; row.model = id(Namespace::PotionId, potion.index());
            row.detail = [id(Namespace::PotionRarity, meta.rarity as usize), id(Namespace::PotionUsageType, meta.usage as usize),
                id(Namespace::PotionTargetType, meta.target as usize), ContentId::ABSENT];
        }
        let handle = Self::row(obs, row)?;
        if potion.is_some() { self.parameters(obs, row.model, handle, owner)?; }
        Ok(handle)
    }
    fn card_row(&self, card: &Card, owner: RowHandle, parent: RowHandle,
        pile: PileKind, order: Option<usize>, live: Option<(&Combat, KRef)>,
        preview: &[sts2_sim::card_preview::CardPreviewVar]) -> Result<Row> {
        let target = live.map_or(card.target, |(c, k)| c.card_target_type(k));
        let mut row = Row { kind: RowKind::Card, model: id(Namespace::CardId, card.id.index()),
            detail: [id(Namespace::CardType, card.card_type as usize), id(Namespace::CardRarity, card.rarity as usize),
                id(Namespace::TargetType, target as usize), id(Namespace::PileKind, pile as usize)], owner, parent,
            ..Row::default() };
        let cost = live.map_or(Ok(card.cost), |(c, k)| c.modified_card_cost_readonly(card.owner, k)).map_err(EncodeError::Gameplay)?;
        let stars = live.map_or(Ok(card.star_cost), |(c, k)| c.modified_star_cost_readonly(k)).map_err(EncodeError::Gameplay)?;
        ints(&mut row.numbers, &[(0, cost), (1, stars), (2, i32::from(card.upgraded)), (7, card.replay)])?;
        if let Some(order) = order { ints(&mut row.numbers, &[(3, as_i32(order)?)])?; }
        if self.legacy_mode {
            if let Some(damage) = card.damage {
                let damage = live.map_or(Ok(damage), |(c, k)| c.preview_damage(damage, Some(card.owner), None,
                    damage_props(card.id), Some(k))).map_err(EncodeError::Gameplay)?;
                ints(&mut row.numbers, &[(5, damage)])?;
            }
            if let Some(block) = card.block {
                let block = live.map_or(Ok(block), |(c, k)| c.preview_block(block, card.owner,
                    block_props(card.id), Some(k))).map_err(EncodeError::Gameplay)?;
                ints(&mut row.numbers, &[(6, block)])?;
            }
        } else {
            for value in preview {
                if value.damage { ints(&mut row.numbers, &[(5, value.value)])?; }
                if value.block { ints(&mut row.numbers, &[(6, value.value)])?; }
            }
        }
        for (flag, value) in [(Flag::Upgraded, card.upgraded), (Flag::XCost, card.x_cost), (Flag::StarXCost, card.star_x),
            (Flag::Retained, card.should_retain_this_turn()), (Flag::Exhausts, card.exhausts()),
            (Flag::Ethereal, card.is_ethereal()), (Flag::Innate, card.is_innate()), (Flag::Sly, card.is_sly()),
            (Flag::Eternal, card.has_kw(KW_ETERNAL))] { row.flags.set(flag, value); }
        Ok(row)
    }
    fn card(&self, obs: &mut impl RowWriter, card: &Card, owner: RowHandle, parent: RowHandle,
        pile: PileKind, order: Option<usize>, live: Option<(&Combat, KRef)>) -> Result<RowHandle> {
        let preview = if self.legacy_mode { Vec::new() } else if let Some((c, k)) = live {
            c.card_preview(k, None).map_err(EncodeError::Gameplay)?
        } else { card.native_card_preview().map_err(EncodeError::Gameplay)? };
        let handle = self.card_from_row(obs, card, self.card_row(card, owner, parent, pile, order, live, &preview)?)?;
        if !self.legacy_mode {
            self.display_variables(obs, card.id, handle, owner, None, &preview,
                live.map_or((None, None), |(c, k)| c.card_preview_x(k)))?;
        }
        Ok(handle)
    }
    fn card_from_row(&self, obs: &mut impl RowWriter, card: &Card, row: Row) -> Result<RowHandle> {
        let owner = row.owner; let handle = Self::row(obs, row)?;
        for &(enchantment, amount) in &card.enchantments {
            self.modifier(obs, owner, handle, enchantment.into(), ContentId::ABSENT, amount)?;
        }
        if let Some(affliction) = card.cv.affliction {
            self.modifier(obs, owner, handle, id(Namespace::CardAffliction, affliction as usize),
                ContentId::ABSENT, card.observation_affliction_amount())?;
        }
        // Retained native vars only. No synthetic Shiv variables or FranticEscape provenance.
        for &(key, amount) in &card.vars {
            self.modifier(obs, owner, handle, ContentId::ABSENT, id(Namespace::VarKey, key.index()), amount)?;
        }
        for tag in CardTag::ALL {
            if card.tags & (1 << tag.index()) != 0 {
                self.modifier(obs, owner, handle, id(Namespace::CardTag, tag.index()), ContentId::ABSENT, 1)?;
            }
        }
        Ok(handle)
    }
    fn modifier(&self, obs: &mut impl RowWriter, owner: RowHandle, parent: RowHandle,
        model: ContentId, variable: ContentId, amount: i32) -> Result<RowHandle> {
        let mut row = Row { kind: RowKind::Modifier, model, detail: [variable, ContentId::ABSENT,
            ContentId::ABSENT, ContentId::ABSENT], owner, parent, ..Row::default() };
        ints(&mut row.numbers, &[(0, amount)])?; Self::row(obs, row)
    }
    fn map(&mut self, obs: &mut NativeObsV1, map: &ActMap, act: i32, current: Option<(i32, i32)>,
        reachable: impl Fn(usize) -> bool, visited: impl Fn(usize) -> Option<usize>) -> Result<()> {
        if map.points.len() > self.map_rows.len() { return Err(capacity(BufferKind::Rows, map.points.len(), self.map_rows.len())); }
        self.map_cache.prepare(map);
        let len = map.points.len();
        let mut reachable_now = [false; 256];
        for at in 0..len {
            let index = self.map_cache.order[at]; let point = &map.points[index];
            if !(if self.legacy_mode { self.map_cache.visible[index] } else { self.map_cache.canonical[index] }) { continue; }
            let mut row = Row { kind: RowKind::MapNode, detail: [ContentId::ABSENT,
                id(Namespace::MapPointType, point.point_type as usize), ContentId::ABSENT, ContentId::ABSENT], ..Row::default() };
            ints(&mut row.numbers, &[(0, point.coord.col), (1, point.coord.row), (2, act)])?;
            reachable_now[index] = reachable(index);
            row.flags.set(Flag::Revealed, true); row.flags.set(Flag::Reachable, reachable_now[index]);
            row.flags.set(Flag::Current, current == Some((point.coord.col, point.coord.row)));
            let visit = visited(index); row.flags.set(Flag::Visited, visit.is_some());
            if let Some(order) = visit { ints(&mut row.numbers, &[(3, as_i32(order)?)])?; }
            self.map_rows[index] = Self::row(obs, row)?;
        }
        for from in 0..len {
            let index = self.map_cache.order[from]; let source = self.map_rows[index];
            if source == RowHandle::ABSENT { continue; }
            for &child in &self.map_cache.children[index] {
                let target = self.map_rows[usize::from(child)];
                if target != RowHandle::ABSENT {
                    obs.push_edge(Edge { from: source, to: target, relation: Relation::MapConnection })?;
                }
            }
        }
        for index in 0..len {
            if reachable_now[index] { self.route_summary(obs, map, index)?; }
        }
        Ok(())
    }

    fn combat(&mut self, obs: &mut NativeObsV1, c: &Combat, global: RowHandle, multi: bool,
        character: impl Fn(usize) -> Character, can_discard: impl Fn(usize) -> bool) -> Result<DecisionStatus> {
        let mut deferred = core::mem::take(&mut self.deferred_piles);
        let actor = self.decision.actor; let actor_index = usize::from(actor);
        if !c.players.is_player(actor) { return Err(ContractError::InvalidSeat.into()); }
        if c.creatures.len() > self.creature_rows.len() { return Err(capacity(BufferKind::Rows, c.creatures.len(), self.creature_rows.len())); }
        for owner in 0..c.players.len() {
            let p = &c.players[owner as u8]; let cr = c.cr(owner as u8);
            let mut row = Row { kind: RowKind::Seat, detail: [id(Namespace::Character, character(owner) as usize),
                ContentId::ABSENT, ContentId::ABSENT, ContentId::ABSENT], parent: global, ..Row::default() };
            ints(&mut row.numbers, &[(0, as_i32(owner)?), (1, as_i32((owner + c.players.len() - if self.team { 0 } else { actor_index }) % c.players.len())?),
                (2, cr.hp), (3, cr.max_hp), (4, cr.block), (5, p.energy), (6, p.base_max_energy), (7, p.stars),
                (10, as_i32(p.hand.len())?), (11, as_i32(p.draw.len())?), (12, as_i32(p.discard.len())?),
                (13, as_i32(p.exhaust.len())?), (14, p.turn_number)])?;
            if let Some(gold) = if owner == 0 { Some(c.run.gold) } else { p.gold } { ints(&mut row.numbers, &[(8, gold)])?; }
            if owner == 0 { ints(&mut row.numbers, &[(9, c.run.max_potion_slots)])?; }
            else if let Some(potions) = &p.potions { ints(&mut row.numbers, &[(9, as_i32(potions.len())?)])?; }
            row.flags.set(Flag::Alive, cr.is_alive()); row.flags.set(Flag::Participating, c.is_player_taking_turn(owner as u8));
            row.flags.set(Flag::Ready, c.is_player_ready_to_end_turn(owner as u8));
            self.seat_rows[owner] = Self::row(obs, row)?;
        }
        for index in 0..c.creatures.len() {
            let cr = &c.creatures[index]; if !cr.in_combat && !cr.is_player { continue; }
            let mut row = Row { kind: RowKind::Creature, parent: global, ..Row::default() };
            if let Some(ai) = &cr.ai { row.model = id(Namespace::MonsterKind, ai.kind.index()); }
            row.detail[0] = id(Namespace::Side, cr.side as usize);
            row.owner = if cr.is_player { self.seat_rows[index] }
                else { cr.pet_owner.map_or(RowHandle::ABSENT, |owner| self.seat_rows[usize::from(owner)]) };
            ints(&mut row.numbers, &[(0, cr.hp), (1, cr.max_hp), (2, cr.block)])?;
            row.flags.set(Flag::Alive, cr.is_alive()); row.flags.set(Flag::Pet, cr.is_pet);
            row.flags.set(Flag::Targetable, if cr.side == Side::Enemy { c.can_hit(index as u8) } else { cr.is_alive() });
            if let Some(ai) = &cr.ai && let Some(next) = ai.peek_move() {
                if let Some(intent) = ai.intents(next).first() { row.detail[1] = id(Namespace::IntentType, intent.kind.index()); }
            }
            self.creature_rows[index] = Self::row(obs, row)?;
        }
        for index in 0..c.creatures.len() {
            let parent = self.creature_rows[index]; if parent == RowHandle::ABSENT { continue; }
            let cr = &c.creatures[index];
            if let Some(ai) = &cr.ai && let Some(next) = ai.peek_move() {
                for at in 0..ai.intents(next).len() {
                    let intent = c.public_intent(index as u8, if self.team { 0 } else { actor }, at).map_err(EncodeError::Gameplay)?
                        .ok_or(EncodeError::InvalidState("displayed intent missing"))?;
                    let mut row = Row { kind: RowKind::Intent, model: id(Namespace::IntentType, intent.kind.index()),
                        parent, owner: parent, ..Row::default() };
                    row.detail[0] = row.model;
                    ints(&mut row.numbers, &[(3, as_i32(at)?)])?;
                    if matches!(intent.kind, IntentType::Attack | IntentType::MultiAttack | IntentType::DeathBlow) {
                        ints(&mut row.numbers, &[(0, intent.damage), (1, intent.hits)])?;
                    }
                    if intent.kind == IntentType::StatusCard { ints(&mut row.numbers, &[(2, intent.cards)])?; }
                    Self::row(obs, row)?;
                }
            }
            for power_ref in cr.power_list() {
                let power = &c.powers[usize::from(power_ref)]; let meta = power_meta(power.id);
                if !meta.visible { continue; }
                let mut row = Row { kind: RowKind::Power, model: id(Namespace::PowerId, power.id.index()),
                    detail: [id(Namespace::PowerType, meta.power_type as usize), id(Namespace::PowerStackType, meta.stack_type as usize),
                        ContentId::ABSENT, ContentId::ABSENT], parent, owner: parent, ..Row::default() };
                ints(&mut row.numbers, &[(0, power.amount), (1, power_counter(c, power_ref))])?;
                let handle = Self::row(obs, row)?;
                if let Some(target) = power.target { let target = self.creature_rows[usize::from(target)];
                    if target != RowHandle::ABSENT { obs.push_edge(Edge { from: handle, to: target, relation: Relation::Target })?; } }
            }
        }
        for owner in 0..c.players.len() {
            let p = &c.players[owner as u8]; let seat = self.seat_rows[owner];
            let deck = if owner == 0 { &c.run.deck } else { &p.persistent_deck };
            let potions = c.potion_slots_for_player(owner as u8).unwrap_or(&[]);
            let slots = if owner == 0 { c.run.max_potion_slots.max(0) as usize } else { potions.len() };
            self.inventory(obs, owner, deck, &c.run.relics, potions, slots, Some(c))?;
            for (order, &orb) in p.orbs.orbs.iter().enumerate() {
                use sts2_sim::orbs::OrbKind;
                let kind = match orb.kind { OrbKind::Lightning => OrbType::Lightning, OrbKind::Frost => OrbType::Frost,
                    OrbKind::Dark => OrbType::Dark, OrbKind::Plasma => OrbType::Plasma, OrbKind::Glass => OrbType::Glass };
                let mut row = Row { kind: RowKind::Orb, model: id(Namespace::OrbType, kind.index()), owner: seat, parent: seat, ..Row::default() };
                ints(&mut row.numbers, &[(0, as_i32(order)?), (1, c.orb_passive_value(owner as u8, orb)),
                    (2, c.orb_evoke_value(owner as u8, orb))])?; Self::row(obs, row)?;
            }
            if owner != actor_index && !self.team { continue; }
            for (pile, cards, ordered) in [(PileKind::Hand, p.hand.as_slice(), true), (PileKind::Draw, p.draw.as_slice(), false),
                (PileKind::Discard, p.discard.as_slice(), false), (PileKind::Exhaust, p.exhaust.as_slice(), false),
                (PileKind::Play, p.play.as_slice(), false)] {
                let summary = self.pile_summary(obs, seat, pile, cards.iter().map(|&k|
                    c.modified_card_cost_readonly(owner as u8, k).map(|cost| (&c.cards[usize::from(k)], cost))
                        .map_err(EncodeError::Gameplay)), 0, 0)?;
                if cards.len() > self.order.len() {
                    self.order.resize(cards.len(), 0); self.card_keys.resize(cards.len(), Row::default());
                    self.preview_keys.resize_with(cards.len(), Vec::new);
                }
                for (at, &k) in cards.iter().enumerate() {
                    self.order[at] = at;
                    if !ordered {
                        self.preview_keys[at].clear();
                        if !self.legacy_mode {
                            c.card_preview_into(k, None, &mut self.preview_keys[at]).map_err(EncodeError::Gameplay)?;
                        }
                        self.card_keys[at] = self.card_row(&c.cards[usize::from(k)], seat, seat, pile, None,
                            Some((c, k)), &self.preview_keys[at])?;
                    }
                }
                if !ordered {
                    let keys = &self.card_keys;
                    let previews = &self.preview_keys;
                    self.order[..cards.len()].sort_unstable_by(|&a, &b| public_row_cmp(&keys[a], &keys[b])
                        .then_with(|| public_card_cmp(&c.cards[usize::from(cards[a])], &c.cards[usize::from(cards[b])]))
                        .then_with(|| public_preview_cmp(&previews[a], &previews[b])));
                }
                let mut at = 0;
                while at < cards.len() {
                    let k = cards[self.order[at]]; let card = &c.cards[usize::from(k)];
                    if !self.legacy_mode && !ordered {
                        let mut end = at + 1;
                        while end < cards.len() {
                            let next = cards[self.order[end]];
                            if public_row_cmp(&self.card_keys[self.order[at]], &self.card_keys[self.order[end]]).is_ne()
                                || public_card_cmp(card, &c.cards[usize::from(next)]).is_ne()
                                || public_preview_cmp(&self.preview_keys[self.order[at]], &self.preview_keys[self.order[end]]).is_ne() { break; }
                            end += 1;
                        }
                        if !obs.checking_binding() {
                            self.deferred_card(&mut deferred, card, self.card_keys[self.order[at]],
                                &self.preview_keys[self.order[at]], c.card_preview_x(k), summary,
                                u32::try_from(end - at).map_err(|_| ContractError::InvalidNumericField)?)?;
                        }
                        at = end; continue;
                    }
                    let handle = if ordered { self.card(obs, card, seat, seat, pile, Some(at), Some((c, k)))? }
                        else {
                            let handle = self.card_from_row(obs, card, self.card_keys[self.order[at]])?;
                            if !self.legacy_mode {
                                self.display_variables(obs, card.id, handle, seat, None,
                                    &self.preview_keys[self.order[at]], c.card_preview_x(k))?;
                            }
                            handle
                        };
                    self.remember(arena_key(k), handle)?;
                    if !self.legacy_mode { obs.push_edge(Edge { from: summary, to: handle, relation: Relation::Member })?; }
                    at += 1;
                }
            }
        }
        self.deferred_piles = deferred;
        self.combat_state_len = obs.lengths().0;
        if c.fault.is_some() { return Ok(DecisionStatus::GameplayFault); }
        if c.is_over { return Ok(DecisionStatus::Terminal); }
        if let Some(choice) = c.pending_choice_for_player(actor) {
            let options = c.choice_options_for_player(actor).ok_or(EncodeError::InvalidState("choice options missing"))?;
            let selected = c.choice_selected_for_player(actor).ok_or(EncodeError::InvalidState("choice selection missing"))?;
            let (model, purpose) = match choice.resolver.src {
                ChoiceSrc::Card(card) => (id(Namespace::CardId, card.index()), ChoicePurpose::ChoiceSrcCard),
                ChoiceSrc::Potion(potion) => (id(Namespace::PotionId, potion.index()), ChoicePurpose::ChoiceSrcPotion),
                ChoiceSrc::Relic(relic) => (id(Namespace::RelicId, relic.index()), ChoicePurpose::ChoiceSrcRelic),
                ChoiceSrc::Monster(monster) => (id(Namespace::MonsterKind, monster.index()), ChoicePurpose::ChoiceSrcMonster),
            };
            let ordered = choice.source == PileKind::Hand || matches!(choice.resolver.src, ChoiceSrc::Card(CardId::Stratagem));
            let confirm = if choice.is_multi() { selected.len() >= usize::from(choice.min) || choice.allow_skip && selected.is_empty() }
                else { choice.allow_skip };
            let mut row = Row { kind: RowKind::Choice, model, detail: [purpose.into(), ContentId::ABSENT,
                id(Namespace::PileKind, choice.source as usize), ContentId::ABSENT], owner: self.seat_rows[actor_index], ..Row::default() };
            ints(&mut row.numbers, &[(0, i32::from(choice.resolver.step)), (1, i32::from(choice.min)),
                (2, i32::from(choice.max)), (3, as_i32(selected.len())?), (4, i32::from(actor))])?;
            for (flag, value) in [(Flag::Optional, choice.allow_skip), (Flag::SingleSelect, choice.single),
                (Flag::Ordered, ordered), (Flag::CanConfirm, confirm)] { row.flags.set(flag, value); }
            let parent = Self::row(obs, row)?;
            for (index, &k) in options.iter().enumerate() {
                let card = c.cards.get(usize::from(k)).ok_or(EncodeError::InvalidState("choice card missing"))?;
                let mut source = self.card_handle(arena_key(k));
                if source == RowHandle::ABSENT {
                    source = self.card(obs, card, self.seat_rows[actor_index], parent, choice.source, None, Some((c, k)))?;
                    self.remember(arena_key(k), source)?;
                }
                let selected_at = selected.iter().position(|&s| usize::from(s) == index);
                let mut row = Row { kind: RowKind::ChoiceOption, model: id(Namespace::CardId, card.id.index()),
                    owner: self.seat_rows[actor_index], parent, ..Row::default() };
                ints(&mut row.numbers, &[(0, as_i32(index)?)])?; row.flags.set(Flag::Selected, selected_at.is_some());
                if let Some(at) = selected_at { let retrieval = if ordered { at } else { selected.iter().filter(|&&s| usize::from(s) < index).count() };
                    ints(&mut row.numbers, &[(1, as_i32(at)?), (2, as_i32(retrieval)?)])?; }
                let option = Self::row(obs, row)?; obs.push_edge(Edge { from: option, to: source, relation: Relation::Source })?;
                let command = if multi { Command::MpResolveCombatChoice { index: Some(index as u32) } }
                    else { Command::Solo(SoloCommand::Choose { index: index as u32 }) };
                let legal = selected_at.is_some() || !choice.is_multi() || selected.len() < usize::from(choice.max);
                self.action(obs, Candidate { model: row.model, source, target: option, flags: row.flags, legal, ..Candidate::default() },
                    command, Some(card.instance_id), None, Some(index as u64))?;
            }
            let command = if multi { Command::MpResolveCombatChoice { index: None } } else { Command::Solo(SoloCommand::ConfirmChoice) };
            if confirm { self.simple(obs, command, parent, true)?; }
            return Ok(DecisionStatus::Actionable);
        }
        if multi && c.needs_enemy_turn_acknowledgement(actor) {
            self.simple(obs, Command::MpReadyForEnemyTurn, self.seat_rows[actor_index], true)?;
            return Ok(DecisionStatus::Actionable);
        }
        if self.team && c.accepts_combat_play_requests() && c.cr(actor).is_alive()
            && c.is_player_ready_to_end_turn(actor) {
            self.combat_potions(obs, c, multi, &can_discard, true)?;
            return Ok(if obs.lengths().1 > 0 { DecisionStatus::Actionable } else { DecisionStatus::Waiting });
        }
        if c.side != Side::Player || c.is_ending || !c.cr(actor).is_alive()
            || !c.is_player_taking_turn(actor) || c.is_player_ready_to_end_turn(actor) || (!self.team && c.pending_choice.is_some()) {
            return Ok(DecisionStatus::Waiting);
        }
        for (hand, &k) in c.players[actor].hand.iter().enumerate() {
            let card = &c.cards[usize::from(k)];
            let playable = c.can_play_card_readonly(k).map_err(EncodeError::Gameplay)?;
            let target_type = c.card_target_type(k); let source = self.card_handle(arena_key(k));
            let count = match target_type { TargetType::AnyEnemy => c.enemies.len(), TargetType::AnyAlly => c.player_side().len(), _ => 1 };
            for index in 0..count {
                let recipient = match target_type { TargetType::AnyEnemy => Some(c.enemies[index]), TargetType::AnyAlly => Some(c.player_side()[index]), _ => None };
                let legal = playable && recipient.is_none_or(|recipient| if target_type == TargetType::AnyEnemy { c.can_hit(recipient) } else { c.cr(recipient).is_alive() });
                let target_arg = recipient.map(|_| index as u16);
                let command = if multi { Command::MpPlayCard { hand: hand as u32, target: target_arg } }
                    else { Command::Solo(SoloCommand::PlayCard { hand: hand as u32, target: target_arg }) };
                let mut candidate = Candidate { model: id(Namespace::CardId, card.id.index()), source,
                    target: recipient.map_or(RowHandle::ABSENT, |target| self.creature_rows[usize::from(target)]), legal, ..Candidate::default() };
                ints(&mut candidate.numbers, &[(0, c.modified_card_cost_readonly(actor, k).map_err(EncodeError::Gameplay)?),
                    (1, c.modified_star_cost_readonly(k).map_err(EncodeError::Gameplay)?)])?;
                if self.legacy_mode {
                    if let Some(damage) = card.damage {
                        let damage = c.preview_damage(damage, Some(actor), recipient, damage_props(card.id), Some(k))
                            .map_err(EncodeError::Gameplay)?;
                        ints(&mut candidate.numbers, &[(11, damage)])?;
                    }
                    if let Some(block) = card.block {
                        let block = c.preview_block(block, actor, block_props(card.id), Some(k)).map_err(EncodeError::Gameplay)?;
                        ints(&mut candidate.numbers, &[(12, block)])?;
                    }
                } else {
                    let preview = c.card_preview(k, recipient).map_err(EncodeError::Gameplay)?;
                    for value in &preview {
                        if value.damage { ints(&mut candidate.numbers, &[(11, value.value)])?; }
                        if value.block { ints(&mut candidate.numbers, &[(12, value.value)])?; }
                    }
                    if let Some(recipient) = recipient {
                        self.display_variables(obs, card.id, source, self.seat_rows[actor_index],
                            Some(self.creature_rows[usize::from(recipient)]), &preview, c.card_preview_x(k))?;
                    }
                }
                self.action(obs, candidate, command, Some(card.instance_id), recipient.map(u32::from), None)?;
            }
        }
        self.combat_potions(obs, c, multi, &can_discard, false)?;
        self.simple(obs, if multi { Command::MpEndTurn } else { Command::Solo(SoloCommand::EndTurn) }, self.seat_rows[actor_index], true)?;
        Ok(DecisionStatus::Actionable)
    }
    fn combat_potions(&self, obs: &mut NativeObsV1, c: &Combat, multi: bool,
        can_discard: &impl Fn(usize) -> bool, any_time_only: bool) -> Result<()> {
        let actor = self.decision.actor; let actor_index = actor as usize;
        for (slot, potion) in c.potion_slots_for_player(actor).unwrap_or(&[]).iter().copied().enumerate() {
            let Some(potion) = potion else { continue; }; let info = &POTION_INFO[potion.index()];
            if any_time_only && info.usage != PotionUsageType::AnyTime { continue; }
            if !any_time_only {
                self.action(obs, Candidate { model: id(Namespace::PotionId, potion.index()),
                    source: self.seat_rows[actor_index], legal: can_discard(slot), ..Candidate::default() },
                    if multi { Command::MpDiscardPotion { slot: slot as u32 } }
                    else { Command::Solo(SoloCommand::DiscardPotion { slot: slot as u32 }) },
                    None, None, Some(slot as u64))?;
            }
            let count = match info.target { PotionTargetType::AnyEnemy => c.enemies.len(), PotionTargetType::AnyPlayer => c.players.len(), _ => 1 };
            for index in 0..count {
                let argument = match info.target { PotionTargetType::AnyEnemy | PotionTargetType::AnyPlayer => Some(index), _ => None };
                let recipient = match info.target { PotionTargetType::AnyEnemy => Some(c.enemies[index]), PotionTargetType::AnyPlayer => Some(index as u8), _ => None };
                let legal = c.can_use_potion_for_player(actor, slot, argument);
                if any_time_only && !legal { continue; }
                let command = if multi { Command::MpUsePotion { slot: slot as u32, target: argument.map(|i| i as u16) } }
                    else { Command::Solo(SoloCommand::UsePotion { slot: slot as u32,
                        target: match info.target { PotionTargetType::AnyPlayer => PotionRecipient::Player(index as u8),
                            PotionTargetType::AnyEnemy => PotionRecipient::Creature(index as u16), _ => PotionRecipient::None } }) };
                self.action(obs, Candidate { model: id(Namespace::PotionId, potion.index()), source: self.seat_rows[actor_index],
                    target: recipient.map_or(RowHandle::ABSENT, |target| self.creature_rows[usize::from(target)]),
                    legal, ..Candidate::default() }, command, None, recipient.map(u32::from), Some(slot as u64))?;
            }
        }
        Ok(())
    }
}

// Preserve the original V1 tooltip lanes for the transferred residual branch.
fn damage_props(card: CardId) -> Props {
    match card {
        CardId::Burn | CardId::Decay | CardId::Infection | CardId::Toxic | CardId::Wither => Props::UNPOWERED.or(Props::MOVE),
        CardId::CaptureSpirit => Props::UNPOWERED.or(Props::UNBLOCKABLE).or(Props::MOVE),
        CardId::Cacophony => Props::UNPOWERED,
        _ => Props::MOVE,
    }
}
fn block_props(card: CardId) -> Props {
    match card { CardId::PillarOfCreation | CardId::Shroud => Props::UNPOWERED, _ => Props::MOVE }
}

fn event_offer_model(offer: Option<sts2_sim::run::NativeEventOffer>) -> Option<ContentId> {
    use sts2_sim::run::NativeEventOffer as O;
    offer.map(|offer| match offer {
        O::Relic(relic) => id(Namespace::RelicId, relic.index()),
        O::Potion(potion) => id(Namespace::PotionId, potion.index()),
        O::Pool(character) => id(Namespace::Character, character as usize),
    })
}
impl NativeEncoder {
    fn event_offer_detail(&self, obs: &mut NativeObsV1, parent: RowHandle,
        detail: (Option<CardId>, Option<CardType>)) -> Result<()> {
        for model in [detail.0.map(|card| id(Namespace::CardId, card.index())),
            detail.1.map(|kind| id(Namespace::CardType, kind as usize))].into_iter().flatten() {
            Self::row(obs, Row { kind: RowKind::EventOption, model, parent,
                owner: self.seat_rows[usize::from(self.decision.actor)], ..Row::default() })?;
        }
        Ok(())
    }
}
fn id(namespace: Namespace, index: usize) -> ContentId { ContentId::from_index(namespace, index).expect("generated native registry ordinal") }
fn as_i32(value: usize) -> Result<i32> { i32::try_from(value).map_err(|_| ContractError::InvalidNumericField.into()) }
fn ints(numbers: &mut Numbers, fields: &[(usize, i32)]) -> Result<()> {
    for &(lane, value) in fields { numbers.set_integer(lane, value)?; } Ok(())
}
fn capacity(buffer: BufferKind, needed: usize, capacity: usize) -> EncodeError { ContractError::Capacity { buffer, needed, capacity }.into() }
fn arena_key(k: KRef) -> u64 { u64::from(k) }
fn deck_key(owner: usize, instance: u32) -> u64 { (1u64 << 63) | ((owner as u64) << 32) | u64::from(instance) }
fn public_card_cmp(a: &Card, b: &Card) -> core::cmp::Ordering {
    a.enchantments.as_slice().cmp(b.enchantments.as_slice())
        .then_with(|| a.vars.iter().map(|(key, value)| (key.index(), value))
            .cmp(b.vars.iter().map(|(key, value)| (key.index(), value))))
        .then_with(|| a.tags.cmp(&b.tags))
        .then_with(|| (a.cv.affliction.map(|v| v as u8), a.observation_affliction_amount())
            .cmp(&(b.cv.affliction.map(|v| v as u8), b.observation_affliction_amount())))
}
fn public_row_cmp(a: &Row, b: &Row) -> core::cmp::Ordering {
    (a.model.value, a.detail.map(|id| (id.namespace as u16, id.value)),
        a.flags.present, a.flags.value, a.numbers.integer_present, a.numbers.integers)
        .cmp(&(b.model.value, b.detail.map(|id| (id.namespace as u16, id.value)),
            b.flags.present, b.flags.value, b.numbers.integer_present, b.numbers.integers))
        .then_with(|| a.numbers.float_present.cmp(&b.numbers.float_present))
        .then_with(|| a.numbers.floats.map(f32::to_bits).cmp(&b.numbers.floats.map(f32::to_bits)))
}
fn public_preview_cmp(a: &[sts2_sim::card_preview::CardPreviewVar],
    b: &[sts2_sim::card_preview::CardPreviewVar]) -> core::cmp::Ordering {
    a.iter().map(|v| (v.name, v.value, v.enchanted))
        .cmp(b.iter().map(|v| (v.name, v.value, v.enchanted)))
}
fn power_counter(c: &Combat, reference: sts2_sim::creature::PRef) -> i32 {
    let p = &c.powers[usize::from(reference)];
    // Audited Models/Powers/*Power.cs DisplayAmount overrides, capture 0861BFA1.
    match p.id {
        PowerId::Automation | PowerId::Monologue | PowerId::Sloth | PowerId::Tender | PowerId::WitheringPresence => p.vars[0],
        PowerId::Cacophony => (33 - p.vars[0]).max(0),
        PowerId::Feral | PowerId::HardenedShell => (p.amount - p.vars[0]).max(0),
        PowerId::Orbit => 4 - p.vars[0].rem_euclid(4),
        PowerId::PaleBlueDot => (5 - c.hist.play_start_cards.iter().filter(|&&k| {
            let card = &c.cards[usize::from(k)]; card.owner == p.owner && card.card_type == CardType::Attack }).count() as i32).max(0),
        PowerId::Panache => (5 - p.vars[0]).max(0), PowerId::Slow => p.vars[0] * 10,
        PowerId::TagTeam => 1, _ => p.amount,
    }
}
pub(crate) fn relic_counter(relic: &Relic, combat: Option<&Combat>) -> Option<i32> {
    // Original Models/Relics/* ShowCounter/DisplayAmount, decision boundaries
    // only (the C# animation-only IsActivating value is not a policy decision).
    let n = relic.counter.max(0);
    match relic.id {
        RelicId::HappyFlower | RelicId::Pendulum | RelicId::FakeHappyFlower | RelicId::PollinousCore | RelicId::Girya => Some(relic.counter.max(0)),
        RelicId::EmberTea => Some((5 - relic.counter).max(0)),
        RelicId::SilverCrucible | RelicId::WingedBoots => (n < 3).then_some(3 - n),
        RelicId::FishingRod => Some(n % 3),
        RelicId::BookOfFiveRings | RelicId::JossPaper => Some(n % 5),
        RelicId::GalacticDust | RelicId::Nunchaku | RelicId::PenNib => Some(n % 10),
        RelicId::TuningFork | RelicId::PumpkinCandle | RelicId::SwordOfStone => Some(n),
        RelicId::LastingCandy | RelicId::PaelsWing => Some(n % 2),
        RelicId::WongosMysteryTicket => (n < 5).then_some(5 - n),
        RelicId::ToyBox => (n < 15).then_some(n % 3),
        RelicId::PaelsTooth => (n > 0).then_some(n),
        RelicId::BrilliantScarf => combat.and_then(|_| (n < 5).then_some(n)),
        RelicId::Metronome => combat.and_then(|_| (n < 7).then_some(n)),
        RelicId::PaelsLegion => combat.and_then(|_| (n > 0).then_some(n)),
        RelicId::Pocketwatch | RelicId::VelvetChoker => combat.map(|_| n),
        RelicId::PaelsFlesh => combat.map(|c| &c.players[relic.owner])
            .and_then(|player| (player.turn_number < 3).then_some(player.turn_number as i32)),
        RelicId::StoneCalendar => combat.map(|c| &c.players[relic.owner])
            .and_then(|player| (player.turn_number < 7).then_some(player.turn_number as i32)),
        RelicId::Kusarigama | RelicId::Kunai | RelicId::OrnamentalFan |
        RelicId::Shuriken | RelicId::LetterOpener => combat.map(|_| n % 3),
        RelicId::IronClub => Some(n % 4),
        _ => None,
    }
}
