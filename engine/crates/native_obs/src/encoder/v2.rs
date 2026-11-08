use super::*;

#[derive(Clone, Copy)]
enum PublicAmount { Integer(i32), Float(f32) }
include!(concat!(env!("OUT_DIR"), "/public_vars.rs"));

impl NativeEncoder {
    fn parameter(&self, obs: &mut NativeObsV1, parent: RowHandle, owner: RowHandle,
        name: &str, amount: PublicAmount, effect: ContentId) -> Result<RowHandle> {
        let mut row = Row { kind: RowKind::EffectParameter,
            model: public_variable(name).ok_or(EncodeError::InvalidState("unknown public parameter"))?,
            detail: [effect, ContentId::ABSENT, ContentId::ABSENT, ContentId::ABSENT], parent, owner, ..Row::default() };
        match amount { PublicAmount::Integer(n) => row.numbers.set_integer(0, n)?,
            PublicAmount::Float(n) => row.numbers.set_float(0, n)? }
        Self::row(obs, row)
    }

    pub(super) fn parameters(&self, obs: &mut NativeObsV1, model: ContentId, parent: RowHandle, owner: RowHandle) -> Result<()> {
        if self.legacy_mode { return Ok(()); }
        for &(name, amount) in model_parameters(model) {
            self.parameter(obs, parent, owner, name, amount, ContentId::ABSENT)?;
        }
        Ok(())
    }

    pub(super) fn relic_parameters(&self, obs: &mut NativeObsV1, relic: &Relic, parent: RowHandle, owner: RowHandle) -> Result<()> {
        if self.legacy_mode { return Ok(()); }
        let model = id(Namespace::RelicId, relic.id.index());
        for &(name, amount) in model_parameters(model) {
            let amount = match (relic.id, name) {
                (RelicId::WingedBoots, "Rooms") => PublicAmount::Integer((3 - relic.counter).max(0)),
                (RelicId::WongosMysteryTicket, "RemainingCombats") => PublicAmount::Integer((5 - relic.counter).max(0)),
                _ => amount,
            };
            self.parameter(obs, parent, owner, name, amount, ContentId::ABSENT)?;
        }
        let remaining = match relic.id { RelicId::BoneTea | RelicId::TeaOfDiscourtesy => Some(1 - relic.counter),
            RelicId::EmberTea => Some(5 - relic.counter), _ => None };
        if let Some(n) = remaining { self.parameter(obs, parent, owner, "Combats", PublicAmount::Integer(n.max(0)), ContentId::ABSENT)?; }
        Ok(())
    }

    pub(super) fn offered_relic(&self, obs: &mut NativeObsV1, relic: RelicId, parent: RowHandle, owner: RowHandle) -> Result<RowHandle> {
        if self.legacy_mode { return Ok(RowHandle::ABSENT); }
        let meta = &RELIC_INFO[relic.index()];
        let row = Self::row(obs, Row { kind: RowKind::Relic, model: id(Namespace::RelicId, relic.index()),
            detail: [id(Namespace::RelicRarity, meta.rarity as usize), id(Namespace::RelicPool, meta.pool as usize),
                ContentId::ABSENT, ContentId::ABSENT], parent, owner,
            legacy_role: LegacyRowRole::V2Only, ..Row::default() })?;
        self.relic_parameters(obs, &Relic::new(relic), row, owner)?;
        Ok(row)
    }

    pub(super) fn rest_parameters(&self, obs: &mut NativeObsV1, parent: RowHandle, owner: RowHandle,
        kind: sts2_sim::run::RestOptionKind, heal: i32, deck: &[Card], relics: &[Relic]) -> Result<()> {
        if self.legacy_mode { return Ok(()); }
        use sts2_sim::run::RestOptionKind as K;
        let effect = ContentId::ABSENT;
        let mut add = |name, n| self.parameter(obs, parent, owner, name, PublicAmount::Integer(n), effect).map(|_| ());
        match kind {
            K::Heal => { add("Heal", heal)?;
                if relics.iter().any(|r| r.id == RelicId::StoneHumidifier) { add("MaxHpGain", 5)?; }
                if relics.iter().any(|r| r.id == RelicId::TinyMailbox) { add("PotionsGain", 2)?; }
                if relics.iter().any(|r| r.id == RelicId::DreamCatcher && r.enabled) { add("CardsGain", 1)?; }
            }
            K::Smith => add("CardsUpgrade", 1)?,
            K::Dig | K::Hatch => add("RelicsGain", 1)?,
            K::Lift => {
                add("StrengthGain", 1)?;
                if let Some(r) = relics.iter().find(|r| r.id == RelicId::Girya) { add("LiftsLeft", (3 - r.counter).max(0))?; }
            }
            K::Cook => { add("CardsRemove", 2)?; add("MaxHpGain", 9)?; }
            K::Clone => add("CardsDuplicate", as_i32(deck.iter().filter(|c|
                c.enchantments.iter().any(|&(id, _)| id == EnchantmentId::Clone)).count())?)?,
            K::Kindle => add("Combats", 5)?,
            K::Mend => {},
        }
        Ok(())
    }

    pub(super) fn event_parameters(&self, obs: &mut NativeObsV1, parent: RowHandle, owner: RowHandle,
        parameters: &[sts2_sim::run::event_preview::PublicEventParameter]) -> Result<()> {
        if self.legacy_mode { return Ok(()); }
        use sts2_sim::run::event_preview::PublicEventSubject as S;
        for value in parameters {
            let effect = match value.variable {
                "HpCost" => Some(EffectId::EventEffectLoseHp),
                "MaxHpCost" => Some(EffectId::EventEffectLoseMaxHp),
                "GoldCost" => Some(EffectId::EventEffectLoseGold),
                "GoldGain" => Some(EffectId::EventEffectGainGold),
                "MaxHpGain" => Some(EffectId::EventEffectGainMaxHp),
                "Heal" => Some(EffectId::EventEffectHeal),
                "CardsGain" => Some(EffectId::EventEffectAddCard),
                "RelicsGain" => Some(EffectId::EventEffectAddRelic),
                _ => None,
            };
            let subject = value.subject.map_or(ContentId::ABSENT, |subject| match subject {
                S::Card(c) => id(Namespace::CardId, c.index()),
                S::Relic(r) => id(Namespace::RelicId, r.index()),
                S::Potion(p) => id(Namespace::PotionId, p.index()),
                S::Enchantment(e) => e.into(),
                S::CardType(t) => id(Namespace::CardType, t as usize),
            });
            let mut row = Row { kind: RowKind::EffectParameter,
                model: public_variable(value.variable).ok_or(EncodeError::InvalidState("unknown event parameter variable"))?,
                parent, owner, detail: [effect.map_or(ContentId::ABSENT, Into::into),
                    id(Namespace::PublicEventTiming, value.timing as usize), subject, ContentId::ABSENT], ..Row::default() };
            row.numbers.set_integer(0, value.amount)?;
            if value.fraction != [0.; 2] {
                row.numbers.set_float(1, value.fraction[0])?; row.numbers.set_float(2, value.fraction[1])?;
            }
            Self::row(obs, row)?;
        }
        Ok(())
    }

    pub(super) fn route_summary(&mut self, obs: &mut NativeObsV1, map: &ActMap, start: usize) -> Result<()> {
        if self.legacy_mode { return Ok(()); }
        let parent = self.map_rows[start];
        for (kind, count) in MapPointType::ALL.into_iter().zip(self.map_cache.route(map, start)) {
            let mut row = Row { kind: RowKind::RouteSummary, parent,
                detail: [id(Namespace::MapPointType, kind as usize), ContentId::ABSENT, ContentId::ABSENT, ContentId::ABSENT],
                ..Row::default() };
            ints(&mut row.numbers, &[(0, count.count)])?;
            if count.count > 0 {
                ints(&mut row.numbers, &[(1, i32::from(count.nearest)), (2, i32::from(count.farthest))])?;
            }
            Self::row(obs, row)?;
        }
        Ok(())
    }

    pub(super) fn bosses(&self, obs: &mut NativeObsV1, global: RowHandle, act: i32,
        bosses: [Option<sts2_sim::run::NativeBoss>; 2]) -> Result<()> {
        if self.legacy_mode { return Ok(()); }
        for (order, boss) in bosses.into_iter().enumerate() {
            let Some(boss) = boss else { continue; };
            let target = self.map_rows[boss.map_index];
            if target == RowHandle::ABSENT { return Err(EncodeError::InvalidState("visible boss map node missing")); }
            let mut row = Row { kind: RowKind::Boss, model: boss.id.into(), parent: global, ..Row::default() };
            ints(&mut row.numbers, &[(0, as_i32(order)?), (1, act)])?;
            row.flags.set(Flag::Complete, boss.complete); row.flags.set(Flag::Current, boss.current);
            let from = Self::row(obs, row)?; obs.push_edge(Edge { from, to: target, relation: Relation::Target })?;
        }
        Ok(())
    }

    pub(super) fn upgrade_preview(&self, obs: &mut NativeObsV1, ctx: &RunCtx, card: &Card,
        source: RowHandle, parent: RowHandle, purpose: ChoicePurpose) -> Result<()> {
        if self.legacy_mode { return Ok(()); }
        let Some(upgraded) = ctx.native_upgrade_preview(card) else { return Ok(()); };
        let owner = self.seat_rows[usize::from(self.decision.actor)];
        let marker = Self::row(obs, Row { kind: RowKind::UpgradePreview, model: id(Namespace::CardId, card.id.index()),
            detail: [purpose.into(), ContentId::ABSENT, ContentId::ABSENT, ContentId::ABSENT],
            owner, parent, ..Row::default() })?;
        obs.push_edge(Edge { from: marker, to: source, relation: Relation::Source })?;
        let after = self.card(obs, &upgraded, owner, marker, PileKind::None, None, None)?;
        obs.push_edge(Edge { from: marker, to: after, relation: Relation::Target })?;
        Ok(())
    }

    pub(super) fn pile_summary<'a>(&self, obs: &mut NativeObsV1, owner: RowHandle, pile: PileKind,
        cards: impl Iterator<Item = Result<(&'a Card, i32)>>, relics: usize, potions: usize) -> Result<RowHandle> {
        if self.legacy_mode { return Ok(RowHandle::ABSENT); }
        let mut counts = [0; NUMERIC_LANES];
        counts[14] = as_i32(relics)?; counts[15] = as_i32(potions)?;
        for item in cards {
            let (card, cost) = item?; counts[0] += 1;
            let lane = match card.card_type { CardType::Attack => Some(1), CardType::Skill => Some(2),
                CardType::Power => Some(3), CardType::Curse => Some(4), CardType::Status => Some(5), _ => None };
            if let Some(lane) = lane { counts[lane] += 1; }
            counts[6] += i32::from(card.upgraded);
            let lane = if card.x_cost || card.star_x { 12 } else if cost < 0 { 13 }
                else { 7 + cost.min(4) as usize };
            counts[lane] += 1;
        }
        let mut row = Row { kind: RowKind::PileSummary,
            detail: [id(Namespace::PileKind, pile as usize), ContentId::ABSENT, ContentId::ABSENT, ContentId::ABSENT],
            owner, parent: owner, ..Row::default() };
        for (lane, count) in counts.into_iter().enumerate() { row.numbers.set_integer(lane, count)?; }
        Self::row(obs, row)
    }

    pub(super) fn display_variables(&self, obs: &mut impl RowWriter, card: CardId, parent: RowHandle,
        owner: RowHandle, target: Option<RowHandle>, values: &[sts2_sim::card_preview::CardPreviewVar],
        x: (Option<i32>, Option<i32>)) -> Result<()> {
        let (energy, stars) = x;
        let hits = values.iter().find(|v| v.name == "CalculatedHits").map(|v| v.value);
        for value in values {
            let variable = public_variable(value.name).ok_or(EncodeError::InvalidState("unknown display variable"))?;
            let mut row = Row { kind: RowKind::CardPreview,
                model: id(Namespace::CardId, card.index()),
                detail: [variable, ContentId::ABSENT, ContentId::ABSENT, ContentId::ABSENT],
                parent, owner, ..Row::default() };
            ints(&mut row.numbers, &[(0, value.value), (1, value.enchanted)])?;
            for (lane, value) in [(2, hits), (3, energy), (4, stars)] {
                if let Some(value) = value { ints(&mut row.numbers, &[(lane, value)])?; }
            }
            let handle = Self::row(obs, row)?;
            if let Some(target) = target {
                obs.push_edge(Edge { from: handle, to: target, relation: Relation::Target })?;
            }
        }
        if values.is_empty() {
            for (name, lane, amount) in [("EnergyX", 3, energy), ("StarX", 4, stars)] {
                if let Some(amount) = amount {
                    let mut row = Row { kind: RowKind::CardPreview, model: id(Namespace::CardId, card.index()),
                        detail: [public_variable(name).unwrap(), ContentId::ABSENT, ContentId::ABSENT, ContentId::ABSENT],
                        parent, owner, ..Row::default() };
                    ints(&mut row.numbers, &[(0, amount), (lane, amount)])?; Self::row(obs, row)?;
                }
            }
        }
        Ok(())
    }
}

pub(super) fn is_upgrade_purpose(purpose: ChoicePurpose) -> bool {
    matches!(purpose, ChoicePurpose::RunDeckChoiceKindSmith | ChoicePurpose::RunChoiceEffectUpgrade
        | ChoicePurpose::RunChoiceEffectUpgradeAndDamage | ChoicePurpose::EventCardActionUpgrade
        | ChoicePurpose::EventCardActionUpgradeAndDamage | ChoicePurpose::NativeModalKindSmith)
}
