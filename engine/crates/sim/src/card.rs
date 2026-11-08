//! r7 `cards/base.py` `CardInstance`, as a compact `Copy`-free struct living in a combat arena.

use crate::tables::*;
use arrayvec::ArrayVec;

#[path = "enchant.rs"]
pub(crate) mod enchant;

/// Index of a creature in `Combat::creatures`.
pub type CId = u8;
/// Index of a card in `Combat::cards`.
pub type KRef = u16;

/// r7 CardInstance's single combat affliction slot. The current native corpus uses these
/// four names; only one may be present on a card at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardAffliction {
    Bound,
    Smog,
    Entangled,
    Galvanized,
    Hexed,
    Ringing,
    Tainted,
}

/// Effect vars per card: the r7 maximum is 12 (MAD_SCIENCE); build.rs asserts the table fits.
pub const MAX_CARD_VARS: usize = 12;

/// Audited enchantment definition order; the generated EnchantmentId uses these ordinals.
pub const ENCHANTMENT_NAMES: &[&str] = &[
    // Models/Enchantments/Clone.cs is an empty EnchantmentModel subclass: no combat
    // hook overrides. Its marker is consumed by CloneRestSiteOption, not combat.
    "Adroit", "Clone", "Corrupted", "Favored", "Glam", "Goopy", "Imbued", "Inky", "Instinct", "Momentum",
    "Nimble", "PerfectFit", "RoyallyApproved", "Sharp", "Slither", "SlumberingEssence", "SoulsPower",
    "Sown", "Spiral", "Steady", "Swift", "TezcatarasEmber", "Vigorous",
];
pub const ENCHANT_SHARP: &str = "Sharp";
/// Enchantments whose every r7 effect is ported.
pub const PORTED_ENCHANTMENTS: &[&str] = ENCHANTMENT_NAMES;

/// Card-level hooks that never act inside a combat (run layer only).
pub const RUN_ONLY_CARD_HOOKS: u32 = (1 << CardHook::NextEventHooks as u32)
    | (1 << CardHook::UnknownRoomTypesHooks as u32)
    | (1 << CardHook::BeforeRoomEnteredHooks as u32)
    | (1 << CardHook::GeneratedMapHooks as u32)
    | (1 << CardHook::GeneratedMapLateHooks as u32)
    | (1 << CardHook::AfterMapGeneratedHooks as u32)
    | (1 << CardHook::QuestCompleteHooks as u32)
    | (1 << CardHook::RestSiteOptionsHooks as u32)
    // run: cards/status.py:1710
    | (1 << CardHook::AfterCombatEndHooks as u32);
/// Card-level hooks ported for this card (the engine calls them; see content `card_hook_*`).
pub const fn wired_card_hooks(id: CardId) -> u32 {
    match id {
        CardId::Melancholy => 1 << CardHook::AfterDeathHooks as u32,
        CardId::RightHandHand => 1 << CardHook::LateEffects as u32,
        CardId::Flatten => (1 << CardHook::AfterAttackHooks as u32)
            | (1 << CardHook::AfterCardEnteredCombatHooks as u32),
        CardId::HighFive => 1 << CardHook::PlayabilityHooks as u32,
        CardId::BansheesCry => (1 << CardHook::AfterCardPlayedHooks as u32)
            | (1 << CardHook::AfterCardEnteredCombatHooks as u32),
        CardId::Burn | CardId::Wither | CardId::Beckon | CardId::Infection | CardId::Toxic | CardId::BadLuck
        | CardId::Debt | CardId::Decay | CardId::Doubt | CardId::Regret | CardId::Shame => {
            1 << CardHook::TurnEndInHandHooks as u32
        }
        CardId::Void => 1 << CardHook::AfterCardDrawnHooks as u32,
        // IChoosable.OnChosen, called by Knowledge Demon's Curse of Knowledge choice.
        CardId::Disintegration | CardId::MindRot | CardId::SlothStatus | CardId::WasteAway => {
            1 << CardHook::ChosenHooks as u32
        }
        CardId::DrumOfBattleCard => 1 << CardHook::AfterCardExhaustedHooks as u32,
        // C# Midnight.AfterCardEnteredCombat / AfterCardExhausted.
        CardId::Midnight => (1 << CardHook::AfterCardEnteredCombatHooks as u32)
            | (1 << CardHook::AfterCardExhaustedHooks as u32),
        // fire: cards/ironclad.py:1534-1556
        CardId::Stomp => (1 << CardHook::AfterCardEnteredCombatHooks as u32)
            | (1 << CardHook::BeforeCardPlayedHooks as u32),
        // fire: cards/status.py:1710-1715
        CardId::Guilty => 1 << CardHook::AfterCombatEndHooks as u32,
        // fire: cards/status.py:1731-1740
        CardId::Normality | CardId::Enthralled => 1 << CardHook::ShouldPlayHooks as u32,
        // fire: cards/silent.py PINPOINT entered-combat + after-card-played hooks
        CardId::Pinpoint => (1 << CardHook::AfterCardEnteredCombatHooks as u32)
            | (1 << CardHook::AfterCardPlayedHooks as u32),
        // fire: cards/silent.py GRAND_FINALE playability hook
        CardId::GrandFinale => 1 << CardHook::PlayabilityHooks as u32,
        // fire: cards/status.py:1200-1204
        CardId::Shiv => 1 << CardHook::TargetTypeHooks as u32,
        // c2: cards/defect.py ROCKET_PUNCH after_card_generated_for_combat
        CardId::RocketPunch => 1 << CardHook::AfterCardGeneratedForCombatHooks as u32,
        // c3: cards/regent.py
        CardId::KinglyKick | CardId::KinglyPunch => 1 << CardHook::AfterCardDrawnHooks as u32,
        CardId::Bombardment => 1 << CardHook::BeforeHandDrawHooks as u32,
        CardId::MakeItSo => 1 << CardHook::LateEffects as u32,
        CardId::SovereignBlade => 1 << CardHook::TargetTypeHooks as u32,
        // C# MadScience.TargetType / GainsBlock are determined by TinkerTimeType.
        CardId::MadScience => (1 << CardHook::TargetTypeHooks as u32)
            | (1 << CardHook::GainsBlockHooks as u32),
        // cards/status.py CLASH playability.
        CardId::Clash => 1 << CardHook::PlayabilityHooks as u32,
        _ => 0,
    }
}

/// True when this card registers an in-combat card hook the engine does not call yet.
/// (Self-mutating damage/block cards only matter to `upgrade_card`, which ports them.)
pub fn has_unwired_card_hook(id: CardId) -> bool {
    CARD_HOOKS[id.index()] & !(RUN_ONLY_CARD_HOOKS | wired_card_hooks(id) | auto_post_play_legacy_hooks(id)) != 0
}

/// C# callbacks remapped out of the frozen r7 hook enum into TurnEndWalk tier zero.
pub(crate) const fn auto_post_play_legacy_hooks(id: CardId) -> u32 {
    match id {
        // Models/Cards/IAmInvincible.cs:29-36: AfterAutoPostPlayPhaseEntered.
        CardId::IAmInvincible => 1 << CardHook::AfterTurnEndHooks as u32,
        // Models/Cards/HowlFromBeyond.cs:32-38: AfterAutoPostPlayPhaseEntered.
        CardId::HowlFromBeyond => 1 << CardHook::BeforeHandDrawHooks as u32,
        _ => 0,
    }
}

/// Legacy snapshot ingestion boundary; retained cards store numeric identities.
pub fn enchantment_name(name: &str) -> Option<EnchantmentId> {
    EnchantmentId::from_name(name)
}

/// The combat_vars keys r7 reads or writes, as typed fields (r7 keeps a str-keyed dict).
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct CombatVars {
    /// Provenance of FranticEscape.OnPlay's AddThisCombat(1) modifiers.
    /// Observation-only: never participates in gameplay cost calculation.
    pub frantic_escape_cost_increase: i32,
    /// TheScythe.IncreasedDamage, independent of other combat damage modifiers.
    pub scythe_growth: i32,
    /// GeneticAlgorithm.IncreasedBlock, separate from CurrentBlock.
    pub genetic_algorithm_growth: i32,
    pub turn_star_cost_override: Option<i32>,
    pub combat_star_cost_override: Option<i32>,
    pub stars_spent_for_play: Option<i32>,
    pub is_clone: bool,
    pub is_dupe: bool,
    pub sly_this_turn: bool,
    pub return_before_hand_draw_round: Option<i32>,
    pub enchant_momentum_bonus: i32,
    pub enchant_used_vigorous: bool,
    pub enchant_used_glam: bool,
    pub enchant_used_swift: bool,
    pub enchant_used_sown: bool,
    pub affliction: Option<CardAffliction>,
    pub affliction_amount: i32,
    /// Hex tracks whether it introduced Ethereal so removal does not erase a card's native keyword.
    pub affliction_added_ethereal: bool,
    /// Dampen tracks the exact combat card instances it temporarily downgraded.
    pub dampened: bool,
}

impl core::fmt::Debug for CombatVars {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // screen-v2 selector evidence includes this Debug representation.
        // Preserve existing zero-provenance identities; nonzero provenance
        // remains part of the exact shape and cannot reuse a zero-state proof.
        let mut d=f.debug_struct("CombatVars");
        d.field("scythe_growth",&self.scythe_growth)
            .field("turn_star_cost_override",&self.turn_star_cost_override)
            .field("combat_star_cost_override",&self.combat_star_cost_override)
            .field("stars_spent_for_play",&self.stars_spent_for_play)
            .field("is_clone",&self.is_clone)
            .field("is_dupe",&self.is_dupe)
            .field("sly_this_turn",&self.sly_this_turn)
            .field("return_before_hand_draw_round",&self.return_before_hand_draw_round)
            .field("enchant_momentum_bonus",&self.enchant_momentum_bonus)
            .field("enchant_used_vigorous",&self.enchant_used_vigorous)
            .field("enchant_used_glam",&self.enchant_used_glam)
            .field("enchant_used_swift",&self.enchant_used_swift)
            .field("enchant_used_sown",&self.enchant_used_sown)
            .field("affliction",&self.affliction)
            .field("affliction_amount",&self.affliction_amount)
            .field("affliction_added_ethereal",&self.affliction_added_ethereal)
            .field("dampened",&self.dampened);
        if self.frantic_escape_cost_increase != 0 {
            d.field("frantic_escape_cost_increase",&self.frantic_escape_cost_increase);
        }
        if self.genetic_algorithm_growth != 0 {
            d.field("genetic_algorithm_growth",&self.genetic_algorithm_growth);
        }
        d.finish()
    }
}

/// C# LocalCostModifierExpiration (combat-local modifiers disappear with the combat card).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CostExpiry { Combat, Turn, Played, TurnOrPlayed }

#[derive(Clone, Debug)]
struct EnergyCostModifier {
    amount: i32,
    relative: bool,
    reduce_only: bool,
    expiry: CostExpiry,
}

#[derive(Clone, Debug)]
pub struct Card {
    pub id: CardId,
    pub upgraded: bool,
    pub cost: i32,
    pub original_cost: i32,
    /// C# CardEnergyCost._localModifiers, in application order. `cost` caches the local result
    /// except for X cards, whose wire representation retains the factory's X sentinel.
    cost_modifiers: Vec<EnergyCostModifier>,
    pub card_type: CardType,
    pub target: TargetType,
    pub rarity: CardRarity,
    pub damage: Option<i32>,
    pub block: Option<i32>,
    /// `KW_*` bits.
    pub keywords: u16,
    /// Bit i set = `CardTag::ALL[i]`; already merged with r7's reference canonical tags.
    pub tags: u8,
    pub gen_combat: bool,
    pub gen_modifiers: bool,
    /// Effect vars in r7 dict insertion order.
    pub vars: ArrayVec<(VarKey, i32), MAX_CARD_VARS>,
    pub enchantments: ArrayVec<(EnchantmentId, i32), 2>,
    pub instance_id: u32,
    /// CardModel.FloorAddedToDeck is null until the card enters a run deck.
    /// SerializableCard.floor_added_to_deck is preserved across load, clone and upgrade.
    pub floor_added_to_deck: Option<i32>,
    pub x_cost: bool,
    pub star_cost: i32,
    pub star_x: bool,
    pub turn_end_in_hand: bool,
    pub replay: i32,
    pub single_turn_retain: bool,
    pub bound: bool,
    pub energy_spent: i32,
    /// CardEnergyCost.CapturedXValue, before Hook.ModifyXValue (e.g. Chemical X).
    pub captured_x: i32,
    pub owner: CId,
    pub cv: CombatVars,
    /// Index of the persistent deck card this combat copy came from (`_deck_version`).
    pub deck_version: Option<u16>,
    /// Owner of the persistent DeckVersion, independent of a combat copy's
    /// current Owner (TheBall may give that copy to another player).
    pub deck_owner: CId,
}

#[inline]
pub fn tag_bits(tags: &[CardTag]) -> u8 {
    tags.iter().fold(0u8, |acc, t| acc | (1 << t.index()))
}

impl Card {
    /// Scalar published by the frozen card DTO for the C# afflictions ported
    /// explicitly here; Bound keeps its existing frozen-wire representation.
    pub fn observation_affliction_amount(&self)->i32 {
        match self.cv.affliction {
            Some(CardAffliction::Smog|CardAffliction::Entangled|CardAffliction::Tainted)=>self.cv.affliction_amount,
            // These existing C# afflictions use marker-only frozen DTO state
            // when no explicit amount is stored. Do not erase their presence.
            Some(CardAffliction::Galvanized|CardAffliction::Hexed|CardAffliction::Ringing)=>
                if self.cv.affliction_amount==0 {1} else {self.cv.affliction_amount},
            _=>i32::from(self.bound),
        }
    }
    /// `create_card(card_id, upgraded)` from the generated factory table.
    pub fn from_factory(id: CardId, upgraded: bool, instance_id: u32) -> Option<Card> {
        let info = card_info(id, upgraded)?;
        Some(Card {
            id,
            upgraded: info.upgraded,
            cost: info.cost,
            original_cost: info.cost,
            cost_modifiers: Vec::new(),
            card_type: info.card_type,
            target: info.target,
            rarity: info.rarity,
            damage: info.damage,
            block: info.block,
            keywords: info.keywords,
            tags: tag_bits(info.tags),
            gen_combat: info.gen_combat,
            gen_modifiers: info.gen_modifiers,
            vars: info.vars.iter().copied().collect(),
            enchantments: info.enchantments.iter().copied().collect(),
            instance_id,
            floor_added_to_deck: None,
            x_cost: info.x_cost,
            star_cost: info.star_cost,
            star_x: info.star_x,
            turn_end_in_hand: info.turn_end_in_hand,
            replay: info.replay,
            single_turn_retain: info.single_turn_retain,
            bound: false,
            energy_spent: 0,
            captured_x: 0,
            owner: 0,
            cv: CombatVars::default(),
            deck_version: None,
            deck_owner: 0,
        })
    }

    /// CardModel.FromSerializable applies Enchantment.ModifyCard during loading.
    pub fn apply_static_enchantment(&mut self, name: impl TryInto<EnchantmentId>, amount: i32) -> bool {
        enchant::apply_static_enchantment(self, name, amount)
    }

    /// `CardInstance.clone(new_id)`; `combat_vars` gains `_is_clone`.
    pub fn clone_with_id(&self, instance_id: u32) -> Card {
        let mut c = self.clone();
        c.instance_id = instance_id;
        c.cv.is_clone = true;
        // DampenPower re-upgrades only the card instances it downgraded (its
        // downgradedCardsToOldUpgradeLevels dictionary); a clone made later is not one of them.
        c.cv.dampened = false;
        c.energy_spent = 0;
        c
    }

    #[inline] pub fn has_kw(&self, kw: u16) -> bool { self.keywords & kw != 0 }
    #[inline] pub fn exhausts(&self) -> bool { self.has_kw(KW_EXHAUST) }
    #[inline] pub fn is_unplayable(&self) -> bool { self.has_kw(KW_UNPLAYABLE) }
    #[inline] pub fn is_ethereal(&self) -> bool { self.has_kw(KW_ETHEREAL) }
    #[inline] pub fn is_innate(&self) -> bool { self.has_kw(KW_INNATE) }
    #[inline] pub fn is_retain(&self) -> bool { self.has_kw(KW_RETAIN) }
    #[inline] pub fn is_sly(&self) -> bool { self.has_kw(KW_SLY) || self.cv.sly_this_turn }
    #[inline] pub fn should_retain_this_turn(&self) -> bool { self.is_retain() || self.single_turn_retain }
    #[inline] pub fn has_tag(&self, tag: CardTag) -> bool { self.tags & (1 << tag.index()) != 0 }
    #[inline] pub fn is_status(&self) -> bool { self.card_type == CardType::Status }

    #[inline]
    pub fn var(&self, key: VarKey) -> Option<i32> {
        self.vars.iter().find(|(k, _)| *k == key).map(|&(_, v)| v)
    }

    /// C# CardEnergyCost.GetWithModifiers / LocalCostModifier.Modify. Clamp only after all
    /// modifiers (including global modifiers in Combat::modified_card_cost) have run.
    pub fn local_energy_cost(&self) -> i32 {
        // CardEnergyCost's constructor sets Canonical and _base to zero for CostsX.
        if self.x_cost {
            return 0;
        }
        if self.original_cost < 0 {
            return self.original_cost;
        }
        self.cost_modifiers.iter().fold(self.original_cost, |cost, m| {
            let next = if m.relative { cost + m.amount } else { m.amount };
            if m.reduce_only { cost.min(next) } else { next }
        })
    }

    fn refresh_energy_cost(&mut self) {
        if self.x_cost {
            self.cost = self.original_cost;
            return;
        }
        let cost = self.local_energy_cost();
        self.cost = if self.original_cost < 0 { cost } else { cost.max(0) };
    }

    /// C# CardEnergyCost.SetThisCombat/SetThisTurn/SetUntilPlayed/SetThisTurnOrUntilPlayed.
    pub fn set_energy_cost(&mut self, amount: i32, expiry: CostExpiry, reduce_only: bool) {
        if amount == 0 && card_info(self.id, false).is_some_and(|info| info.cost < 0) && !self.x_cost {
            return;
        }
        self.cost_modifiers.push(EnergyCostModifier { amount, relative: false, reduce_only, expiry });
        self.refresh_energy_cost();
    }

    /// C# CardEnergyCost.AddThisCombat/AddThisTurn/AddUntilPlayed.
    pub fn add_energy_cost(&mut self, amount: i32, expiry: CostExpiry) {
        if amount != 0 {
            self.cost_modifiers.push(EnergyCostModifier { amount, relative: true, reduce_only: false, expiry });
            self.refresh_energy_cost();
        }
    }

    pub fn add_combat_cost(&mut self, amount: i32) {
        self.add_energy_cost(amount, CostExpiry::Combat);
    }

    /// C# CardEnergyCost.SetCustomBaseCost / ResetForDowngrade.
    pub fn set_base_energy_cost(&mut self, amount: i32) {
        self.original_cost = amount;
        self.refresh_energy_cost();
    }

    /// r7 persistent snapshots store a current local cost separately from the base.
    /// A persistent override has no turn/play expiration in that source representation.
    pub fn load_persistent_energy_cost(&mut self, current: i32, base: i32) {
        self.set_base_energy_cost(base);
        if current != base {
            self.set_energy_cost(current, CostExpiry::Combat, false);
        }
    }

    /// C# CardEnergyCost.UpgradeBy also lowers every absolute modifier to the new base.
    pub fn upgrade_energy_cost(&mut self, amount: i32) {
        if !self.x_cost && amount < self.original_cost {
            for m in &mut self.cost_modifiers {
                if !m.relative {
                    m.amount = m.amount.min(amount);
                }
            }
        }
        self.set_base_energy_cost(amount);
    }

    pub(crate) fn apply_canonical_energy_upgrade(&mut self, base_cost:i32, upgraded_cost:i32) {
        let delta=upgraded_cost-base_cost;
        if delta!=0 {
            self.upgrade_energy_cost((self.original_cost+delta).max(0));
        } else {
            self.refresh_energy_cost();
        }
    }

    /// C# CardEnergyCost.AfterCardPlayedCleanup, once after the complete play wrapper,
    /// not between repeats of a single play.
    pub fn after_card_played_cost_cleanup(&mut self) {
        self.cost_modifiers.retain(|m| !matches!(m.expiry, CostExpiry::Played | CostExpiry::TurnOrPlayed));
        self.refresh_energy_cost();
        // TemporaryCardCost.ThisTurn clears on either turn end or card play.
        self.cv.turn_star_cost_override = None;
    }

    /// `CardInstance.end_of_turn_cleanup`.
    pub fn end_of_turn_cleanup(&mut self) {
        self.single_turn_retain = false;
        self.bound = false;
        if self.cv.affliction == Some(CardAffliction::Bound) {
            self.cv.affliction = None;
            self.cv.affliction_amount = 0;
        }
        self.cv.sly_this_turn = false;
        self.cost_modifiers.retain(|m| !matches!(m.expiry, CostExpiry::Turn | CostExpiry::TurnOrPlayed));
        self.refresh_energy_cost();
        self.cv.turn_star_cost_override = None;
    }
}
