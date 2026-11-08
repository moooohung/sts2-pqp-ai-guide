//! Read-only original DynamicVar.UpdateCardPreview for replay and observation.
use crate::{card::{enchant, KRef}, combat::{Combat, Props}, tables::*};

#[derive(Clone, Copy)]
enum Kind { Damage, Block, OstyDamage, ExtraDamage, CalculatedDamage, CalculatedOstyDamage, CalculatedBlock, Power(PowerId) }
enum Source { Damage, Block, Var(VarKey) }
struct Definition { name: &'static str, kind: Kind, source: Source, extra: Option<VarKey>, props: Props }
include!(concat!(env!("OUT_DIR"), "/gen_card_preview.rs"));

impl Definition {
    fn base(&self, card: &crate::card::Card) -> Result<i32, &'static str> {
        match self.source {
            Source::Damage => card.damage.or_else(|| card.var(VarKey::Damage)),
            Source::Block => card.block.or_else(|| card.var(VarKey::Block)),
            Source::Var(key) => card.var(key),
        }.ok_or("card is missing its declared display variable")
    }
}

#[derive(Debug)]
pub struct CardPreviewVar {
    pub name: &'static str,
    pub value: i32,
    pub enchanted: i32,
    pub damage: bool,
    pub block: bool,
}

impl crate::card::Card {
    /// DamageVar/BlockVar preview with runGlobalHooks=false, as on the deck,
    /// merchant and upgrade screens. Enchantments still modify displayed values.
    pub fn native_card_preview(&self) -> Result<Vec<CardPreviewVar>, &'static str> {
        definitions(self.id).iter().map(|def| {
            let block = matches!(def.kind, Kind::Block | Kind::CalculatedBlock);
            let extra = matches!(def.kind, Kind::ExtraDamage);
            let power = matches!(def.kind, Kind::Power(_));
            let base = def.base(self)?;
            let value = if power { f64::from(base) } else if block {
                f64::from(base + enchant::block_additive(self, def.props))
            } else {
                (f64::from(base) + if extra { 0. } else { f64::from(enchant::damage_additive(self, def.props)) })
                    * enchant::damage_multiplicative(self, def.props)
            };
            let value = value.min(999_999_999.0) as i32;
            Ok(CardPreviewVar { name: def.name, value, enchanted: value, damage: !block && !extra && !power, block })
        }).collect()
    }
}

impl Combat {
    /// Same read-only damage/block hooks used by gameplay and public intents. In
    /// particular target=None is not an arbitrary enemy or the card's owner.
    pub fn hand_card_preview(&self, k: KRef) -> Result<Vec<CardPreviewVar>, &'static str> {
        self.card_preview(k, None)
    }

    pub fn card_preview(&self, k: KRef, target: Option<crate::card::CId>) -> Result<Vec<CardPreviewVar>, &'static str> {
        let mut result = Vec::new();
        self.card_preview_into(k, target, &mut result)?;
        Ok(result)
    }

    /// Reuse display scratch without changing combat or the preview calculation.
    pub fn card_preview_into(&self, k: KRef, target: Option<crate::card::CId>,
        result: &mut Vec<CardPreviewVar>) -> Result<(), &'static str> {
        let card = &self.cards[k as usize];
        result.clear();
        for def in definitions(card.id) {
            if let Kind::Power(power) = def.kind {
                let base = def.base(card)?;
                let value = self.preview_power_amount_given(k, power, base)?;
                result.push(CardPreviewVar { name: def.name, value, enchanted: base, damage: false, block: false });
                continue;
            }
            let calculated = matches!(def.kind, Kind::CalculatedDamage | Kind::CalculatedOstyDamage | Kind::CalculatedBlock);
            let base = def.base(card)?;
            let block = matches!(def.kind, Kind::Block | Kind::CalculatedBlock);
            let extra_damage = matches!(def.kind, Kind::ExtraDamage);
            let enchanted = if extra_damage {
                base as f64 * enchant::damage_multiplicative(card, def.props)
            } else if block { self.enchanted_block(base, def.props, k) }
                else { self.enchanted_damage(base, def.props, k) };
            let value = if extra_damage { enchanted as i32 } else {
                let amount = if calculated {
                    let extra = match def.extra {
                        Some(key) => card.var(key).ok_or("card is missing its declared display multiplier")?,
                        None => 0,
                    };
                    base + extra * self.card_preview_multiplier(k, target)?
                } else { base };
                if block { self.preview_block(amount, card.owner, def.props, Some(k))? }
                else {
                    let dealer = if matches!(def.kind, Kind::OstyDamage | Kind::CalculatedOstyDamage) {
                        self.osty(card.owner)
                    } else { Some(card.owner) };
                    self.preview_damage(amount, dealer, target, def.props, Some(k))?
                }
            };
            result.push(CardPreviewVar { name:def.name, value:value.min(999_999_999),
                enchanted:enchanted.min(999_999_999.0) as i32,
                damage: !block && !extra_damage, block });
        }
        if let Some((name, multiplier)) = self.card_preview_effect_multiplier(k, target)? {
            let value = card.var(VarKey::CalcBase).unwrap_or(0)
                + card.var(VarKey::CalcExtra).unwrap_or(1) * multiplier;
            result.push(CardPreviewVar { name, value, enchanted: value, damage: false, block: false });
        }
        Ok(())
    }

    /// PowerVar.UpdateCardPreview calls only ModifyPowerAmountGiven, never the
    /// received/Artifact hook. Both original given listeners ignore the target.
    pub fn preview_power_amount_given(&self, k: KRef, power: PowerId, base: i32) -> Result<i32, &'static str> {
        let mut fault = None;
        let owner = self.cards[usize::from(k)].owner;
        let mut amount = base;
        for r in 0..self.run.relics.len() {
            amount += self.query_relic_power_given_additive(r, Some(owner), power, &mut fault);
        }
        for r in 0..self.run.relics.len() {
            amount = self.query_relic_modify_power_amount_given(r, self.run.relics[r].owner,
                power, amount, Some(k), &mut fault);
        }
        if let Some(reason) = fault { Err(reason) } else { Ok(amount) }
    }

    // CalculatedVar.Calculate(null) delegates from the original CanonicalVars.
    // These are card base-value calculations, never an alternate modifier list.
    fn card_preview_multiplier(&self, k: KRef, target: Option<crate::card::CId>) -> Result<i32, &'static str> {
        let card = &self.cards[k as usize];
        let owner = card.owner;
        let p = &self.players[owner];
        let all = || self.pile_cards(owner);
        let osty = || self.osty(owner).filter(|&o| self.cr(o).is_alive());
        Ok(match card.id {
            CardId::AshenStrike => p.exhaust.len() as i32,
            CardId::BodySlam | CardId::DemonicShield => self.cr(owner).block,
            CardId::Bully => target.map_or(0, |t| self.power_amount(t, PowerId::Vulnerable)),
            CardId::TimesUp => target.map_or(0, |t| self.power_amount(t, PowerId::Doom)),
            CardId::Mimic => target.map_or(0, |t| self.cr(t).block),
            CardId::GangUp => self.hist.damage[self.hist.damage_turn_start..].iter().filter(|e|
                Some(e.target) == target && e.props.is_powered_attack()
                    && e.dealer.is_some_and(|d| d != owner && self.cr(d).side == self.cr(owner).side)).count() as i32,
            CardId::Rend => target.map_or(0, |t| self.cr(t).power_list().into_iter().filter(|&p| {
                let p = &self.powers[usize::from(p)];
                !power_meta(p.id).temporary && Self::power_type_for_amount(p.id, p.amount) == PowerType::Debuff
            }).count() as i32),
            CardId::CrescentSpear => all().iter().filter(|&&q| {
                let c = &self.cards[q as usize];
                card_info(c.id, false).is_some_and(|info| info.star_cost > 0 || info.star_x)
            }).count() as i32,
            CardId::DeathMarch => self.hist.extra_draws_this_turn[owner as usize],
            CardId::ExpectAFight => self.power_amount(owner, PowerId::Strength).max(0),
            CardId::GoldAxe => self.hist.finished_plays.len() as i32,
            CardId::MindBlast => p.draw.len() as i32,
            CardId::Mirage => self.alive_enemies().iter().map(|&e| self.power_amount(e, PowerId::Poison)).sum(),
            CardId::PerfectedStrike => all().iter().filter(|&&q| self.cards[q as usize].has_tag(CardTag::Strike)).count() as i32,
            CardId::PreciseCut => -(p.hand.len() as i32 - i32::from(p.hand.contains(&k))),
            CardId::Protector => osty().map_or(0, |o| self.cr(o).max_hp),
            CardId::Sacrifice => osty().map_or(0, |o| self.cr(o).max_hp * 3),
            CardId::SoulStorm => p.exhaust.iter().filter(|&&q| self.cards[q as usize].id == CardId::Soul).count() as i32,
            CardId::SovereignBlade => self.power_amount(owner, PowerId::Parry),
            CardId::Squeeze => all().iter().filter(|&&q| q != k && self.cards[q as usize].has_tag(CardTag::OstyAttack)).count() as i32,
            CardId::Stack => p.discard.len() as i32,
            CardId::Supermassive => self.hist.generated_by_creator[owner as usize],
            CardId::Unleash => osty().map_or(0, |o| self.cr(o).hp),
            CardId::MementoMori => self.hist.discarded_by_owner_this_turn[owner as usize],
            CardId::Murder => self.hist.drawn_by_owner_combat[owner as usize],
            _ => return Err("unported calculated card preview"),
        })
    }

    fn card_preview_effect_multiplier(&self, k: KRef, target: Option<crate::card::CId>)
        -> Result<Option<(&'static str, i32)>, &'static str> {
        let card = &self.cards[usize::from(k)];
        let owner = card.owner; let p = &self.players[owner];
        let finished = |kind| self.hist.finished_plays.iter().filter(|&&(who, turn, q)|
            who == owner && turn == p.turn_number && self.cards[usize::from(q)].card_type == kind).count() as i32;
        let orb_types = || {
            let mut kinds = Vec::new();
            for orb in &p.orbs.orbs { if !kinds.contains(&orb.kind) { kinds.push(orb.kind); } }
            kinds.len() as i32
        };
        Ok(Some(match card.id {
            CardId::Barrage => ("CalculatedHits", p.orbs.orbs.len() as i32),
            CardId::Finisher => ("CalculatedHits", finished(CardType::Attack)),
            CardId::LunarBlast => ("CalculatedHits", finished(CardType::Skill)),
            CardId::Flechettes => ("CalculatedHits", p.hand.iter().filter(|&&q|
                self.cards[usize::from(q)].card_type == CardType::Skill).count() as i32),
            CardId::FlakCannon => ("CalculatedHits", self.pile_cards(owner).iter().filter(|&&q|
                self.cards[usize::from(q)].card_type == CardType::Status && !p.exhaust.contains(&q)).count() as i32),
            CardId::HelixDrill => ("CalculatedHits", self.hist.energy_spent_this_turn[owner as usize]
                - if p.play.contains(&k) { self.modified_card_cost_readonly(owner, k)? } else { 0 }),
            CardId::Rattle => ("CalculatedHits", 1 + self.osty(owner).map_or(0, |o|
                self.hist.attacked_this_turn.iter().filter(|&&a| a == o).count() as i32)),
            CardId::TearAsunder => ("CalculatedHits", 1 + self.hist.unblocked_hits_received_combat(owner)),
            CardId::PullFromBelow => ("CalculatedHits", self.hist.ethereal_plays_finished[owner as usize]),
            CardId::Radiate => ("CalculatedHits", self.hist.stars_gained_this_turn[owner as usize]),
            CardId::CompileDriver => ("CalculatedCards", orb_types()),
            CardId::Synchronize => ("CalculatedFocus", orb_types()),
            CardId::KnifeTrap => ("CalculatedShivs", p.exhaust.iter().filter(|&&q|
                self.cards[usize::from(q)].has_tag(CardTag::Shiv)).count() as i32),
            CardId::Normality => ("CalculatedCards", (self.hist.played_this_turn.iter()
                .filter(|&&(who, _)| who == owner).count() as i32).min(3)),
            CardId::NoEscape => ("CalculatedDoom", target.map_or(0, |t|
                self.power_amount(t, PowerId::Doom).div_euclid(card.var(VarKey::DoomThreshold).unwrap_or(10)))),
            CardId::BeatIntoShape => ("CalculatedForge", self.hist.damage[self.hist.damage_turn_start..].iter()
                .filter(|e| Some(e.target) == target && e.dealer == Some(owner) && e.props.is_powered_attack()).count() as i32),
            CardId::Voltaic => ("CalculatedChannels", self.native_lightning_channels(owner).ok_or("Voltaic owner missing")?),
            _ => return Ok(None),
        }))
    }

    pub fn card_preview_x(&self, k: KRef) -> (Option<i32>, Option<i32>) {
        let card = &self.cards[usize::from(k)]; let player = &self.players[card.owner];
        let bonus = self.run.owned_relics(card.owner).filter(|r|
            r.enabled && r.id == RelicId::ChemicalX).count() as i32 * 2;
        (card.x_cost.then_some(player.energy + bonus), card.star_x.then_some(player.stars + bonus))
    }

    /// Voltaic.cs filters OrbChanneledEntry.Actor.Player, not the current queue
    /// or a party total; evoking/removing an orb never removes its history.
    pub fn native_lightning_channels(&self, owner: crate::card::CId) -> Option<i32> {
        self.players.is_player(owner).then(|| self.hist.public_lightning_channels[usize::from(owner)])
    }
}
