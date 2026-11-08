use super::PlayerRngs;
use crate::{card::Card, relic::Relic, tables::PotionId};

pub use crate::character::Character;

#[derive(Clone, Copy, Debug)]
pub struct PlayerSpec { pub net_id: u64, pub character: Character }

/// ExtraPlayerFields / SerializableExtraPlayerFields (default-valued members
/// are omitted by the C# JSON serializer, but remain independent player state).
#[derive(Clone, Debug, Default)]
pub struct MpExtraPlayerFields {
    pub card_shop_removals_used: i32,
    pub wongo_points: i32,
    pub ccccombo_badge_unlocked: bool,
    pub damage_dealt: i32,
    pub debuffs_applied: i32,
}

/// Permanent player data at RunState.CreateForNewRun, BEFORE SetUpTest/ascension,
/// relic-bag population or Neow. No combat piles or temporary powers live here.
#[derive(Clone, Debug)]
pub struct MpPlayer {
    pub net_id: u64,
    pub character: Character,
    pub hp: i32,
    pub max_hp: i32,
    pub max_energy: i32,
    pub gold: i32,
    pub max_potion_slots: usize,
    pub base_orb_slots: usize,
    pub deck: Vec<Card>,
    pub paels_tooth_cards: Vec<Card>,
    pub relics: Vec<Relic>,
    pub potions: Vec<Option<PotionId>>,
    pub rngs: PlayerRngs,
    pub card_rarity_offset: f32,
    pub potion_reward_odds: f32,
    // Player.cs constructor initializes each discovery collection independently;
    // Player.ToSerializable copies them without deriving from the inventory.
    pub discovered_cards: Vec<(String, String)>,
    pub discovered_enemies: Vec<(String, String)>,
    pub discovered_epochs: Vec<String>,
    pub discovered_potions: Vec<(String, String)>,
    pub discovered_relics: Vec<(String, String)>,
    pub extra_fields: MpExtraPlayerFields,
    pub unlock_state: super::MpUnlockState,
}

impl MpPlayer{
    pub(crate) fn remove_deck_card(&mut self,index:usize)->Card {
        crate::relic::remove_deck_object(&mut self.deck,&mut self.relics,index)
    }
    pub(crate) fn retain_deck_cards(&mut self,predicate:impl FnMut(&Card)->bool) {
        crate::relic::retain_deck_objects(&mut self.deck,&mut self.relics,predicate);
    }
    /// PlayerCmd.GainGold: reuse run modifiers and AfterGoldGained hooks with
    /// this owner's inventory/resources, never another seat's projection.
    pub(super) fn gain_gold(&mut self,shared:&crate::combat::RunCtx,amount:i32)->i32{
        let mut ctx=shared.clone();
        ctx.gold=self.gold;ctx.hp=self.hp;ctx.max_hp=self.max_hp;
        ctx.relics=self.relics.clone();
        for r in &mut ctx.relics{r.owner=0;}
        let gained=ctx.gain_run_gold(amount);
        self.gold=ctx.gold;self.hp=ctx.hp;self.max_hp=ctx.max_hp;
        gained
    }
}

/// Stable roster order is the C# player slot identity. Explicit network IDs
/// may be non-contiguous; duplicate characters are legal, duplicate IDs are not.
#[derive(Clone, Debug)]
pub struct MpRoster {
    players: Vec<MpPlayer>,
    pub next_card_id: u32,
    pub(super) ascension_applied: bool,
}

impl MpRoster {
    pub fn new(seed: &str, specs: &[PlayerSpec]) -> Result<Self, &'static str> {
        Self::from_numeric_seed(crate::rng::deterministic_hash64(seed), specs)
    }

    pub fn from_numeric_seed(seed: u64, specs: &[PlayerSpec]) -> Result<Self, &'static str> {
        if !(2..=4).contains(&specs.len()) { return Err("multiplayer roster requires 2-4 seats"); }
        for (slot, spec) in specs.iter().enumerate() {
            if specs[..slot].iter().any(|other| other.net_id == spec.net_id) {
                return Err("duplicate multiplayer network id");
            }
        }
        let mut next_card_id = 0u32;
        let mut players = Vec::new();
        for (slot, spec) in specs.iter().enumerate() {
            let (hp, strikes, strike, defend, extra, relic) = spec.character.starting();
            let defends = if spec.character == Character::Ironclad { 4 } else { strikes };
            let mut deck = Vec::new();
            for id in std::iter::repeat_n(strike, strikes)
                .chain(std::iter::repeat_n(defend, defends)).chain(extra.iter().copied()) {
                next_card_id += 1;
                let mut card = Card::from_factory(id, false, next_card_id).ok_or("starting card factory")?;
                card.owner = slot as u8;
                card.floor_added_to_deck = Some(1); // Player.PopulateStartingDeck
                deck.push(card);
            }
            let mut relic = Relic::new(relic);
            relic.owner = slot as u8;
            relic.floor_added_to_deck = 1; // Player.PopulateStartingRelics
            players.push(MpPlayer {
                net_id: spec.net_id, character: spec.character, hp, max_hp: hp,
                max_energy: 3, gold: 99, max_potion_slots: 3,
                base_orb_slots: if spec.character == Character::Defect { 3 } else { 0 },
                deck, paels_tooth_cards: Vec::new(), relics: vec![relic], potions: vec![None; 3],
                rngs: PlayerRngs::from_numeric_seed(seed, slot),
                card_rarity_offset: -0.05, potion_reward_odds: 0.4,
                discovered_cards: Vec::new(), discovered_enemies: Vec::new(),
                discovered_epochs: Vec::new(), discovered_potions: Vec::new(), discovered_relics: Vec::new(),
                extra_fields: MpExtraPlayerFields::default(),
                unlock_state: super::MpUnlockState::all(),
            });
        }
        Ok(Self { players, next_card_id, ascension_applied: false })
    }

    pub fn players(&self) -> &[MpPlayer] { &self.players }

    pub fn player(&self, seat: usize) -> Option<&MpPlayer> { self.players.get(seat) }

    pub fn player_mut(&mut self, seat: usize) -> Option<&mut MpPlayer> { self.players.get_mut(seat) }

    pub fn seat_for_net_id(&self, net_id: u64) -> Option<usize> {
        self.players.iter().position(|p| p.net_id == net_id)
    }

    /// Bind an incoming action before its subsystem may mutate the roster.
    pub fn actor(&self, net_id: u64, seat: usize) -> Result<&MpPlayer, &'static str> {
        self.player(seat).filter(|p| p.net_id == net_id).ok_or("multiplayer action actor mismatch")
    }
}
